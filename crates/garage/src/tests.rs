//! First-slice tests (DR-0002, tests 1–7). Temp databases only (G9) and an
//! injected clock; nothing here reads the wall clock.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use jiff::civil::{Date, date};
use rusqlite::{Connection, ErrorCode};
use tempfile::TempDir;

use super::*;
use crate::schema::{self, Migration};

/// UTC now is the 14th; the host's local date is still the 13th, so a test
/// can tell which one a default date came from.
pub(crate) struct FixedClock;

pub(crate) const NOW: &str = "2026-11-14T03:30:00Z";
pub(crate) const NOW_STORED: &str = "2026-11-14T03:30:00.000000Z";
pub(crate) const TODAY: Date = date(2026, 11, 13);

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        NOW.parse().unwrap()
    }
    fn today(&self) -> Date {
        TODAY
    }
}

pub(crate) fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock)
}

pub(crate) fn new_garage() -> (TempDir, PathBuf, Garage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garage.db");
    let g = Garage::create(&path, clock(), &Options::default()).unwrap();
    (dir, path, g)
}

pub(crate) fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

pub(crate) fn sidecars_absent(path: &Path) -> bool {
    sidecars(path)[1..]
        .iter()
        .all(|p| !entry_exists(p).unwrap())
}

pub(crate) fn oil(date: Option<Date>) -> ServiceEventInput {
    ServiceEventInput {
        date,
        summary: "Oil and filter".into(),
        odometer: Some(Odometer {
            value: 120_000,
            unit: OdometerUnit::Mi,
        }),
    }
}

pub(crate) fn resolved<T: std::fmt::Debug>(o: Outcome<T>) -> T {
    match o {
        Outcome::Resolved(t) => t,
        other => panic!("expected Resolved, got {other:?}"),
    }
}

pub(crate) fn label_of(label: &Option<Identifier>) -> Option<&str> {
    label.as_ref().map(|l| l.value.as_str())
}

pub(crate) fn is_constraint(r: rusqlite::Result<usize>) -> bool {
    matches!(r, Err(e) if e.sqlite_error_code() == Some(ErrorCode::ConstraintViolation))
}

/// Every row of `table`, as text, to show a refused statement changed nothing.
pub(crate) fn dump(conn: &Connection, table: &str) -> Vec<String> {
    let cols: Vec<String> = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let row = cols
        .iter()
        .map(|c| format!("quote({c})"))
        .collect::<Vec<_>>()
        .join(" || '|' || ");
    conn.prepare(&format!("SELECT {row} FROM {table} ORDER BY id"))
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

// 1. Replay 1: an ambiguous reference writes nothing; a unique alias writes.
#[test]
fn replay1_ambiguous_reference_writes_nothing() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    let coupe = w.add_vehicle("Silver coupe").unwrap();
    let sedan = w.add_vehicle("Blue sedan").unwrap();
    resolved(
        w.add_identifier("Silver coupe", IdentKind::Chassis, "E46")
            .unwrap(),
    );
    resolved(
        w.add_identifier("Blue sedan", IdentKind::Chassis, "E46")
            .unwrap(),
    );
    resolved(
        w.add_identifier("silver coupe", IdentKind::Alias, "the coupe")
            .unwrap(),
    );

    let before = count(&g.conn, "service_event");
    let out = g
        .record_writer()
        .add_service_event("e46", &oil(None))
        .unwrap();
    let Outcome::Ambiguous(candidates) = out else {
        panic!("expected Ambiguous, got {out:?}");
    };
    assert_eq!(
        candidates
            .iter()
            .map(|c| (c.vehicle_id, label_of(&c.label)))
            .collect::<Vec<_>>(),
        [
            (coupe.id, Some("Silver coupe")),
            (sedan.id, Some("Blue sedan"))
        ]
    );
    assert_eq!(count(&g.conn, "service_event"), before);

    let event = resolved(
        g.record_writer()
            .add_service_event("  The   COUPE ", &oil(None))
            .unwrap(),
    );
    assert_eq!(event.vehicle_id, coupe.id);
    assert_eq!(label_of(&event.vehicle_label), Some("Silver coupe"));
    assert_eq!(count(&g.conn, "service_event"), before + 1);
    assert_eq!(
        g.record_writer()
            .add_service_event("e90", &oil(None))
            .unwrap(),
        Outcome::NotFound
    );
}

