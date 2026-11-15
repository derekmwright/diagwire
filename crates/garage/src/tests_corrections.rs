//! Third-slice tests: identifier correction and vehicle retraction
//! (DR-0002 G4, G5, G6, G7 and the tests amendment A1 owes). Temp databases
//! only (G9), injected clock.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::*;
use crate::tests::{
    NOW, NOW_STORED, SteppingClock, count, dump, is_constraint, label_of, new_garage, oil, resolved,
};

const RECORD_TABLES: [&str; 5] = [
    "vehicle",
    "vehicle_ident",
    "service_event",
    "note",
    "vehicle_retraction",
];
const ALL_TABLES: [&str; 8] = [
    "vehicle",
    "vehicle_ident",
    "service_event",
    "note",
    "vehicle_retraction",
    "session",
    "fault_snapshot",
    "fault_dtc",
];

fn counts<const N: usize>(conn: &Connection, tables: [&str; N]) -> [i64; N] {
    tables.map(|t| count(conn, t))
}

fn note(body: &str) -> NoteInput {
    NoteInput {
        about_date: None,
        body: body.into(),
    }
}

fn add(g: &mut Garage, reference: &str, kind: IdentKind, value: &str) -> Identifier {
    resolved(
        g.record_writer()
            .add_identifier(reference, kind, value)
            .unwrap(),
    )
}

/// `(id, supersedes_id, retracted)` of each row.
fn chain(idents: &[Identifier]) -> Vec<(i64, Option<i64>, bool)> {
    idents
        .iter()
        .map(|i| (i.id, i.supersedes_id, i.retracted))
        .collect()
}

fn ident_ids(idents: &[Identifier]) -> Vec<i64> {
    idents.iter().map(|i| i.id).collect()
}

fn vehicle_ids(vehicles: &[Vehicle]) -> Vec<i64> {
    vehicles.iter().map(|v| v.id).collect()
}

fn resolves_to(g: &Garage, reference: &str) -> Option<i64> {
    match g.resolve(reference).unwrap() {
        Outcome::Resolved(r) => Some(r.vehicle_id),
        Outcome::NotFound => None,
        Outcome::Ambiguous(c) => panic!("{reference} is ambiguous: {c:?}"),
    }
}

