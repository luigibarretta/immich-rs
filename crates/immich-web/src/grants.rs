use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use immich_rs_application::{PreparedUploadApply, ProductionWriteRequest, prepare_upload_apply};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::jobs::AdmissionError;
use crate::state_store::{
    ConsoleStore, DryRunBinding, ReceiptRef, now_unix, source_configuration_digest,
};
use crate::{ServerMode, WebConfig, WebLimits};

const IDEMPOTENCY_BYTES: usize = 16;

pub struct GrantStore {
    config: Arc<WebConfig>,
    store: Arc<ConsoleStore>,
    limits: WebLimits,
    state: Mutex<GrantState>,
}

pub struct GrantRequest {
    pub receipt: ReceiptRef,
    pub plan_sha256: String,
    pub max_logical_effects: u64,
    pub backup_reference: String,
}

pub struct GrantView {
    pub idempotency_key: String,
    pub production: bool,
}

pub struct ApplyCapability {
    pub prepared: PreparedUploadApply,
    pub binding: GrantBinding,
}

#[derive(Clone)]
pub struct GrantBinding {
    pub owner: [u8; 32],
    pub receipt: ReceiptRef,
    pub dry_run: DryRunBinding,
    pub source_profile_id: String,
    pub server_profile_id: String,
    pub backup_reference_sha256: Option<String>,
}

