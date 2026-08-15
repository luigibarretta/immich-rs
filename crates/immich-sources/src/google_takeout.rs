use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use immich_rs_core::{Cancellation, MetadataKind, NormalizedPlan, SourceKind, rule_id};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::reconcile::{attach_sidecar, complete, diagnostic, prepare};
use crate::{
    DiscoveredSidecar, FolderScanConfig, MAX_DIAGNOSTIC_PATHS, ProgressObserver, ScanError,
    ScanState, ScanStrategy, scan_resolved_internal,
};

const MAX_JSON_BYTES: u64 = 256 * 1_024;
const MAX_TITLE_BYTES: usize = 4_096;

#[derive(Deserialize)]
struct GoogleSidecar {
    title: String,
}

#[derive(Clone, Copy)]
enum ParseError {
    Invalid,
    Oversized,
    SourceChanged,
}

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
            reconcile_state: reconcile,
        },
    )
    .map(|resolved| resolved.plan)
}

pub fn validate_layout(root: &Path) -> Result<(), ScanError> {
    let photos = root.join("Takeout").join("Google Photos");
    let metadata = std::fs::symlink_metadata(&photos)
        .map_err(|_| ScanError::UnsupportedLayout("expected Takeout/Google Photos directory"))?;
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(ScanError::UnsupportedLayout(
            "expected Takeout/Google Photos directory",
        ))
    }
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
        let title = match parse_title(&sidecar) {
            Ok(title) => title,
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
                    && filename(&media.relative_path) == title
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

fn parse_title(sidecar: &DiscoveredSidecar) -> Result<String, ParseError> {
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
    let parsed: GoogleSidecar = serde_json::from_slice(&bytes).map_err(|_| ParseError::Invalid)?;
    if parsed.title.is_empty()
        || parsed.title.len() > MAX_TITLE_BYTES
        || parsed.title.chars().any(char::is_control)
        || parsed.title.contains(['/', '\\'])
    {
        return Err(ParseError::Invalid);
    }
    Ok(parsed.title)
}

fn record_parse_error(state: &mut ScanState, sidecar: &DiscoveredSidecar, error: ParseError) {
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
