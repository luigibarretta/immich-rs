use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use immich_rs_core::UploadPlan;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass};

const CHECKPOINT_SCHEMA: &str = "checkpoint-v1";
const APPLICATION_ID: i64 = 0x4952_5331;
const EXECUTION_SCOPE_KEY: &str = "execution_scope";
const BACKUP_REFERENCE_KEY: &str = "backup_reference_sha256";
const PRODUCTION_SCOPE: &str = "production";

pub struct Journal {
    connection: Connection,
}

pub struct CompletedAsset {
    pub asset_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutcomeKind {
    Created,
    Duplicate,
    Failed,
    Indeterminate,
}

impl OutcomeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Duplicate => "duplicate",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
        }
    }
}

pub struct JournalEvent<'a> {
    pub operation_id: &'a str,
    pub kind: OutcomeKind,
    pub asset_id: Option<&'a str>,
    pub retry_count: u32,
}

impl Journal {
    pub fn open(path: &Path, plan: &UploadPlan) -> Result<Self, ExecutorError> {
        Self::open_with_scope(path, plan, None)
    }

    pub fn open_production(
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
        reject_symlink(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags).map_err(checkpoint_error)?;
        configure(&connection)?;
        create_schema(&connection)?;
        let mut journal = Self { connection };
        journal.bind_or_validate(plan, backup_reference_sha256)?;
        Ok(journal)
    }

    pub fn validate_existing(
        path: &Path,
        plan: &UploadPlan,
    ) -> Result<BTreeMap<String, CompletedAsset>, ExecutorError> {
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        reject_symlink(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags).map_err(checkpoint_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(checkpoint_error)?;
        validate_application(&connection)?;
        validate_binding_any_scope(&connection, plan)?;
        completed_assets(&connection)
    }

    pub fn completed(&self) -> Result<BTreeMap<String, CompletedAsset>, ExecutorError> {
        completed_assets(&self.connection)
    }

    pub fn append(&mut self, event: &JournalEvent<'_>) -> Result<(), ExecutorError> {
        let asset_contract = match event.kind {
            OutcomeKind::Created | OutcomeKind::Duplicate => event
                .asset_id
                .is_some_and(|id| !id.is_empty() && id.len() <= 4_096),
            OutcomeKind::Failed | OutcomeKind::Indeterminate => event.asset_id.is_none(),
        };
        if event.operation_id.is_empty() || !asset_contract {
            return Err(ExecutorError::new(ExecutorErrorClass::Invariant));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(checkpoint_error)?;
        transaction
            .execute(
                "INSERT INTO events(operation_id, outcome, asset_id, retry_count, recorded_unix_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    event.operation_id,
                    event.kind.as_str(),
                    event.asset_id,
                    event.retry_count,
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
        if count == 0 {
            let expected = expected_binding(plan, backup_reference_sha256)?;
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(checkpoint_error)?;
            for (key, value) in expected {
                transaction
                    .execute(
                        "INSERT INTO metadata(key, value) VALUES (?1, ?2)",
                        params![key, value],
                    )
                    .map_err(checkpoint_error)?;
            }
            transaction.commit().map_err(checkpoint_error)
        } else {
            validate_binding_for_scope(&self.connection, plan, backup_reference_sha256)
        }
    }
}

fn configure(connection: &Connection) -> Result<(), ExecutorError> {
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(checkpoint_error)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;\
             PRAGMA synchronous=FULL;\
             PRAGMA foreign_keys=ON;\
             PRAGMA trusted_schema=OFF;\
             PRAGMA application_id=1230132017;\
             PRAGMA user_version=1;",
        )
        .map_err(checkpoint_error)
}

fn create_schema(connection: &Connection) -> Result<(), ExecutorError> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS metadata( \
               key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL \
             ) STRICT; \
             CREATE TABLE IF NOT EXISTS events( \
               sequence INTEGER PRIMARY KEY AUTOINCREMENT, \
               operation_id TEXT NOT NULL, \
               outcome TEXT NOT NULL CHECK(outcome IN ('created','duplicate','failed','indeterminate')), \
               asset_id TEXT, retry_count INTEGER NOT NULL CHECK(retry_count >= 0), \
               recorded_unix_ms INTEGER NOT NULL \
             ) STRICT; \
             CREATE INDEX IF NOT EXISTS events_operation_sequence \
               ON events(operation_id, sequence);",
        )
        .map_err(checkpoint_error)
}

