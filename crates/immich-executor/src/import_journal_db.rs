use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use immich_rs_core::{UPLOAD_PLAN_SCHEMA_VERSION_V2, UploadPlan};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

use crate::{ExecutorError, ExecutorErrorClass};

const CHECKPOINT_SCHEMA: &str = "checkpoint-v2";
const APPLICATION_ID: i64 = 0x4952_5332;

pub fn open_connection(path: &Path) -> Result<Connection, ExecutorError> {
    reject_symlink(path)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags).map_err(checkpoint_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(checkpoint_error)?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; \
             PRAGMA trusted_schema=OFF; PRAGMA application_id=1230132018; PRAGMA user_version=2; \
             CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL) STRICT; \
             CREATE TABLE IF NOT EXISTS effects( \
               sequence INTEGER PRIMARY KEY AUTOINCREMENT,effect_key TEXT NOT NULL, \
               outcome TEXT NOT NULL CHECK(outcome IN ('created','duplicate','completed','reused','failed','indeterminate')), \
               remote_id TEXT,retry_count INTEGER NOT NULL CHECK(retry_count >= 0),recorded_unix_ms INTEGER NOT NULL \
             ) STRICT; CREATE INDEX IF NOT EXISTS effects_key_sequence ON effects(effect_key,sequence);",
        )
        .map_err(checkpoint_error)?;
    Ok(connection)
}

pub fn expected_binding(
    plan: &UploadPlan,
    backup_reference_sha256: Option<&str>,
) -> Result<BTreeMap<String, String>, ExecutorError> {
    let bytes =
        serde_json::to_vec(plan).map_err(|_| ExecutorError::new(ExecutorErrorClass::Invariant))?;
    let mut binding = [
        ("schema".to_owned(), CHECKPOINT_SCHEMA.to_owned()),
        (
            "upload_plan_sha256".to_owned(),
            format!("{:x}", Sha256::digest(bytes)),
        ),
        (
            "normalized_plan_sha256".to_owned(),
            plan.normalized_plan_sha256.clone(),
        ),
        (
            "source_sha256".to_owned(),
            plan.source.fingerprint_sha256.clone(),
        ),
        (
            "configuration_sha256".to_owned(),
            plan.configuration_sha256.clone(),
        ),
        (
            "server_identity_sha256".to_owned(),
            plan.server.identity_sha256.clone(),
        ),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();
    if let Some(digest) = backup_reference_sha256 {
        binding.insert("execution_scope".to_owned(), "production".to_owned());
        binding.insert("backup_reference_sha256".to_owned(), digest.to_owned());
    }
    Ok(binding)
}

pub fn read_binding(connection: &Connection) -> Result<BTreeMap<String, String>, ExecutorError> {
    validate_application(connection)?;
    let mut statement = connection
        .prepare("SELECT key,value FROM metadata ORDER BY key")
        .map_err(checkpoint_error)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(checkpoint_error)?;
    let mut result = BTreeMap::new();
    for row in rows {
        let (key, value) = row.map_err(checkpoint_error)?;
        if result.insert(key, value).is_some() {
            return Err(ExecutorError::new(ExecutorErrorClass::Checkpoint));
        }
    }
    Ok(result)
}

pub fn validate_plan(plan: &UploadPlan) -> Result<(), ExecutorError> {
    plan.validate()
        .map_err(|_| ExecutorError::new(ExecutorErrorClass::InvalidPlan))?;
    (plan.schema_version == UPLOAD_PLAN_SCHEMA_VERSION_V2)
        .then_some(())
        .ok_or_else(|| ExecutorError::new(ExecutorErrorClass::InvalidPlan))
}

fn validate_application(connection: &Connection) -> Result<(), ExecutorError> {
    let application: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(checkpoint_error)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(checkpoint_error)?;
    if application == APPLICATION_ID && version == 2 {
        Ok(())
    } else {
        Err(ExecutorError::new(ExecutorErrorClass::Checkpoint))
    }
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

pub fn checkpoint_error(_error: rusqlite::Error) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::Checkpoint)
}
