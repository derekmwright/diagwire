//! Maintainer records: vehicles, identifiers and service events. Inserts
//! only; a correction supersedes, a mistake is retracted (G5).

use std::fmt;

use jiff::Timestamp;
use jiff::civil::Date;
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::{Clock, Error, Garage, REVISION, Result};

/// The source every [`RecordWriter`] row carries. No write method takes one.
const RECORD_SOURCE: Source = Source::MaintainerEntry;

/// A row is live when nothing supersedes it and it is not a retraction.
macro_rules! live {
    ($t:literal, $a:literal) => {
        concat!(
            $a,
            ".retracted = 0 AND NOT EXISTS (SELECT 1 FROM ",
            $t,
            " s WHERE s.supersedes_id = ",
            $a,
            ".id)"
        )
    };
}

macro_rules! ident_cols {
    () => {
        "i.id, i.vehicle_id, i.kind, i.value, i.supersedes_id, i.retracted, \
         i.source, i.recorded_at, i.diagwire_rev"
    };
}

macro_rules! event_cols {
    () => {
        "e.id, e.vehicle_id, e.event_date, e.summary, e.odo_value, e.odo_unit, \
         e.supersedes_id, e.retracted, e.source, e.recorded_at, e.diagwire_rev"
    };
}
/// Where a fact came from (G4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    MaintainerEntry,
    ImportedNote,
    DiagwireRead,
    /// `canonical-capture:<tool>`.
    CanonicalCapture(String),
}

impl Source {
    pub fn as_str(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Source::MaintainerEntry => "maintainer-entry".into(),
            Source::ImportedNote => "imported-note".into(),
            Source::DiagwireRead => "diagwire-read".into(),
            Source::CanonicalCapture(tool) => format!("canonical-capture:{tool}").into(),
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "maintainer-entry" => Source::MaintainerEntry,
            "imported-note" => Source::ImportedNote,
            "diagwire-read" => Source::DiagwireRead,
            _ => Source::CanonicalCapture(s.strip_prefix("canonical-capture:")?.to_owned()),
        })
    }
}

/// Carried by every read result (G4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub source: Source,
    /// UTC.
    pub recorded_at: Timestamp,
    pub diagwire_rev: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentKind {
    Label,
    Alias,
    Vin,
    Chassis,
}

impl IdentKind {
    fn as_str(self) -> &'static str {
        match self {
            IdentKind::Label => "label",
            IdentKind::Alias => "alias",
            IdentKind::Vin => "vin",
            IdentKind::Chassis => "chassis",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "label" => IdentKind::Label,
            "alias" => IdentKind::Alias,
            "vin" => IdentKind::Vin,
            "chassis" => IdentKind::Chassis,
            _ => return None,
        })
    }
}

impl fmt::Display for IdentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OdometerUnit {
    Mi,
    Km,
}

impl OdometerUnit {
    fn as_str(self) -> &'static str {
        match self {
            OdometerUnit::Mi => "mi",
            OdometerUnit::Km => "km",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "mi" => Some(OdometerUnit::Mi),
            "km" => Some(OdometerUnit::Km),
            _ => None,
        }
    }
}

/// An odometer reading always has its unit (G10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Odometer {
    pub value: u32,
    pub unit: OdometerUnit,
}

/// The result of anything that names a vehicle by reference (G7).
/// `Ambiguous` and `NotFound` mean nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Resolved(T),
    Ambiguous(Vec<VehicleRef>),
    NotFound,
}

/// A reference resolved to one vehicle.
pub type Resolution = Outcome<VehicleRef>;

