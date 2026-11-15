//! Second-slice tests: migrating an existing garage, and the backup that
//! must come first (DR-0002 G3); third-slice test: v2 to v3. Temp databases
//! only (G9), injected clock.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;

use super::*;
use crate::backup::{self, BackupFile};
use crate::schema::{self, Migration};
use crate::tests::{clock, count, dump, resolved, sidecars_absent};

const V1_TABLES: [&str; 3] = ["vehicle", "vehicle_ident", "service_event"];
const V2_TABLES: [&str; 3] = ["session", "fault_snapshot", "fault_dtc"];
const V3_TABLES: [&str; 2] = ["note", "vehicle_retraction"];

/// The backup's name for `garage.db`, migrated from v1 at `tests::NOW`.
const BACKUP_NAME: &str = "garage.db.v1-20261114T033000Z.backup";
/// The same, migrated from v2.
const V2_BACKUP_NAME: &str = "garage.db.v2-20261114T033000Z.backup";

/// Opens the way `Garage::open` does, without migrating.
fn raw_open(path: &Path) -> Connection {
    let conn = Connection::open_with_flags(path, open_flags()).unwrap();
    configure(&conn, 5000).unwrap();
    conn
}

/// The rows of a populated v1 garage: two vehicles, an alias and a VIN, and
/// service events including a supersede and a retraction. Written as SQL
/// because this binary's `RecordWriter` needs the current schema; the rows
/// are the ones the v1 writer stored for these records.
const V1_ROWS: &str = "
INSERT INTO vehicle (source, recorded_at, diagwire_rev) VALUES
 ('maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 ('maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0');
INSERT INTO vehicle_ident
    (vehicle_id, kind, value, value_norm, source, recorded_at, diagwire_rev) VALUES
 (1, 'label', 'Silver coupe', 'silver coupe',
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (2, 'label', 'Blue sedan', 'blue sedan',
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (1, 'alias', 'the coupe', 'the coupe',
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (2, 'vin', 'TESTVIN0000000001', 'testvin0000000001',
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0');
INSERT INTO service_event
    (vehicle_id, event_date, summary, odo_value, odo_unit, supersedes_id, retracted,
     source, recorded_at, diagwire_rev) VALUES
 (1, '2026-11-13', 'Oil and filter', 120000, 'mi', NULL, 0,
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (1, '2026-11-13', 'Oil and filter', 120000, 'mi', 1, 0,
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (1, '2026-11-13', 'Oil and filter', 120000, 'mi', 2, 1,
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (2, '2026-11-13', 'Oil and filter', 120000, 'mi', NULL, 0,
  'maintainer-entry', '2026-11-14T03:30:00.000000Z', '0.1.0');
";

/// What a v2 garage adds: a clean and a flagged read of the coupe (a
/// complete read with two faults, then a failed one), and a refused read of
/// the sedan.
const V2_ROWS: &str = "
INSERT INTO session
    (vehicle_id, reported_vin, reported_chassis, vin_conflict, chassis_conflict,
     source, recorded_at, diagwire_rev) VALUES
 (1, NULL, 'E46', 0, 0, 'diagwire-read', '2026-11-14T03:30:00.000000Z', '0.1.0'),
 (2, 'TESTVIN0000000009', NULL, 1, 0,
  'diagwire-read', '2026-11-14T03:30:00.000000Z', '0.1.0');
INSERT INTO fault_snapshot (session_id, ecu_address, outcome, nrc, read_at) VALUES
 (1, 18, 'complete', NULL, '2026-11-14T03:30:00.000000Z'),
 (1, 18, 'no-response', NULL, '2026-11-14T03:30:00.000000Z'),
 (2, 18, 'negative', 49, '2026-11-14T03:30:00.000000Z');
INSERT INTO fault_dtc (snapshot_id, code, status) VALUES
 (1, 10882, 36),
 (1, 10887, 8);
";

/// A cleanly closed garage made by the real migrations up to version
/// `version`, holding `rows`.
fn old_garage(version: usize, rows: &[&str]) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garage.db");
    create_private_file(&path).unwrap();
    let mut conn = raw_open(&path);
    assert_eq!(
        schema::migrate(&mut conn, &schema::MIGRATIONS[..version]).unwrap(),
        i64::try_from(version).unwrap()
    );
    for sql in rows {
        conn.execute_batch(sql).unwrap();
    }
    drop(conn);
    assert!(sidecars_absent(&path));
    (dir, path)
}

/// A cleanly closed, populated garage at schema v1. Returns the dump of
/// each v1 table.
fn v1_garage() -> (TempDir, PathBuf, Vec<Vec<String>>) {
    let (dir, path) = old_garage(1, &[V1_ROWS]);
    let dumps = v1_dumps(&Connection::open(&path).unwrap());
    assert_eq!(
        dumps.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 4, 4],
        "the fixture has rows in every v1 table"
    );
    assert!(sidecars_absent(&path));
    (dir, path, dumps)
}

/// A cleanly closed, populated garage at schema v2. Returns the dump of
/// each v1 and v2 table.
fn v2_garage() -> (TempDir, PathBuf, Vec<Vec<String>>) {
    let (dir, path) = old_garage(2, &[V1_ROWS, V2_ROWS]);
    let dumps = v2_dumps(&Connection::open(&path).unwrap());
    assert_eq!(
        dumps.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 4, 4, 2, 3, 2],
        "the fixture has rows in every v1 and v2 table"
    );
    assert!(sidecars_absent(&path));
    (dir, path, dumps)
}

fn v1_dumps(conn: &Connection) -> Vec<Vec<String>> {
    V1_TABLES.iter().map(|t| dump(conn, t)).collect()
}

fn v2_dumps(conn: &Connection) -> Vec<Vec<String>> {
    V1_TABLES
        .iter()
        .chain(&V2_TABLES)
        .map(|t| dump(conn, t))
        .collect()
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
    for table in V2_TABLES.iter().chain(&V3_TABLES) {
        assert!(!has_table(&conn, table), "{table} in a v1 garage");
    }
}

// Opening a populated v1 garage migrates it to the latest version (both
// later steps), keeps every v1 row, and leaves a backup next to it that is
// a valid v1 garage with the same rows.
#[test]
fn v1_garage_migrates_and_leaves_a_v1_backup() {
    let (dir, path, rows) = v1_garage();
    let backup_path = dir.path().join(BACKUP_NAME);

    let g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&g.conn), 3);
    assert_eq!(v1_dumps(&g.conn), rows);
    for table in V2_TABLES.iter().chain(&V3_TABLES) {
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
    assert_eq!(user_version(&restored.conn), 3);
    assert_eq!(v1_dumps(&restored.conn), rows);
}

// Slice 3: opening a populated v2 garage migrates it to v3 through the same
// backup-then-migrate path. Every v1 and v2 row is kept, the two new tables
// exist and are empty, and the backup is a valid v2 garage with the same
// rows and neither new table. The migrated garage then takes the new
// writes, and reads what the v2 binary stored.
#[test]
fn v2_garage_migrates_to_v3_and_leaves_a_v2_backup() {
    let (dir, path, rows) = v2_garage();
    let backup_path = dir.path().join(V2_BACKUP_NAME);

    let mut g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&g.conn), 3);
    assert_eq!(v2_dumps(&g.conn), rows);
    for table in V3_TABLES {
        assert!(has_table(&g.conn, table), "{table}");
        assert_eq!(count(&g.conn, table), 0, "{table}");
    }

    // The backup: a sound v2 garage, same rows, no v3 table.
    assert!(entry_exists(&backup_path).unwrap(), "no backup was left");
    {
        let backup = Connection::open(&backup_path).unwrap();
        let app_id: i32 = backup
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .unwrap();
        assert_eq!(app_id, APPLICATION_ID);
        assert_eq!(user_version(&backup), 2);
        let integrity: String = backup
            .pragma_query_value(None, "integrity_check", |r| r.get(0))
            .unwrap();
        assert_eq!(integrity, "ok");
        assert_eq!(v2_dumps(&backup), rows);
        for table in V3_TABLES {
            assert!(!has_table(&backup, table), "{table} in a v2 backup");
        }
    }

    // What the v2 binary stored reads back through this binary.
    let Outcome::Resolved(faults) = g.latest_measured_faults("the coupe").unwrap() else {
        panic!("the coupe did not resolve");
    };
    assert_eq!(faults.len(), 1);
    assert_eq!(
        faults[0].latest_complete.as_ref().map(|s| &s.read),
        Some(&FaultRead::Complete(vec![
            Dtc {
                code: 10882,
                status: 36
            },
            Dtc {
                code: 10887,
                status: 8
            },
        ]))
    );
    assert_eq!(
        faults[0]
            .newer_attempts
            .iter()
            .map(|s| &s.read)
            .collect::<Vec<_>>(),
        [&FaultRead::NoResponse]
    );
    assert_eq!(
        resolved(g.service_events("blue sedan").unwrap()).len(),
        1,
        "the sedan's one live service event"
    );

    // The new writes work on the migrated rows and change none of them.
    let mut w = g.record_writer();
    let note = resolved(
        w.add_note(
            "the coupe",
            &NoteInput {
                about_date: None,
                body: "Coils are from another model".into(),
            },
        )
        .unwrap(),
    );
    assert_eq!(note.vehicle_id, 1);
    let retraction = resolved(w.retract_vehicle("blue sedan", "Sold").unwrap());
    assert_eq!(retraction.vehicle_id, 2);
    assert_eq!(g.resolve("TESTVIN0000000001").unwrap(), Outcome::NotFound);
    assert_eq!(
        g.vehicles()
            .unwrap()
            .iter()
            .map(|v| v.id)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(v2_dumps(&g.conn), rows);
    assert_eq!(V3_TABLES.map(|t| count(&g.conn, t)), [1, 1]);
    drop(g);
    assert_eq!(entries(dir.path()), ["garage.db", V2_BACKUP_NAME]);

    // A migrated garage opens again without another backup.
    let g = Garage::open(&path, clock(), &Options::default()).unwrap();
    assert_eq!(user_version(&g.conn), 3);
    drop(g);
    assert_eq!(entries(dir.path()), ["garage.db", V2_BACKUP_NAME]);
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
    assert_eq!(user_version(&g.conn), 3);
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

    assert_eq!(version, 3);
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

// Ruling 3: when another writer commits after every backup, the migration
// gives up after three backups, leaves no backup behind and migrates
// nothing.
#[test]
fn migration_gives_up_after_three_outrun_backups() {
    // More commits than the bound, but finitely many: without the bound the
    // migration would succeed on a later backup instead of looping forever.
    const COMMITS: usize = 5;
    let (dir, path, rows) = v1_garage();
    let target = dir.path().join("copy.backup");
    let mut conn = raw_open(&path);
    let other = raw_open(&path);
    let mut calls = 0;

    let result = schema::upgrade(&mut conn, schema::MIGRATIONS, &mut |c, _| {
        calls += 1;
        let taken = backup::take(c, &target)?;
        if calls <= COMMITS {
            other
                .execute(
                    "INSERT INTO vehicle (source, recorded_at, diagwire_rev)
                     VALUES ('maintainer-entry', 'late', 'rev')",
                    [],
                )
                .unwrap();
        }
        Ok(taken)
    });

    assert!(matches!(result, Err(Error::BackupOutrun(3))), "{result:?}");
    assert_eq!(calls, 3);
    // Not migrated: still v1, holding its own rows and the other writer's.
    assert_eq!(user_version(&conn), 1);
    for table in V2_TABLES.iter().chain(&V3_TABLES) {
        assert!(!has_table(&conn, table), "{table}");
    }
    assert_eq!(count(&conn, "vehicle"), 5);
    assert_eq!(dump(&conn, "vehicle")[..2], rows[0][..]);
    assert_eq!(v1_dumps(&conn)[1..], rows[1..]);
    drop(conn);
    drop(other);
    assert_eq!(entries(dir.path()), ["garage.db"]);
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
    conn.pragma_update(None, "user_version", 4).unwrap();
    assert!(matches!(
        schema::upgrade(&mut conn, schema::MIGRATIONS, &mut no_backup),
        Err(Error::TooNew {
            found: 4,
            supported: 3
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
