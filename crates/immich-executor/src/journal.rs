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
        reject_symlink(path)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags).map_err(checkpoint_error)?;
        configure(&connection)?;
        create_schema(&connection)?;
        let mut journal = Self { connection };
        journal.bind_or_validate(plan)?;
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
        validate_binding(&connection, plan)?;
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

    fn bind_or_validate(&mut self, plan: &UploadPlan) -> Result<(), ExecutorError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM metadata", [], |row| row.get(0))
            .map_err(checkpoint_error)?;
        if count == 0 {
            let expected = expected_binding(plan)?;
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
            validate_binding(&self.connection, plan)
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

fn validate_binding(connection: &Connection, plan: &UploadPlan) -> Result<(), ExecutorError> {
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
    if actual != expected_binding(plan)? {
        return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
    }
    Ok(())
}

fn expected_binding(plan: &UploadPlan) -> Result<BTreeMap<String, String>, ExecutorError> {
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
    Ok(entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect())
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
