use std::collections::BTreeMap;

use immich_rs_core::{NormalizedMetadata, RuleEvidence, rule_id};

use crate::google_takeout::{parse_document, record_parse_error};
use crate::reconcile::{attach_sidecar, complete, diagnostic, prepare};
use crate::{DiscoveredMedia, MAX_DIAGNOSTIC_PATHS, ScanState};

#[derive(Clone, Copy)]
struct Match {
    sidecar: usize,
    rule: &'static str,
}

pub fn reconcile(state: &mut ScanState) {
    prepare(state);
    let mut matches = BTreeMap::<usize, Vec<Match>>::new();
    let mut albums = BTreeMap::<String, (String, usize)>::new();
    for sidecar_index in 0..state.sidecars.len() {
        let sidecar = state.sidecars[sidecar_index].clone();
        if sidecar.kind != immich_rs_core::MetadataKind::Json {
            state.warnings.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_UNSUPPORTED,
                "unsupported_takeout_metadata_family",
                vec![sidecar.relative_path],
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
        state.sidecars[sidecar_index].takeout_document = Some(document.clone());
        let exact = candidates_for_title(state, &sidecar.relative_path, &document.title);
        let (candidates, rule) = if exact.is_empty() {
            (
                candidates_for_sidecar_name(state, &sidecar.relative_path),
                rule_id::GOOGLE_TAKEOUT_SUPPLEMENTAL,
            )
        } else {
            (exact, rule_id::GOOGLE_TAKEOUT_TITLE)
        };
        match candidates.as_slice() {
            [media] => matches.entry(*media).or_default().push(Match {
                sidecar: sidecar_index,
                rule,
            }),
            [] if is_album_document(&sidecar.relative_path) => {
                albums.insert(
                    parent(&sidecar.relative_path).to_owned(),
                    (document.title, sidecar_index),
                );
            }
            [] => state.warnings.push(diagnostic(
                rule_id::ORPHAN_SIDECAR,
                "takeout_metadata_without_media",
                vec![sidecar.relative_path],
            )),
            _ => record_ambiguous(state, sidecar.relative_path, &candidates),
        }
    }
    attach_asset_documents(state, matches);
    attach_albums(state, &albums);
    collapse_content_aliases(state);
    for media in &state.media {
        if !media.metadata.iter().any(|candidate| {
            matches!(
                candidate.rule_id.as_str(),
                rule_id::GOOGLE_TAKEOUT_TITLE | rule_id::GOOGLE_TAKEOUT_SUPPLEMENTAL
            )
        }) {
            state.warnings.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_UNMATCHED_MEDIA,
                "takeout_media_without_json",
                vec![media.relative_path.clone()],
            ));
        }
    }
    complete(state);
}

fn candidates_for_title(state: &ScanState, sidecar: &str, title: &str) -> Vec<usize> {
    state
        .media
        .iter()
        .enumerate()
        .filter(|(_, media)| {
            parent(&media.relative_path) == parent(sidecar)
                && filename(&media.relative_path) == title
        })
        .map(|(index, _)| index)
        .collect()
}

fn candidates_for_sidecar_name(state: &ScanState, sidecar: &str) -> Vec<usize> {
    let name = filename(sidecar);
    let base = name
        .strip_suffix(".supplemental-metadata.json")
        .or_else(|| name.strip_suffix(".json"));
    let Some(base) = base.filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let same_directory = state
        .media
        .iter()
        .enumerate()
        .filter(|(_, media)| parent(&media.relative_path) == parent(sidecar))
        .collect::<Vec<_>>();
    let exact = same_directory
        .iter()
        .filter(|(_, media)| filename(&media.relative_path) == base)
        .map(|(index, _)| *index)
        .collect::<Vec<_>>();
    if exact.is_empty() {
        same_directory
            .into_iter()
            .filter(|(_, media)| filename(&media.relative_path).starts_with(base))
            .map(|(index, _)| index)
            .collect()
    } else {
        exact
    }
}

fn attach_asset_documents(state: &mut ScanState, matches: BTreeMap<usize, Vec<Match>>) {
    for (media_index, selected) in matches {
        let documents = selected
            .iter()
            .filter_map(|item| state.sidecars[item.sidecar].takeout_document.clone())
            .collect::<Vec<_>>();
        if documents.windows(2).any(|pair| pair[0] != pair[1]) {
            let mut paths = vec![state.media[media_index].relative_path.clone()];
            paths.extend(
                selected
                    .iter()
                    .map(|item| state.sidecars[item.sidecar].relative_path.clone()),
            );
            state.errors.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_METADATA_CONFLICT,
                "conflicting_takeout_asset_metadata",
                paths,
            ));
            continue;
        }
        if let Some(document) = documents.first() {
            state.media[media_index].normalized_metadata = Some(document.metadata.clone());
        }
        for item in selected {
            attach_sidecar(
                &mut state.media[media_index],
                &state.sidecars[item.sidecar],
                item.rule,
            );
        }
    }
}

