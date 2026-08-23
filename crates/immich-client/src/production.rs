use std::fmt::{self, Debug, Formatter};

use immich_rs_core::UploadPlan;
use sha2::{Digest, Sha256};

use crate::{ClientError, ClientErrorClass, ImmichUploadClient};

/// SHA-256 of the canonical compact JSON representation of an upload plan.
pub fn upload_plan_sha256(plan: &UploadPlan) -> Result<String, ClientError> {
    plan.validate()
        .map_err(|_| ClientError::new(ClientErrorClass::Compatibility))?;
    let bytes =
        serde_json::to_vec(plan).map_err(|_| ClientError::new(ClientErrorClass::Protocol))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Unforgeable binding between one authorized production client and upload plan.
pub struct ProductionUploadAuthorization {
    plan_sha256: String,
    expected_operations: u64,
    backup_reference_sha256: String,
}

impl ProductionUploadAuthorization {
    pub(crate) const fn new(
        plan_sha256: String,
        expected_operations: u64,
        backup_reference_sha256: String,
    ) -> Self {
        Self {
            plan_sha256,
            expected_operations,
            backup_reference_sha256,
        }
    }

    /// Verify that this authorization still describes the supplied immutable plan.
    #[must_use]
    pub fn matches_plan(&self, plan: &UploadPlan) -> bool {
        upload_plan_sha256(plan).is_ok_and(|digest| {
            digest == self.plan_sha256
                && plan.summary.operations == self.expected_operations
                && plan.operations.len() as u64 == self.expected_operations
        })
    }

    /// Return the hashed backup reference for durable checkpoint binding.
    #[must_use]
    pub fn backup_reference_sha256(&self) -> &str {
        &self.backup_reference_sha256
    }
}

impl Debug for ProductionUploadAuthorization {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductionUploadAuthorization")
            .field("plan_sha256", &"[REDACTED]")
            .field("expected_operations", &self.expected_operations)
            .field("backup_reference_sha256", &"[REDACTED]")
            .finish()
    }
}

/// Upload capability that can exist only after exact production authorization.
pub struct ProductionImmichUploadClient {
    upload: ImmichUploadClient,
    proof: ProductionUploadAuthorization,
}

impl ProductionImmichUploadClient {
    pub(crate) const fn new(
        upload: ImmichUploadClient,
        proof: ProductionUploadAuthorization,
    ) -> Self {
        Self { upload, proof }
    }

    /// Return the upload transport while preserving ownership of the authorization proof.
    #[must_use]
    pub const fn upload(&self) -> &ImmichUploadClient {
        &self.upload
    }

    /// Return the plan and backup binding used by the executor checkpoint.
    #[must_use]
    pub const fn authorization(&self) -> &ProductionUploadAuthorization {
        &self.proof
    }
}

impl Debug for ProductionImmichUploadClient {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductionImmichUploadClient")
            .field("upload", &self.upload)
            .field("authorization", &self.proof)
            .finish_non_exhaustive()
    }
}
