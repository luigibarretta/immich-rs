use std::collections::BTreeMap;

use immich_rs_client::{ClientErrorClass, ImmichImportClient, RemoteAlbum};
use immich_rs_core::{
    Cancellation, CancellationToken, ImportApplyReport, UploadOperation, UploadPlan,
};

use crate::import_effect_support::{
    album_members, has_assignment, retry_allowed, uncertain, wait_retry,
};
use crate::import_journal::{
    ImportEvent, ImportJournal, ImportOutcome, ImportState, album_identity, metadata_key,
};
use crate::retry::RetryBudget;
use crate::{ExecutorError, UploadExecutionConfig};

#[allow(clippy::too_many_arguments)]
pub async fn apply_metadata(
    operation: &UploadOperation,
    asset_id: &str,
    state: &ImportState,
    journal: &mut ImportJournal,
    report: &mut ImportApplyReport,
    client: &ImmichImportClient,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
    cancellation: &CancellationToken,
) -> Result<bool, ExecutorError> {
    let Some(metadata) = operation
        .normalized_metadata
        .as_ref()
        .filter(|metadata| has_assignment(metadata))
    else {
        return Ok(true);
    };
    if state.metadata_done(&operation.operation_id) {
        report.resumed_effects = report.resumed_effects.saturating_add(1);
        return Ok(true);
    }
    let key = metadata_key(&operation.operation_id);
    let mut retries = 0_u32;
    loop {
        match client
            .update_asset_metadata(asset_id, metadata, cancellation)
            .await
        {
            Ok(()) => {
                append(journal, &key, ImportOutcome::Completed, None, retries)?;
                report.metadata_updated = report.metadata_updated.saturating_add(1);
                report.retried = report.retried.saturating_add(u64::from(retries));
                return Ok(true);
            }
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                append(journal, &key, ImportOutcome::Indeterminate, None, retries)?;
                report.indeterminate = report.indeterminate.saturating_add(1);
                report.retried = report.retried.saturating_add(u64::from(retries));
                report.cancelled = true;
                return Ok(false);
            }
            Err(error) if retry_allowed(error, retries, config, budget) => {
                if !wait_retry(&key, retries, error, config, cancellation).await {
                    append(journal, &key, ImportOutcome::Indeterminate, None, retries)?;
                    report.indeterminate = report.indeterminate.saturating_add(1);
                    report.retried = report.retried.saturating_add(u64::from(retries));
                    report.cancelled = true;
                    return Ok(false);
                }
                retries = retries.saturating_add(1);
            }
            Err(error) => {
                let outcome = if uncertain(error) {
                    report.indeterminate = report.indeterminate.saturating_add(1);
                    ImportOutcome::Indeterminate
                } else {
                    report.failed = report.failed.saturating_add(1);
                    ImportOutcome::Failed
                };
                append(journal, &key, outcome, None, retries)?;
                report.retried = report.retried.saturating_add(u64::from(retries));
                return Ok(true);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_albums(
    plan: &UploadPlan,
    asset_ids: &BTreeMap<String, String>,
    state: &ImportState,
    journal: &mut ImportJournal,
    report: &mut ImportApplyReport,
    client: &ImmichImportClient,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
    cancellation: &CancellationToken,
) -> Result<(), ExecutorError> {
    for (name, members) in album_members(plan, asset_ids) {
        if cancellation.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let identity = album_identity(&name);
        let album = if let Some((_, album_id)) = state.album(&identity) {
            report.resumed_effects = report.resumed_effects.saturating_add(1);
            RemoteAlbumReference(album_id.to_owned())
        } else {
            let effect = converge_album(&name, client, config, budget, cancellation).await;
            let Some(album) = record_album(&identity, effect, journal, report)? else {
                if report.cancelled {
                    break;
                }
                continue;
            };
            album
        };
        if state.membership_done(&identity) {
            report.resumed_effects = report.resumed_effects.saturating_add(1);
            continue;
        }
        let member_refs = members.iter().map(String::as_str).collect::<Vec<_>>();
        if !apply_membership(
            &identity,
            &album.0,
            &member_refs,
            client,
            config,
            budget,
            cancellation,
            journal,
            report,
        )
        .await?
        {
            break;
        }
    }
    Ok(())
}

struct RemoteAlbumReference(String);

enum EffectResult<T> {
    Done(T, ImportOutcome, u32),
    Failed(bool, u32),
    Cancelled(bool, u32),
}

async fn converge_album(
    name: &str,
    client: &ImmichImportClient,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
    cancellation: &CancellationToken,
) -> EffectResult<RemoteAlbumReference> {
    let mut retries = 0_u32;
    let albums = match lookup_albums(name, client, config, budget, cancellation, &mut retries).await
    {
        Ok(albums) => albums,
        Err(result) => return result,
    };
    match albums.as_slice() {
        [album] => {
            return EffectResult::Done(
                RemoteAlbumReference(album.id().to_owned()),
                ImportOutcome::Reused,
                retries,
            );
        }
        [] => {}
        _ => return EffectResult::Failed(false, retries),
    }
    loop {
        match client.create_album(name, cancellation).await {
            Ok(album) => {
                return EffectResult::Done(
                    RemoteAlbumReference(album.id().to_owned()),
                    ImportOutcome::Created,
                    retries,
                );
            }
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                return EffectResult::Cancelled(true, retries);
            }
            Err(error) if error.is_retryable() => {
                let observed =
                    lookup_albums(name, client, config, budget, cancellation, &mut retries).await;
                match observed {
                    Ok(albums) if albums.len() == 1 => {
                        return EffectResult::Done(
                            RemoteAlbumReference(albums[0].id().to_owned()),
                            ImportOutcome::Created,
                            retries,
                        );
                    }
                    Ok(albums) if albums.is_empty() => {}
                    Ok(_) => return EffectResult::Failed(true, retries),
                    Err(EffectResult::Cancelled(_, count)) => {
                        return EffectResult::Cancelled(true, count);
                    }
                    Err(EffectResult::Failed(_, count)) => {
                        return EffectResult::Failed(true, count);
                    }
                    Err(EffectResult::Done(_, _, _)) => {
                        return EffectResult::Failed(true, retries);
                    }
                }
                if !retry_allowed(error, retries, config, budget)
                    || !wait_retry(name, retries, error, config, cancellation).await
                {
                    return if cancellation.is_cancelled() {
                        EffectResult::Cancelled(true, retries)
                    } else {
                        EffectResult::Failed(uncertain(error), retries)
                    };
                }
                retries = retries.saturating_add(1);
            }
            Err(_) => return EffectResult::Failed(false, retries),
        }
    }
}

async fn lookup_albums(
    name: &str,
    client: &ImmichImportClient,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
    cancellation: &CancellationToken,
    retries: &mut u32,
) -> Result<Vec<RemoteAlbum>, EffectResult<RemoteAlbumReference>> {
    loop {
        match client.find_owned_albums(name, cancellation).await {
            Ok(albums) => return Ok(albums),
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                return Err(EffectResult::Cancelled(false, *retries));
            }
            Err(error) if retry_allowed(error, *retries, config, budget) => {
                if !wait_retry(name, *retries, error, config, cancellation).await {
                    return Err(EffectResult::Cancelled(false, *retries));
                }
                *retries = retries.saturating_add(1);
            }
            Err(_) => return Err(EffectResult::Failed(false, *retries)),
        }
    }
}

fn record_album(
    identity: &str,
    effect: EffectResult<RemoteAlbumReference>,
    journal: &mut ImportJournal,
    report: &mut ImportApplyReport,
) -> Result<Option<RemoteAlbumReference>, ExecutorError> {
    let key = format!("album:{identity}");
    match effect {
        EffectResult::Done(album, outcome, retries) => {
            append(journal, &key, outcome, Some(&album.0), retries)?;
            match outcome {
                ImportOutcome::Created => {
                    report.albums_created = report.albums_created.saturating_add(1);
                }
                ImportOutcome::Reused => {
                    report.albums_reused = report.albums_reused.saturating_add(1);
                }
                _ => {}
            }
            report.retried = report.retried.saturating_add(u64::from(retries));
            Ok(Some(album))
        }
        EffectResult::Failed(indeterminate, retries) => {
            record_effect_failure(&key, indeterminate, retries, journal, report)?;
            Ok(None)
        }
        EffectResult::Cancelled(indeterminate, retries) => {
            record_effect_failure(&key, indeterminate, retries, journal, report)?;
            report.cancelled = true;
            Ok(None)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn apply_membership(
    identity: &str,
    album_id: &str,
    member_ids: &[&str],
    client: &ImmichImportClient,
    config: &UploadExecutionConfig,
    budget: &RetryBudget,
    cancellation: &CancellationToken,
    journal: &mut ImportJournal,
    report: &mut ImportApplyReport,
) -> Result<bool, ExecutorError> {
    let key = format!("membership:{identity}");
    let mut retries = 0_u32;
    loop {
        match client
            .add_album_assets(album_id, member_ids, cancellation)
            .await
        {
            Ok(()) => {
                append(journal, &key, ImportOutcome::Completed, None, retries)?;
                report.album_memberships_updated =
                    report.album_memberships_updated.saturating_add(1);
                report.retried = report.retried.saturating_add(u64::from(retries));
                return Ok(true);
            }
            Err(error) if error.class() == ClientErrorClass::Cancelled => {
                record_effect_failure(&key, true, retries, journal, report)?;
                report.cancelled = true;
                return Ok(false);
            }
            Err(error) if retry_allowed(error, retries, config, budget) => {
                if !wait_retry(&key, retries, error, config, cancellation).await {
                    record_effect_failure(&key, true, retries, journal, report)?;
                    report.cancelled = true;
                    return Ok(false);
                }
                retries = retries.saturating_add(1);
            }
            Err(error) => {
                record_effect_failure(&key, uncertain(error), retries, journal, report)?;
                return Ok(true);
            }
        }
    }
}

fn record_effect_failure(
    key: &str,
    indeterminate: bool,
    retries: u32,
    journal: &mut ImportJournal,
    report: &mut ImportApplyReport,
) -> Result<(), ExecutorError> {
    let outcome = if indeterminate {
        report.indeterminate = report.indeterminate.saturating_add(1);
        ImportOutcome::Indeterminate
    } else {
        report.failed = report.failed.saturating_add(1);
        ImportOutcome::Failed
    };
    append(journal, key, outcome, None, retries)?;
    report.retried = report.retried.saturating_add(u64::from(retries));
    Ok(())
}

fn append(
    journal: &mut ImportJournal,
    key: &str,
    outcome: ImportOutcome,
    remote_id: Option<&str>,
    retries: u32,
) -> Result<(), ExecutorError> {
    journal.append(&ImportEvent {
        effect_key: key,
        outcome,
        remote_id,
        retries,
    })
}