// 2. Provenance: the writer sets source, revision and time from the clock.
// No public write method takes a source (by the API's shape).
#[test]
fn provenance_set_by_writer() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    let v = w.add_vehicle("Test car").unwrap();
    let ident = resolved(
        w.add_identifier("test car", IdentKind::Vin, "TESTVIN0000000001")
            .unwrap(),
    );
    let dated = resolved(
        w.add_service_event("test car", &oil(Some(date(2026, 1, 2))))
            .unwrap(),
    );
    let undated = resolved(w.add_service_event("test car", &oil(None)).unwrap());
    let fixed = resolved(
        w.supersede_service_event("test car", dated.id, &oil(None))
            .unwrap(),
    );
    let gone = resolved(w.retract_service_event("test car", undated.id).unwrap());

    let expected = Provenance {
        source: Source::MaintainerEntry,
        recorded_at: NOW.parse().unwrap(),
        diagwire_rev: REVISION.into(),
    };
    for p in [
        &v.provenance,
        &ident.provenance,
        &dated.provenance,
        &undated.provenance,
        &fixed.provenance,
        &gone.provenance,
    ] {
        assert_eq!(p, &expected);
    }
    assert_eq!(dated.event_date, date(2026, 1, 2));
    assert_eq!(undated.event_date, TODAY);
    assert_eq!(fixed.event_date, TODAY);

    for table in ["vehicle", "vehicle_ident", "service_event"] {
        let mut stmt = g
            .conn
            .prepare(&format!(
                "SELECT source, recorded_at, diagwire_rev FROM {table}"
            ))
            .unwrap();
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!rows.is_empty(), "{table} has rows");
        for row in rows {
            assert_eq!(
                row,
                (
                    "maintainer-entry".into(),
                    NOW_STORED.into(),
                    REVISION.into()
                ),
                "{table}"
            );
        }
    }
    assert_eq!(REVISION, env!("CARGO_PKG_VERSION"));
}

// 3. Append-only: storage rejects edits; corrections and retractions append.
#[test]
fn append_only() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    w.add_vehicle("Daily").unwrap();
    resolved(
        w.add_identifier("daily", IdentKind::Alias, "commuter")
            .unwrap(),
    );
    let first = resolved(w.add_service_event("daily", &oil(None)).unwrap());

    let events = dump(&g.conn, "service_event");
    let idents = dump(&g.conn, "vehicle_ident");
    let vehicles = dump(&g.conn, "vehicle");
    for sql in [
        "UPDATE service_event SET summary = 'edited'",
        "DELETE FROM service_event",
        "UPDATE vehicle_ident SET value = 'edited'",
        "DELETE FROM vehicle_ident",
        "UPDATE vehicle SET source = 'imported-note'",
        "DELETE FROM vehicle",
    ] {
        assert!(
            is_constraint(g.conn.execute(sql, [])),
            "{sql} was not refused"
        );
    }
    assert_eq!(dump(&g.conn, "service_event"), events);
    assert_eq!(dump(&g.conn, "vehicle_ident"), idents);
    assert_eq!(dump(&g.conn, "vehicle"), vehicles);

    // Supersede adds a head; the old row stays in history.
    let corrected = ServiceEventInput {
        summary: "Oil, filter and plug".into(),
        ..oil(None)
    };
    let second = resolved(
        g.record_writer()
            .supersede_service_event("daily", first.id, &corrected)
            .unwrap(),
    );
    assert_eq!(second.supersedes_id, Some(first.id));
    let live = resolved(g.service_events("daily").unwrap());
    assert_eq!(live.iter().map(|e| e.id).collect::<Vec<_>>(), [second.id]);

    // A second supersede of the same row fails, in the API and in storage.
    let before = count(&g.conn, "service_event");
    assert!(matches!(
        g.record_writer().supersede_service_event("daily", first.id, &oil(None)),
        Err(Error::AlreadySuperseded(id)) if id == first.id
    ));
    assert!(is_constraint(g.conn.execute(
        "INSERT INTO service_event (vehicle_id, event_date, summary, supersedes_id, source, recorded_at, diagwire_rev)
         VALUES (?1, '2026-01-01', 'x', ?2, 'maintainer-entry', 'now', 'rev')",
        [first.vehicle_id, first.id],
    )));
    assert_eq!(count(&g.conn, "service_event"), before);

    // A retracted head is hidden by default and shown in history.
    let retraction = resolved(
        g.record_writer()
            .retract_service_event("daily", second.id)
            .unwrap(),
    );
    assert!(retraction.retracted);
    assert_eq!(resolved(g.service_events("daily").unwrap()), []);
    let history = resolved(g.service_event_history("daily").unwrap());
    assert_eq!(
        history
            .iter()
            .map(|e| (e.id, e.retracted))
            .collect::<Vec<_>>(),
        [(first.id, false), (second.id, false), (retraction.id, true)]
    );

    // A retraction is final (A1): superseding the retracted head is refused
    // and writes nothing; a fresh record undoes it, and history keeps the
    // retraction.
    let rows = count(&g.conn, "service_event");
    assert!(matches!(
        g.record_writer()
            .supersede_service_event("daily", retraction.id, &oil(None)),
        Err(Error::RetractionIsFinal(id)) if id == retraction.id
    ));
    assert_eq!(count(&g.conn, "service_event"), rows);
    let fresh = resolved(
        g.record_writer()
            .add_service_event("daily", &corrected)
            .unwrap(),
    );
    assert_eq!(fresh.supersedes_id, None);
    let live = resolved(g.service_events("daily").unwrap());
    assert_eq!(live.iter().map(|e| e.id).collect::<Vec<_>>(), [fresh.id]);
    let history = resolved(g.service_event_history("daily").unwrap());
    assert_eq!(
        history
            .iter()
            .map(|e| (e.id, e.retracted))
            .collect::<Vec<_>>(),
        [
            (first.id, false),
            (second.id, false),
            (retraction.id, true),
            (fresh.id, false)
        ]
    );

    // A duplicate live alias on another vehicle is refused.
    g.record_writer().add_vehicle("Weekend").unwrap();
    let idents = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("weekend", IdentKind::Alias, " Commuter "),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), idents);

    // An odometer value without a unit is refused by storage. (The API's
    // `Odometer` type can't express one.)
    assert!(is_constraint(g.conn.execute(
        "INSERT INTO service_event (vehicle_id, event_date, summary, odo_value, source, recorded_at, diagwire_rev)
         VALUES (?1, '2026-01-01', 'x', 1000, 'maintainer-entry', 'now', 'rev')",
        [first.vehicle_id],
    )));
    // `before` plus the retraction and the fresh record.
    assert_eq!(count(&g.conn, "service_event"), before + 2);
}