fn attach_albums(state: &mut ScanState, albums: &BTreeMap<String, (String, usize)>) {
    for media in &mut state.media {
        let Some((title, sidecar_index)) = albums.get(parent(&media.relative_path)) else {
            continue;
        };
        let metadata = media
            .normalized_metadata
            .get_or_insert_with(NormalizedMetadata::default);
        metadata.albums.push(title.clone());
        metadata.albums.sort();
        metadata.albums.dedup();
        attach_sidecar(
            media,
            &state.sidecars[*sidecar_index],
            rule_id::GOOGLE_TAKEOUT_ALBUM,
        );
    }
}

fn collapse_content_aliases(state: &mut ScanState) {
    let mut groups = BTreeMap::<(u64, String), Vec<DiscoveredMedia>>::new();
    for media in std::mem::take(&mut state.media) {
        groups
            .entry((media.byte_len, media.content_sha256.clone()))
            .or_default()
            .push(media);
    }
    for mut aliases in groups.into_values() {
        aliases.sort_by_key(|media| {
            (
                canonical_rank(&media.relative_path),
                media.relative_path.clone(),
            )
        });
        let mut canonical = aliases.remove(0);
        if aliases.is_empty() {
            state.media.push(canonical);
            continue;
        }
        let mut paths = vec![canonical.relative_path.clone()];
        let mut metadata_conflicted = false;
        for alias in aliases {
            paths.push(alias.relative_path.clone());
            merge_alias(
                &mut canonical,
                alias,
                &mut metadata_conflicted,
                &mut state.errors,
            );
        }
        canonical.evidence.push(RuleEvidence {
            rule_id: rule_id::GOOGLE_TAKEOUT_CONTENT_ALIAS.to_owned(),
            outcome: "content_identical_paths_collapsed".to_owned(),
        });
        state.warnings.push(diagnostic(
            rule_id::GOOGLE_TAKEOUT_CONTENT_ALIAS,
            "content_identical_takeout_aliases",
            paths,
        ));
        state.media.push(canonical);
    }
}

fn merge_alias(
    canonical: &mut DiscoveredMedia,
    alias: DiscoveredMedia,
    metadata_conflicted: &mut bool,
    errors: &mut Vec<immich_rs_core::PlanDiagnostic>,
) {
    canonical.metadata.extend(alias.metadata);
    canonical.evidence.extend(alias.evidence);
    if *metadata_conflicted {
        canonical.normalized_metadata = None;
        return;
    }
    match (
        &mut canonical.normalized_metadata,
        alias.normalized_metadata,
    ) {
        (None, value) => canonical.normalized_metadata = value,
        (Some(left), Some(right)) => {
            *metadata_conflicted = merge_normalized(left, right, &canonical.relative_path, errors);
            if *metadata_conflicted {
                canonical.normalized_metadata = None;
            }
        }
        _ => {}
    }
}

fn merge_normalized(
    left: &mut NormalizedMetadata,
    right: NormalizedMetadata,
    path: &str,
    errors: &mut Vec<immich_rs_core::PlanDiagnostic>,
) -> bool {
    let mut conflicted = merge_value(
        &mut left.description,
        right.description,
        path,
        "conflicting_takeout_description",
        errors,
    );
    conflicted |= merge_value(
        &mut left.taken_at_utc,
        right.taken_at_utc,
        path,
        "conflicting_takeout_timestamp",
        errors,
    );
    conflicted |= merge_value(
        &mut left.location,
        right.location,
        path,
        "conflicting_takeout_location",
        errors,
    );
    left.albums.extend(right.albums);
    left.albums.sort();
    left.albums.dedup();
    conflicted
}

fn merge_value<T: Eq>(
    target: &mut Option<T>,
    incoming: Option<T>,
    path: &str,
    code: &str,
    errors: &mut Vec<immich_rs_core::PlanDiagnostic>,
) -> bool {
    match (&*target, incoming) {
        (None, value) => {
            *target = value;
            false
        }
        (Some(left), Some(right)) if left != &right => {
            errors.push(diagnostic(
                rule_id::GOOGLE_TAKEOUT_METADATA_CONFLICT,
                code,
                vec![path.to_owned()],
            ));
            true
        }
        _ => false,
    }
}

fn canonical_rank(path: &str) -> u8 {
    let parent_name = filename(parent(path));
    let year = parent_name.strip_prefix("Photos from ");
    u8::from(
        !year.is_some_and(|value| {
            value.len() == 4 && value.bytes().all(|byte| byte.is_ascii_digit())
        }),
    )
}

fn is_album_document(path: &str) -> bool {
    if filename(path) != "metadata.json" {
        return false;
    }
    let directory = filename(parent(path));
    !matches!(directory, "Archive" | "Trash" | "Bin") && !directory.starts_with("Photos from ")
}

fn record_ambiguous(state: &mut ScanState, sidecar: String, candidates: &[usize]) {
    let mut paths = vec![sidecar];
    paths.extend(
        candidates
            .iter()
            .take(MAX_DIAGNOSTIC_PATHS - 1)
            .map(|index| state.media[*index].relative_path.clone()),
    );
    state.errors.push(diagnostic(
        rule_id::GOOGLE_TAKEOUT_AMBIGUOUS,
        "ambiguous_takeout_metadata",
        paths,
    ));
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn filename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}