/// A vehicle a reference named, as a resolution result or an ambiguity
/// candidate. Carries the anchor's provenance and, through each
/// [`Identifier`], the provenance of its label and of what matched (G4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleRef {
    pub vehicle_id: i64,
    /// The `vehicle` anchor row's provenance.
    pub provenance: Provenance,
    pub label: Option<Identifier>,
    /// The live identifiers equal to the reference.
    pub matched: Vec<Identifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier {
    pub id: i64,
    pub vehicle_id: i64,
    pub kind: IdentKind,
    pub value: String,
    pub supersedes_id: Option<i64>,
    pub retracted: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vehicle {
    pub id: i64,
    pub label: Option<Identifier>,
    /// Live identifiers only.
    pub identifiers: Vec<Identifier>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEvent {
    pub id: i64,
    pub vehicle_id: i64,
    pub vehicle_label: Option<Identifier>,
    /// The local date meant.
    pub event_date: Date,
    pub summary: String,
    pub odometer: Option<Odometer>,
    pub supersedes_id: Option<i64>,
    pub retracted: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEventInput {
    /// `None` means today, per the garage's clock.
    pub date: Option<Date>,
    pub summary: String,
    pub odometer: Option<Odometer>,
}

/// Trim, collapse spaces and apply Unicode full default case folding
/// (`Straße` and `STRASSE` both fold to `strasse`). Used for stored
/// `value_norm` and for incoming references alike. No fuzzy matching (G7).
pub(crate) fn normalize(s: &str) -> String {
    caseless::default_case_fold_str(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Adds maintainer records. Every write is one `BEGIN IMMEDIATE`
/// transaction, which resolves its reference before inserting.
pub struct RecordWriter<'g> {
    conn: &'g mut Connection,
    clock: &'g dyn Clock,
}

struct Stamp {
    source: String,
    recorded_at: String,
}

impl<'g> RecordWriter<'g> {
    pub(crate) fn new(conn: &'g mut Connection, clock: &'g dyn Clock) -> Self {
        Self { conn, clock }
    }

    fn stamp(&self) -> Stamp {
        Stamp {
            source: RECORD_SOURCE.as_str().into_owned(),
            recorded_at: format_timestamp(self.clock.now()),
        }
    }

    /// Adds a vehicle anchor with its label.
    pub fn add_vehicle(&mut self, label: &str) -> Result<Vehicle> {
        let stamp = self.stamp();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO vehicle (source, recorded_at, diagwire_rev) VALUES (?1, ?2, ?3)",
            params![stamp.source, stamp.recorded_at, REVISION],
        )?;
        let vehicle_id = tx.last_insert_rowid();
        insert_ident(&tx, &stamp, vehicle_id, IdentKind::Label, label)?;
        let vehicle = load_vehicle(&tx, vehicle_id)?;
        tx.commit()?;
        Ok(vehicle)
    }

    /// Adds a label, alias, VIN or chassis to the vehicle `reference` names.
    /// Refused if it equals another vehicle's live alias or VIN.
    pub fn add_identifier(
        &mut self,
        reference: &str,
        kind: IdentKind,
        value: &str,
    ) -> Result<Outcome<Identifier>> {
        let stamp = self.stamp();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        let id = insert_ident(&tx, &stamp, vehicle_id, kind, value)?;
        let ident = tx.query_row(
            concat!(
                "SELECT ",
                ident_cols!(),
                " FROM vehicle_ident i WHERE i.id = ?1"
            ),
            [id],
            ident_from_row,
        )?;
        tx.commit()?;
        Ok(Outcome::Resolved(ident))
    }

    pub fn add_service_event(
        &mut self,
        reference: &str,
        input: &ServiceEventInput,
    ) -> Result<Outcome<ServiceEvent>> {
        self.write_event(reference, None, Some(input))
    }

    /// Adds a new head that corrects `target`, which must be a head on the
    /// same vehicle.
    pub fn supersede_service_event(
        &mut self,
        reference: &str,
        target: i64,
        input: &ServiceEventInput,
    ) -> Result<Outcome<ServiceEvent>> {
        self.write_event(reference, Some(target), Some(input))
    }

    /// Hides `target` from default queries; it stays in history.
    pub fn retract_service_event(
        &mut self,
        reference: &str,
        target: i64,
    ) -> Result<Outcome<ServiceEvent>> {
        self.write_event(reference, Some(target), None)
    }

    /// `input` is `None` for a retraction, which copies the target's fields.
    fn write_event(
        &mut self,
        reference: &str,
        target: Option<i64>,
        input: Option<&ServiceEventInput>,
    ) -> Result<Outcome<ServiceEvent>> {
        let stamp = self.stamp();
        let today = self.clock.today();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        let previous = match target {
            Some(t) => Some(head_event(&tx, vehicle_id, t)?),
            None => None,
        };
        // A retraction is final (DR-0002 A1): undo it with a fresh record.
        if let (Some(_), Some(p)) = (input, &previous)
            && p.retracted
        {
            return Err(Error::RetractionIsFinal(p.id));
        }
        let (date, summary, odometer, retracted) = match (input, &previous) {
            (Some(i), _) => {
                let summary = i.summary.trim();
                if summary.is_empty() {
                    return Err(Error::EmptySummary);
                }
                (
                    i.date.unwrap_or(today),
                    summary.to_owned(),
                    i.odometer,
                    false,
                )
            }
            (None, Some(p)) => {
                if p.retracted {
                    return Err(Error::AlreadyRetracted(p.id));
                }
                (p.event_date, p.summary.clone(), p.odometer, true)
            }
            (None, None) => unreachable!("a retraction always has a target"),
        };
        tx.execute(
            "INSERT INTO service_event (vehicle_id, event_date, summary, odo_value, odo_unit,
                 supersedes_id, retracted, source, recorded_at, diagwire_rev)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                vehicle_id,
                date.to_string(),
                summary,
                odometer.map(|o| o.value),
                odometer.map(|o| o.unit.as_str()),
                target,
                retracted,
                stamp.source,
                stamp.recorded_at,
                REVISION,
            ],
        )?;
        let event = load_event(&tx, tx.last_insert_rowid())?;
        tx.commit()?;
        Ok(Outcome::Resolved(event))
    }
}

/// Reads. Each runs in one read transaction, so it sees one snapshot.
impl Garage {
    pub fn resolve(&self, reference: &str) -> Result<Resolution> {
        let tx = self.conn.unchecked_transaction()?;
        resolve(&tx, reference)
    }

    pub fn vehicle(&self, reference: &str) -> Result<Outcome<Vehicle>> {
        let tx = self.conn.unchecked_transaction()?;
        Ok(match resolve(&tx, reference)? {
            Outcome::Resolved(r) => Outcome::Resolved(load_vehicle(&tx, r.vehicle_id)?),
            Outcome::Ambiguous(c) => Outcome::Ambiguous(c),
            Outcome::NotFound => Outcome::NotFound,
        })
    }

    /// Live service events: superseded rows and retracted heads are hidden.
    pub fn service_events(&self, reference: &str) -> Result<Outcome<Vec<ServiceEvent>>> {
        self.events(
            reference,
            concat!(
                "SELECT ",
                event_cols!(),
                " FROM service_event e WHERE e.vehicle_id = ?1 AND ",
                live!("service_event", "e"),
                " ORDER BY e.event_date, e.id"
            ),
        )
    }

    /// Every service event row, including superseded rows and retractions.
    pub fn service_event_history(&self, reference: &str) -> Result<Outcome<Vec<ServiceEvent>>> {
        self.events(
            reference,
            concat!(
                "SELECT ",
                event_cols!(),
                " FROM service_event e WHERE e.vehicle_id = ?1 ORDER BY e.id"
            ),
        )
    }

    fn events(&self, reference: &str, sql: &str) -> Result<Outcome<Vec<ServiceEvent>>> {
        let tx = self.conn.unchecked_transaction()?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        let label = live_label(&tx, vehicle_id)?;
        let mut stmt = tx.prepare(sql)?;
        let events = stmt
            .query_map([vehicle_id], |r| event_from_row(r, label.clone()))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Outcome::Resolved(events))
    }
}

