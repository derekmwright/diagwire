//! Second-slice tests: migrating an existing garage, and the backup that
//! must come first (DR-0002 G3). Temp databases only (G9), injected clock.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;

use super::*;
use crate::backup::{self, BackupFile};
use crate::schema::{self, Migration};
use crate::tests::{FixedClock, clock, count, dump, oil, resolved, sidecars_absent};

const V1_TABLES: [&str; 3] = ["vehicle", "vehicle_ident", "service_event"];
const V2_TABLES: [&str; 3] = ["session", "fault_snapshot", "fault_dtc"];

/// The backup's name for `garage.db`, migrated from v1 at `tests::NOW`.
const BACKUP_NAME: &str = "garage.db.v1-20261114T033000Z.backup";

/// Opens the way `Garage::open` does, without migrating.
fn raw_open(path: &Path) -> Connection {
    let conn = Connection::open_with_flags(path, open_flags()).unwrap();
    configure(&conn, 5000).unwrap();
    conn
}

/// A cleanly closed, populated garage at schema v1, made by the real v1
/// migration and the real `RecordWriter`. Returns the dump of each v1 table.
fn v1_garage() -> (TempDir, PathBuf, Vec<Vec<String>>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garage.db");
    create_private_file(&path).unwrap();
    let mut conn = raw_open(&path);
    assert_eq!(
        schema::migrate(&mut conn, &schema::MIGRATIONS[..1]).unwrap(),
        1
    );
    let mut w = RecordWriter::new(&mut conn, &FixedClock);
    w.add_vehicle("Silver coupe").unwrap();
    w.add_vehicle("Blue sedan").unwrap();
    resolved(
        w.add_identifier("silver coupe", IdentKind::Alias, "the coupe")
            .unwrap(),
    );
    resolved(
        w.add_identifier("blue sedan", IdentKind::Vin, "TESTVIN0000000001")
            .unwrap(),
    );
    let first = resolved(w.add_service_event("the coupe", &oil(None)).unwrap());
    let second = resolved(
        w.supersede_service_event("the coupe", first.id, &oil(None))
            .unwrap(),
    );
    resolved(w.retract_service_event("the coupe", second.id).unwrap());
    resolved(w.add_service_event("blue sedan", &oil(None)).unwrap());
    let dumps = v1_dumps(&conn);
    assert_eq!(
        dumps.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 4, 4],
        "the fixture has rows in every v1 table"
    );
    drop(conn);
    assert!(sidecars_absent(&path));
    (dir, path, dumps)
}

fn v1_dumps(conn: &Connection) -> Vec<Vec<String>> {
    V1_TABLES.iter().map(|t| dump(conn, t)).collect()
}

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn has_table(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
    .unwrap()
}

fn entries(dir: &Path) -> Vec<OsString> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    names
}

/// Checks that `path` holds a sound v1 garage with exactly `rows`.
fn assert_v1_garage(path: &Path, rows: &[Vec<String>]) {
    let conn = Connection::open(path).unwrap();
    let app_id: i32 = conn
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .unwrap();
    assert_eq!(app_id, APPLICATION_ID);
    assert_eq!(user_version(&conn), 1);
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |r| r.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    assert_eq!(v1_dumps(&conn), rows);
    for table in V2_TABLES {
        assert!(!has_table(&conn, table), "{table} in a v1 garage");
    }
}

// Opening a populated v1 garage migrates it to v2, keeps every v1 row, and
// leaves a backup next to it that is a valid v1 garage with the same rows.
#[test]
fn v1_garage_migrates_and_leaves_a_v1_backup() {
    let (dir, path, rows) = v1_garage();
    let backup_path = dir.path().join(BACKUP_NAME);

    let g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&g.conn), 2);
    assert_eq!(v1_dumps(&g.conn), rows);
    for table in V2_TABLES {
        assert_eq!(count(&g.conn, table), 0, "{table}");
    }
    drop(g);
    assert_eq!(entries(dir.path()), ["garage.db", BACKUP_NAME]);

    assert_v1_garage(&backup_path, &rows);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&backup_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // A migrated garage opens again without another backup.
    let g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(v1_dumps(&g.conn), rows);
    drop(g);
    assert_eq!(entries(dir.path()), ["garage.db", BACKUP_NAME]);

    // The backup is itself a garage this binary opens (and migrates).
    let restored = Garage::open(&backup_path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&restored.conn), 2);
    assert_eq!(v1_dumps(&restored.conn), rows);
}

