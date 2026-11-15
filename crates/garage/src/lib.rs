//! The garage: vehicles, their identifiers and service history in one local
//! SQLite file (DR-0002). Synchronous, with no transport or car I/O. Callers
//! never see a `rusqlite::Connection` or raw SQL; writes go through
//! [`RecordWriter`], which sets each row's provenance itself.
//!
//! `create`/`open` take an explicit path. There is no default-path function
//! here (G9); front ends resolve the path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags};

mod clock;
mod records;
mod schema;

#[cfg(test)]
mod tests;

pub use clock::{Clock, SystemClock};
pub use records::{
    IdentKind, Identifier, Odometer, OdometerUnit, Outcome, Provenance, RecordWriter, Resolution,
    ServiceEvent, ServiceEventInput, Source, Vehicle, VehicleRef,
};

/// Header `application_id` marking a diagwire garage: ASCII "dwgr".
pub const APPLICATION_ID: i32 = 0x6477_6772;

/// The writing binary's version, stored on every row as `diagwire_rev` (G4).
pub const REVISION: &str = env!("CARGO_PKG_VERSION");

/// Oldest SQLite without the WAL-reset corruption bug (G1).
const MIN_SQLITE: (u32, u32, u32) = (3, 51, 3);
const MIN_SQLITE_STR: &str = "3.51.3";

const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("garage file or one of its sidecars already exists")]
    Exists,
    #[error("no garage at that path")]
    Missing,
    #[error("not a diagwire garage (application_id {0:#010x})")]
    Foreign(i32),
    #[error("garage schema version {found} is newer than this binary supports ({supported})")]
    TooNew { found: i64, supported: i64 },
    #[error("garage schema version {0} is not recognised")]
    UnknownVersion(i64),
    #[error("SQLite {found} is older than the required {MIN_SQLITE_STR}")]
    SqliteTooOld { found: String },
    #[error("pragma {name} reads back {got:?}, expected {expected}")]
    Pragma {
        name: &'static str,
        got: String,
        expected: String,
    },
    #[error("busy timeout {0:?} is outside SQLite's range (0 to {max} ms)", max = i32::MAX)]
    BusyTimeoutOutOfRange(Duration),
    #[error("garage is locked by another writer")]
    Busy,
    #[error("identifier is empty")]
    EmptyIdentifier,
    #[error("service event summary is empty")]
    EmptySummary,
    #[error("that {0} already names another vehicle")]
    IdentifierTaken(IdentKind),
    #[error("record {0} not found for this vehicle")]
    RecordNotFound(i64),
    #[error("record {0} has already been superseded")]
    AlreadySuperseded(i64),
    #[error("record {0} is already retracted")]
    AlreadyRetracted(i64),
    #[error("record {0} is retracted, and a retraction is final; add a new record instead")]
    RetractionIsFinal(i64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(rusqlite::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy) => Error::Busy,
            _ => Error::Sqlite(e),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Options {
    /// How long a write waits for another writer's lock before failing with
    /// [`Error::Busy`]. Set explicitly; rusqlite's own default may change.
    pub busy_timeout: Duration,
}

impl Options {
    /// The busy timeout in whole milliseconds, as SQLite takes it. Checked
    /// before anything touches the filesystem: rusqlite panics past `i32::MAX`.
    fn busy_timeout_ms(&self) -> Result<i32> {
        let t = self.busy_timeout;
        t.as_secs()
            .checked_mul(1000)
            .and_then(|ms| ms.checked_add(t.subsec_millis().into()))
            .and_then(|ms| i32::try_from(ms).ok())
            .ok_or(Error::BusyTimeoutOutOfRange(t))
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            busy_timeout: DEFAULT_BUSY_TIMEOUT,
        }
    }
}

pub struct Garage {
    conn: Connection,
    clock: Arc<dyn Clock>,
}