/// Distinct vehicles with a live identifier equal to the normalised
/// reference: one resolves, several are ambiguous, none is not found.
fn resolve(conn: &Connection, reference: &str) -> Result<Resolution> {
    let norm = normalize(reference);
    let mut stmt = conn.prepare(concat!(
        "SELECT ",
        ident_cols!(),
        " FROM vehicle_ident i WHERE i.value_norm = ?1 AND ",
        live!("vehicle_ident", "i"),
        " ORDER BY i.vehicle_id, i.id"
    ))?;
    let matches = stmt
        .query_map([norm], ident_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut refs: Vec<VehicleRef> = Vec::new();
    for ident in matches {
        match refs.last_mut() {
            Some(r) if r.vehicle_id == ident.vehicle_id => r.matched.push(ident),
            _ => refs.push(vehicle_ref(conn, ident)?),
        }
    }
    Ok(match refs.len() {
        0 => Outcome::NotFound,
        1 => Outcome::Resolved(refs.remove(0)),
        _ => Outcome::Ambiguous(refs),
    })
}

/// A [`VehicleRef`] for the vehicle `first` (a matched identifier) is on.
fn vehicle_ref(conn: &Connection, first: Identifier) -> Result<VehicleRef> {
    let vehicle_id = first.vehicle_id;
    Ok(VehicleRef {
        vehicle_id,
        provenance: vehicle_provenance(conn, vehicle_id)?,
        label: live_label(conn, vehicle_id)?,
        matched: vec![first],
    })
}

fn vehicle_provenance(conn: &Connection, vehicle_id: i64) -> rusqlite::Result<Provenance> {
    conn.query_row(
        "SELECT source, recorded_at, diagwire_rev FROM vehicle WHERE id = ?1",
        [vehicle_id],
        |r| provenance_from_row(r, 0),
    )
}

fn insert_ident(
    conn: &Connection,
    stamp: &Stamp,
    vehicle_id: i64,
    kind: IdentKind,
    value: &str,
) -> Result<i64> {
    let value = value.trim();
    let norm = normalize(value);
    if norm.is_empty() {
        return Err(Error::EmptyIdentifier);
    }
    let taken: bool = conn.query_row(
        concat!(
            "SELECT EXISTS (SELECT 1 FROM vehicle_ident i WHERE i.value_norm = ?1
                 AND i.vehicle_id <> ?2 AND i.kind IN ('alias', 'vin') AND ",
            live!("vehicle_ident", "i"),
            ")"
        ),
        params![norm, vehicle_id],
        |r| r.get(0),
    )?;
    if taken {
        return Err(Error::IdentifierTaken(kind));
    }
    conn.execute(
        "INSERT INTO vehicle_ident (vehicle_id, kind, value, value_norm, source, recorded_at, diagwire_rev)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![vehicle_id, kind.as_str(), value, norm, stamp.source, stamp.recorded_at, REVISION],
    )?;
    Ok(conn.last_insert_rowid())
}

/// `target` must exist on this vehicle and not be superseded already.
fn head_event(conn: &Connection, vehicle_id: i64, target: i64) -> Result<ServiceEvent> {
    let event = conn
        .query_row(
            concat!(
                "SELECT ",
                event_cols!(),
                " FROM service_event e WHERE e.id = ?1 AND e.vehicle_id = ?2"
            ),
            [target, vehicle_id],
            |r| event_from_row(r, None),
        )
        .optional()?
        .ok_or(Error::RecordNotFound(target))?;
    let superseded: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM service_event WHERE supersedes_id = ?1)",
        [target],
        |r| r.get(0),
    )?;
    if superseded {
        return Err(Error::AlreadySuperseded(target));
    }
    Ok(event)
}

