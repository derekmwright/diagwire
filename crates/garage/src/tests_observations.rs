//! Second-slice tests: sessions, fault snapshots and the latest-measured-
//! faults query (DR-0002 G2, G4, G5, replays 3 and 5). Temp databases only
//! (G9), injected clock.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::*;
use crate::tests::{
    NOW, NOW_STORED, SteppingClock, count, dump, is_constraint, new_garage, oil, resolved,
};

const OBSERVATION_TABLES: [&str; 3] = ["session", "fault_snapshot", "fault_dtc"];

const T1: &str = "2026-05-01T10:00:00Z";
const T2: &str = "2026-05-02T10:00:00Z";
const T3: &str = "2026-05-03T10:00:00Z";

fn dtc(code: u32, status: u8) -> Dtc {
    Dtc { code, status }
}

fn measured(recorded_at: &str) -> Provenance {
    Provenance {
        source: Source::DiagwireRead,
        recorded_at: recorded_at.parse().unwrap(),
        diagwire_rev: REVISION.into(),
    }
}

fn start(g: &mut Garage, reference: &str) -> Session {
    resolved(
        g.observation_writer()
            .start_session(reference, &SessionInput::default())
            .unwrap(),
    )
}

fn observation_counts(conn: &Connection) -> [i64; 3] {
    OBSERVATION_TABLES.map(|t| count(conn, t))
}

// Provenance: the observation writer sets source, revision and time itself.
// A snapshot's provenance is its session's; `read_at` is its own.
#[test]
fn observations_carry_diagwire_read_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    let car = g.record_writer().add_vehicle("Test car").unwrap();

    clock.set(T1);
    let input = SessionInput {
        reported_vin: Some("TESTVIN0000000001".into()),
        reported_chassis: Some("G05".into()),
    };
    let session = resolved(
        g.observation_writer()
            .start_session(" TEST  car ", &input)
            .unwrap(),
    );
    assert_eq!(
        session,
        Session {
            id: session.id,
            vehicle_id: Some(car.id),
            reported_vin: input.reported_vin.clone(),
            reported_chassis: input.reported_chassis.clone(),
            identity_conflict: IdentityConflict::default(),
            provenance: measured(T1),
        }
    );

    clock.set(T2);
    let faults = FaultRead::Complete(vec![dtc(0x2A82, 0x24), dtc(0x2A87, 0x08)]);
    let complete = g
        .observation_writer()
        .record_fault_read(session.id, 0x12, &faults)
        .unwrap();
    assert_eq!(
        complete,
        FaultSnapshot {
            id: complete.id,
            session_id: session.id,
            ecu_address: 0x12,
            read_at: T2.parse().unwrap(),
            read: faults,
            identity_conflict: IdentityConflict::default(),
            provenance: measured(T1),
        }
    );
    clock.set(T3);
    let failed = g
        .observation_writer()
        .record_fault_read(session.id, 0x12, &FaultRead::NoResponse)
        .unwrap();
    assert_eq!(failed.provenance, measured(T1));
    assert_eq!(failed.read_at, T3.parse().unwrap());

    // Every stored session row, not only the returned structs.
    let rows: Vec<(String, String, String)> = g
        .conn
        .prepare("SELECT source, recorded_at, diagwire_rev FROM session")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        rows,
        [(
            "diagwire-read".to_owned(),
            "2026-05-01T10:00:00.000000Z".to_owned(),
            REVISION.to_owned()
        )]
    );

    // Backstop: storage refuses a session with a record source.
    let before = observation_counts(&g.conn);
    for source in ["maintainer-entry", "imported-note"] {
        let refused = g
            .conn
            .execute(
                "INSERT INTO session (vehicle_id, vin_conflict, chassis_conflict,
                     source, recorded_at, diagwire_rev)
                 VALUES (?1, 0, 0, ?2, ?3, 'rev')",
                rusqlite::params![car.id, source, NOW_STORED],
            )
            .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("CHECK constraint failed: source"),
            "{source}: {refused}"
        );
    }

    // A reference that names no vehicle, or more than one, writes nothing.
    g.record_writer().add_vehicle("Other car").unwrap();
    for v in ["test car", "other car"] {
        resolved(
            g.record_writer()
                .add_identifier(v, IdentKind::Chassis, "G05")
                .unwrap(),
        );
    }
    let mut w = g.observation_writer();
    assert!(matches!(
        w.start_session("g05", &input).unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert_eq!(
        w.start_session("no such car", &input).unwrap(),
        Outcome::NotFound
    );
    // A snapshot needs an existing session.
    assert!(matches!(
        w.record_fault_read(session.id + 100, 0x12, &FaultRead::NoResponse),
        Err(Error::SessionNotFound(id)) if id == session.id + 100
    ));
    assert_eq!(observation_counts(&g.conn), before);
}

// Replay 3: nothing the record writer does creates a session or snapshot,
// so a fault list a maintainer (or an import) reports stays a record and is
// never returned as a measurement.
#[test]
fn replay3_record_writer_creates_no_observation() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    w.add_vehicle("Shop car").unwrap();
    resolved(
        w.add_identifier("shop car", IdentKind::Alias, "the loaner")
            .unwrap(),
    );
    let reported = ServiceEventInput {
        summary: "Shop read faults 2A82 and 2A87 from the DME".into(),
        ..oil(None)
    };
    let first = resolved(w.add_service_event("shop car", &reported).unwrap());
    let second = resolved(
        w.supersede_service_event("shop car", first.id, &reported)
            .unwrap(),
    );
    resolved(w.retract_service_event("shop car", second.id).unwrap());

    assert_eq!(observation_counts(&g.conn), [0, 0, 0]);
    assert_eq!(
        g.latest_measured_faults("the loaner").unwrap(),
        Outcome::Resolved(vec![]),
        "nothing was measured on this vehicle"
    );
}