pub enum GrantAdmission {
    Admitted(String),
    Existing(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantError {
    Invalid,
    Expired,
    Unavailable,
    Admission(AdmissionError),
}

struct GrantEntry {
    receipt: ReceiptRef,
    idempotency_key: String,
    deadline: Instant,
    status: GrantStatus,
}

struct GrantState {
    entries: BTreeMap<[u8; 32], GrantEntry>,
    spent_receipts: BTreeSet<ReceiptRef>,
}

enum GrantStatus {
    Pending(Option<Box<ApplyCapability>>),
    Consumed(String),
}

impl GrantStore {
    pub fn new(config: Arc<WebConfig>, store: Arc<ConsoleStore>) -> Self {
        let limits = config.limits();
        Self {
            config,
            store,
            limits,
            state: Mutex::new(GrantState {
                entries: BTreeMap::new(),
                spent_receipts: BTreeSet::new(),
            }),
        }
    }

    pub fn confirm(&self, owner: [u8; 32], request: GrantRequest) -> Result<GrantView, GrantError> {
        self.confirm_at(owner, request, Instant::now())
    }

    pub fn admit_with(
        &self,
        owner: [u8; 32],
        receipt: ReceiptRef,
        idempotency_key: &str,
        admit: impl FnOnce(ApplyCapability) -> Result<String, AdmissionError>,
    ) -> Result<GrantAdmission, GrantError> {
        self.admit_at(owner, receipt, idempotency_key, Instant::now(), admit)
    }

    pub fn revoke_owner(&self, owner: &[u8; 32]) {
        if let Ok(mut state) = self.state.lock() {
            state.entries.remove(owner);
        }
    }

    fn confirm_at(
        &self,
        owner: [u8; 32],
        request: GrantRequest,
        now: Instant,
    ) -> Result<GrantView, GrantError> {
        let receipt = request.receipt;
        let (capability, production) = self.prepare(owner, request)?;
        let idempotency_key = random_idempotency_key()?;
        let deadline = now
            .checked_add(Duration::from_secs(self.limits.production_grant_seconds))
            .ok_or(GrantError::Unavailable)?;
        let entry = GrantEntry {
            receipt: capability.binding.receipt,
            idempotency_key: idempotency_key.clone(),
            deadline,
            status: GrantStatus::Pending(Some(Box::new(capability))),
        };
        let mut state = self.state.lock().map_err(|_| GrantError::Unavailable)?;
        if state.spent_receipts.contains(&receipt) {
            return Err(GrantError::Invalid);
        }
        state.entries.insert(owner, entry);
        drop(state);
        Ok(GrantView {
            idempotency_key,
            production,
        })
    }

    fn admit_at(
        &self,
        owner: [u8; 32],
        receipt: ReceiptRef,
        idempotency_key: &str,
        now: Instant,
        admit: impl FnOnce(ApplyCapability) -> Result<String, AdmissionError>,
    ) -> Result<GrantAdmission, GrantError> {
        let mut state = self.state.lock().map_err(|_| GrantError::Unavailable)?;
        let result = self.admit_locked(&mut state, owner, receipt, idempotency_key, now, admit);
        drop(state);
        result
    }

    fn admit_locked(
        &self,
        state: &mut GrantState,
        owner: [u8; 32],
        receipt: ReceiptRef,
        idempotency_key: &str,
        now: Instant,
        admit: impl FnOnce(ApplyCapability) -> Result<String, AdmissionError>,
    ) -> Result<GrantAdmission, GrantError> {
        let entry = state.entries.get_mut(&owner).ok_or(GrantError::Invalid)?;
        if entry.receipt != receipt || !same_key(&entry.idempotency_key, idempotency_key) {
            return Err(GrantError::Invalid);
        }
        if now >= entry.deadline {
            state.entries.remove(&owner);
            return Err(GrantError::Expired);
        }
        if let GrantStatus::Consumed(job_id) = &entry.status {
            return Ok(GrantAdmission::Existing(job_id.clone()));
        }
        let GrantStatus::Pending(slot) = &mut entry.status else {
            return Err(GrantError::Unavailable);
        };
        if state.spent_receipts.contains(&receipt)
            || state.spent_receipts.len() >= self.limits.history_retained_rows
        {
            state.entries.remove(&owner);
            return Err(GrantError::Unavailable);
        }
        let capability = slot.take().ok_or(GrantError::Unavailable)?;
        if capability.prepared.is_production()
            != capability.binding.backup_reference_sha256.is_some()
            || !self.is_current(&capability.binding)
        {
            state.entries.remove(&owner);
            return Err(GrantError::Invalid);
        }
        match admit(*capability) {
            Ok(job_id) => {
                state.spent_receipts.insert(receipt);
                entry.status = GrantStatus::Consumed(job_id.clone());
                Ok(GrantAdmission::Admitted(job_id))
            }
            Err(error) => {
                state.entries.remove(&owner);
                Err(GrantError::Admission(error))
            }
        }
    }

    fn prepare(
        &self,
        owner: [u8; 32],
        request: GrantRequest,
    ) -> Result<(ApplyCapability, bool), GrantError> {
        let receipt = self
            .store
            .history()
            .dry_run_receipt(request.receipt)
            .map_err(|_| GrantError::Unavailable)?
            .ok_or(GrantError::Invalid)?;
        validate_receipt_age(&receipt.binding, self.limits)?;
        if request.plan_sha256 != receipt.binding.plan_sha256
            || request.max_logical_effects != receipt.binding.max_logical_effects
        {
            return Err(GrantError::Invalid);
        }
        let stored = self
            .store
            .plans()
            .load(receipt.binding.plan_reference)
            .map_err(|_| GrantError::Invalid)?;
        if stored.plan.schema_version != 1 {
            return Err(GrantError::Invalid);
        }
        if !self
            .store
            .receipt_is_newer_than_checkpoint(
                receipt.binding.plan_reference,
                receipt.binding.completed_unix,
            )
            .map_err(|_| GrantError::Unavailable)?
        {
            return Err(GrantError::Invalid);
        }
        let server = self
            .config
            .server(&stored.binding.server_profile_id)
            .ok_or(GrantError::Invalid)?;
        let production = server.mode() == ServerMode::ProductionRead;
        let backup_reference_sha256 =
            validate_backup(&request.backup_reference, production, self.limits)?;
        let production_request = production.then_some(ProductionWriteRequest {
            plan_sha256: request.plan_sha256,
            expected_operations: request.max_logical_effects,
            backup_reference: request.backup_reference,
        });
        let prepared = prepare_upload_apply(stored.plan, production_request)
            .map_err(|_| GrantError::Invalid)?;
        let binding = GrantBinding {
            owner,
            receipt: request.receipt,
            dry_run: receipt.binding,
            source_profile_id: stored.binding.source_profile_id,
            server_profile_id: stored.binding.server_profile_id,
            backup_reference_sha256,
        };
        let capability = ApplyCapability { prepared, binding };
        self.is_current(&capability.binding)
            .then_some((capability, production))
            .ok_or(GrantError::Invalid)
    }

    fn is_current(&self, grant: &GrantBinding) -> bool {
        binding_is_current(&self.config, &self.store, grant)
    }
}

pub fn binding_is_current(config: &WebConfig, store: &ConsoleStore, grant: &GrantBinding) -> bool {
    let Ok(Some(receipt)) = store.history().dry_run_receipt(grant.receipt) else {
        return false;
    };
    let Ok(stored) = store.plans().load(grant.dry_run.plan_reference) else {
        return false;
    };
    let Some(source) = config.source(&grant.source_profile_id) else {
        return false;
    };
    let Some(server) = config.server(&grant.server_profile_id) else {
        return false;
    };
    let Ok(resolved) = source.resolve() else {
        return false;
    };
    let Ok(state) = config
        .history_state()
        .and_then(crate::StateProfile::resolve)
    else {
        return false;
    };
    receipt.binding == grant.dry_run
        && store.matches_state(&state)
        && stored.binding.plan_sha256 == grant.dry_run.plan_sha256
        && stored.binding.source_profile_id == grant.source_profile_id
        && stored.binding.source_profile_sha256 == resolved.generation_sha256()
        && source_configuration_digest(
            resolved.generation_sha256(),
            &stored.plan.configuration_sha256,
        ) == grant.dry_run.source_configuration_sha256
        && stored.binding.server_profile_id == grant.server_profile_id
        && stored.binding.server_identity_sha256 == grant.dry_run.server_identity_sha256
        && stored.binding.server_profile_sha256 == grant.dry_run.server_profile_sha256
        && stored.binding.credential_generation == grant.dry_run.credential_generation
        && server.generation_sha256() == grant.dry_run.server_profile_sha256
        && server.credential_generation() == grant.dry_run.credential_generation
        && stored.binding.max_logical_effects == grant.dry_run.max_logical_effects
}

fn validate_receipt_age(binding: &DryRunBinding, limits: WebLimits) -> Result<(), GrantError> {
    let now = now_unix().map_err(|_| GrantError::Unavailable)?;
    let maximum = i64::try_from(limits.dry_run_receipt_seconds).map_err(|_| GrantError::Invalid)?;
    if now < binding.completed_unix || now.saturating_sub(binding.completed_unix) > maximum {
        return Err(GrantError::Expired);
    }
    Ok(())
}

fn validate_backup(
    reference: &str,
    production: bool,
    limits: WebLimits,
) -> Result<Option<String>, GrantError> {
    if !production {
        return reference
            .is_empty()
            .then_some(None)
            .ok_or(GrantError::Invalid);
    }
    if reference.is_empty()
        || reference.len() > limits.backup_reference_bytes
        || reference.trim() != reference
        || reference.chars().any(char::is_control)
    {
        return Err(GrantError::Invalid);
    }
    Ok(Some(format!("{:x}", Sha256::digest(reference.as_bytes()))))
}

fn random_idempotency_key() -> Result<String, GrantError> {
    let mut bytes = [0_u8; IDEMPOTENCY_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| GrantError::Unavailable)?;
    let key = URL_SAFE_NO_PAD.encode(bytes);
    bytes.fill(0);
    Ok(key)
}

fn same_key(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