/// The vehicle's newest live label, with its provenance.
fn live_label(conn: &Connection, vehicle_id: i64) -> rusqlite::Result<Option<Identifier>> {
    conn.query_row(
        concat!(
            "SELECT ",
            ident_cols!(),
            " FROM vehicle_ident i WHERE i.vehicle_id = ?1 AND i.kind = 'label' AND ",
            live!("vehicle_ident", "i"),
            " ORDER BY i.id DESC LIMIT 1"
        ),
        [vehicle_id],
        ident_from_row,
    )
    .optional()
}

fn load_vehicle(conn: &Connection, vehicle_id: i64) -> Result<Vehicle> {
    let provenance = vehicle_provenance(conn, vehicle_id)?;
    let mut stmt = conn.prepare(concat!(
        "SELECT ",
        ident_cols!(),
        " FROM vehicle_ident i WHERE i.vehicle_id = ?1 AND ",
        live!("vehicle_ident", "i"),
        " ORDER BY i.id"
    ))?;
    let identifiers = stmt
        .query_map([vehicle_id], ident_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Vehicle {
        id: vehicle_id,
        label: live_label(conn, vehicle_id)?,
        identifiers,
        provenance,
    })
}

fn load_event(conn: &Connection, id: i64) -> Result<ServiceEvent> {
    let mut event = conn.query_row(
        concat!(
            "SELECT ",
            event_cols!(),
            " FROM service_event e WHERE e.id = ?1"
        ),
        [id],
        |r| event_from_row(r, None),
    )?;
    event.vehicle_label = live_label(conn, event.vehicle_id)?;
    Ok(event)
}

