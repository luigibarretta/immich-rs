use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use immich_rs_core::{
    CandidateAsset, LivePhotoMember, LivePhotoRole, MediaKind, MetadataCandidate, MetadataKind,
    NORMALIZED_PLAN_SCHEMA_VERSION, NormalizedPlan, PlanDiagnostic, PlanSummary, RuleEvidence,
    SourceDescriptor, SourceKind, UnicodeNormalization, rule_id,
};
use sha2::{Digest, Sha256};

use crate::discovery::extension;
use crate::{
    DiscoveredMedia, DiscoveredSidecar, FolderScanConfig, MAX_DIAGNOSTIC_PATHS, ScanState,
};

pub fn reconcile(state: &mut ScanState) {
    remove_unicode_collisions(state);
    state
        .media
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    state
        .sidecars
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    reconcile_sidecars(state);
    reconcile_live_photos(&mut state.media);
    detect_case_collisions(&state.media, &mut state.errors);
    detect_duplicate_basenames(&state.media, &mut state.warnings);
    for media in &mut state.media {
        media.metadata.sort();
        media.evidence.sort();
    }
    state.warnings.sort();
    state.warnings.dedup();
    state.errors.sort();
    state.errors.dedup();
}

fn remove_unicode_collisions(state: &mut ScanState) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for media in &state.media {
        *counts.entry(media.relative_path.clone()).or_default() += 1;
    }
    for sidecar in &state.sidecars {
        *counts.entry(sidecar.relative_path.clone()).or_default() += 1;
    }
    let collisions = counts
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect::<BTreeSet<_>>();
    for path in &collisions {
        state.errors.push(diagnostic(
            rule_id::UNICODE_COLLISION,
            "native_paths_share_nfc_path",
            vec![path.clone()],
        ));
    }
    state
        .media
        .retain(|media| !collisions.contains(&media.relative_path));
    state
        .sidecars
        .retain(|sidecar| !collisions.contains(&sidecar.relative_path));
}

