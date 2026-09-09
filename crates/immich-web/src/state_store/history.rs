use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

mod receipts;

use super::TerminalRecord;
use super::types::StoredHistory;
use crate::WebConfigError;

const SCHEMA_VERSION: i64 = 2;
const SQLITE_PAGE_BYTES: u64 = 4_096;

pub struct HistoryStore {
    connection: Mutex<Connection>,
    path: PathBuf,
    maximum_bytes: u64,
    retained_rows: usize,
    retention_seconds: i64,
}

impl HistoryStore {
    pub fn open(
        path: &Path,
        maximum_bytes: u64,
        retained_rows: usize,
        retention_days: u64,
        now: i64,
    ) -> Result<Self, WebConfigError> {
        prepare_database_file(path, maximum_bytes)?;
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(db_error)?;
        configure(&mut connection, maximum_bytes)?;
        let retention_seconds = i64::try_from(retention_days.saturating_mul(86_400))
            .map_err(|_| WebConfigError::new("history retention is invalid"))?;
        let store = Self {
            connection: Mutex::new(connection),
            path: path.to_owned(),
            maximum_bytes,
            retained_rows,
            retention_seconds,
        };
        store.maintain(now)?;
        Ok(store)
    }

    pub fn record_terminal(&self, record: &TerminalRecord) -> Result<u64, WebConfigError> {
        if record.recorded_unix < 0 || record.plan.is_some_and(|(_, schema)| schema == 0) {
            return Err(WebConfigError::new("terminal history record is invalid"));
        }
        self.check_size()?;
        let mut connection = self.connection.lock().map_err(lock_error)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let (plan_ref, plan_schema) = record.plan.map_or((None, None), |(reference, schema)| {
            (Some(reference.0.to_vec()), Some(i64::from(schema)))
        });
        transaction
            .execute(
                "INSERT INTO history \
                 (recorded_unix, kind, status, plan_ref, plan_schema, assets, sidecars, \
                  bytes_read, warnings, errors, max_logical_effects) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    record.recorded_unix,
                    record.kind as i64,
                    record.status as i64,
                    plan_ref,
                    plan_schema,
                    checked_u64(record.counters.assets)?,
                    checked_u64(record.counters.sidecars)?,
                    checked_u64(record.counters.bytes_read)?,
                    checked_u64(record.counters.warnings)?,
                    checked_u64(record.counters.errors)?,
                    checked_u64(record.counters.max_logical_effects)?,
                ],
            )
            .map_err(db_error)?;
        let sequence = u64::try_from(transaction.last_insert_rowid())
            .map_err(|_| WebConfigError::new("history sequence is invalid"))?;
        transaction.commit().map_err(db_error)?;
        checkpoint(&connection)?;
        drop(connection);
        self.maintain(record.recorded_unix)?;
        Ok(sequence)
    }

    pub fn latest(&self, maximum: usize) -> Result<Vec<StoredHistory>, WebConfigError> {
        if maximum == 0 || maximum > self.retained_rows {
            return Err(WebConfigError::new("history page bound is invalid"));
        }
        let connection = self.connection.lock().map_err(lock_error)?;
        let mut statement = connection
            .prepare(
                "SELECT sequence, recorded_unix, kind, status, plan_ref, plan_schema, assets, \
                 sidecars, bytes_read, warnings, errors, max_logical_effects \
                 FROM history ORDER BY sequence DESC LIMIT ?1",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([to_i64(maximum)?], StoredHistory::from_row)
            .map_err(db_error)?;
        let records = rows.collect::<Result<Vec<_>, _>>();
        drop(statement);
        drop(connection);
        records.map_err(db_error)
    }

    pub fn maintain(&self, now: i64) -> Result<(), WebConfigError> {
        self.check_size()?;
        let mut connection = self.connection.lock().map_err(lock_error)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let cutoff = now.saturating_sub(self.retention_seconds);
        transaction
            .execute("DELETE FROM history WHERE recorded_unix < ?1", [cutoff])
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM history WHERE sequence NOT IN \
                 (SELECT sequence FROM history ORDER BY sequence DESC LIMIT ?1)",
                [to_i64(self.retained_rows)?],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM dry_run_receipts WHERE completed_unix < ?1",
                [cutoff],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM dry_run_receipts WHERE rowid NOT IN \
                 (SELECT rowid FROM dry_run_receipts \
                  ORDER BY completed_unix DESC, rowid DESC LIMIT ?1)",
                [to_i64(self.retained_rows)?],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        checkpoint(&connection)?;
        drop(connection);
        protect_database_files(&self.path)?;
        self.check_size()
    }

    fn check_size(&self) -> Result<(), WebConfigError> {
        let wal = PathBuf::from(format!("{}-wal", self.path.display()));
        let total = file_size(&self.path)?.saturating_add(file_size(&wal)?);
        if total > self.maximum_bytes {
            return Err(WebConfigError::new("history storage bound exceeded"));
        }
        Ok(())
    }
}

