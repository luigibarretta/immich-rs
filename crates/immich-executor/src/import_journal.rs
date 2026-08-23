use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use immich_rs_core::UploadPlan;
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::import_journal_db::{
    checkpoint_error, expected_binding, open_connection, read_binding, validate_plan,
};
use crate::{ExecutorError, ExecutorErrorClass};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportOutcome {
    Created,
    Duplicate,
    Completed,
    Reused,
    Failed,
    Indeterminate,
}

impl ImportOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Duplicate => "duplicate",
            Self::Completed => "completed",
            Self::Reused => "reused",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "created" => Some(Self::Created),
            "duplicate" => Some(Self::Duplicate),
            "completed" => Some(Self::Completed),
            "reused" => Some(Self::Reused),
            "failed" => Some(Self::Failed),
            "indeterminate" => Some(Self::Indeterminate),
            _ => None,
        }
    }
}

pub struct ImportEvent<'a> {
    pub effect_key: &'a str,
    pub outcome: ImportOutcome,
    pub remote_id: Option<&'a str>,
    pub retries: u32,
}

#[derive(Default)]
pub struct ImportState {
    latest: BTreeMap<String, StoredEffect>,
}

struct StoredEffect {
    outcome: ImportOutcome,
    remote_id: Option<String>,
}

impl ImportState {
    pub(crate) fn validate_for_plan(&self, plan: &UploadPlan) -> Result<(), ExecutorError> {
        let mut allowed = BTreeSet::new();
        for operation in &plan.operations {
            allowed.insert(asset_key(&operation.operation_id));
            if operation
                .normalized_metadata
                .as_ref()
                .is_some_and(has_metadata_assignment)
            {
                allowed.insert(metadata_key(&operation.operation_id));
            }
            if let Some(metadata) = &operation.normalized_metadata {
                for album in &metadata.albums {
                    let identity = album_identity(album);
                    allowed.insert(format!("album:{identity}"));
                    allowed.insert(format!("membership:{identity}"));
                }
            }
        }
        self.latest
            .keys()
            .all(|key| allowed.contains(key))
            .then_some(())
            .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Checkpoint))
    }

    pub(crate) fn asset_id(&self, operation_id: &str) -> Option<&str> {
        self.remote_id(
            &asset_key(operation_id),
            &[ImportOutcome::Created, ImportOutcome::Duplicate],
        )
    }

    pub(crate) fn metadata_done(&self, operation_id: &str) -> bool {
        self.completed(&metadata_key(operation_id))
    }

    pub(crate) fn album(&self, album_key: &str) -> Option<(ImportOutcome, &str)> {
        let effect = self.latest.get(&format!("album:{album_key}"))?;
        matches!(
            effect.outcome,
            ImportOutcome::Created | ImportOutcome::Reused
        )
        .then(|| effect.remote_id.as_deref().map(|id| (effect.outcome, id)))
        .flatten()
    }

    pub(crate) fn membership_done(&self, album_key: &str) -> bool {
        self.completed(&format!("membership:{album_key}"))
    }

    fn completed(&self, key: &str) -> bool {
        self.latest
            .get(key)
            .is_some_and(|effect| effect.outcome == ImportOutcome::Completed)
    }

    fn remote_id(&self, key: &str, outcomes: &[ImportOutcome]) -> Option<&str> {
        let effect = self.latest.get(key)?;
        outcomes
            .contains(&effect.outcome)
            .then_some(effect.remote_id.as_deref())
            .flatten()
    }
}

const fn has_metadata_assignment(metadata: &immich_rs_core::NormalizedMetadata) -> bool {
    metadata.description.is_some() || metadata.taken_at_utc.is_some() || metadata.location.is_some()
}

pub struct ImportJournal {
    connection: Connection,
}

impl ImportJournal {
    pub(crate) fn open(path: &Path, plan: &UploadPlan) -> Result<Self, ExecutorError> {
        Self::open_with_scope(path, plan, None)
    }