fn reconcile_sidecars(state: &mut ScanState) {
    for sidecar in &state.sidecars {
        let exact_target = match sidecar.kind {
            MetadataKind::Json => sidecar.relative_path.strip_suffix(".json"),
            MetadataKind::Xmp => None,
        };
        let exact_match = exact_target.and_then(|target| {
            state
                .media
                .iter()
                .position(|media| media.relative_path == target)
        });
        if let Some(index) = exact_match {
            attach_sidecar(
                &mut state.media[index],
                sidecar,
                rule_id::SIDECAR_EXACT_NAME,
            );
            continue;
        }
        let sidecar_parent = parent(&sidecar.relative_path);
        let sidecar_stem = stem(&sidecar.relative_path);
        let basename_matches = state
            .media
            .iter()
            .enumerate()
            .filter(|(_, media)| {
                parent(&media.relative_path) == sidecar_parent
                    && stem(&media.relative_path) == sidecar_stem
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let image_matches = basename_matches
            .iter()
            .copied()
            .filter(|index| state.media[*index].kind == MediaKind::Image)
            .collect::<Vec<_>>();
        let matches = if image_matches.len() == 1 {
            image_matches
        } else {
            basename_matches
        };
        match matches.as_slice() {
            [index] => attach_sidecar(&mut state.media[*index], sidecar, rule_id::SIDECAR_BASENAME),
            [] => state.warnings.push(diagnostic(
                rule_id::ORPHAN_SIDECAR,
                "sidecar_without_media",
                vec![sidecar.relative_path.clone()],
            )),
            _ => {
                let mut paths = vec![sidecar.relative_path.clone()];
                paths.extend(
                    matches
                        .iter()
                        .take(MAX_DIAGNOSTIC_PATHS - 1)
                        .map(|index| state.media[*index].relative_path.clone()),
                );
                state.errors.push(diagnostic(
                    rule_id::AMBIGUOUS_SIDECAR,
                    "ambiguous_sidecar_match",
                    paths,
                ));
            }
        }
    }
}

fn attach_sidecar(media: &mut DiscoveredMedia, sidecar: &DiscoveredSidecar, selected_rule: &str) {
    media.metadata.push(MetadataCandidate {
        relative_path: sidecar.relative_path.clone(),
        kind: sidecar.kind,
        rule_id: selected_rule.to_owned(),
    });
    media.evidence.push(RuleEvidence {
        rule_id: selected_rule.to_owned(),
        outcome: "metadata_candidate_associated".to_owned(),
    });
}

fn reconcile_live_photos(media: &mut [DiscoveredMedia]) {
    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, item) in media.iter().enumerate() {
        groups
            .entry((
                parent(&item.relative_path).to_owned(),
                stem(&item.relative_path).to_owned(),
            ))
            .or_default()
            .push(index);
    }
    for indices in groups.values() {
        let images = indices
            .iter()
            .copied()
            .filter(|index| media[*index].kind == MediaKind::Image)
            .collect::<Vec<_>>();
        let motion_videos = indices
            .iter()
            .copied()
            .filter(|index| {
                media[*index].kind == MediaKind::Video
                    && extension(&media[*index].relative_path).as_deref() == Some("mov")
            })
            .collect::<Vec<_>>();
        if let ([image], [video]) = (images.as_slice(), motion_videos.as_slice()) {
            let pair_id = sha256_fields(&[
                "live-photo-v1",
                &media[*image].relative_path,
                &media[*video].relative_path,
            ]);
            media[*image].live_photo = Some(LivePhotoMember {
                pair_id: pair_id.clone(),
                role: LivePhotoRole::Image,
            });
            media[*video].live_photo = Some(LivePhotoMember {
                pair_id,
                role: LivePhotoRole::Video,
            });
            for index in [*image, *video] {
                media[index].evidence.push(RuleEvidence {
                    rule_id: rule_id::LIVE_PHOTO_BASENAME.to_owned(),
                    outcome: "live_photo_member".to_owned(),
                });
            }
        }
    }
}

fn detect_case_collisions(media: &[DiscoveredMedia], errors: &mut Vec<PlanDiagnostic>) {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in media {
        groups
            .entry(item.relative_path.to_lowercase())
            .or_default()
            .push(item.relative_path.clone());
    }
    for paths in groups.into_values().filter(|paths| paths.len() > 1) {
        errors.push(diagnostic(
            rule_id::CASE_COLLISION,
            "portable_case_collision",
            paths.into_iter().take(MAX_DIAGNOSTIC_PATHS).collect(),
        ));
    }
}

fn detect_duplicate_basenames(media: &[DiscoveredMedia], warnings: &mut Vec<PlanDiagnostic>) {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in media {
        let basename = Path::new(&item.relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .map_or_else(String::new, str::to_lowercase);
        groups
            .entry(basename)
            .or_default()
            .push(item.relative_path.clone());
    }
    for paths in groups.into_values().filter(|paths| {
        paths.len() > 1
            && paths
                .iter()
                .map(|path| parent(path))
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair[0] != pair[1])
    }) {
        warnings.push(diagnostic(
            rule_id::DUPLICATE_BASENAME,
            "duplicate_basename_across_directories",
            paths.into_iter().take(MAX_DIAGNOSTIC_PATHS).collect(),
        ));
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/')
        .map_or("", |(parent_value, _)| parent_value)
}

fn stem(path: &str) -> &str {
    let filename = path
        .rsplit_once('/')
        .map_or(path, |(_, filename_value)| filename_value);
    filename
        .rsplit_once('.')
        .map_or(filename, |(stem_value, _)| stem_value)
}

pub fn diagnostic(rule: &str, code: &str, mut paths: Vec<String>) -> PlanDiagnostic {
    paths.sort();
    paths.dedup();
    paths.truncate(MAX_DIAGNOSTIC_PATHS);
    PlanDiagnostic {
        rule_id: rule.to_owned(),
        code: code.to_owned(),
        paths,
    }
}

pub fn finalize_plan(
    source_label: &str,
    config: &FolderScanConfig,
    state: ScanState,
) -> NormalizedPlan {
    let mut assets = state
        .media
        .into_iter()
        .map(|media| {
            let operation_id = sha256_fields(&[
                "normalized-plan-operation-v1",
                &media.relative_path,
                &media.content_sha256,
                &media.byte_len.to_string(),
            ]);
            CandidateAsset {
                operation_id,
                relative_path: media.relative_path,
                media_kind: media.kind,
                byte_len: media.byte_len,
                content_sha256: media.content_sha256,
                metadata: media.metadata,
                live_photo: media.live_photo,
                evidence: media.evidence,
            }
        })
        .collect::<Vec<_>>();
    assets.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let source_fingerprint =
        source_fingerprint(&assets, &state.sidecars, &state.warnings, &state.errors);
    let sidecars = assets.iter().map(|asset| asset.metadata.len() as u64).sum();
    NormalizedPlan {
        schema_version: NORMALIZED_PLAN_SCHEMA_VERSION,
        source: SourceDescriptor {
            kind: SourceKind::Folder,
            label: source_label.to_owned(),
            fingerprint_sha256: source_fingerprint,
            case_sensitive: config.case_sensitive,
            unicode_normalization: UnicodeNormalization::Nfc,
        },
        summary: PlanSummary {
            assets: assets.len() as u64,
            sidecars,
            bytes_read: state.bytes_read,
        },
        assets,
        warnings: state.warnings,
        errors: state.errors,
    }
}

fn source_fingerprint(
    assets: &[CandidateAsset],
    sidecars: &[DiscoveredSidecar],
    warnings: &[PlanDiagnostic],
    errors: &[PlanDiagnostic],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"normalized-plan-source-v1\0");
    for asset in assets {
        update_field(&mut digest, &asset.relative_path);
        update_field(&mut digest, &asset.content_sha256);
        update_field(&mut digest, &asset.byte_len.to_string());
        for metadata in &asset.metadata {
            update_field(&mut digest, &metadata.relative_path);
            update_field(&mut digest, &metadata.rule_id);
        }
    }
    for sidecar in sidecars {
        update_field(&mut digest, &sidecar.relative_path);
        update_field(&mut digest, &sidecar.byte_len.to_string());
        update_field(&mut digest, &sidecar.content_sha256);
    }
    for diagnostic in warnings.iter().chain(errors) {
        update_field(&mut digest, &diagnostic.rule_id);
        for path in &diagnostic.paths {
            update_field(&mut digest, path);
        }
    }
    format!("{:x}", digest.finalize())
}

fn sha256_fields(fields: &[&str]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        update_field(&mut digest, field);
    }
    format!("{:x}", digest.finalize())
}

fn update_field(digest: &mut Sha256, field: &str) {
    digest.update(field.len().to_le_bytes());
    digest.update(field.as_bytes());
}
