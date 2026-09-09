use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};

const MAX_BACKUP_REFERENCE_BYTES: usize = 256;

/// Operator-originated confirmation required before remote production writes.
pub struct ProductionWriteConfirmation {
    plan_sha256: String,
    expected_operations: u64,
    backup_reference: String,
}

impl ProductionWriteConfirmation {
    /// Validate the complete operator confirmation without exposing its backup reference.
    pub fn new(
        acknowledged: bool,
        plan_sha256: String,
        expected_operations: u64,
        backup_reference: String,
    ) -> Result<Self, ProductionConfirmationError> {
        if !acknowledged {
            return Err(ProductionConfirmationError::NotAcknowledged);
        }
        if !is_lower_sha256(&plan_sha256) {
            return Err(ProductionConfirmationError::InvalidPlanDigest);
        }
        if expected_operations == 0 {
            return Err(ProductionConfirmationError::InvalidOperationCount);
        }
        if backup_reference.is_empty()
            || backup_reference.len() > MAX_BACKUP_REFERENCE_BYTES
            || backup_reference.trim() != backup_reference
            || backup_reference.chars().any(char::is_control)
        {
            return Err(ProductionConfirmationError::InvalidBackupReference);
        }
        Ok(Self {
            plan_sha256,
            expected_operations,
            backup_reference,
        })
    }

    /// Return the exact canonical plan digest supplied by the operator.
    #[must_use]
    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }

    /// Return the operator's exact mutation budget.
    #[must_use]
    pub const fn expected_operations(&self) -> u64 {
        self.expected_operations
    }

    /// Return the validated reference only to the authorization boundary for hashing.
    #[must_use]
    pub fn backup_reference(&self) -> &str {
        &self.backup_reference
    }
}

impl Debug for ProductionWriteConfirmation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProductionWriteConfirmation")
            .field("plan_sha256", &"[REDACTED]")
            .field("expected_operations", &self.expected_operations)
            .field("backup_reference", &"[REDACTED]")
            .finish()
    }
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Stable reason an operator production confirmation was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductionConfirmationError {
    /// The command-line write acknowledgement was absent.
    NotAcknowledged,
    /// The plan digest was not lowercase SHA-256.
    InvalidPlanDigest,
    /// The mutation budget was empty.
    InvalidOperationCount,
    /// The backup reference was empty, unbounded or unsafe to retain in memory.
    InvalidBackupReference,
}

impl Display for ProductionConfirmationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::NotAcknowledged => "production write was not acknowledged",
            Self::InvalidPlanDigest => "production plan digest is invalid",
            Self::InvalidOperationCount => "production operation count is invalid",
            Self::InvalidBackupReference => "production backup reference is invalid",
        };
        formatter.write_str(message)
    }
}

impl Error for ProductionConfirmationError {}

#[cfg(test)]
mod tests {
    use super::ProductionWriteConfirmation;

    #[test]
    fn confirmation_is_complete_bounded_and_redacted() -> Result<(), Box<dyn std::error::Error>> {
        let reference = "synthetic-backup-reference";
        let confirmation =
            ProductionWriteConfirmation::new(true, "a".repeat(64), 2, reference.to_owned())?;
        assert_eq!(confirmation.expected_operations(), 2);
        assert_eq!(confirmation.plan_sha256(), "a".repeat(64));
        assert_eq!(confirmation.backup_reference(), reference);
        assert!(!format!("{confirmation:?}").contains(reference));

        assert!(
            ProductionWriteConfirmation::new(false, "a".repeat(64), 2, reference.to_owned())
                .is_err()
        );
        assert!(
            ProductionWriteConfirmation::new(true, "A".repeat(64), 2, reference.to_owned())
                .is_err()
        );
        assert!(
            ProductionWriteConfirmation::new(true, "a".repeat(64), 0, reference.to_owned())
                .is_err()
        );
        assert!(
            ProductionWriteConfirmation::new(true, "a".repeat(64), 2, " spaced ".to_owned())
                .is_err()
        );
        Ok(())
    }
}