// 4. Replay 4: open refuses a newer, foreign or unrecognised file and leaves
// a cleanly closed file byte-identical with no sidecar.
fn assert_open_refuses(path: &Path, check: impl Fn(&Error) -> bool) {
    assert!(sidecars_absent(path), "test file must be cleanly closed");
    let before = fs::read(path).unwrap();
    let result = Garage::open(path, clock(), &Options::default());
    match result {
        Err(e) => assert!(check(&e), "wrong refusal: {e:?}"),
        Ok(_) => panic!("open accepted a file it must refuse"),
    }
    assert_eq!(fs::read(path).unwrap(), before, "main file changed");
    assert!(sidecars_absent(path), "sidecar left behind");
}

#[test]
fn open_refuses_newer_version() {
    let (_dir, path, g) = new_garage();
    drop(g);
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", schema::LATEST + 1)
        .unwrap();
    assert_open_refuses(&path, |e| {
        matches!(
            e,
            Error::TooNew {
                found: 4,
                supported: 3
            }
        )
    });
}

#[test]
fn open_refuses_foreign_and_empty_files() {
    let dir = tempfile::tempdir().unwrap();

    let foreign = dir.path().join("foreign.db");
    Connection::open(&foreign)
        .unwrap()
        .execute_batch("PRAGMA application_id = 305419896; CREATE TABLE t (x);")
        .unwrap();
    assert_open_refuses(&foreign, |e| matches!(e, Error::Foreign(0x1234_5678)));

    // A database with a header but no application ID.
    let unmarked = dir.path().join("unmarked.db");
    Connection::open(&unmarked)
        .unwrap()
        .execute_batch("CREATE TABLE t (x); DROP TABLE t;")
        .unwrap();
    assert!(fs::metadata(&unmarked).unwrap().len() > 0);
    assert_open_refuses(&unmarked, |e| matches!(e, Error::Foreign(0)));

    // A zero-length file, which SQLite treats as an empty database.
    let empty = dir.path().join("empty.db");
    fs::write(&empty, b"").unwrap();
    assert_open_refuses(&empty, |e| matches!(e, Error::Foreign(0)));
}

#[test]
fn open_missing_creates_nothing_and_create_refuses_existing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garage.db");
    assert!(matches!(
        Garage::open(&path, clock(), &Options::default()),
        Err(Error::Missing)
    ));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);

    fs::write(&path, b"not ours").unwrap();
    assert!(matches!(
        Garage::create(&path, clock(), &Options::default()),
        Err(Error::Exists)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"not ours");

    let other = dir.path().join("other.db");
    let wal = dir.path().join("other.db-wal");
    fs::write(&wal, b"").unwrap();
    assert!(matches!(
        Garage::create(&other, clock(), &Options::default()),
        Err(Error::Exists)
    ));
    assert!(!other.exists());
}