fn checked_u64(value: u64) -> Result<i64, WebConfigError> {
    i64::try_from(value).map_err(|_| WebConfigError::new("history counter is invalid"))
}

fn configure(connection: &mut Connection, maximum_bytes: u64) -> Result<(), WebConfigError> {
    connection
        .pragma_update(None, "trusted_schema", "OFF")
        .map_err(db_error)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(db_error)?;
    connection
        .pragma_update(None, "temp_store", "MEMORY")
        .map_err(db_error)?;
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    if version > SCHEMA_VERSION {
        return Err(WebConfigError::new(
            "history schema is newer than supported",
        ));
    }
    if version == 0 {
        migrate_v1(connection)?;
    }
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    if version == 1 {
        migrate_v2(connection)?;
    }
    validate_schema(connection)?;
    let result: String = connection
        .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
        .map_err(db_error)?;
    if !result.eq_ignore_ascii_case("wal") {
        return Err(WebConfigError::new("history WAL mode is unavailable"));
    }
    connection
        .pragma_update(None, "synchronous", "FULL")
        .map_err(db_error)?;
    let database_budget = maximum_bytes.saturating_mul(3) / 4;
    let maximum_pages = database_budget / SQLITE_PAGE_BYTES;
    let configured_pages: u64 = connection
        .pragma_update_and_check(None, "max_page_count", maximum_pages, |row| row.get(0))
        .map_err(db_error)?;
    if configured_pages > maximum_pages {
        return Err(WebConfigError::new(
            "history database exceeds its page bound",
        ));
    }
    connection
        .pragma_update(None, "journal_size_limit", maximum_bytes / 4)
        .map_err(db_error)?;
    connection
        .pragma_update(None, "wal_autocheckpoint", 64_u64)
        .map_err(db_error)?;
    let integrity: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(db_error)?;
    if integrity != "ok" {
        return Err(WebConfigError::new("history database is corrupt"));
    }
    Ok(())
}

fn migrate_v1(connection: &mut Connection) -> Result<(), WebConfigError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    transaction
        .execute_batch(include_str!("history-v1.sql"))
        .map_err(db_error)?;
    transaction
        .pragma_update(None, "user_version", 1_i64)
        .map_err(db_error)?;
    transaction.commit().map_err(db_error)
}

fn migrate_v2(connection: &mut Connection) -> Result<(), WebConfigError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db_error)?;
    transaction
        .execute_batch(include_str!("history-v2.sql"))
        .map_err(db_error)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(db_error)?;
    transaction.commit().map_err(db_error)
}

fn validate_schema(connection: &Connection) -> Result<(), WebConfigError> {
    connection
        .prepare("SELECT sequence, recorded_unix FROM history LIMIT 0")
        .map_err(db_error)?;
    connection
        .prepare("SELECT receipt_ref, completed_unix FROM dry_run_receipts LIMIT 0")
        .map_err(db_error)?;
    Ok(())
}

fn checkpoint(connection: &Connection) -> Result<(), WebConfigError> {
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|_| ())
        .map_err(db_error)
}

fn prepare_database_file(path: &Path, maximum_bytes: u64) -> Result<(), WebConfigError> {
    if maximum_bytes < SQLITE_PAGE_BYTES.saturating_mul(4) {
        return Err(WebConfigError::new("history storage bound is invalid"));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= maximum_bytes =>
        {
            protect_file(path)?;
        }
        Ok(_) => return Err(WebConfigError::new("history database file is invalid")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_private_file(path)?,
        Err(_) => return Err(WebConfigError::new("cannot inspect history database")),
    }
    Ok(())
}

fn file_size(path: &Path) -> Result<u64, WebConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(metadata.len())
        }
        Ok(_) => Err(WebConfigError::new("history database file is invalid")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(WebConfigError::new("cannot inspect history database")),
    }
}

fn protect_database_files(path: &Path) -> Result<(), WebConfigError> {
    protect_file(path)?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{}", path.display(), suffix));
        if sidecar.exists() {
            protect_file(&sidecar)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<(), WebConfigError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map(|_| ())
        .map_err(|_| WebConfigError::new("cannot create history database"))
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<(), WebConfigError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|_| WebConfigError::new("cannot create history database"))
}

#[cfg(unix)]
fn protect_file(path: &Path) -> Result<(), WebConfigError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| WebConfigError::new("cannot protect history database"))
}

#[cfg(windows)]
fn protect_file(_path: &Path) -> Result<(), WebConfigError> {
    Ok(())
}

fn to_i64(value: usize) -> Result<i64, WebConfigError> {
    i64::try_from(value).map_err(|_| WebConfigError::new("history limit is invalid"))
}

fn db_error(_error: rusqlite::Error) -> WebConfigError {
    WebConfigError::new("history database operation failed")
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> WebConfigError {
    WebConfigError::new("history database lock failed")
}
