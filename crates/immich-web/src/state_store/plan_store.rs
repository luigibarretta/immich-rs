use std::fs::{self, File};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};

use immich_rs_application::{UploadPlan, upload_plan_sha256};

use super::plan_io::{
    file_stamp, load_json, load_json_file, publish_new, sync_directory, write_json,
};
use super::{ArtifactRef, PlanArtifact, PlanBinding, StoredUploadPlan};
use crate::WebConfigError;
use crate::profiles::validate_id;

const BINDING_SCHEMA_VERSION: u32 = 1;
const MAX_BINDING_BYTES: u64 = 16 * 1_024;
const CREATE_ATTEMPTS: usize = 8;

pub struct PlanStore {
    directory: PathBuf,
    maximum_bytes: u64,
    maximum_store_bytes: u64,
    maximum_files: usize,
}

impl PlanStore {
    pub(super) const fn new(
        directory: PathBuf,
        maximum_bytes: u64,
        maximum_store_bytes: u64,
        maximum_files: usize,
    ) -> Self {
        Self {
            directory,
            maximum_bytes,
            maximum_store_bytes,
            maximum_files,
        }
    }

    pub(super) fn is_below(&self, state_root: &Path) -> bool {
        self.directory.starts_with(state_root)
    }

    pub(super) fn validate(&self) -> Result<(), WebConfigError> {
        self.check_store_bound()
    }

    pub fn write(
        &self,
        plan: &UploadPlan,
        source_profile_id: &str,
        source_profile_sha256: &str,
        server_profile_id: &str,
        server_profile_sha256: &str,
        credential_generation: u64,
    ) -> Result<PlanArtifact, WebConfigError> {
        plan.validate()
            .map_err(|_| WebConfigError::new("upload plan is invalid"))?;
        validate_binding_inputs(
            source_profile_id,
            source_profile_sha256,
            server_profile_id,
            server_profile_sha256,
            credential_generation,
        )?;
        let plan_sha256 = upload_plan_sha256(plan)
            .map_err(|_| WebConfigError::new("upload plan digest failed"))?;
        let max_logical_effects = logical_effects(plan);
        let binding = PlanBinding {
            schema_version: BINDING_SCHEMA_VERSION,
            plan_sha256: plan_sha256.clone(),
            source_profile_id: source_profile_id.to_owned(),
            source_profile_sha256: source_profile_sha256.to_owned(),
            server_profile_id: server_profile_id.to_owned(),
            server_profile_sha256: server_profile_sha256.to_owned(),
            credential_generation,
            server_identity_sha256: plan.server.identity_sha256.clone(),
            max_logical_effects,
        };
        for _attempt in 0..CREATE_ATTEMPTS {
            let reference = ArtifactRef::random()?;
            if self.write_new(reference, plan, &binding)? {
                return Ok(PlanArtifact {
                    reference,
                    schema_version: plan.schema_version,
                    plan_sha256,
                    max_logical_effects,
                });
            }
        }
        Err(WebConfigError::new("plan reference capacity exhausted"))
    }

    pub fn load(&self, reference: ArtifactRef) -> Result<StoredUploadPlan, WebConfigError> {
        let binding_path = self.binding_path(reference);
        let (binding, binding_stamp) = load_json(&binding_path, MAX_BINDING_BYTES)?;
        let plan_path = self.plan_path(reference);
        let (plan, _plan_stamp) = load_json(&plan_path, self.maximum_bytes)?;
        if file_stamp(&binding_path, MAX_BINDING_BYTES)? != binding_stamp {
            return Err(WebConfigError::new("plan binding identity changed"));
        }
        validate_loaded(&plan, &binding)?;
        Ok(StoredUploadPlan {
            reference,
            plan,
            binding,
        })
    }