fn validate_application(connection: &Connection) -> Result<(), ExecutorError> {
    let application_id: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(checkpoint_error)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(checkpoint_error)?;
    if application_id != APPLICATION_ID || version != 1 {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    Ok(())
}

fn validate_binding_for_scope(
    connection: &Connection,
    plan: &UploadPlan,
    backup_reference_sha256: Option<&str>,
) -> Result<(), ExecutorError> {
    let actual = read_binding(connection)?;
    if actual != expected_binding(plan, backup_reference_sha256)? {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    Ok(())
}

fn validate_binding_any_scope(
    connection: &Connection,
    plan: &UploadPlan,
) -> Result<(), ExecutorError> {
    let mut actual = read_binding(connection)?;
    let scope = actual.remove(EXECUTION_SCOPE_KEY);
    let backup = actual.remove(BACKUP_REFERENCE_KEY);
    let valid_scope = match (scope.as_deref(), backup.as_deref()) {
        (None, None) => true,
        (Some(PRODUCTION_SCOPE), Some(digest)) => is_lower_sha256(digest),
        _ => false,
    };
    if !valid_scope || actual != expected_binding(plan, None)? {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    Ok(())
}

fn read_binding(connection: &Connection) -> Result<BTreeMap<String, String>, ExecutorError> {
    validate_application(connection)?;
    let mut statement = connection
        .prepare("SELECT key, value FROM metadata ORDER BY key")
        .map_err(checkpoint_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(checkpoint_error)?;
    let mut actual = BTreeMap::new();
    for row in rows {
        let (key, value) = row.map_err(checkpoint_error)?;
        if actual.insert(key, value).is_some() {
            return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
        }
    }
    Ok(actual)
}

fn expected_binding(
    plan: &UploadPlan,
    backup_reference_sha256: Option<&str>,
) -> Result<BTreeMap<String, String>, ExecutorError> {
    let plan_bytes =
        serde_json::to_vec(plan).map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let entries = [
        ("schema", CHECKPOINT_SCHEMA.to_owned()),
        (
            "upload_plan_sha256",
            format!("{:x}", Sha256::digest(plan_bytes)),
        ),
        (
            "normalized_plan_sha256",
            plan.normalized_plan_sha256.clone(),
        ),
        ("source_sha256", plan.source.fingerprint_sha256.clone()),
        ("configuration_sha256", plan.configuration_sha256.clone()),
        (
            "server_identity_sha256",
            plan.server.identity_sha256.clone(),
        ),
        (
            "server_version",
            format!(
                "{}.{}.{}",
                plan.server.version.major, plan.server.version.minor, plan.server.version.patch
            ),
        ),
    ];
    let mut binding = entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect::<BTreeMap<_, _>>();
    if let Some(digest) = backup_reference_sha256 {
        binding.insert(EXECUTION_SCOPE_KEY.to_owned(), PRODUCTION_SCOPE.to_owned());
        binding.insert(BACKUP_REFERENCE_KEY.to_owned(), digest.to_owned());
    }
    Ok(binding)
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn completed_assets(
    connection: &Connection,
) -> Result<BTreeMap<String, CompletedAsset>, ExecutorError> {
    let mut statement = connection
        .prepare(
            "SELECT event.operation_id, event.asset_id \
             FROM events AS event \
             JOIN (SELECT operation_id, MAX(sequence) AS sequence FROM events GROUP BY operation_id) latest \
               ON latest.sequence = event.sequence \
             WHERE event.outcome IN ('created','duplicate') \
             ORDER BY event.operation_id",
        )
        .map_err(checkpoint_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(checkpoint_error)?;
    let mut completed = BTreeMap::new();
    for row in rows {
        let (operation_id, asset_id) = row.map_err(checkpoint_error)?;
        if operation_id.is_empty()
            || asset_id.is_empty()
            || asset_id.len() > 4_096
            || completed
                .insert(operation_id, CompletedAsset { asset_id })
                .is_some()
        {
            return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
        }
    }
    Ok(completed)
}

fn reject_symlink(path: &Path) -> Result<(), ExecutorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(ExecutorError::new(ExecutorErrorClass::Checkpoint))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ExecutorError::new(ExecutorErrorClass::Checkpoint)),
    }
}

fn now_unix_ms() -> Result<i64, ExecutorError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Checkpoint))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::Checkpoint))
}

fn checkpoint_error(_error: rusqlite::Error) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::Checkpoint)
}