fn ident_from_row(r: &Row<'_>) -> rusqlite::Result<Identifier> {
    Ok(Identifier {
        id: r.get(0)?,
        vehicle_id: r.get(1)?,
        kind: parse_col(r, 2, IdentKind::parse)?,
        value: r.get(3)?,
        supersedes_id: r.get(4)?,
        retracted: r.get(5)?,
        provenance: provenance_from_row(r, 6)?,
    })
}

fn event_from_row(
    r: &Row<'_>,
    vehicle_label: Option<Identifier>,
) -> rusqlite::Result<ServiceEvent> {
    let odo_value: Option<u32> = r.get(4)?;
    let odo_unit: Option<String> = r.get(5)?;
    let odometer = match (odo_value, odo_unit) {
        (Some(value), Some(unit)) => Some(Odometer {
            value,
            unit: OdometerUnit::parse(&unit).ok_or_else(|| bad_value(5, unit))?,
        }),
        _ => None,
    };
    Ok(ServiceEvent {
        id: r.get(0)?,
        vehicle_id: r.get(1)?,
        vehicle_label,
        event_date: parse_col(r, 2, |s| s.parse().ok())?,
        summary: r.get(3)?,
        odometer,
        supersedes_id: r.get(6)?,
        retracted: r.get(7)?,
        provenance: provenance_from_row(r, 8)?,
    })
}

fn provenance_from_row(r: &Row<'_>, first: usize) -> rusqlite::Result<Provenance> {
    Ok(Provenance {
        source: parse_col(r, first, Source::parse)?,
        recorded_at: parse_col(r, first + 1, |s| s.parse().ok())?,
        diagwire_rev: r.get(first + 2)?,
    })
}

fn parse_col<T>(r: &Row<'_>, idx: usize, parse: impl Fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    let s: String = r.get(idx)?;
    parse(&s).ok_or_else(|| bad_value(idx, s))
}

#[derive(Debug, thiserror::Error)]
#[error("unrecognised stored value {0:?}")]
struct BadValue(String);

fn bad_value(idx: usize, s: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(BadValue(s)))
}

/// Fixed-width UTC ISO-8601 (microseconds), so stored text sorts by time.
pub(crate) fn format_timestamp(ts: Timestamp) -> String {
    format!("{ts:.6}")
}
