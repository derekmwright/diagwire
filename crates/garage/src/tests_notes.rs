//! Third-slice tests: notes (DR-0002 G4, G5 and amendment A1). Temp
//! databases only (G9), injected clock.

use std::sync::{Arc, Mutex};

use jiff::civil::{Date, date};

use super::*;
use crate::tests::{
    NOW, NOW_STORED, SteppingClock, TODAY, count, dump, is_constraint, new_garage, resolved,
};

fn note(about_date: Option<Date>, body: &str) -> NoteInput {
    NoteInput {
        about_date,
        body: body.into(),
    }
}

fn ids(notes: &[Note]) -> Vec<i64> {
    notes.iter().map(|n| n.id).collect()
}

/// `(id, supersedes_id, retracted)` of each row.
fn chain(notes: &[Note]) -> Vec<(i64, Option<i64>, bool)> {
    notes
        .iter()
        .map(|n| (n.id, n.supersedes_id, n.retracted))
        .collect()
}

// Notes are added, superseded and retracted by appending. Default queries
// hide superseded rows and retracted heads; history shows every row. A
// retraction is final (A1).
#[test]
fn notes_add_supersede_and_retract() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    let daily = w.add_vehicle("Daily").unwrap();
    let weekend = w.add_vehicle("Weekend").unwrap();

    // The stored row is returned: label, absolute date, trimmed body.
    let dated = resolved(
        w.add_note(
            " DAILY ",
            &note(Some(date(2026, 1, 2)), "  Coils are from another model "),
        )
        .unwrap(),
    );
    assert_eq!(
        dated,
        Note {
            id: dated.id,
            vehicle_id: daily.id,
            vehicle_label: daily.label.clone(),
            about_date: date(2026, 1, 2),
            body: "Coils are from another model".into(),
            supersedes_id: None,
            retracted: false,
            provenance: Provenance {
                source: Source::MaintainerEntry,
                recorded_at: NOW.parse().unwrap(),
                diagwire_rev: REVISION.into(),
            },
        }
    );
    assert!(daily.label.is_some());
    // No date given: the garage clock's local date, not the UTC date.
    let undated = resolved(
        w.add_note("daily", &note(None, "Rear tyres are new"))
            .unwrap(),
    );
    assert_eq!(undated.about_date, TODAY);
    let stored: String = g
        .conn
        .query_row(
            "SELECT about_date FROM note WHERE id = ?1",
            [undated.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, "2026-11-13");
    let other = resolved(
        g.record_writer()
            .add_note("weekend", &note(None, "Stored for winter"))
            .unwrap(),
    );
    assert_eq!(other.vehicle_id, weekend.id);
    assert_eq!(
        resolved(g.notes("daily").unwrap()),
        [dated.clone(), undated.clone()]
    );

    // Supersede adds a head; the old row is hidden by default.
    let second = resolved(
        g.record_writer()
            .supersede_note("daily", dated.id, &note(None, "Coils are from a hatchback"))
            .unwrap(),
    );
    assert_eq!(second.supersedes_id, Some(dated.id));
    assert!(!second.retracted);
    assert_eq!(
        ids(&resolved(g.notes("daily").unwrap())),
        [undated.id, second.id]
    );

    // A second supersede of the same row fails, in the API and in storage.
    let rows = count(&g.conn, "note");
    assert!(matches!(
        g.record_writer().supersede_note("daily", dated.id, &note(None, "x")),
        Err(Error::AlreadySuperseded(id)) if id == dated.id
    ));
    assert!(matches!(
        g.record_writer().retract_note("daily", dated.id),
        Err(Error::AlreadySuperseded(id)) if id == dated.id
    ));
    let refused = g
        .conn
        .execute(
            "INSERT INTO note (vehicle_id, about_date, body, supersedes_id,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, '2026-01-01', 'x', ?2, 'maintainer-entry', ?3, 'rev')",
            rusqlite::params![daily.id, dated.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("UNIQUE constraint failed: note.supersedes_id"),
        "{refused}"
    );

    // A correction stays on its target's vehicle, in the API and in
    // storage. `second` is a head, so only the same-vehicle rule refuses.
    assert!(matches!(
        g.record_writer().supersede_note("weekend", second.id, &note(None, "x")),
        Err(Error::RecordNotFound(id)) if id == second.id
    ));
    assert!(matches!(
        g.record_writer().retract_note("weekend", second.id),
        Err(Error::RecordNotFound(id)) if id == second.id
    ));
    let refused = g
        .conn
        .execute(
            "INSERT INTO note (vehicle_id, about_date, body, supersedes_id,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, '2026-01-01', 'x', ?2, 'maintainer-entry', ?3, 'rev')",
            rusqlite::params![weekend.id, second.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("supersede must stay on the same vehicle"),
        "{refused}"
    );

    // An empty note, an unknown record and a reference that names no
    // vehicle or more than one write nothing.
    for v in ["daily", "weekend"] {
        resolved(
            g.record_writer()
                .add_identifier(v, IdentKind::Chassis, "E46")
                .unwrap(),
        );
    }
    let mut w = g.record_writer();
    assert!(matches!(
        w.add_note("daily", &note(None, "  \t ")),
        Err(Error::EmptyNote)
    ));
    assert!(matches!(
        w.supersede_note("daily", second.id, &note(None, "")),
        Err(Error::EmptyNote)
    ));
    assert!(matches!(
        w.supersede_note("daily", 9999, &note(None, "x")),
        Err(Error::RecordNotFound(9999))
    ));
    assert!(matches!(
        w.add_note("e46", &note(None, "x")).unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert!(matches!(
        w.supersede_note("e46", second.id, &note(None, "x")).unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert!(matches!(
        w.retract_note("e46", second.id).unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert_eq!(
        w.add_note("no such car", &note(None, "x")).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.retract_note("no such car", second.id).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(count(&g.conn, "note"), rows);

    // A retracted head is hidden by default and shown in history. The
    // retraction row repeats its target's date and body.
    let retraction = resolved(g.record_writer().retract_note("daily", second.id).unwrap());
    assert_eq!(
        retraction,
        Note {
            id: retraction.id,
            supersedes_id: Some(second.id),
            retracted: true,
            ..second.clone()
        }
    );
    assert_eq!(ids(&resolved(g.notes("daily").unwrap())), [undated.id]);
    assert_eq!(
        chain(&resolved(g.note_history("daily").unwrap())),
        [
            (dated.id, None, false),
            (undated.id, None, false),
            (second.id, Some(dated.id), false),
            (retraction.id, Some(second.id), true),
        ]
    );

    // A retraction is final (A1): the retracted head can be neither
    // superseded nor retracted again, and nothing is written.
    let rows = count(&g.conn, "note");
    let mut w = g.record_writer();
    assert!(matches!(
        w.supersede_note("daily", retraction.id, &note(None, "Coils again")),
        Err(Error::RetractionIsFinal(id)) if id == retraction.id
    ));
    assert!(matches!(
        w.retract_note("daily", retraction.id),
        Err(Error::AlreadyRetracted(id)) if id == retraction.id
    ));
    assert!(matches!(
        w.supersede_note("daily", second.id, &note(None, "Coils again")),
        Err(Error::AlreadySuperseded(id)) if id == second.id
    ));
    assert_eq!(count(&g.conn, "note"), rows);

    // A fresh note undoes it; history keeps the retraction.
    let fresh = resolved(
        g.record_writer()
            .add_note("daily", &note(None, "Coils are from a hatchback"))
            .unwrap(),
    );
    assert_eq!(fresh.supersedes_id, None);
    assert_eq!(
        ids(&resolved(g.notes("daily").unwrap())),
        [undated.id, fresh.id]
    );
    assert_eq!(
        chain(&resolved(g.note_history("daily").unwrap())),
        [
            (dated.id, None, false),
            (undated.id, None, false),
            (second.id, Some(dated.id), false),
            (retraction.id, Some(second.id), true),
            (fresh.id, None, false),
        ]
    );
    // The other vehicle's notes were never touched.
    assert_eq!(
        resolved(g.notes("weekend").unwrap()),
        std::slice::from_ref(&other)
    );
    assert_eq!(resolved(g.note_history("weekend").unwrap()), [other]);
}

// Provenance (G4): the record writer sets source, revision and time on
// every note row, and every note read returns that row's own provenance.
#[test]
fn notes_carry_maintainer_entry_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    let at = |ts: &str| Provenance {
        source: Source::MaintainerEntry,
        recorded_at: ts.parse().unwrap(),
        diagwire_rev: REVISION.into(),
    };
    let (t1, t2, t3) = (
        "2026-01-01T00:00:00Z",
        "2026-02-01T00:00:00Z",
        "2026-03-01T00:00:00Z",
    );
    let car = g.record_writer().add_vehicle("Test car").unwrap();
    clock.set(t1);
    let first = resolved(
        g.record_writer()
            .add_note("test car", &note(None, "First"))
            .unwrap(),
    );
    clock.set(t2);
    let second = resolved(
        g.record_writer()
            .supersede_note("test car", first.id, &note(None, "Second"))
            .unwrap(),
    );
    let kept = resolved(
        g.record_writer()
            .add_note("test car", &note(None, "Kept"))
            .unwrap(),
    );
    clock.set(t3);
    let gone = resolved(
        g.record_writer()
            .retract_note("test car", second.id)
            .unwrap(),
    );

    // From the writer.
    assert_eq!(
        [
            &first.provenance,
            &second.provenance,
            &kept.provenance,
            &gone.provenance
        ],
        [&at(t1), &at(t2), &at(t2), &at(t3)]
    );
    // From the reads: each row's own provenance, and the label's.
    let live = resolved(g.notes("test car").unwrap());
    assert_eq!(live, std::slice::from_ref(&kept));
    assert_eq!(live[0].provenance, at(t2));
    assert_eq!(
        live[0].vehicle_label.as_ref().map(|l| &l.provenance),
        car.label.as_ref().map(|l| &l.provenance)
    );
    assert!(live[0].vehicle_label.is_some());
    let history = resolved(g.note_history("test car").unwrap());
    assert_eq!(
        history
            .iter()
            .map(|n| (n.id, n.provenance.clone()))
            .collect::<Vec<_>>(),
        [
            (first.id, at(t1)),
            (second.id, at(t2)),
            (kept.id, at(t2)),
            (gone.id, at(t3)),
        ]
    );

    // Every stored row, not only the returned structs.
    let rows: Vec<(String, String, String)> = g
        .conn
        .prepare("SELECT source, recorded_at, diagwire_rev FROM note ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let stored = |ts: &str| {
        (
            "maintainer-entry".to_owned(),
            ts.to_owned(),
            REVISION.to_owned(),
        )
    };
    assert_eq!(
        rows,
        [
            stored("2026-01-01T00:00:00.000000Z"),
            stored("2026-02-01T00:00:00.000000Z"),
            stored("2026-02-01T00:00:00.000000Z"),
            stored("2026-03-01T00:00:00.000000Z"),
        ]
    );

    // Backstop: storage refuses a note with a measured source. Every other
    // column is valid, so only the source CHECK can refuse it.
    let before = count(&g.conn, "note");
    let refused = g
        .conn
        .execute(
            "INSERT INTO note (vehicle_id, about_date, body, source, recorded_at, diagwire_rev)
             VALUES (?1, '2026-01-01', 'x', 'diagwire-read', ?2, 'rev')",
            rusqlite::params![car.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("CHECK constraint failed: source"),
        "{refused}"
    );
    assert_eq!(count(&g.conn, "note"), before);
}

// Append-only: storage rejects edits and deletes of notes.
#[test]
fn note_table_is_append_only() {
    let (_dir, _path, mut g) = new_garage();
    let mut w = g.record_writer();
    w.add_vehicle("Test car").unwrap();
    let first = resolved(w.add_note("test car", &note(None, "First")).unwrap());
    resolved(
        w.supersede_note("test car", first.id, &note(None, "Second"))
            .unwrap(),
    );
    // A head nothing refers to, so that only the trigger (and no foreign
    // key from a superseding row) stands between it and a DELETE.
    let lone = resolved(w.add_note("test car", &note(None, "Lone")).unwrap());
    let before = dump(&g.conn, "note");
    assert_eq!(before.len(), 3);

    for sql in [
        "DELETE FROM note WHERE id = ?1",
        "UPDATE note SET body = 'edited' WHERE id = ?1",
    ] {
        let refused = g.conn.execute(sql, [lone.id]).unwrap_err();
        assert!(
            refused.to_string().contains("append-only"),
            "{sql} was refused by something else: {refused}"
        );
    }
    for sql in [
        "UPDATE note SET body = 'edited'",
        "UPDATE note SET vehicle_id = vehicle_id",
        "DELETE FROM note",
    ] {
        assert!(
            is_constraint(g.conn.execute(sql, [])),
            "{sql} was not refused"
        );
    }
    assert_eq!(dump(&g.conn, "note"), before);
}
