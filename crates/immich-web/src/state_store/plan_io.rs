use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::WebConfigError;
use crate::profiles::ResourceIdentity;

pub(super) fn publish_new(pending: &Path, final_path: &Path) -> Result<(), std::io::Error> {
    fs::hard_link(pending, final_path)?;
    fs::remove_file(pending)
}

pub(super) fn write_json<T: Serialize>(
    path: &Path,
    value: &T,
    maximum: u64,
) -> Result<(), WebConfigError> {
    let file = create_private_file(path)?;
    let mut writer = BoundedWriter::new(file, maximum);
    serde_json::to_writer_pretty(&mut writer, value)
        .map_err(|_| WebConfigError::new("cannot serialize plan state"))?;
    writer
        .write_all(b"\n")
        .map_err(|_| WebConfigError::new("plan state size limit exceeded"))?;
    writer
        .flush()
        .map_err(|_| WebConfigError::new("cannot flush plan state"))?;
    writer
        .inner
        .sync_all()
        .map_err(|_| WebConfigError::new("cannot sync plan state"))
}

pub(super) fn load_json<T: DeserializeOwned>(
    path: &Path,
    maximum: u64,
) -> Result<(T, FileStamp), WebConfigError> {
    let (value, _file, stamp) = load_json_file(path, maximum)?;
    Ok((value, stamp))
}

pub(super) fn load_json_file<T: DeserializeOwned>(
    path: &Path,
    maximum: u64,
) -> Result<(T, File, FileStamp), WebConfigError> {
    let before = file_stamp(path, maximum)?;
    let file = File::open(path).map_err(|_| WebConfigError::new("cannot open plan state"))?;
    let opened = stamp(
        &file
            .metadata()
            .map_err(|_| WebConfigError::new("cannot inspect plan state"))?,
        ResourceIdentity::from_file(&file)
            .map_err(|_| WebConfigError::new("plan state identity is unavailable"))?,
        maximum,
    )?;
    if before != opened {
        return Err(WebConfigError::new("plan state identity changed"));
    }
    let mut reader = BufReader::new(file);
    let value = serde_json::from_reader(&mut reader)
        .map_err(|_| WebConfigError::new("stored plan state is invalid"))?;
    let file = reader.into_inner();
    if file_stamp(path, maximum)? != opened {
        return Err(WebConfigError::new("plan state identity changed"));
    }
    Ok((value, file, opened))
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) struct FileStamp {
    identity: ResourceIdentity,
    pub length: u64,
}

pub(super) fn file_stamp(path: &Path, maximum: u64) -> Result<FileStamp, WebConfigError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| WebConfigError::new("cannot inspect plan state"))?;
    if metadata.file_type().is_symlink() {
        return Err(WebConfigError::new("plan state must not be a symlink"));
    }
    let identity = ResourceIdentity::from_path(path)
        .map_err(|_| WebConfigError::new("plan state identity is unavailable"))?;
    stamp(&metadata, identity, maximum)
}

fn stamp(
    metadata: &fs::Metadata,
    identity: ResourceIdentity,
    maximum: u64,
) -> Result<FileStamp, WebConfigError> {
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > maximum
        || !private_permissions(metadata)
    {
        return Err(WebConfigError::new("plan state file is invalid"));
    }
    Ok(FileStamp {
        identity,
        length: metadata.len(),
    })
}

struct BoundedWriter {
    inner: File,
    remaining: u64,
}

impl BoundedWriter {
    const fn new(inner: File, maximum: u64) -> Self {
        Self {
            inner,
            remaining: maximum,
        }
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let length = u64::try_from(bytes.len()).map_err(|_| std::io::Error::other("plan bound"))?;
        if length > self.remaining {
            return Err(std::io::Error::other("plan bound"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining = self.remaining.saturating_sub(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<File, WebConfigError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| WebConfigError::new("cannot create plan state"))
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<File, WebConfigError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| WebConfigError::new("cannot create plan state"))
}

#[cfg(unix)]
fn private_permissions(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode().trailing_zeros() >= 6
}

#[cfg(windows)]
fn private_permissions(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<(), WebConfigError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| WebConfigError::new("cannot sync plan directory"))
}

#[cfg(windows)]
pub(super) fn sync_directory(_path: &Path) -> Result<(), WebConfigError> {
    Ok(())
}