// Replay 3, structurally: `RecordWriter` and everything it calls live in
// `records.rs`, which names no observation table and no observation code.
#[test]
fn replay3_record_writer_source_names_no_observation_table() {
    let records = include_str!("records.rs");
    for word in [
        "session",
        "fault_snapshot",
        "fault_dtc",
        "observations",
        "ObservationWriter",
    ] {
        assert!(!records.contains(word), "records.rs mentions {word}");
    }
}

// Replay 5: a failure between fault rows leaves no part of the snapshot.
#[test]
fn replay5_failure_between_fault_rows_leaves_no_snapshot() {
    let (_dir, path, mut g) = new_garage();
    g.record_writer().add_vehicle("Test car").unwrap();
    let session = start(&mut g, "test car");
    let read = FaultRead::Complete(vec![dtc(0x1111, 1), dtc(0x2222, 2), dtc(0x3333, 3)]);

    // Fails the second fault row, after the snapshot row and the first
    // fault row have been inserted.
    g.conn
        .execute_batch(
            "CREATE TEMP TRIGGER injected_failure BEFORE INSERT ON fault_dtc
             WHEN NEW.code = 8738
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    let result = g
        .observation_writer()
        .record_fault_read(session.id, 0x12, &read);
    assert!(
        matches!(&result, Err(Error::Sqlite(e)) if e.to_string().contains("injected failure")),
        "{result:?}"
    );
    assert_eq!(observation_counts(&g.conn), [1, 0, 0]);
    // Another connection sees the same: nothing was committed.
    assert_eq!(
        observation_counts(&Connection::open(&path).unwrap()),
        [1, 0, 0]
    );
    assert_eq!(
        g.latest_measured_faults("test car").unwrap(),
        Outcome::Resolved(vec![])
    );

    // Without the injected failure the same read is stored whole.
    g.conn
        .execute_batch("DROP TRIGGER injected_failure")
        .unwrap();
    let stored = g
        .observation_writer()
        .record_fault_read(session.id, 0x12, &read)
        .unwrap();
    assert_eq!(stored.read, read);
    assert_eq!(observation_counts(&g.conn), [1, 1, 3]);
}

// A failed read is not "no faults": only a complete read has a fault list,
// and the query returns the newest complete read together with the attempts
// made after it.
#[test]
fn failed_read_is_distinct_from_no_faults() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    g.record_writer().add_vehicle("Test car").unwrap();
    g.record_writer().add_vehicle("Other car").unwrap();
    let session = start(&mut g, "test car");
    let other = start(&mut g, "other car");
    let some_faults = FaultRead::Complete(vec![dtc(0x2A82, 0x24)]);
    let mut record = |at: &str, session: &Session, ecu: u8, read: &FaultRead| {
        clock.set(at);
        let snapshot = g
            .observation_writer()
            .record_fault_read(session.id, ecu, read)
            .unwrap();
        assert_eq!(&snapshot.read, read);
        assert_eq!(snapshot.read_at, at.parse().unwrap());
        snapshot
    };

    // 0x12: read complete with no faults, then a later attempt got no reply.
    let empty_12 = record(T1, &session, 0x12, &FaultRead::Complete(vec![]));
    let failed_12 = record(T2, &session, 0x12, &FaultRead::NoResponse);
    // 0x18: never read completely.
    let refused_18 = record(T1, &session, 0x18, &FaultRead::Negative(0x31));
    // 0x29: faults, then a newer complete read with none.
    record(T1, &session, 0x29, &some_faults);
    let empty_29 = record(T3, &session, 0x29, &FaultRead::Complete(vec![]));
    // 0x40: faults, then two failed attempts.
    let faults_40 = record(T1, &session, 0x40, &some_faults);
    let malformed_40 = record(T2, &session, 0x40, &FaultRead::Malformed);
    let expired_40 = record(T3, &session, 0x40, &FaultRead::PendingExpired);
    // Another vehicle's newer reads of the same ECUs change nothing here.
    let other_12 = record(T3, &other, 0x12, &some_faults);

    // The stored rows differ: a failed read has its failure kind and no
    // fault rows; the empty complete read is 'complete' with no fault rows.
    let stored: Vec<(i64, String, Option<u8>, i64)> = g
        .conn
        .prepare(
            "SELECT f.id, f.outcome, f.nrc,
                    (SELECT count(*) FROM fault_dtc d WHERE d.snapshot_id = f.id)
             FROM fault_snapshot f WHERE f.id IN (?1, ?2, ?3, ?4, ?5) ORDER BY f.id",
        )
        .unwrap()
        .query_map(
            [
                empty_12.id,
                failed_12.id,
                refused_18.id,
                faults_40.id,
                malformed_40.id,
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        stored,
        [
            (empty_12.id, "complete".to_owned(), None, 0),
            (failed_12.id, "no-response".to_owned(), None, 0),
            (refused_18.id, "negative".to_owned(), Some(0x31), 0),
            (faults_40.id, "complete".to_owned(), None, 1),
            (malformed_40.id, "malformed".to_owned(), None, 0),
        ]
    );
    assert_ne!(empty_12.read, failed_12.read);

    assert_eq!(
        g.latest_measured_faults("test car").unwrap(),
        Outcome::Resolved(vec![
            EcuFaults {
                ecu_address: 0x12,
                latest_complete: Some(empty_12),
                newer_attempts: vec![failed_12],
            },
            EcuFaults {
                ecu_address: 0x18,
                latest_complete: None,
                newer_attempts: vec![refused_18],
            },
            EcuFaults {
                ecu_address: 0x29,
                latest_complete: Some(empty_29),
                newer_attempts: vec![],
            },
            EcuFaults {
                ecu_address: 0x40,
                latest_complete: Some(faults_40),
                newer_attempts: vec![malformed_40, expired_40],
            },
        ])
    );
    assert_eq!(
        g.latest_measured_faults("other car").unwrap(),
        Outcome::Resolved(vec![EcuFaults {
            ecu_address: 0x12,
            latest_complete: Some(other_12),
            newer_attempts: vec![],
        }])
    );
}

/// One session with a complete read (one fault) and a failed read.
fn observed_garage() -> (tempfile::TempDir, Garage, FaultSnapshot, FaultSnapshot) {
    let (dir, _path, mut g) = new_garage();
    g.record_writer().add_vehicle("Test car").unwrap();
    let session = start(&mut g, "test car");
    let mut w = g.observation_writer();
    let complete = w
        .record_fault_read(
            session.id,
            0x12,
            &FaultRead::Complete(vec![dtc(0x2A82, 0x24)]),
        )
        .unwrap();
    let failed = w
        .record_fault_read(session.id, 0x12, &FaultRead::NoResponse)
        .unwrap();
    (dir, g, complete, failed)
}

// Append-only: storage rejects edits and deletes on the observation tables.
#[test]
fn observation_tables_are_append_only() {
    let (_dir, mut g, _complete, _failed) = observed_garage();
    // A session nothing refers to, so that only the trigger (and no foreign
    // key) stands between it and a DELETE.
    let lone = start(&mut g, "test car");
    assert_eq!(observation_counts(&g.conn), [2, 2, 1]);
    let before = OBSERVATION_TABLES.map(|t| dump(&g.conn, t));
    let refused = g
        .conn
        .execute("DELETE FROM session WHERE id = ?1", [lone.id])
        .unwrap_err();
    assert!(
        refused.to_string().contains("append-only"),
        "the lone session's DELETE was refused by something else: {refused}"
    );
    for sql in [
        "UPDATE session SET reported_vin = 'edited'",
        "DELETE FROM session",
        "UPDATE fault_snapshot SET outcome = 'complete'",
        "DELETE FROM fault_snapshot",
        "UPDATE fault_dtc SET status = 0",
        "DELETE FROM fault_dtc",
    ] {
        assert!(
            is_constraint(g.conn.execute(sql, [])),
            "{sql} was not refused"
        );
    }
    assert_eq!(OBSERVATION_TABLES.map(|t| dump(&g.conn, t)), before);
}

// Backstop: storage refuses fault rows under a failed read, and an outcome
// whose NRC column doesn't fit it.
#[test]
fn storage_refuses_faults_under_a_failed_read() {
    let (_dir, g, complete, failed) = observed_garage();
    let before = observation_counts(&g.conn);
    assert!(is_constraint(g.conn.execute(
        "INSERT INTO fault_dtc (snapshot_id, code, status) VALUES (?1, 1, 1)",
        [failed.id],
    )));
    for (outcome, nrc) in [
        ("negative", None),
        ("complete", Some(0x31)),
        ("unknown", None),
    ] {
        assert!(
            is_constraint(g.conn.execute(
                "INSERT INTO fault_snapshot (session_id, ecu_address, outcome, nrc, read_at)
                 VALUES (?1, 18, ?2, ?3, ?4)",
                rusqlite::params![complete.session_id, outcome, nrc, NOW_STORED],
            )),
            "{outcome} {nrc:?}"
        );
    }
    assert_eq!(observation_counts(&g.conn), before);
}

// A session bound to no vehicle (the column allows it; only raw SQL can
// make one today) is stored and can hold reads, and is returned for no
// vehicle.
#[test]
fn unbound_session_belongs_to_no_vehicle() {
    let (_dir, _path, mut g) = new_garage();
    g.record_writer().add_vehicle("Test car").unwrap();
    let bound = start(&mut g, "test car");
    g.conn
        .execute(
            "INSERT INTO session (vehicle_id, reported_vin, vin_conflict, chassis_conflict,
                 source, recorded_at, diagwire_rev)
             VALUES (NULL, 'TESTVIN0000000009', 0, 0, 'diagwire-read', ?1, ?2)",
            [NOW_STORED, REVISION],
        )
        .unwrap();
    let unbound = g.conn.last_insert_rowid();

    let faults = FaultRead::Complete(vec![dtc(0x2A82, 0x24)]);
    let in_unbound = g
        .observation_writer()
        .record_fault_read(unbound, 0x12, &faults)
        .unwrap();
    assert_eq!(
        in_unbound,
        FaultSnapshot {
            id: in_unbound.id,
            session_id: unbound,
            ecu_address: 0x12,
            read_at: NOW.parse().unwrap(),
            read: faults,
            identity_conflict: IdentityConflict::default(),
            provenance: measured(NOW),
        }
    );
    assert_eq!(observation_counts(&g.conn), [2, 1, 1]);

    // Nothing was measured on the vehicle, and a later read of the same ECU
    // on it does not pick up the unbound session's complete read.
    assert_eq!(
        g.latest_measured_faults("test car").unwrap(),
        Outcome::Resolved(vec![])
    );
    let in_bound = g
        .observation_writer()
        .record_fault_read(bound.id, 0x12, &FaultRead::NoResponse)
        .unwrap();
    assert_eq!(
        g.latest_measured_faults("test car").unwrap(),
        Outcome::Resolved(vec![EcuFaults {
            ecu_address: 0x12,
            latest_complete: None,
            newer_attempts: vec![in_bound],
        }])
    );
}
// G7, replay 6: a reported VIN or chassis that disagrees with the vehicle's
// stored one is flagged on the session and on what was read in it. The
// session is still written and the vehicle's record is untouched.
#[test]
fn replay6_identity_conflict_is_flagged_and_changes_nothing() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    let known = w.add_vehicle("Known car").unwrap();
    resolved(
        w.add_identifier("known car", IdentKind::Vin, "TESTVIN0000000001")
            .unwrap(),
    );
    resolved(
        w.add_identifier("known car", IdentKind::Chassis, "E46")
            .unwrap(),
    );
    // No stored VIN or chassis to disagree with.
    w.add_vehicle("Bare car").unwrap();
    let vehicles = ["vehicle", "vehicle_ident"].map(|t| dump(&g.conn, t));

    let report = |vin: Option<&str>, chassis: Option<&str>| SessionInput {
        reported_vin: vin.map(Into::into),
        reported_chassis: chassis.map(Into::into),
    };
    let flags = |vin, chassis| IdentityConflict { vin, chassis };
    let cases = [
        // (vehicle, reported, expected flags)
        (
            "known car",
            report(Some("TESTVIN0000000002"), None),
            flags(true, false),
        ),
        (
            "known car",
            report(Some(" testvin0000000001 "), Some("E90")),
            flags(false, true),
        ),
        (
            "known car",
            report(Some("OTHERVIN000000009"), Some("G05")),
            flags(true, true),
        ),
        (
            "known car",
            report(Some("TestVin0000000001"), Some(" e46")),
            flags(false, false),
        ),
        ("known car", report(None, None), flags(false, false)),
        (
            "known car",
            report(Some("  "), Some("")),
            flags(false, false),
        ),
        (
            "bare car",
            report(Some("TESTVIN0000000002"), Some("E90")),
            flags(false, false),
        ),
    ];
    let mut sessions = Vec::new();
    for (reference, input, expected) in &cases {
        let session = resolved(
            g.observation_writer()
                .start_session(reference, input)
                .unwrap(),
        );
        assert_eq!(
            session.identity_conflict, *expected,
            "{reference} {input:?}"
        );
        assert_eq!(
            session.identity_conflict.any(),
            expected.vin || expected.chassis
        );
        // Stored as reported, not normalised and not corrected.
        assert_eq!(session.reported_vin, input.reported_vin);
        assert_eq!(session.reported_chassis, input.reported_chassis);
        sessions.push(session);
    }

    // Every session was written, with the flags stored on its row.
    let stored: Vec<(i64, bool, bool)> = g
        .conn
        .prepare("SELECT id, vin_conflict, chassis_conflict FROM session ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let expected: Vec<(i64, bool, bool)> = sessions
        .iter()
        .zip(&cases)
        .map(|(s, (_, _, f))| (s.id, f.vin, f.chassis))
        .collect();
    assert_eq!(stored, expected);

    // What is read in a flagged session carries the flag, from the writer
    // and from the query; a clean session's reads carry none.
    let flagged = &sessions[2];
    let clean = &sessions[3];
    let mut w = g.observation_writer();
    let in_flagged = w
        .record_fault_read(flagged.id, 0x12, &FaultRead::Complete(vec![]))
        .unwrap();
    let in_clean = w
        .record_fault_read(clean.id, 0x18, &FaultRead::NoResponse)
        .unwrap();
    assert_eq!(in_flagged.identity_conflict, flags(true, true));
    assert_eq!(in_clean.identity_conflict, flags(false, false));
    assert_eq!(
        g.latest_measured_faults("known car").unwrap(),
        Outcome::Resolved(vec![
            EcuFaults {
                ecu_address: 0x12,
                latest_complete: Some(in_flagged),
                newer_attempts: vec![],
            },
            EcuFaults {
                ecu_address: 0x18,
                latest_complete: None,
                newer_attempts: vec![in_clean],
            },
        ])
    );

    // The vehicle record is unchanged, and still resolves by its own VIN.
    assert_eq!(
        ["vehicle", "vehicle_ident"].map(|t| dump(&g.conn, t)),
        vehicles
    );
    assert_eq!(
        resolved(g.resolve("TESTVIN0000000001").unwrap()).vehicle_id,
        known.id
    );
    assert_eq!(g.resolve("TESTVIN0000000002").unwrap(), Outcome::NotFound);
}

// Ruling 5: "newest" is the order reads were written in (row id), not the
// order of their timestamps. The time is taken before the write lock, so
// two writers can commit out of timestamp order.
#[test]
fn newest_is_write_order_not_timestamp_order() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    g.record_writer().add_vehicle("Test car").unwrap();
    let session = start(&mut g, "test car");
    let mut record = |at: &str, read: FaultRead| {
        clock.set(at);
        g.observation_writer()
            .record_fault_read(session.id, 0x12, &read)
            .unwrap()
    };
    // Written first, with the latest timestamp.
    let first = record(T3, FaultRead::Complete(vec![dtc(0x2A82, 0x24)]));
    let second = record(T2, FaultRead::NoResponse);
    // Written last, with the earliest timestamp.
    let last = record(T1, FaultRead::Complete(vec![]));
    assert!(first.id < second.id && second.id < last.id);
    assert!(first.read_at > second.read_at && second.read_at > last.read_at);

    assert_eq!(
        g.latest_measured_faults("test car").unwrap(),
        Outcome::Resolved(vec![EcuFaults {
            ecu_address: 0x12,
            latest_complete: Some(last),
            newer_attempts: vec![],
        }])
    );
}