    pub(crate) fn open_production(
        path: &Path,
        plan: &UploadPlan,
        backup_reference_sha256: &str,
    ) -> Result<Self, ExecutorError> {
        if !is_lower_sha256(backup_reference_sha256) {
            return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
        }
        Self::open_with_scope(path, plan, Some(backup_reference_sha256))
    }

    fn open_with_scope(
        path: &Path,
        plan: &UploadPlan,
        backup_reference_sha256: Option<&str>,
    ) -> Result<Self, ExecutorError> {
        validate_plan(plan)?;
        let connection = open_connection(path)?;
        let mut journal = Self { connection };
        journal.bind_or_validate(plan, backup_reference_sha256)?;
        Ok(journal)
    }

    pub(crate) fn state(&self) -> Result<ImportState, ExecutorError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT event.effect_key, event.outcome, event.remote_id FROM effects AS event \
                 JOIN (SELECT effect_key, MAX(sequence) AS sequence FROM effects GROUP BY effect_key) latest \
                   ON latest.sequence = event.sequence ORDER BY event.effect_key",
            )
            .map_err(checkpoint_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(checkpoint_error)?;
        let mut latest = BTreeMap::new();
        for row in rows {
            let (key, outcome, remote_id) = row.map_err(checkpoint_error)?;
            let outcome = ImportOutcome::parse(&outcome)
                .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::Checkpoint))?;
            validate_effect(&key, outcome, remote_id.as_deref())?;
            if latest
                .insert(key, StoredEffect { outcome, remote_id })
                .is_some()
            {
                return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
            }
        }
        Ok(ImportState { latest })
    }

    pub(crate) fn append(&mut self, event: &ImportEvent<'_>) -> Result<(), ExecutorError> {
        validate_effect(event.effect_key, event.outcome, event.remote_id)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(checkpoint_error)?;
        transaction
            .execute(
                "INSERT INTO effects(effect_key,outcome,remote_id,retry_count,recorded_unix_ms) \
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    event.effect_key,
                    event.outcome.as_str(),
                    event.remote_id,
                    event.retries,
                    now_unix_ms()?
                ],
            )
            .map_err(checkpoint_error)?;
        transaction.commit().map_err(checkpoint_error)
    }

    fn bind_or_validate(
        &mut self,
        plan: &UploadPlan,
        backup_reference_sha256: Option<&str>,
    ) -> Result<(), ExecutorError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))
            .map_err(checkpoint_error)?;
        let expected = expected_binding(plan, backup_reference_sha256)?;
        if count == 0 {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(checkpoint_error)?;
            for (key, value) in expected {
                transaction
                    .execute(
                        "INSERT INTO metadata(key,value) VALUES (?1,?2)",
                        params![key, value],
                    )
                    .map_err(checkpoint_error)?;
            }
            transaction.commit().map_err(checkpoint_error)
        } else if read_binding(&self.connection)? == expected {
            Ok(())
        } else {
            Err(ExecutorError::new(ExecutorErrorClass::Checkpoint))
        }
    }
}

pub fn asset_key(operation_id: &str) -> String {
    format!("asset:{operation_id}")
}

pub fn metadata_key(operation_id: &str) -> String {
    format!("metadata:{operation_id}")
}

pub fn album_identity(name: &str) -> String {
    format!("{:x}", Sha256::digest(name.as_bytes()))
}

fn validate_effect(
    key: &str,
    outcome: ImportOutcome,
    remote_id: Option<&str>,
) -> Result<(), ExecutorError> {
    let known_prefix = ["asset:", "metadata:", "album:", "membership:"]
        .iter()
        .any(|prefix| key.strip_prefix(prefix).is_some_and(is_lower_sha256));
    let expects_id = matches!(
        outcome,
        ImportOutcome::Created | ImportOutcome::Duplicate | ImportOutcome::Reused
    );
    let id_valid = remote_id.is_some_and(|id| !id.is_empty() && id.len() <= 4_096);
    if known_prefix && expects_id == id_valid {
        Ok(())
    } else {
        Err(ExecutorError::new(ExecutorErrorClass::Checkpoint))
    }
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn now_unix_ms() -> Result<i64, ExecutorError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Checkpoint))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Checkpoint))
}
