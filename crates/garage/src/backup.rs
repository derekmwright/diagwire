//! The backup taken before an existing garage is migrated (G3).
//!
//! `VACUUM INTO` writes a consistent snapshot of the logical database (main
//! file and WAL, read through the connection) into one new file, and syncs
//! it when the source connection's `synchronous` is NORMAL or FULL, which
//! every garage connection is (sqlite.org/lang_vacuum.html).

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rusqlite::Connection;

use crate::{Error, Result, create_private_file};

/// A backup file this process created.
#[derive(Debug)]
pub(crate) struct BackupFile {
    path: PathBuf,
}

impl BackupFile {
    /// Removes the file. Only ever called on a file [`take`] created.
    pub(crate) fn discard(self) -> std::io::Result<()> {
        fs::remove_file(&self.path)
    }
}

/// Where the backup of the garage at `garage`, taken at `now` before
/// migrating from schema version `from`, goes: next to the garage file, so
/// inside the same user-private directory (G8).
pub(crate) fn path_for(garage: &Path, from: i64, now: Timestamp) -> PathBuf {
    let mut name = OsString::from(garage.as_os_str());
    name.push(format!(
        ".v{from}-{}.backup",
        now.strftime("%Y%m%dT%H%M%SZ")
    ));
    PathBuf::from(name)
}

/// Backs up the database `conn` has open into a new file at `path`. The
/// file is created here, exclusively and user-private, so an existing entry
/// is never overwritten; SQLite then fills the empty file. On any failure
/// the file created here is removed again.
pub(crate) fn take(conn: &Connection, path: &Path) -> Result<BackupFile> {
    let target = path.to_str().ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "backup path is not valid UTF-8",
        ))
    })?;
    create_private_file(path)?;
    let file = BackupFile {
        path: path.to_path_buf(),
    };
    match conn.execute("VACUUM INTO ?1", [target]) {
        Ok(_) => Ok(file),
        Err(e) => {
            // The original error is the one to report.
            let _ = file.discard();
            Err(e.into())
        }
    }
}
