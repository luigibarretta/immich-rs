use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use immich_rs_application::FolderScanConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::source_config::{
    RawFolderScanConfig, RawImportOptions, RawUploadConfig, SourceKind, SourceSettings,
};
use super::{
    ResourceIdentity, canonical_directory, canonical_resource, update_path_digest,
    valid_relative_root, validate_id, validate_label,
};
use crate::WebConfigError;

const MAX_SOURCE_INPUTS: usize = 64;

/// Operator-configured source addressed by an opaque browser ID.
pub struct SourceProfile {
    id: String,
    label: String,
    configured_inputs: Vec<PathBuf>,
    canonical_inputs: Vec<PathBuf>,
    identities: Vec<ResourceIdentity>,
    generation_sha256: String,
    settings: SourceSettings,
}

impl SourceProfile {
    pub(crate) fn from_raw(raw: RawSourceProfile) -> Result<Self, WebConfigError> {
        validate_id(&raw.id)?;
        validate_label(&raw.label)?;
        if raw.generation == 0 || !raw.allowed_root.is_absolute() {
            return Err(WebConfigError::new("source profile paths are invalid"));
        }
        let relative_inputs = select_inputs(raw.kind, raw.relative_root, raw.relative_inputs)?;
        let settings = SourceSettings::from_raw(raw.kind, raw.scan, &raw.options, raw.upload)?;
        if relative_inputs.len() > settings.maximum_inputs() {
            return Err(WebConfigError::new("source profile input count is invalid"));
        }
        let allowed = canonical_directory(&raw.allowed_root)?;
        let configured_inputs = relative_inputs
            .iter()
            .map(|relative| raw.allowed_root.join(relative))
            .collect::<Vec<_>>();
        let resolved = configured_inputs
            .iter()
            .map(|path| resolve_contained(path, &allowed))
            .collect::<Result<Vec<_>, _>>()?;
        validate_input_types(raw.kind, &resolved)?;
        let (canonical_inputs, identities): (Vec<_>, Vec<_>) = resolved.into_iter().unzip();
        if canonical_inputs.iter().collect::<BTreeSet<_>>().len() != canonical_inputs.len() {
            return Err(WebConfigError::new("source profile inputs must be unique"));
        }
        let generation_sha256 = source_generation(
            raw.generation,
            &raw.id,
            &raw.label,
            &canonical_inputs,
            &identities,
            &settings,
        );
        Ok(Self {
            id: raw.id,
            label: raw.label,
            configured_inputs,
            canonical_inputs,
            identities,
            generation_sha256,
            settings,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        self.settings.kind()
    }

    pub fn resolve(&self) -> Result<ResolvedSourceProfile, WebConfigError> {
        let resolved = self
            .configured_inputs
            .iter()
            .map(|path| canonical_resource(path))
            .collect::<Result<Vec<_>, _>>()?;
        let (canonical_inputs, identities): (Vec<_>, Vec<_>) = resolved.into_iter().unzip();
        if canonical_inputs != self.canonical_inputs || identities != self.identities {
            return Err(WebConfigError::new("source profile identity changed"));
        }
        Ok(ResolvedSourceProfile {
            inputs: canonical_inputs,
            label: self.label.clone(),
            generation_sha256: self.generation_sha256.clone(),
            settings: self.settings.clone(),
        })
    }
}

/// Revalidated source facts passed directly to the application facade.
pub struct ResolvedSourceProfile {
    inputs: Vec<PathBuf>,
    label: String,
    generation_sha256: String,
    settings: SourceSettings,
}

impl ResolvedSourceProfile {
    #[must_use]
    pub fn inputs(&self) -> &[PathBuf] {
        &self.inputs
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn generation_sha256(&self) -> &str {
        &self.generation_sha256
    }

    #[must_use]
    pub const fn settings(&self) -> &SourceSettings {
        &self.settings
    }

    #[must_use]
    pub const fn config(&self) -> &FolderScanConfig {
        self.settings.scan()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSourceProfile {
    pub id: String,
    label: String,
    allowed_root: PathBuf,
    #[serde(default)]
    relative_root: Option<PathBuf>,
    #[serde(default)]
    relative_inputs: Vec<PathBuf>,
    generation: u64,
    #[serde(default)]
    kind: SourceKind,
    #[serde(default)]
    scan: RawFolderScanConfig,
    #[serde(default)]
    options: RawImportOptions,
    #[serde(default)]
    upload: RawUploadConfig,
}

fn select_inputs(
    kind: SourceKind,
    relative_root: Option<PathBuf>,
    relative_inputs: Vec<PathBuf>,
) -> Result<Vec<PathBuf>, WebConfigError> {
    let inputs = match (kind, relative_root, relative_inputs.is_empty()) {
        (SourceKind::Folder, Some(root), true) => vec![root],
        (SourceKind::Folder, _, _) => {
            return Err(WebConfigError::new(
                "folder profile requires exactly one relative_root",
            ));
        }
        (_, None, false) => relative_inputs,
        _ => {
            return Err(WebConfigError::new(
                "import profile requires relative_inputs",
            ));
        }
    };
    if inputs.is_empty()
        || inputs.len() > MAX_SOURCE_INPUTS
        || inputs.iter().any(|path| !valid_relative_root(path))
    {
        return Err(WebConfigError::new("source profile inputs are invalid"));
    }
    Ok(inputs)
}

fn resolve_contained(
    path: &Path,
    allowed: &Path,
) -> Result<(PathBuf, ResourceIdentity), WebConfigError> {
    let (canonical, identity) = canonical_resource(path)?;
    if !canonical.starts_with(allowed) {
        return Err(WebConfigError::new(
            "source profile escapes its allowed root",
        ));
    }
    Ok((canonical, identity))
}

fn validate_input_types(
    kind: SourceKind,
    resolved: &[(PathBuf, ResourceIdentity)],
) -> Result<(), WebConfigError> {
    let metadata = resolved
        .iter()
        .map(|(path, _)| fs::metadata(path))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| WebConfigError::new("cannot inspect source profile"))?;
    if kind == SourceKind::Folder && metadata.iter().all(fs::Metadata::is_dir) {
        return Ok(());
    }
    if metadata.len() == 1 && metadata[0].is_dir() {
        return Ok(());
    }
    let archives = metadata.iter().all(fs::Metadata::is_file)
        && resolved.iter().all(|(path, _)| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
        });
    archives
        .then_some(())
        .ok_or_else(|| WebConfigError::new("import inputs must be one directory or ZIP files"))
}

fn source_generation(
    generation: u64,
    id: &str,
    label: &str,
    canonical_inputs: &[PathBuf],
    identities: &[ResourceIdentity],
    settings: &SourceSettings,
) -> String {
    let mut digest = Sha256::new();
    digest.update(generation.to_le_bytes());
    update_bytes(&mut digest, id.as_bytes());
    update_bytes(&mut digest, label.as_bytes());
    digest.update(canonical_inputs.len().to_le_bytes());
    for (path, identity) in canonical_inputs.iter().zip(identities) {
        digest.update(path.as_os_str().as_encoded_bytes().len().to_le_bytes());
        update_path_digest(&mut digest, path);
        identity.update_digest(&mut digest);
    }
    settings.update_digest(&mut digest);
    format!("{:x}", digest.finalize())
}

fn update_bytes(digest: &mut Sha256, value: &[u8]) {
    digest.update(value.len().to_le_bytes());
    digest.update(value);
}