// If the backup can't be taken, the open fails and the garage is unchanged.
#[test]
fn backup_failure_aborts_migration_with_garage_unchanged() {
    let (dir, path, rows) = v1_garage();
    // Something already sits where the backup would go; it is never
    // overwritten, so the backup fails.
    let blocker = dir.path().join(BACKUP_NAME);
    fs::write(&blocker, b"in the way").unwrap();
    let before = fs::read(&path).unwrap();

    match Garage::open(&path, clock(), &Options::default()) {
        Err(Error::Backup(cause)) => assert!(
            matches!(&*cause, Error::Io(e) if e.kind() == std::io::ErrorKind::AlreadyExists),
            "wrong cause: {cause:?}"
        ),
        Err(e) => panic!("wrong refusal: {e:?}"),
        Ok(_) => panic!("open migrated a garage it could not back up"),
    }
    assert_eq!(fs::read(&path).unwrap(), before, "main file changed");
    assert!(sidecars_absent(&path), "sidecar left behind");
    assert_eq!(fs::read(&blocker).unwrap(), b"in the way");
    assert_eq!(entries(dir.path()), ["garage.db", BACKUP_NAME]);
    assert_v1_garage(&path, &rows);

    // With the obstacle gone the same open migrates.
    fs::remove_file(&blocker).unwrap();
    let g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&g.conn), 2);
}

// A backup that fails partway leaves no file behind to be mistaken for one.
#[test]
fn failed_backup_leaves_no_file() {
    let (dir, path, rows) = v1_garage();
    let conn = raw_open(&path);
    let target = dir.path().join("copy.backup");

    // VACUUM refuses to run inside a transaction, after `take` has already
    // created the file.
    conn.execute_batch("BEGIN").unwrap();
    assert!(matches!(
        backup::take(&conn, &target),
        Err(Error::Sqlite(_))
    ));
    assert!(
        !entry_exists(&target).unwrap(),
        "partial backup left behind"
    );
    conn.execute_batch("ROLLBACK").unwrap();

    // Outside a transaction the same call succeeds.
    backup::take(&conn, &target).unwrap();
    drop(conn);
    assert_v1_garage(&target, &rows);
}

// A commit that lands after the backup but before the migration's lock
// makes the backup stale: it is replaced by one that holds that commit.
#[test]
fn commit_after_backup_forces_a_new_backup() {
    let (dir, path, rows) = v1_garage();
    let target = dir.path().join("copy.backup");
    let mut conn = raw_open(&path);
    let other = raw_open(&path);
    let mut calls = 0;

    let version = schema::upgrade(&mut conn, schema::MIGRATIONS, &mut |c, from| {
        calls += 1;
        assert_eq!(from, 1);
        let taken = backup::take(c, &target)?;
        if calls == 1 {
            // Another connection commits after the first backup's snapshot.
            other
                .execute(
                    "INSERT INTO vehicle (source, recorded_at, diagwire_rev)
                     VALUES ('maintainer-entry', 'late', 'rev')",
                    [],
                )
                .unwrap();
        }
        Ok(taken)
    })
    .unwrap();

    assert_eq!(version, 2);
    assert_eq!(calls, 2, "one stale backup, then one that held");
    assert_eq!(count(&conn, "vehicle"), 3);
    drop(conn);
    drop(other);
    assert_eq!(entries(dir.path()), ["copy.backup", "garage.db"]);

    // The kept backup is v1 and includes the late commit.
    let kept = Connection::open(&target).unwrap();
    assert_eq!(user_version(&kept), 1);
    assert_eq!(count(&kept, "vehicle"), 3);
    assert_eq!(dump(&kept, "vehicle")[..2], rows[0][..]);
    let late: i64 = kept
        .query_row(
            "SELECT count(*) FROM vehicle WHERE recorded_at = 'late'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(late, 1);
}

fn no_backup(_: &Connection, _: i64) -> Result<BackupFile> {
    panic!("a backup was requested");
}

// A garage that needs no step is not backed up, and one already past this
// binary's steps is refused, also without a backup.
#[test]
fn upgrade_without_pending_steps_takes_no_backup() {
    let (dir, path, _rows) = v1_garage();
    let mut conn = raw_open(&path);
    assert_eq!(
        schema::upgrade(&mut conn, &schema::MIGRATIONS[..1], &mut no_backup).unwrap(),
        1
    );
    conn.pragma_update(None, "user_version", 3).unwrap();
    assert!(matches!(
        schema::upgrade(&mut conn, schema::MIGRATIONS, &mut no_backup),
        Err(Error::TooNew {
            found: 3,
            supported: 2
        })
    ));
    drop(conn);
    assert_eq!(entries(dir.path()), ["garage.db"]);
}

// A migration that fails after its backup changes nothing and keeps no
// backup.
#[test]
fn failed_migration_after_backup_keeps_nothing() {
    const FAILING: &[Migration] = &[Migration {
        version: 2,
        sql: "CREATE TABLE probe (x INTEGER);
              INSERT INTO no_such_table VALUES (1);",
    }];
    let (dir, path, rows) = v1_garage();
    let target = dir.path().join("copy.backup");
    let mut conn = raw_open(&path);
    let mut calls = 0;
    let result = schema::upgrade(&mut conn, FAILING, &mut |c, _| {
        calls += 1;
        backup::take(c, &target)
    });
    assert!(matches!(result, Err(Error::Sqlite(_))), "{result:?}");
    assert_eq!(calls, 1);
    assert!(!has_table(&conn, "probe"));
    drop(conn);
    assert_eq!(entries(dir.path()), ["garage.db"]);
    assert_v1_garage(&path, &rows);
}
