use std::fmt::Write as _;

use rusqlite::{TransactionBehavior, params};

use super::{HistoryStore, checkpoint, db_error, lock_error};
use crate::WebConfigError;
use crate::state_store::{
    ArtifactRef, DryRunBinding, DryRunReceipt, HistoryKind, ReceiptRef, TerminalRecord,
    TerminalStatus,
};

const RECEIPT_CREATE_ATTEMPTS: usize = 8;

impl HistoryStore {
    pub fn record_dry_run(
        &self,
        binding: DryRunBinding,
        terminal: &TerminalRecord,
    ) -> Result<DryRunReceipt, WebConfigError> {
        validate_dry_run(&binding)?;
        validate_terminal(&binding, terminal)?;
        self.check_size()?;
        for _attempt in 0..RECEIPT_CREATE_ATTEMPTS {
            let reference = ReceiptRef::random()?;
            let mut connection = self.connection.lock().map_err(lock_error)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            let inserted = transaction
                .execute(
                    "INSERT OR IGNORE INTO dry_run_receipts \
                     (receipt_ref, plan_ref, plan_sha256, source_configuration_sha256, \
                      server_identity_sha256, server_profile_sha256, credential_generation, \
                      max_logical_effects, completed_unix) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        reference.0.to_vec(),
                        binding.plan_reference.0.to_vec(),
                        decode_sha256(&binding.plan_sha256)?,
                        decode_sha256(&binding.source_configuration_sha256)?,
                        decode_sha256(&binding.server_identity_sha256)?,
                        decode_sha256(&binding.server_profile_sha256)?,
                        checked_u64(binding.credential_generation)?,
                        checked_u64(binding.max_logical_effects)?,
                        binding.completed_unix,
                    ],
                )
                .map_err(db_error)?;
            if inserted == 1 {
                insert_terminal(&transaction, terminal)?;
            }
            transaction.commit().map_err(db_error)?;
            checkpoint(&connection)?;
            drop(connection);
            if inserted == 1 {
                self.maintain(binding.completed_unix)?;
                return Ok(DryRunReceipt { reference, binding });
            }
        }
        Err(WebConfigError::new("receipt reference capacity exhausted"))
    }

    pub fn dry_run_receipt(
        &self,
        reference: ReceiptRef,
    ) -> Result<Option<DryRunReceipt>, WebConfigError> {
        let connection = self.connection.lock().map_err(lock_error)?;
        let mut statement = connection
            .prepare(
                "SELECT plan_ref, plan_sha256, source_configuration_sha256, \
                 server_identity_sha256, server_profile_sha256, credential_generation, \
                 max_logical_effects, completed_unix FROM dry_run_receipts \
                 WHERE receipt_ref = ?1",
            )
            .map_err(db_error)?;
        let result = statement.query_row([reference.0.to_vec()], |row| {
            Ok(DryRunReceipt {
                reference,
                binding: DryRunBinding {
                    plan_reference: ArtifactRef::from_vec(row.get(0)?)?,
                    plan_sha256: encode_sha256(&row.get::<_, Vec<u8>>(1)?)?,
                    source_configuration_sha256: encode_sha256(&row.get::<_, Vec<u8>>(2)?)?,
                    server_identity_sha256: encode_sha256(&row.get::<_, Vec<u8>>(3)?)?,
                    server_profile_sha256: encode_sha256(&row.get::<_, Vec<u8>>(4)?)?,
                    credential_generation: read_u64_value(row, 5)?,
                    max_logical_effects: read_u64_value(row, 6)?,
                    completed_unix: row.get(7)?,
                },
            })
        });
        let response = match result {
            Ok(receipt) => Ok(Some(receipt)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(db_error(error)),
        };
        drop(statement);
        drop(connection);
        response
    }
}

fn insert_terminal(
    transaction: &rusqlite::Transaction<'_>,
    terminal: &TerminalRecord,
) -> Result<(), WebConfigError> {
    let (plan_ref, plan_schema) = terminal.plan.map_or((None, None), |(reference, schema)| {
        (Some(reference.0.to_vec()), Some(i64::from(schema)))
    });
    transaction
        .execute(
            "INSERT INTO history \
             (recorded_unix, kind, status, plan_ref, plan_schema, assets, sidecars, bytes_read, \
              warnings, errors, max_logical_effects) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                terminal.recorded_unix,
                terminal.kind as i64,
                terminal.status as i64,
                plan_ref,
                plan_schema,
                checked_u64(terminal.counters.assets)?,
                checked_u64(terminal.counters.sidecars)?,
                checked_u64(terminal.counters.bytes_read)?,
                checked_u64(terminal.counters.warnings)?,
                checked_u64(terminal.counters.errors)?,
                checked_u64(terminal.counters.max_logical_effects)?,
            ],
        )
        .map(|_| ())
        .map_err(db_error)
}

fn validate_terminal(
    binding: &DryRunBinding,
    terminal: &TerminalRecord,
) -> Result<(), WebConfigError> {
    let matching_plan = terminal
        .plan
        .is_some_and(|(reference, schema)| reference == binding.plan_reference && schema > 0);
    if terminal.recorded_unix != binding.completed_unix
        || terminal.status != TerminalStatus::Completed
        || !matches!(
            terminal.kind,
            HistoryKind::FolderDryRun
                | HistoryKind::GoogleTakeoutDryRun
                | HistoryKind::ApplePhotosDryRun
                | HistoryKind::PicasaDryRun
        )
        || terminal.counters.max_logical_effects != binding.max_logical_effects
        || !matching_plan
    {
        return Err(WebConfigError::new("dry-run terminal record is invalid"));
    }
    Ok(())
}

fn validate_dry_run(binding: &DryRunBinding) -> Result<(), WebConfigError> {
    if binding.completed_unix < 0
        || binding.credential_generation == 0
        || !is_sha256(&binding.plan_sha256)
        || !is_sha256(&binding.source_configuration_sha256)
        || !is_sha256(&binding.server_identity_sha256)
        || !is_sha256(&binding.server_profile_sha256)
    {
        return Err(WebConfigError::new("dry-run receipt is invalid"));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_sha256(value: &str) -> Result<Vec<u8>, WebConfigError> {
    if !is_sha256(value) {
        return Err(WebConfigError::new("receipt digest is invalid"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)
                .map_err(|_| WebConfigError::new("receipt digest is invalid"))?;
            u8::from_str_radix(text, 16)
                .map_err(|_| WebConfigError::new("receipt digest is invalid"))
        })
        .collect()
}

fn encode_sha256(bytes: &[u8]) -> rusqlite::Result<String> {
    if bytes.len() != 32 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut output = String::with_capacity(64);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").map_err(|_| rusqlite::Error::InvalidQuery)?;
    }
    Ok(output)
}

fn checked_u64(value: u64) -> Result<i64, WebConfigError> {
    i64::try_from(value).map_err(|_| WebConfigError::new("receipt counter is invalid"))
}

fn read_u64_value(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
