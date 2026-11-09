//! Bounded reads of operator-configured files (ADR-0035).
//!
//! Owner-private files (secrets, credentials, private keys) are read on Unix
//! with `auth_modules::secrets::files`: no symlink, a regular non-empty file
//! within the size bound, no group or other permission bit, and the same
//! device/inode before the open, after the open and after the read. On Linux
//! the file must also be owned by the effective UID. Other Unix targets cannot
//! resolve that UID without `unsafe`, so they skip only the owner check, as
//! the Web Console did before adopting the library.
//!
//! Public files (CA bundles, certificates), and private files on targets
//! without Unix permissions, use [`read_regular`]: the same type, size and
//! identity checks without the permission check.

use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;

use crate::WebConfigError;
use crate::profiles::ResourceIdentity;

/// Reads an owner-private file of at most `maximum` bytes; every rejection is
/// reported as `error`, which never names the path.
#[cfg(unix)]
pub fn read_private(
    path: &Path,
    maximum: u64,
    error: WebConfigError,
) -> Result<Vec<u8>, WebConfigError> {
    use auth_modules::secrets::Redacted;
    use auth_modules::secrets::files::{PrivateFileOptions, read_private_file_with};

    let options = PrivateFileOptions::new(maximum).owner(owner_policy());
    read_private_file_with(path, &options)
        .map(Redacted::into_inner)
        .map_err(|_| error)
}

/// Without Unix permission bits a private file gets the bounded regular-file
/// checks only.
#[cfg(not(unix))]
pub fn read_private(
    path: &Path,
    maximum: u64,
    error: WebConfigError,
) -> Result<Vec<u8>, WebConfigError> {
    read_regular(path, maximum, error)
}

#[cfg(target_os = "linux")]
const fn owner_policy() -> auth_modules::secrets::files::FileOwner {
    auth_modules::secrets::files::FileOwner::CurrentProcess
}

#[cfg(all(unix, not(target_os = "linux")))]
const fn owner_policy() -> auth_modules::secrets::files::FileOwner {
    auth_modules::secrets::files::FileOwner::Any
}

/// Reads a non-empty regular file of at most `maximum` bytes whose identity
/// does not change between the inspection, the open and the end of the read.
pub fn read_regular(
    path: &Path,
    maximum: u64,
    error: WebConfigError,
) -> Result<Vec<u8>, WebConfigError> {
    let before = fs::symlink_metadata(path).map_err(|_| error)?;
    validate(&before, maximum, error)?;
    let before_identity = ResourceIdentity::from_path(path).map_err(|_| error)?;
    let mut file = File::open(path).map_err(|_| error)?;
    validate(&file.metadata().map_err(|_| error)?, maximum, error)?;
    let opened_identity = ResourceIdentity::from_file(&file).map_err(|_| error)?;
    let mut bytes = Vec::new();
    let read = file
        .by_ref()
        .take(maximum.saturating_add(1))
        .read_to_end(&mut bytes);
    let unchanged = fs::symlink_metadata(path)
        .is_ok_and(|after| validate(&after, maximum, error).is_ok())
        && ResourceIdentity::from_path(path).is_ok_and(|after| after == opened_identity);
    let within_bound = u64::try_from(bytes.len()).is_ok_and(|length| length <= maximum);
    if read.is_err()
        || !unchanged
        || opened_identity != before_identity
        || bytes.is_empty()
        || !within_bound
    {
        bytes.fill(0);
        return Err(error);
    }
    Ok(bytes)
}

fn validate(
    metadata: &Metadata,
    maximum: u64,
    error: WebConfigError,
) -> Result<(), WebConfigError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > maximum
    {
        return Err(error);
    }
    Ok(())
}

/// Whether `metadata` grants no group or other permission. Used where a file
/// is streamed instead of read whole; always true without Unix permissions.
#[cfg(unix)]
pub fn private_permissions(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    // The low six bits are the group (0o070) and other (0o007) permissions.
    metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(not(unix))]
pub const fn private_permissions(_metadata: &Metadata) -> bool {
    true
}

#[cfg(test)]
#[path = "bounded_file_tests.rs"]
mod tests;
