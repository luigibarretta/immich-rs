use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use immich_rs_core::{Cancellation, MetadataKind, NormalizedPlan, SourceKind, rule_id};
use sha2::{Digest, Sha256};

use crate::reconcile::{attach_sidecar, complete, diagnostic, prepare};
use crate::takeout_metadata::{MAX_JSON_BYTES, ParseError, TakeoutDocument, parse_bytes};
use crate::{
    DiscoveredSidecar, FolderScanConfig, MAX_DIAGNOSTIC_PATHS, ProgressObserver,
    ResolvedFolderPlan, ScanError, ScanState, ScanStrategy, TakeoutScanConfig,
    scan_resolved_internal,
};

/// Scan one decompressed Google Takeout layout into a read-only normalized plan.
pub fn scan_google_takeout(
    root: &Path,
    source_label: &str,
    config: &FolderScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    validate_layout(root)?;
    scan_resolved_internal(
        root,
        source_label,
        config,
        cancellation,
        observer,
        &mut |_| {},
        ScanStrategy {
            source_kind: SourceKind::GoogleTakeout,
            schema_version: immich_rs_core::NORMALIZED_PLAN_SCHEMA_VERSION,
            reconcile_state: reconcile,
            skip_path: |_| None,
        },
    )
    .map(|resolved| resolved.plan)
}

/// Scan one decompressed export or a bounded set of independent ZIP parts.
pub fn scan_google_takeout_inputs(
    inputs: &[PathBuf],
    source_label: &str,
    config: &TakeoutScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<NormalizedPlan, ScanError> {
    scan_google_takeout_inputs_resolved(inputs, source_label, config, cancellation, observer)
        .map(|resolved| resolved.plan)
}

/// Scan Takeout inputs and retain exact native or ZIP-entry source locators.
pub fn scan_google_takeout_inputs_resolved(
    inputs: &[PathBuf],
    source_label: &str,
    config: &TakeoutScanConfig,
    cancellation: &impl Cancellation,
    observer: &mut impl ProgressObserver,
) -> Result<ResolvedFolderPlan, ScanError> {
    config.validate()?;
    if let [input] = inputs {
        if std::fs::symlink_metadata(input).is_ok_and(|metadata| {
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
        }) {
            validate_layout(input)?;
            return scan_resolved_internal(
                input,
                source_label,
                &config.scan,
                cancellation,
                observer,
                &mut |_| {},
                ScanStrategy {
                    source_kind: SourceKind::GoogleTakeout,
                    schema_version: immich_rs_core::NORMALIZED_PLAN_SCHEMA_VERSION_V2,
                    reconcile_state: crate::takeout_reconcile::reconcile,
                    skip_path: |_| None,
                },
            );
        }
    }
    if inputs.iter().any(|input| {
        std::fs::symlink_metadata(input).is_ok_and(|metadata| metadata.file_type().is_dir())
    }) {
        return Err(ScanError::UnsupportedLayout(
            "directory and ZIP inputs cannot be mixed",
        ));
    }
    crate::takeout_archive::scan_archives_resolved(
        inputs,
        source_label,
        config,
        cancellation,
        observer,
    )
}

pub fn validate_layout(root: &Path) -> Result<(), ScanError> {
    let takeout = root.join("Takeout");
    let photos = takeout.join("Google Photos");
    for directory in [&takeout, &photos] {
        let metadata = std::fs::symlink_metadata(directory).map_err(|_| {
            ScanError::UnsupportedLayout("expected real Takeout/Google Photos directories")
        })?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(ScanError::UnsupportedLayout(
                "expected real Takeout/Google Photos directories",
            ));
        }
    }
    Ok(())
}