// 5. Migration rollback: a failing step leaves no table and no version bump.
#[test]
fn failed_migration_rolls_back_table_and_version() {
    const FAILING: &[Migration] = &[Migration {
        version: 4,
        sql: "CREATE TABLE probe (x INTEGER);
              PRAGMA user_version = 4;
              INSERT INTO no_such_table VALUES (1);",
    }];
    let (_dir, path, mut g) = new_garage();
    let probe = |c: &Connection| -> (i64, i64) {
        let version = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let tables = c
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'probe'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        (version, tables)
    };
    assert_eq!(probe(&g.conn), (3, 0));
    assert!(matches!(
        schema::migrate(&mut g.conn, FAILING),
        Err(Error::Sqlite(_))
    ));
    assert_eq!(probe(&g.conn), (3, 0));
    drop(g);
    assert_eq!(probe(&Connection::open(&path).unwrap()), (3, 0));
}

// 6. Replay 9: a write that meets another writer's lock fails with Busy at
// once, and is not retried.
#[test]
fn busy_write_fails_without_writing() {
    let (_dir, path, g) = new_garage();
    drop(g);
    let mut b = Garage::open(
        &path,
        clock(),
        &Options {
            busy_timeout: Duration::ZERO,
        },
    )
    .unwrap();
    let a = Connection::open(&path).unwrap();
    a.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        b.record_writer().add_vehicle("Late"),
        Err(Error::Busy)
    ));
    a.execute_batch("ROLLBACK").unwrap();
    assert_eq!(count(&b.conn, "vehicle"), 0);
}

// 7. Durability and version.
#[test]
fn durable_connection_and_sqlite_floor() {
    let (_dir, path, g) = new_garage();
    drop(g);
    let options = Options {
        busy_timeout: Duration::from_millis(1234),
    };
    let g = Garage::open(&path, clock(), &options).unwrap();
    let get = |name: &str| -> String {
        g.conn
            .query_row(&format!("SELECT * FROM pragma_{name}"), [], |r| {
                r.get::<_, rusqlite::types::Value>(0)
            })
            .map(|v| match v {
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Text(s) => s,
                other => format!("{other:?}"),
            })
            .unwrap()
    };
    assert_eq!(get("synchronous"), "2");
    assert_eq!(get("foreign_keys"), "1");
    assert_eq!(get("journal_mode"), "wal");
    assert_eq!(get("busy_timeout"), "1234");
    let version: String = g
        .conn
        .query_row("SELECT sqlite_version()", [], |r| r.get(0))
        .unwrap();
    eprintln!("runtime SQLite {version}");
    assert!(
        sqlite_version_ok(&version),
        "runtime SQLite {version} is below {MIN_SQLITE_STR}"
    );

    for (v, ok) in [
        ("3.51.2", false),
        ("3.51.3", true),
        ("3.54.1", true),
        ("3.50.7", false),
        ("4.0", true),
        ("x", false),
    ] {
        assert_eq!(sqlite_version_ok(v), ok, "{v}");
    }
}

