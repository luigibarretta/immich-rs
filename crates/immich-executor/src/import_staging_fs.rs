use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::{ExecutorError, ExecutorErrorClass};

pub fn cleanup_exact_root(root: &Path) -> Result<(), ExecutorError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(destination_error(error)),
    };
    if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
        return Err(ExecutorError::new(ExecutorErrorClass::Destination));
    }
    for entry in fs::read_dir(root).map_err(destination_error)? {
        let entry = entry.map_err(destination_error)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ExecutorError::new(ExecutorErrorClass::Destination))?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(destination_error)?;
        if !metadata.file_type().is_file() || !valid_staging_name(&name) {
            return Err(ExecutorError::new(ExecutorErrorClass::Destination));
        }
        fs::remove_file(entry.path()).map_err(destination_error)?;
    }
    fs::remove_dir(root).map_err(destination_error)
}

fn valid_staging_name(name: &str) -> bool {
    let Some((operation_id, suffix)) = name.split_once('.') else {
        return false;
    };
    operation_id.len() == 64
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && matches!(suffix, "media" | "media.tmp" | "xmp" | "xmp.tmp")
}

pub fn create_private_directory(path: &Path) -> Result<(), ExecutorError> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path).map_err(destination_error)
}

pub fn private_file(path: &Path) -> Result<File, ExecutorError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path).map_err(destination_error)
}

pub fn validate_real_directory(path: &Path) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path).map_err(destination_error)?;
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(ExecutorError::new(ExecutorErrorClass::Destination))
    }
}

pub fn validate_real_file(path: &Path) -> Result<(), ExecutorError> {
    let metadata = fs::symlink_metadata(path).map_err(source_changed)?;
    if metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(ExecutorError::new(ExecutorErrorClass::SourceChanged))
    }
}

fn source_changed(_error: impl std::fmt::Debug) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::SourceChanged)
}

fn destination_error(_error: impl std::fmt::Debug) -> ExecutorError {
    ExecutorError::new(ExecutorErrorClass::Destination)
}