// Superseding an identifier appends a head of the same kind on the same
// vehicle. The old value stops resolving, the new one resolves, and history
// keeps both. The same-vehicle, single-head and uniqueness rules refuse
// without writing.
#[test]
fn identifier_supersede_corrects_what_resolves() {
    let (_dir, _path, mut g) = new_garage();
    let coupe = g.record_writer().add_vehicle("Silver coupe").unwrap();
    let sedan = g.record_writer().add_vehicle("Blue sedan").unwrap();
    let label = coupe.label.clone().unwrap();
    let alias = add(&mut g, "silver coupe", IdentKind::Alias, "the coupe");
    let sedan_alias = add(&mut g, "blue sedan", IdentKind::Alias, "the sedan");

    // Changing the label.
    let new_label = resolved(
        g.record_writer()
            .supersede_identifier("silver coupe", label.id, "  Grey   coupe ")
            .unwrap(),
    );
    assert_eq!(
        new_label,
        Identifier {
            id: new_label.id,
            vehicle_id: coupe.id,
            kind: IdentKind::Label,
            value: "Grey   coupe".into(),
            supersedes_id: Some(label.id),
            retracted: false,
            provenance: label.provenance.clone(),
        }
    );
    assert_eq!(resolves_to(&g, "silver coupe"), None);
    assert_eq!(resolves_to(&g, "GREY coupe"), Some(coupe.id));
    let now = resolved(g.vehicle("the coupe").unwrap());
    assert_eq!(label_of(&now.label), Some("Grey   coupe"));
    assert_eq!(ident_ids(&now.identifiers), [alias.id, new_label.id]);
    assert_eq!(
        chain(&resolved(g.identifier_history("the coupe").unwrap())),
        [
            (label.id, None, false),
            (alias.id, None, false),
            (new_label.id, Some(label.id), false),
        ]
    );

    // An alias keeps its kind when corrected.
    let new_alias = resolved(
        g.record_writer()
            .supersede_identifier("the coupe", alias.id, "the grey one")
            .unwrap(),
    );
    assert_eq!(new_alias.kind, IdentKind::Alias);
    assert_eq!(resolves_to(&g, "the coupe"), None);
    assert_eq!(resolves_to(&g, "The Grey One"), Some(coupe.id));

    let rows = count(&g.conn, "vehicle_ident");
    // A second supersede of the same row fails, in the API and in storage.
    assert!(matches!(
        g.record_writer().supersede_identifier("grey coupe", label.id, "x"),
        Err(Error::AlreadySuperseded(id)) if id == label.id
    ));
    assert!(matches!(
        g.record_writer().retract_identifier("grey coupe", label.id),
        Err(Error::AlreadySuperseded(id)) if id == label.id
    ));
    let refused = g
        .conn
        .execute(
            "INSERT INTO vehicle_ident (vehicle_id, kind, value, value_norm, supersedes_id,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, 'label', 'x', 'x', ?2, 'maintainer-entry', ?3, 'rev')",
            rusqlite::params![coupe.id, label.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("UNIQUE constraint failed: vehicle_ident.supersedes_id"),
        "{refused}"
    );

    // A correction stays on its target's vehicle, in the API and in
    // storage. `new_alias` is a head, so only the same-vehicle rule refuses.
    assert!(matches!(
        g.record_writer().supersede_identifier("blue sedan", new_alias.id, "x"),
        Err(Error::RecordNotFound(id)) if id == new_alias.id
    ));
    assert!(matches!(
        g.record_writer().retract_identifier("blue sedan", new_alias.id),
        Err(Error::RecordNotFound(id)) if id == new_alias.id
    ));
    let refused = g
        .conn
        .execute(
            "INSERT INTO vehicle_ident (vehicle_id, kind, value, value_norm, supersedes_id,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, 'alias', 'x', 'x', ?2, 'maintainer-entry', ?3, 'rev')",
            rusqlite::params![sedan.id, new_alias.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("supersede must stay on the same vehicle"),
        "{refused}"
    );

    // A corrected value that equals another vehicle's live alias is
    // refused, whatever the kind of the identifier being corrected.
    assert!(matches!(
        g.record_writer()
            .supersede_identifier("grey coupe", new_alias.id, " THE  Sedan "),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert!(matches!(
        g.record_writer()
            .supersede_identifier("grey coupe", new_label.id, "the sedan"),
        Err(Error::IdentifierTaken(IdentKind::Label))
    ));

    // An empty value, an unknown record, and a reference that names no
    // vehicle or more than one.
    add(&mut g, "grey coupe", IdentKind::Chassis, "E46");
    let sedan_chassis = add(&mut g, "blue sedan", IdentKind::Chassis, "E46");
    let rows = rows + 2;
    let mut w = g.record_writer();
    assert!(matches!(
        w.supersede_identifier("grey coupe", new_alias.id, "  "),
        Err(Error::EmptyIdentifier)
    ));
    assert!(matches!(
        w.supersede_identifier("grey coupe", 9999, "x"),
        Err(Error::RecordNotFound(9999))
    ));
    assert!(matches!(
        w.supersede_identifier("e46", new_alias.id, "x").unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert!(matches!(
        w.retract_identifier("e46", new_alias.id).unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert_eq!(
        w.supersede_identifier("no such car", new_alias.id, "x")
            .unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.retract_identifier("no such car", new_alias.id).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
    assert_eq!(resolves_to(&g, "the sedan"), Some(sedan.id));
    assert_eq!(
        ident_ids(&resolved(g.vehicle("blue sedan").unwrap()).identifiers),
        [
            sedan.label.as_ref().unwrap().id,
            sedan_alias.id,
            sedan_chassis.id
        ]
    );
}

// A1, same vehicle: a retracted identifier is hidden and final; a fresh
// identifier with the same normalised alias succeeds on the same vehicle,
// history keeps the retraction, and a later live duplicate on another
// vehicle is refused.
#[test]
fn a1_retracted_identifier_is_final_and_fresh_alias_works_on_same_vehicle() {
    let (_dir, _path, mut g) = new_garage();
    let daily = g.record_writer().add_vehicle("Daily").unwrap();
    let weekend = g.record_writer().add_vehicle("Weekend").unwrap();
    let label = daily.label.clone().unwrap();
    let alias = add(&mut g, "daily", IdentKind::Alias, "Commuter");
    assert_eq!(resolves_to(&g, "commuter"), Some(daily.id));

    // Retraction: a new row that repeats its target, hidden by default.
    let retraction = resolved(
        g.record_writer()
            .retract_identifier("commuter", alias.id)
            .unwrap(),
    );
    assert_eq!(
        retraction,
        Identifier {
            id: retraction.id,
            supersedes_id: Some(alias.id),
            retracted: true,
            ..alias.clone()
        }
    );
    assert_eq!(resolves_to(&g, "commuter"), None);
    assert_eq!(
        ident_ids(&resolved(g.vehicle("daily").unwrap()).identifiers),
        [label.id]
    );
    assert_eq!(
        chain(&resolved(g.identifier_history("daily").unwrap())),
        [
            (label.id, None, false),
            (alias.id, None, false),
            (retraction.id, Some(alias.id), true),
        ]
    );

    // A retraction is final: the retracted head can be neither superseded
    // nor retracted again, and nothing is written.
    let rows = count(&g.conn, "vehicle_ident");
    let mut w = g.record_writer();
    assert!(matches!(
        w.supersede_identifier("daily", retraction.id, "Commuter"),
        Err(Error::RetractionIsFinal(id)) if id == retraction.id
    ));
    assert!(matches!(
        w.retract_identifier("daily", retraction.id),
        Err(Error::AlreadyRetracted(id)) if id == retraction.id
    ));
    assert!(matches!(
        w.supersede_identifier("daily", alias.id, "Commuter"),
        Err(Error::AlreadySuperseded(id)) if id == alias.id
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
    assert_eq!(resolves_to(&g, "commuter"), None);

    // A fresh identifier with the same normalised alias, same vehicle.
    let fresh = add(&mut g, "daily", IdentKind::Alias, " COMMUTER ");
    assert_eq!(
        (fresh.vehicle_id, fresh.supersedes_id, fresh.retracted),
        (daily.id, None, false)
    );
    assert_eq!(resolves_to(&g, "commuter"), Some(daily.id));
    // History keeps the retraction.
    assert_eq!(
        chain(&resolved(g.identifier_history("daily").unwrap())),
        [
            (label.id, None, false),
            (alias.id, None, false),
            (retraction.id, Some(alias.id), true),
            (fresh.id, None, false),
        ]
    );

    // A subsequent live duplicate across vehicles is refused, as a new
    // identifier and as a correction, and writes nothing.
    let other = add(&mut g, "weekend", IdentKind::Alias, "Sunday car");
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("weekend", IdentKind::Alias, "commuter"),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert!(matches!(
        g.record_writer()
            .supersede_identifier("weekend", other.id, "Commuter"),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
    assert_eq!(resolves_to(&g, "commuter"), Some(daily.id));
    assert_eq!(resolves_to(&g, "sunday car"), Some(weekend.id));
}

// A1, another vehicle: once an alias is retracted, a fresh identifier with
// the same normalised alias succeeds on another vehicle; the first
// vehicle's history keeps the retraction; a later live duplicate back on
// the first vehicle is refused.
#[test]
fn a1_retracted_alias_can_be_added_to_another_vehicle() {
    let (_dir, _path, mut g) = new_garage();
    let daily = g.record_writer().add_vehicle("Daily").unwrap();
    let weekend = g.record_writer().add_vehicle("Weekend").unwrap();
    let alias = add(&mut g, "daily", IdentKind::Alias, "Commuter");

    // While the alias is live on the first vehicle the other can't have it.
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("weekend", IdentKind::Alias, "commuter"),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);

    let retraction = resolved(
        g.record_writer()
            .retract_identifier("daily", alias.id)
            .unwrap(),
    );
    let fresh = add(&mut g, "weekend", IdentKind::Alias, "commuter");
    assert_eq!(
        (fresh.vehicle_id, fresh.supersedes_id, fresh.retracted),
        (weekend.id, None, false)
    );
    assert_eq!(resolves_to(&g, " COMMUTER"), Some(weekend.id));
    assert_eq!(
        chain(&resolved(g.identifier_history("daily").unwrap())),
        [
            (daily.label.as_ref().unwrap().id, None, false),
            (alias.id, None, false),
            (retraction.id, Some(alias.id), true),
        ]
    );

    // A subsequent live duplicate across vehicles is refused.
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("daily", IdentKind::Alias, "Commuter"),
        Err(Error::IdentifierTaken(IdentKind::Alias))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
    assert_eq!(resolves_to(&g, "commuter"), Some(weekend.id));
}

// The last live identifier of a vehicle cannot be retracted: nothing could
// refer to the vehicle afterwards, not even to retract it.
#[test]
fn last_live_identifier_cannot_be_retracted() {
    let (_dir, _path, mut g) = new_garage();
    let car = g.record_writer().add_vehicle("Only car").unwrap();
    let label = car.label.clone().unwrap();
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer().retract_identifier("only car", label.id),
        Err(Error::LastIdentifier(id)) if id == label.id
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
    assert_eq!(resolves_to(&g, "only car"), Some(car.id));

    // With an alias as well, the label can go; then the alias is the last.
    let alias = add(&mut g, "only car", IdentKind::Alias, "the car");
    resolved(
        g.record_writer()
            .retract_identifier("the car", label.id)
            .unwrap(),
    );
    assert_eq!(resolves_to(&g, "only car"), None);
    let now = resolved(g.vehicle("the car").unwrap());
    assert_eq!(now.label, None);
    assert_eq!(ident_ids(&now.identifiers), [alias.id]);
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer().retract_identifier("the car", alias.id),
        Err(Error::LastIdentifier(id)) if id == alias.id
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);
}

// Vehicle retraction: an appended `vehicle_retraction` row. The vehicle is
// hidden from reference resolution and from default queries, no write that
// takes a reference reaches it, and its record history is kept. A second
// retraction fails and adds no row (A1). (A read in a session started
// before the retraction is still stored: see `tests_observations.rs`.)
#[test]
fn vehicle_retraction_hides_the_vehicle_and_is_final() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    let coupe = g.record_writer().add_vehicle("Silver coupe").unwrap();
    let sedan = g.record_writer().add_vehicle("Blue sedan").unwrap();
    let alias = add(&mut g, "silver coupe", IdentKind::Alias, "the coupe");
    add(&mut g, "silver coupe", IdentKind::Vin, "TESTVIN0000000001");
    add(&mut g, "silver coupe", IdentKind::Chassis, "E46");
    add(&mut g, "blue sedan", IdentKind::Chassis, "E46");
    let event = resolved(
        g.record_writer()
            .add_service_event("the coupe", &oil(None))
            .unwrap(),
    );
    let noted = resolved(
        g.record_writer()
            .add_note("the coupe", &note("Has a tow bar"))
            .unwrap(),
    );
    let read_in = resolved(
        g.observation_writer()
            .start_session("the coupe", &SessionInput::default())
            .unwrap(),
    );
    g.observation_writer()
        .record_fault_read(read_in.id, 0x12, &FaultRead::Complete(vec![]))
        .unwrap();

    assert!(matches!(
        g.resolve("e46").unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert_eq!(vehicle_ids(&g.vehicles().unwrap()), [coupe.id, sedan.id]);
    assert_eq!(
        resolved(g.latest_measured_faults("the coupe").unwrap()).len(),
        1
    );
    let history_before = g.vehicle_history().unwrap();
    assert_eq!(history_before.len(), 2);
    assert_eq!(
        (
            history_before[0].id,
            history_before[0].retraction.clone(),
            history_before[0].identifiers.len(),
            history_before[0].service_events.clone(),
            history_before[0].notes.clone(),
        ),
        (coupe.id, None, 4, vec![event.clone()], vec![noted.clone()])
    );
    let stored_before = ALL_TABLES.map(|t| dump(&g.conn, t));

    // A retraction needs a reason and one vehicle.
    let mut w = g.record_writer();
    assert!(matches!(
        w.retract_vehicle("the coupe", "  "),
        Err(Error::EmptyReason)
    ));
    assert!(matches!(
        w.retract_vehicle("e46", "Entered twice").unwrap(),
        Outcome::Ambiguous(c) if c.len() == 2
    ));
    assert_eq!(
        w.retract_vehicle("no such car", "Entered twice").unwrap(),
        Outcome::NotFound
    );
    assert_eq!(count(&g.conn, "vehicle_retraction"), 0);

    clock.set("2026-06-01T12:00:00Z");
    let retraction = resolved(
        g.record_writer()
            .retract_vehicle("the coupe", "  Entered twice ")
            .unwrap(),
    );
    assert_eq!(
        retraction,
        VehicleRetraction {
            id: retraction.id,
            vehicle_id: coupe.id,
            vehicle_label: coupe.label.clone(),
            reason: "Entered twice".into(),
            provenance: Provenance {
                source: Source::MaintainerEntry,
                recorded_at: "2026-06-01T12:00:00Z".parse().unwrap(),
                diagwire_rev: REVISION.into(),
            },
        }
    );
    assert!(coupe.label.is_some());
    assert_eq!(count(&g.conn, "vehicle_retraction"), 1);
    // The retraction is one appended row; no stored row changed.
    let stored_after = ALL_TABLES.map(|t| dump(&g.conn, t));
    for (i, table) in ALL_TABLES.iter().enumerate() {
        if *table != "vehicle_retraction" {
            assert_eq!(stored_after[i], stored_before[i], "{table}");
        }
    }

    // Hidden from reference resolution: none of its identifiers resolves,
    // and a reference it shared with another vehicle now names only that one.
    for reference in ["silver coupe", "the coupe", "TESTVIN0000000001"] {
        assert_eq!(
            g.resolve(reference).unwrap(),
            Outcome::NotFound,
            "{reference}"
        );
    }
    assert_eq!(resolves_to(&g, "e46"), Some(sedan.id));
    // Hidden from default queries.
    assert_eq!(vehicle_ids(&g.vehicles().unwrap()), [sedan.id]);
    assert_eq!(g.vehicle("the coupe").unwrap(), Outcome::NotFound);
    assert_eq!(g.service_events("the coupe").unwrap(), Outcome::NotFound);
    assert_eq!(
        g.service_event_history("the coupe").unwrap(),
        Outcome::NotFound
    );
    assert_eq!(g.notes("the coupe").unwrap(), Outcome::NotFound);
    assert_eq!(g.note_history("the coupe").unwrap(), Outcome::NotFound);
    assert_eq!(
        g.identifier_history("the coupe").unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        g.latest_measured_faults("the coupe").unwrap(),
        Outcome::NotFound
    );

    // No write that takes a reference reaches it.
    let before = counts(&g.conn, ALL_TABLES);
    let mut w = g.record_writer();
    assert_eq!(
        w.add_identifier("the coupe", IdentKind::Alias, "x")
            .unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.supersede_identifier("the coupe", alias.id, "x").unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.retract_identifier("the coupe", alias.id).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.add_service_event("the coupe", &oil(None)).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.supersede_service_event("the coupe", event.id, &oil(None))
            .unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.retract_service_event("the coupe", event.id).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.add_note("the coupe", &note("x")).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.supersede_note("the coupe", noted.id, &note("x")).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        w.retract_note("the coupe", noted.id).unwrap(),
        Outcome::NotFound
    );
    assert_eq!(
        g.observation_writer()
            .start_session("the coupe", &SessionInput::default())
            .unwrap(),
        Outcome::NotFound
    );
    assert_eq!(counts(&g.conn, ALL_TABLES), before);

    // A second retraction of the same vehicle fails and adds no row: no
    // reference reaches it, and storage allows one retraction per vehicle.
    for reference in ["the coupe", "silver coupe", "TESTVIN0000000001"] {
        assert_eq!(
            g.record_writer()
                .retract_vehicle(reference, "Again")
                .unwrap(),
            Outcome::NotFound,
            "{reference}"
        );
    }
    let refused = g
        .conn
        .execute(
            "INSERT INTO vehicle_retraction (vehicle_id, reason, source, recorded_at, diagwire_rev)
             VALUES (?1, 'Again', 'maintainer-entry', ?2, 'rev')",
            rusqlite::params![coupe.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("UNIQUE constraint failed: vehicle_retraction.vehicle_id"),
        "{refused}"
    );
    assert_eq!(count(&g.conn, "vehicle_retraction"), 1);

    // Kept in history: the same rows as before, plus the retraction.
    let history = g.vehicle_history().unwrap();
    assert_eq!(
        history,
        [
            VehicleHistory {
                retraction: Some(retraction),
                ..history_before[0].clone()
            },
            history_before[1].clone(),
        ]
    );
}

// A1: a replacement for a retracted vehicle is a new anchor. It can take
// the old vehicle's label, alias and VIN, starts with no records, and the
// old vehicle's history is preserved.
#[test]
fn a1_replacement_vehicle_is_a_new_anchor() {
    let (_dir, _path, mut g) = new_garage();
    let old = g.record_writer().add_vehicle("Track car").unwrap();
    add(&mut g, "track car", IdentKind::Alias, "the racer");
    add(&mut g, "track car", IdentKind::Vin, "TESTVIN0000000007");
    let mut w = g.record_writer();
    let first = resolved(w.add_service_event("the racer", &oil(None)).unwrap());
    let second = resolved(
        w.supersede_service_event("the racer", first.id, &oil(None))
            .unwrap(),
    );
    resolved(w.retract_service_event("the racer", second.id).unwrap());
    resolved(w.add_service_event("the racer", &oil(None)).unwrap());
    let n1 = resolved(w.add_note("the racer", &note("Roll cage fitted")).unwrap());
    resolved(
        w.supersede_note("the racer", n1.id, &note("Half cage fitted"))
            .unwrap(),
    );
    let spare = w.add_vehicle("Spare").unwrap();
    let old_history = g.vehicle_history().unwrap().remove(0);
    assert_eq!(
        (
            old_history.id,
            old_history.identifiers.len(),
            old_history.service_events.len(),
            old_history.notes.len()
        ),
        (old.id, 3, 4, 2)
    );
    let stored_before = RECORD_TABLES.map(|t| dump(&g.conn, t));

    // While the old vehicle is live, no other vehicle can take its VIN.
    assert!(matches!(
        g.record_writer()
            .add_identifier("spare", IdentKind::Vin, "testvin0000000007"),
        Err(Error::IdentifierTaken(IdentKind::Vin))
    ));

    let retraction = resolved(
        g.record_writer()
            .retract_vehicle("track car", "Entered against the wrong car")
            .unwrap(),
    );

    // The replacement: a new anchor with the same label, alias and VIN.
    let replacement = g.record_writer().add_vehicle("Track car").unwrap();
    assert_ne!(replacement.id, old.id);
    assert!(replacement.id > spare.id, "a new anchor row");
    assert_eq!(count(&g.conn, "vehicle"), 3);
    let new_alias = add(&mut g, "track car", IdentKind::Alias, "The Racer");
    let new_vin = add(&mut g, "track car", IdentKind::Vin, "TESTVIN0000000007");
    assert_eq!(
        (new_alias.vehicle_id, new_vin.vehicle_id),
        (replacement.id, replacement.id)
    );
    for reference in ["track car", "the racer", "testvin0000000007"] {
        assert_eq!(
            resolves_to(&g, reference),
            Some(replacement.id),
            "{reference}"
        );
    }
    assert_eq!(
        vehicle_ids(&g.vehicles().unwrap()),
        [spare.id, replacement.id]
    );

    // It starts with no records, and new records go to it.
    assert_eq!(resolved(g.service_event_history("the racer").unwrap()), []);
    assert_eq!(resolved(g.note_history("the racer").unwrap()), []);
    let event = resolved(
        g.record_writer()
            .add_service_event("the racer", &oil(None))
            .unwrap(),
    );
    assert_eq!(event.vehicle_id, replacement.id);

    // The now-live VIN is again refused on any other vehicle.
    let rows = count(&g.conn, "vehicle_ident");
    assert!(matches!(
        g.record_writer()
            .add_identifier("spare", IdentKind::Vin, "TESTVIN0000000007"),
        Err(Error::IdentifierTaken(IdentKind::Vin))
    ));
    assert_eq!(count(&g.conn, "vehicle_ident"), rows);

    // The old vehicle's history is preserved: read back whole, with the
    // retraction, and every row stored before is still stored unchanged.
    let history = g.vehicle_history().unwrap();
    assert_eq!(
        history.iter().map(|h| h.id).collect::<Vec<_>>(),
        [old.id, spare.id, replacement.id]
    );
    assert_eq!(
        history[0],
        VehicleHistory {
            retraction: Some(retraction),
            ..old_history
        }
    );
    assert_eq!(history[2].retraction, None);
    assert_eq!(ident_ids(&history[2].identifiers).len(), 3);
    assert_eq!(history[2].service_events, [event]);
    for (table, before) in RECORD_TABLES.iter().zip(&stored_before) {
        let now = dump(&g.conn, table);
        assert_eq!(&now[..before.len()], &before[..], "{table}");
    }
}

// Provenance (G4): identifier corrections and vehicle retractions get
// their source, revision and time from the writer, on the row, and every
// read returns each row's own provenance.
#[test]
fn corrections_carry_maintainer_entry_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(SteppingClock(Mutex::new(NOW.parse().unwrap())));
    let mut g =
        Garage::create(&dir.path().join("g.db"), clock.clone(), &Options::default()).unwrap();
    let at = |ts: &str| Provenance {
        source: Source::MaintainerEntry,
        recorded_at: ts.parse().unwrap(),
        diagwire_rev: REVISION.into(),
    };
    let (t1, t2, t3, t4, t5) = (
        "2026-01-01T00:00:00Z",
        "2026-02-01T00:00:00Z",
        "2026-03-01T00:00:00Z",
        "2026-04-01T00:00:00Z",
        "2026-05-01T00:00:00Z",
    );
    clock.set(t1);
    let car = g.record_writer().add_vehicle("Test car").unwrap();
    let kept = g.record_writer().add_vehicle("Kept car").unwrap();
    clock.set(t2);
    let alias = add(&mut g, "test car", IdentKind::Alias, "the test car");
    clock.set(t3);
    let corrected = resolved(
        g.record_writer()
            .supersede_identifier("test car", alias.id, "the tester")
            .unwrap(),
    );
    clock.set(t4);
    let dropped = resolved(
        g.record_writer()
            .retract_identifier("test car", corrected.id)
            .unwrap(),
    );
    // From the writer.
    assert_eq!(
        [&corrected.provenance, &dropped.provenance],
        [&at(t3), &at(t4)]
    );
    // From the reads, while the vehicle is live.
    assert_eq!(
        resolved(g.identifier_history("test car").unwrap())
            .iter()
            .map(|i| i.provenance.clone())
            .collect::<Vec<_>>(),
        [at(t1), at(t2), at(t3), at(t4)]
    );
    let listed = g.vehicles().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|v| (
                v.id,
                v.provenance.clone(),
                v.label.as_ref().map(|l| l.provenance.clone())
            ))
            .collect::<Vec<_>>(),
        [
            (car.id, at(t1), Some(at(t1))),
            (kept.id, at(t1), Some(at(t1)))
        ]
    );

    clock.set(t5);
    let retraction = resolved(
        g.record_writer()
            .retract_vehicle("test car", "Sold")
            .unwrap(),
    );
    assert_eq!(retraction.provenance, at(t5));
    assert_eq!(
        retraction.vehicle_label.as_ref().map(|l| &l.provenance),
        Some(&at(t1))
    );
    let history = g.vehicle_history().unwrap();
    assert_eq!(history[0].provenance, at(t1), "anchor provenance");
    assert_eq!(
        history[0].retraction.as_ref().map(|r| &r.provenance),
        Some(&at(t5))
    );
    assert_eq!(
        history[0]
            .identifiers
            .iter()
            .map(|i| i.provenance.clone())
            .collect::<Vec<_>>(),
        [at(t1), at(t2), at(t3), at(t4)]
    );

    // Every stored row of the two tables these writes touch.
    let rows = |table: &str| -> Vec<(String, String, String)> {
        g.conn
            .prepare(&format!(
                "SELECT source, recorded_at, diagwire_rev FROM {table} ORDER BY id"
            ))
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    let stored = |ts: &str| {
        (
            "maintainer-entry".to_owned(),
            ts.to_owned(),
            REVISION.to_owned(),
        )
    };
    assert_eq!(
        rows("vehicle_ident"),
        [
            stored("2026-01-01T00:00:00.000000Z"),
            stored("2026-01-01T00:00:00.000000Z"),
            stored("2026-02-01T00:00:00.000000Z"),
            stored("2026-03-01T00:00:00.000000Z"),
            stored("2026-04-01T00:00:00.000000Z"),
        ]
    );
    assert_eq!(
        rows("vehicle_retraction"),
        [stored("2026-05-01T00:00:00.000000Z")]
    );

    // Backstop: storage refuses a vehicle retraction with a measured
    // source. Every other column is valid and the vehicle has no
    // retraction yet, so only the source CHECK can refuse it.
    let refused = g
        .conn
        .execute(
            "INSERT INTO vehicle_retraction (vehicle_id, reason, source, recorded_at, diagwire_rev)
             VALUES (?1, 'x', 'diagwire-read', ?2, 'rev')",
            rusqlite::params![kept.id, NOW_STORED],
        )
        .unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("CHECK constraint failed: source"),
        "{refused}"
    );
    assert_eq!(count(&g.conn, "vehicle_retraction"), 1);
}

// Append-only: storage rejects edits and deletes of a vehicle retraction,
// and the delete of a vehicle anchor that nothing refers to.
#[test]
fn vehicle_retraction_and_vehicle_anchor_are_append_only() {
    let (_dir, _path, mut g) = new_garage();
    g.record_writer().add_vehicle("Test car").unwrap();
    let retraction = resolved(
        g.record_writer()
            .retract_vehicle("test car", "Sold")
            .unwrap(),
    );
    // An anchor with no identifier, record or retraction, which only raw
    // SQL can make: no foreign key stands between it and a DELETE.
    g.conn
        .execute(
            "INSERT INTO vehicle (source, recorded_at, diagwire_rev)
             VALUES ('maintainer-entry', ?1, 'rev')",
            [NOW_STORED],
        )
        .unwrap();
    let lone = g.conn.last_insert_rowid();
    let before = ["vehicle", "vehicle_retraction"].map(|t| dump(&g.conn, t));

    // No table refers to `vehicle_retraction`, so only its triggers refuse.
    for (sql, id) in [
        (
            "DELETE FROM vehicle_retraction WHERE id = ?1",
            retraction.id,
        ),
        (
            "UPDATE vehicle_retraction SET reason = 'edited' WHERE id = ?1",
            retraction.id,
        ),
        ("DELETE FROM vehicle WHERE id = ?1", lone),
    ] {
        let refused = g.conn.execute(sql, [id]).unwrap_err();
        assert!(
            refused.to_string().contains("append-only"),
            "{sql} was refused by something else: {refused}"
        );
    }
    for sql in [
        "UPDATE vehicle_retraction SET vehicle_id = vehicle_id",
        "DELETE FROM vehicle_retraction",
    ] {
        assert!(
            is_constraint(g.conn.execute(sql, [])),
            "{sql} was not refused"
        );
    }
    assert_eq!(
        ["vehicle", "vehicle_retraction"].map(|t| dump(&g.conn, t)),
        before
    );
}

/// The connection holding the write lock in the test below, handed to the
/// busy handler.
static RIVAL: Mutex<Option<Connection>> = Mutex::new(None);

/// Called when the garage's write meets the rival's lock: the rival
/// commits, and the write is told to try again.
fn rival_commits(_attempts: i32) -> bool {
    match RIVAL.lock().unwrap().take() {
        Some(rival) => {
            rival.execute_batch("COMMIT").unwrap();
            true
        }
        None => false,
    }
}

// G6: the uniqueness check runs inside the write transaction. Another
// connection holds the write lock with an uncommitted alias on another
// vehicle and commits only once this write is waiting for the lock. The
// write must see that alias and refuse.
#[test]
fn duplicate_is_refused_against_a_commit_made_while_waiting_for_the_lock() {
    let (_dir, path, mut g) = new_garage();
    g.record_writer().add_vehicle("Daily").unwrap();
    let weekend = g.record_writer().add_vehicle("Weekend").unwrap();
    let alias = add(&mut g, "daily", IdentKind::Alias, "Commuter");
    g.conn.busy_handler(Some(rival_commits)).unwrap();

    // The rival takes the lock and inserts `value` as an alias of the
    // weekend car, uncommitted.
    let rival_holds = |value: &str| {
        let rival = Connection::open(&path).unwrap();
        rival.execute_batch("BEGIN IMMEDIATE").unwrap();
        rival
            .execute(
                "INSERT INTO vehicle_ident (vehicle_id, kind, value, value_norm,
                     source, recorded_at, diagwire_rev)
                 VALUES (?1, 'alias', ?2, ?2, 'maintainer-entry', ?3, ?4)",
                rusqlite::params![weekend.id, value, NOW_STORED, REVISION],
            )
            .unwrap();
        *RIVAL.lock().unwrap() = Some(rival);
    };

    // As a correction.
    rival_holds("runabout");
    assert_eq!(resolves_to(&g, "runabout"), None, "not committed yet");
    let rows = count(&g.conn, "vehicle_ident");
    let result = g
        .record_writer()
        .supersede_identifier("daily", alias.id, "Runabout");
    assert!(
        RIVAL.lock().unwrap().is_none(),
        "the write never waited for the rival's lock"
    );
    assert!(
        matches!(result, Err(Error::IdentifierTaken(IdentKind::Alias))),
        "{result:?}"
    );
    assert_eq!(count(&g.conn, "vehicle_ident"), rows + 1, "the rival's row");
    assert_eq!(resolves_to(&g, "runabout"), Some(weekend.id));

    // As a new identifier.
    rival_holds("shopper");
    let result = g
        .record_writer()
        .add_identifier("daily", IdentKind::Alias, "Shopper");
    assert!(
        RIVAL.lock().unwrap().is_none(),
        "the write never waited for the rival's lock"
    );
    assert!(
        matches!(result, Err(Error::IdentifierTaken(IdentKind::Alias))),
        "{result:?}"
    );
    assert_eq!(
        count(&g.conn, "vehicle_ident"),
        rows + 2,
        "the rival's rows"
    );
    assert_eq!(resolves_to(&g, "shopper"), Some(weekend.id));
}