// Review round 1, finding 1: an out-of-range busy timeout is an error, never
// a panic, and is refused before anything touches the filesystem.
#[test]
fn busy_timeout_out_of_range_refused_before_any_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("garage.db");
    for bad in [Duration::MAX, Duration::from_millis(i32::MAX as u64 + 1)] {
        let options = Options { busy_timeout: bad };
        assert!(matches!(
            Garage::create(&path, clock(), &options),
            Err(Error::BusyTimeoutOutOfRange(d)) if d == bad
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "{bad:?}");
    }

    // The largest value SQLite takes is accepted and read back.
    let max = Options {
        busy_timeout: Duration::from_millis(i32::MAX as u64),
    };
    drop(Garage::create(&path, clock(), &max).unwrap());
    let before = fs::read(&path).unwrap();
    let too_big = Options {
        busy_timeout: Duration::MAX,
    };
    assert!(matches!(
        Garage::open(&path, clock(), &too_big),
        Err(Error::BusyTimeoutOutOfRange(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(sidecars_absent(&path));
}

// Review round 1, finding 2: full Unicode case folding, the same for stored
// identifiers and incoming references.
#[test]
fn unicode_case_folding_for_identifiers_and_references() {
    let (_dir, _path, mut g) = new_garage();
    let street = g.record_writer().add_vehicle("Straße").unwrap();
    for reference in ["STRASSE", "strasse", "Straße", "  STRAßE "] {
        let r = resolved(g.resolve(reference).unwrap());
        assert_eq!(r.vehicle_id, street.id, "{reference}");
    }
    // Final sigma folds like medial sigma.
    let greek = g.record_writer().add_vehicle("ΣΊΣΥΦΟΣ").unwrap();
    assert_eq!(resolved(g.resolve("σίσυφος").unwrap()).vehicle_id, greek.id);

    // The fold also drives the duplicate-alias check.
    resolved(
        g.record_writer()
            .add_identifier("strasse", IdentKind::Alias, "Weiß")
            .unwrap(),
    );
    let idents = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("σίσυφος", IdentKind::Alias, "WEISS"),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), idents);
}

/// A clock whose `now` a test can move, to tell rows apart by time.
pub(crate) struct SteppingClock(pub(crate) std::sync::Mutex<Timestamp>);

impl SteppingClock {
    pub(crate) fn set(&self, ts: &str) {
        *self.0.lock().unwrap() = ts.parse().unwrap();
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap()
    }
    fn today(&self) -> Date {
        TODAY
    }
}

// Review round 1, finding 3: resolution results and ambiguity candidates
// carry provenance, for the anchor and for each identifier, from the right row.
#[test]
fn resolution_carries_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(std::sync::Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    let at = |ts: &str| Provenance {
        source: Source::MaintainerEntry,
        recorded_at: ts.parse().unwrap(),
        diagwire_rev: REVISION.into(),
    };
    let (t1, t2, t3, t4) = (
        "2026-01-01T00:00:00Z",
        "2026-02-01T00:00:00Z",
        "2026-03-01T00:00:00Z",
        "2026-04-01T00:00:00Z",
    );
    clock.set(t1);
    let coupe = g.record_writer().add_vehicle("Coupe").unwrap();
    clock.set(t2);
    resolved(
        g.record_writer()
            .add_identifier("coupe", IdentKind::Alias, "the coupe")
            .unwrap(),
    );
    clock.set(t3);
    let sedan = g.record_writer().add_vehicle("Sedan").unwrap();
    clock.set(t4);
    for v in ["coupe", "sedan"] {
        resolved(
            g.record_writer()
                .add_identifier(v, IdentKind::Chassis, "E46")
                .unwrap(),
        );
    }

    let r = resolved(g.resolve("The Coupe").unwrap());
    assert_eq!(r.vehicle_id, coupe.id);
    assert_eq!(r.provenance, at(t1), "anchor provenance");
    assert_eq!(r.label.as_ref().map(|l| &l.provenance), Some(&at(t1)));
    assert_eq!(
        r.matched
            .iter()
            .map(|m| (m.kind, m.value.as_str(), &m.provenance))
            .collect::<Vec<_>>(),
        [(IdentKind::Alias, "the coupe", &at(t2))]
    );

    let Outcome::Ambiguous(candidates) = g.resolve("e46").unwrap() else {
        panic!("expected Ambiguous");
    };
    let summary: Vec<_> = candidates
        .iter()
        .map(|c| {
            (
                c.vehicle_id,
                c.provenance.clone(),
                c.label.as_ref().map(|l| l.provenance.clone()),
                c.matched
                    .iter()
                    .map(|m| m.provenance.clone())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (coupe.id, at(t1), Some(at(t1)), vec![at(t4)]),
            (sedan.id, at(t3), Some(at(t3)), vec![at(t4)]),
        ]
    );
}

// Review round 1, finding 4: a dangling symlink at a sidecar (or the main
// path) is still a directory entry, so `create` refuses.
#[cfg(unix)]
fn dangling_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn dangling_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[test]
fn create_refuses_dangling_symlink_entries() {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("garage.db");
        let link = PathBuf::from(format!("{}{suffix}", path.display()));
        if let Err(e) = dangling_symlink(&dir.path().join("nowhere"), &link) {
            // Written to stderr directly, which libtest does not capture, so
            // the skip shows in normal `cargo test` output.
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED create_refuses_dangling_symlink_entries: cannot create a symlink \
                 here ({e}); on Windows this needs Developer Mode or \
                 SeCreateSymbolicLinkPrivilege"
            );
            return;
        }
        assert!(!link.exists(), "the link must dangle");
        assert!(
            matches!(
                Garage::create(&path, clock(), &Options::default()),
                Err(Error::Exists)
            ),
            "{suffix:?}"
        );
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, [link.file_name().unwrap()], "{suffix:?}");
    }
}