    pub fn open_export(&self, reference: ArtifactRef) -> Result<(File, u64), WebConfigError> {
        let binding_path = self.binding_path(reference);
        let (binding, binding_stamp) = load_json(&binding_path, MAX_BINDING_BYTES)?;
        let plan_path = self.plan_path(reference);
        let (plan, mut file, plan_stamp) = load_json_file(&plan_path, self.maximum_bytes)?;
        if file_stamp(&binding_path, MAX_BINDING_BYTES)? != binding_stamp {
            return Err(WebConfigError::new("plan binding identity changed"));
        }
        validate_loaded(&plan, &binding)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| WebConfigError::new("cannot rewind upload plan"))?;
        Ok((file, plan_stamp.length))
    }

    pub fn remove(&self, reference: ArtifactRef) -> Result<(), WebConfigError> {
        for path in [self.plan_path(reference), self.binding_path(reference)] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(WebConfigError::new("cannot remove unpublished plan")),
            }
        }
        sync_directory(&self.directory)
    }

    fn write_new(
        &self,
        reference: ArtifactRef,
        plan: &UploadPlan,
        binding: &PlanBinding,
    ) -> Result<bool, WebConfigError> {
        let plan_path = self.plan_path(reference);
        let binding_path = self.binding_path(reference);
        if plan_path.exists() || binding_path.exists() {
            return Ok(false);
        }
        let pending = pending_path(&self.directory, reference)?;
        let pending_binding = pending.with_extension("binding.pending");
        let result = (|| {
            write_json(&pending_binding, binding, MAX_BINDING_BYTES)?;
            write_json(&pending, plan, self.maximum_bytes)?;
            self.check_store_bound()?;
            publish_new(&pending_binding, &binding_path)
                .map_err(|_| WebConfigError::new("cannot publish plan binding"))?;
            if publish_new(&pending, &plan_path).is_err() {
                let _cleanup = fs::remove_file(&binding_path);
                return Err(WebConfigError::new("cannot publish upload plan"));
            }
            sync_directory(&self.directory)
        })();
        if result.is_err() {
            let _cleanup = fs::remove_file(&pending);
            let _cleanup = fs::remove_file(&pending_binding);
        }
        result.map(|()| true)
    }

    fn check_store_bound(&self) -> Result<(), WebConfigError> {
        let mut bytes = 0_u64;
        let mut files = 0_usize;
        let entries = fs::read_dir(&self.directory)
            .map_err(|_| WebConfigError::new("cannot inspect plan directory"))?;
        for entry in entries {
            let entry = entry.map_err(|_| WebConfigError::new("cannot inspect plan directory"))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| WebConfigError::new("cannot inspect plan directory"))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(WebConfigError::new("plan directory entry is invalid"));
            }
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(metadata.len());
            if files > self.maximum_files || bytes > self.maximum_store_bytes {
                return Err(WebConfigError::new("plan storage bound exceeded"));
            }
        }
        Ok(())
    }

    fn plan_path(&self, reference: ArtifactRef) -> PathBuf {
        self.directory.join(format!("{}.json", reference.encode()))
    }

    fn binding_path(&self, reference: ArtifactRef) -> PathBuf {
        self.directory
            .join(format!("{}.binding.json", reference.encode()))
    }
}

fn validate_binding_inputs(
    source_id: &str,
    source_sha256: &str,
    server_id: &str,
    server_sha256: &str,
    credential_generation: u64,
) -> Result<(), WebConfigError> {
    validate_id(source_id)?;
    validate_id(server_id)?;
    if credential_generation == 0 || !is_sha256(source_sha256) || !is_sha256(server_sha256) {
        return Err(WebConfigError::new("plan profile binding is invalid"));
    }
    Ok(())
}

fn validate_loaded(plan: &UploadPlan, binding: &PlanBinding) -> Result<(), WebConfigError> {
    plan.validate()
        .map_err(|_| WebConfigError::new("stored upload plan is invalid"))?;
    validate_binding_inputs(
        &binding.source_profile_id,
        &binding.source_profile_sha256,
        &binding.server_profile_id,
        &binding.server_profile_sha256,
        binding.credential_generation,
    )?;
    let digest = upload_plan_sha256(plan)
        .map_err(|_| WebConfigError::new("stored upload plan digest failed"))?;
    if binding.schema_version != BINDING_SCHEMA_VERSION
        || digest != binding.plan_sha256
        || !is_sha256(&binding.server_identity_sha256)
        || plan.server.identity_sha256 != binding.server_identity_sha256
        || logical_effects(plan) != binding.max_logical_effects
    {
        return Err(WebConfigError::new("stored upload plan binding changed"));
    }
    Ok(())
}

const fn logical_effects(plan: &UploadPlan) -> u64 {
    if plan.summary.max_mutations == 0 {
        plan.summary.operations
    } else {
        plan.summary.max_mutations
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn pending_path(directory: &Path, reference: ArtifactRef) -> Result<PathBuf, WebConfigError> {
    let nonce = ArtifactRef::random()?.encode();
    Ok(directory.join(format!(".{}-{nonce}.pending", reference.encode())))
}