pub fn reconcile(state: &mut ScanState) {
    prepare(state);
    let mut matches: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for sidecar_index in 0..state.sidecars.len() {
        let sidecar = state.sidecars[sidecar_index].clone();
        if sidecar.kind != MetadataKind::Json {
            state.warnings.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_UNSUPPORTED,
                "unsupported_takeout_metadata_family",
                vec![sidecar.relative_path.clone()],
            ));
            continue;
        }
        let document = match parse_document(&sidecar) {
            Ok(document) => document,
            Err(error) => {
                record_parse_error(state, &sidecar, error);
                continue;
            }
        };
        let sidecar_parent = parent_of(&sidecar.relative_path);
        let candidates = state
            .media
            .iter()
            .enumerate()
            .filter(|(_, media)| {
                sidecar_parent == parent_of(&media.relative_path)
                    && filename(&media.relative_path) == document.title
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [media_index] => matches.entry(*media_index).or_default().push(sidecar_index),
            [] => state.warnings.push(diagnostic(
                rule_id::ORPHAN_SIDECAR,
                "takeout_title_without_media",
                vec![sidecar.relative_path.clone()],
            )),
            _ => record_ambiguous_candidates(state, &sidecar, &candidates),
        }
    }
    for (media_index, sidecar_indices) in matches {
        if let [sidecar_index] = sidecar_indices.as_slice() {
            attach_sidecar(
                &mut state.media[media_index],
                &state.sidecars[*sidecar_index],
                rule_id::GOOGLE_TAKEOUT_TITLE,
            );
        } else {
            let mut paths = vec![state.media[media_index].relative_path.clone()];
            paths.extend(
                sidecar_indices
                    .iter()
                    .map(|index| state.sidecars[*index].relative_path.clone()),
            );
            state.errors.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_AMBIGUOUS,
                "multiple_takeout_sidecars_for_media",
                paths,
            ));
        }
    }
    for media in &state.media {
        if media.metadata.is_empty() {
            state.warnings.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_UNMATCHED_MEDIA,
                "takeout_media_without_json",
                vec![media.relative_path.clone()],
            ));
        }
    }
    complete(state);
}

pub fn parse_document(sidecar: &DiscoveredSidecar) -> Result<TakeoutDocument, ParseError> {
    if let Some(error) = sidecar.takeout_parse_error {
        return Err(error);
    }
    if let Some(document) = &sidecar.takeout_document {
        return Ok(document.clone());
    }
    if sidecar.byte_len == 0 {
        return Err(ParseError::Invalid);
    }
    if sidecar.byte_len > MAX_JSON_BYTES {
        return Err(ParseError::Oversized);
    }
    let file = File::open(&sidecar.native_path).map_err(|_| ParseError::Invalid)?;
    let capacity = usize::try_from(sidecar.byte_len).map_err(|_| ParseError::Oversized)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAX_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ParseError::Invalid)?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err(ParseError::Oversized);
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() as u64 != sidecar.byte_len || digest != sidecar.content_sha256 {
        return Err(ParseError::SourceChanged);
    }
    parse_bytes(&bytes)
}

pub fn record_parse_error(state: &mut ScanState, sidecar: &DiscoveredSidecar, error: ParseError) {
    let (rule, code) = match error {
        ParseError::Invalid => (rule_id::GOOGLE_TAKEOUT_JSON_INVALID, "invalid_takeout_json"),
        ParseError::Oversized => (
            rule_id::GOOGLE_TAKEOUT_JSON_OVERSIZED,
            "takeout_json_size_limit_exceeded",
        ),
        ParseError::SourceChanged => (rule_id::SOURCE_CHANGED, "takeout_json_changed_during_scan"),
    };
    state
        .errors
        .push(diagnostic(rule, code, vec![sidecar.relative_path.clone()]));
}

fn record_ambiguous_candidates(
    state: &mut ScanState,
    sidecar: &DiscoveredSidecar,
    candidates: &[usize],
) {
    let mut paths = vec![sidecar.relative_path.clone()];
    paths.extend(
        candidates
            .iter()
            .take(MAX_DIAGNOSTIC_PATHS - 1)
            .map(|index| state.media[*index].relative_path.clone()),
    );
    state.errors.push(diagnostic(
        rule_id::GOOGLE_TAKEOUT_AMBIGUOUS,
        "ambiguous_takeout_title",
        paths,
    ));
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn filename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, filename)| filename)
}
