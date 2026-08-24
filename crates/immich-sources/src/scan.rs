use std::error::Error;
use std::fmt::{self, Display, Formatter};

use immich_rs_core::{PlanValidationError, ProgressEvent};

const MIN_BUFFER_BYTES: usize = 4 * 1_024;
const MAX_BUFFER_BYTES: usize = 4 * 1_024 * 1_024;

/// Explicit memory and portability limits for one folder scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderScanConfig {
    /// Read buffer used for one media file at a time.
    pub buffer_bytes: usize,
    /// Maximum total entries retained by discovery and reconciliation.
    pub max_entries: usize,
    /// Maximum entries accepted in one directory before failing closed.
    pub max_directory_entries: usize,
    /// Maximum UTF-8 bytes in one portable relative path.
    pub max_path_bytes: usize,
    /// Case policy used for the portable source description.
    pub case_sensitive: bool,
}

impl Default for FolderScanConfig {
    fn default() -> Self {
        Self {
            buffer_bytes: 64 * 1_024,
            max_entries: 100_000,
            max_directory_entries: 10_000,
            max_path_bytes: 4_096,
            case_sensitive: true,
        }
    }
}

impl FolderScanConfig {
    pub(crate) fn validate(&self) -> Result<(), ScanError> {
        if !(MIN_BUFFER_BYTES..=MAX_BUFFER_BYTES).contains(&self.buffer_bytes) {
            return Err(ScanError::InvalidConfiguration(
                "buffer_bytes must be in 4096..=4194304",
            ));
        }
        if self.max_entries == 0 || self.max_directory_entries == 0 {
            return Err(ScanError::InvalidConfiguration(
                "entry limits must be greater than zero",
            ));
        }
        if !(64..=65_536).contains(&self.max_path_bytes) {
            return Err(ScanError::InvalidConfiguration(
                "max_path_bytes must be in 64..=65536",
            ));
        }
        Ok(())
    }
}

/// Explicit limits for a directory or split-ZIP Google Takeout scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TakeoutScanConfig {
    /// Common streaming and portability limits.
    pub scan: FolderScanConfig,
    /// Maximum independent ZIP parts accepted in one virtual input.
    pub max_archives: usize,
    /// Maximum declared uncompressed bytes for one archive entry.
    pub max_archive_entry_bytes: u64,
    /// Maximum expansion ratio after the absolute grace threshold.
    pub max_compression_ratio: u64,
    /// Entries at or below this size do not use the ratio check.
    pub compression_ratio_grace_bytes: u64,
}

impl Default for TakeoutScanConfig {
    fn default() -> Self {
        Self {
            scan: FolderScanConfig::default(),
            max_archives: 64,
            max_archive_entry_bytes: 1_099_511_627_776,
            max_compression_ratio: 200,
            compression_ratio_grace_bytes: 1_048_576,
        }
    }
}

impl TakeoutScanConfig {
    /// Validate all directory and archive limits without touching source input.
    pub fn validate(&self) -> Result<(), ScanError> {
        self.scan.validate()?;
        if !(1..=64).contains(&self.max_archives) {
            return Err(ScanError::InvalidConfiguration(
                "max_archives must be in 1..=64",
            ));
        }
        if self.max_archive_entry_bytes == 0 || !(1..=10_000).contains(&self.max_compression_ratio)
        {
            return Err(ScanError::InvalidConfiguration(
                "archive byte and compression limits must be positive and bounded",
            ));
        }
        Ok(())
    }
}

/// Low-cardinality observer called synchronously by the scanner.
pub trait ProgressObserver {
    /// Receive one monotonic read-only progress event.
    fn observe(&mut self, event: ProgressEvent);
}

/// Observer used when callers do not request progress events.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProgress;

impl ProgressObserver for NoProgress {
    fn observe(&mut self, _event: ProgressEvent) {}
}

/// Fail-closed source scan failure.
#[derive(Debug)]
pub enum ScanError {
    /// A configured resource limit is invalid.
    InvalidConfiguration(&'static str),
    /// The source root is not a readable real directory.
    InvalidRoot,
    /// The source does not match the requested adapter layout.
    UnsupportedLayout(&'static str),
    /// An archive is malformed, unsafe or unsupported.
    InvalidArchive(&'static str),
    /// A deterministic memory or path limit was reached.
    LimitExceeded(&'static str),
    /// Cooperative cancellation stopped discovery or hashing.
    Cancelled,
    /// The produced plan violated a core invariant.
    InvalidPlan(PlanValidationError),
}

impl Display for ScanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid scan configuration: {message}")
            }
            Self::InvalidRoot => {
                formatter.write_str("source root must be a readable real directory")
            }
            Self::UnsupportedLayout(message) => {
                write!(formatter, "unsupported source layout: {message}")
            }
            Self::InvalidArchive(message) => write!(formatter, "invalid archive: {message}"),
            Self::LimitExceeded(limit) => write!(formatter, "scan limit exceeded: {limit}"),
            Self::Cancelled => formatter.write_str("scan cancelled cleanly"),
            Self::InvalidPlan(error) => {
                write!(formatter, "scanner produced an invalid plan: {error}")
            }
        }
    }
}

impl Error for ScanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPlan(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PlanValidationError> for ScanError {
    fn from(error: PlanValidationError) -> Self {
        Self::InvalidPlan(error)
    }
}
