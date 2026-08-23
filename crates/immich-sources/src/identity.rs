use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use immich_rs_core::Cancellation;
use sha1::Sha1;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
    len: u64,
    modified_nanos: Option<u128>,
    platform_identity: PlatformIdentity,
}

impl FileSnapshot {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos()),
            platform_identity: platform_identity(metadata),
        }
    }
}

#[cfg(unix)]
type PlatformIdentity = (u64, u64);

#[cfg(not(unix))]
type PlatformIdentity = ();

#[cfg(unix)]
fn platform_identity(metadata: &Metadata) -> PlatformIdentity {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
const fn platform_identity(_metadata: &Metadata) -> PlatformIdentity {}

#[cfg(unix)]
fn lacks_read_permissions(metadata: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o444 == 0
}

#[cfg(not(unix))]
const fn lacks_read_permissions(_metadata: &Metadata) -> bool {
    false
}

pub enum StreamError {
    Cancelled,
    Unreadable,
}

pub fn stream_identity(
    path: &Path,
    discovered_metadata: &Metadata,
    buffer_bytes: usize,
    cancellation: &impl Cancellation,
) -> Result<(u64, String, String, bool), StreamError> {
    if lacks_read_permissions(discovered_metadata) {
        return Err(StreamError::Unreadable);
    }
    let before = FileSnapshot::from_metadata(discovered_metadata);
    let mut file = File::open(path).map_err(|_| StreamError::Unreadable)?;
    let opened = file.metadata().map_err(|_| StreamError::Unreadable)?;
    let opened_snapshot = FileSnapshot::from_metadata(&opened);
    let mut digest = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut buffer = vec![0_u8; buffer_bytes];
    let mut bytes_read = 0_u64;
    while bytes_read < opened_snapshot.len {
        if cancellation.is_cancelled() {
            return Err(StreamError::Cancelled);
        }
        let remaining = opened_snapshot.len - bytes_read;
        let read_limit =
            usize::try_from(remaining).map_or(buffer.len(), |value| value.min(buffer.len()));
        let read = file
            .read(&mut buffer[..read_limit])
            .map_err(|_| StreamError::Unreadable)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        sha1.update(&buffer[..read]);
        bytes_read = bytes_read
            .checked_add(read as u64)
            .ok_or(StreamError::Unreadable)?;
    }
    let after_open = file.metadata().map_err(|_| StreamError::Unreadable)?;
    let after_path = fs::metadata(path).map_err(|_| StreamError::Unreadable)?;
    let changed = before != opened_snapshot
        || opened_snapshot != FileSnapshot::from_metadata(&after_open)
        || opened_snapshot != FileSnapshot::from_metadata(&after_path)
        || bytes_read != opened_snapshot.len;
    Ok((
        bytes_read,
        format!("{:x}", digest.finalize()),
        STANDARD.encode(sha1.finalize()),
        changed,
    ))
}