impl Garage {
    /// Creates a new garage at `path`. Refuses if the file or any sidecar
    /// exists. A missing parent directory is created user-private on Unix.
    /// If setup fails after the file is made, the partial file is left in
    /// place (nothing here deletes files) and `open` refuses it.
    pub fn create(path: &Path, clock: Arc<dyn Clock>, options: &Options) -> Result<Self> {
        let busy_ms = options.busy_timeout_ms()?;
        for entry in sidecars(path) {
            if entry_exists(&entry)? {
                return Err(Error::Exists);
            }
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
            && !entry_exists(dir)?
        {
            create_private_dir(dir)?;
        }
        // Exclusive create, so a racing creator can't share the file.
        create_private_file(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => Error::Exists,
            _ => Error::Io(e),
        })?;
        let mut conn = Connection::open_with_flags(path, open_flags())?;
        check_sqlite_version(&conn)?;
        configure(&conn, busy_ms)?;
        schema::migrate(&mut conn, schema::MIGRATIONS)?;
        Ok(Self { conn, clock })
    }

    /// Opens an existing garage. Never creates a file. Checks the SQLite
    /// version, `application_id` and `user_version` before changing any
    /// configuration, and refuses a foreign or newer file.
    pub fn open(path: &Path, clock: Arc<dyn Clock>, options: &Options) -> Result<Self> {
        let busy_ms = options.busy_timeout_ms()?;
        match fs::metadata(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::Missing),
            Err(e) => return Err(Error::Io(e)),
        }
        let conn = Connection::open_with_flags(path, open_flags())?;
        check_sqlite_version(&conn)?;
        let app_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if app_id != APPLICATION_ID {
            return Err(Error::Foreign(app_id));
        }
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > schema::LATEST {
            return Err(Error::TooNew {
                found: version,
                supported: schema::LATEST,
            });
        }
        // v1 is creation only: there is no older version to migrate from.
        if version != schema::LATEST {
            return Err(Error::UnknownVersion(version));
        }
        configure(&conn, busy_ms)?;
        Ok(Self { conn, clock })
    }

    /// The writer for maintainer records. It sets `source` itself.
    pub fn record_writer(&mut self) -> RecordWriter<'_> {
        RecordWriter::new(&mut self.conn, self.clock.as_ref())
    }
}

/// Read-write without `SQLITE_OPEN_CREATE`: rusqlite's default flags create.
fn open_flags() -> OpenFlags {
    OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX
}

fn sidecars(path: &Path) -> [PathBuf; 4] {
    let with = |suffix: &str| {
        let mut s = path.as_os_str().to_owned();
        s.push(suffix);
        PathBuf::from(s)
    };
    [
        path.to_path_buf(),
        with("-wal"),
        with("-shm"),
        with("-journal"),
    ]
}

/// Whether a directory entry exists, without following symlinks: a dangling
/// `-wal` symlink is still an entry. Errors other than "not found" propagate.
fn entry_exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    // Windows: rely on the per-user directory's ACL (G8), never widen it.
    fs::create_dir_all(dir)
}

fn create_private_file(path: &Path) -> std::io::Result<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path).map(drop)
}

fn check_sqlite_version(conn: &Connection) -> Result<()> {
    let found: String = conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    if sqlite_version_ok(&found) {
        Ok(())
    } else {
        Err(Error::SqliteTooOld { found })
    }
}

fn sqlite_version_ok(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let mut next = || parts.next().flatten();
    match (next(), next(), next()) {
        (Some(a), Some(b), c) => (a, b, c.unwrap_or(0)) >= MIN_SQLITE,
        _ => false,
    }
}

/// Per-connection settings, outside any transaction: journal mode can't
/// change inside one, and `foreign_keys` is a no-op inside one.
/// `busy_ms` is already range-checked by [`Options::busy_timeout_ms`].
fn configure(conn: &Connection, busy_ms: i32) -> Result<()> {
    conn.busy_timeout(Duration::from_millis(busy_ms.unsigned_abs().into()))?;
    expect_pragma(conn, "busy_timeout", busy_ms.into())?;
    conn.pragma_update(None, "foreign_keys", true)?;
    expect_pragma(conn, "foreign_keys", 1)?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    expect_pragma(conn, "synchronous", 2)?;
    let mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::Pragma {
            name: "journal_mode",
            got: mode,
            expected: "wal".into(),
        });
    }
    Ok(())
}

fn expect_pragma(conn: &Connection, name: &'static str, expected: i64) -> Result<()> {
    let got: i64 = conn.pragma_query_value(None, name, |r| r.get(0))?;
    if got == expected {
        Ok(())
    } else {
        Err(Error::Pragma {
            name,
            got: got.to_string(),
            expected: expected.to_string(),
        })
    }
}
