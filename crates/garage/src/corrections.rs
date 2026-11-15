//! Corrections to what identifies a vehicle, and vehicle retraction (G5).
//! An identifier (label, alias, VIN or chassis) is superseded or retracted
//! like any chain-bearing record. A vehicle has no chain: it is retracted
//! by a `vehicle_retraction` row, at most once, and never superseded.

use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use crate::notes::{Note, note_history};
use crate::records::{
    Identifier, Outcome, Provenance, RecordWriter, ServiceEvent, Vehicle, check_correctable,
    event_history, ident_cols, ident_from_row, insert_ident, live, live_label, load_ident,
    load_vehicle, provenance_from_row, resolve, vehicle_provenance,
};
use crate::{Error, Garage, REVISION, Result};

macro_rules! retraction_cols {
    () => {
        "r.id, r.vehicle_id, r.reason, r.source, r.recorded_at, r.diagwire_rev"
    };
}

const IDENT_HISTORY_SQL: &str = concat!(
    "SELECT ",
    ident_cols!(),
    " FROM vehicle_ident i WHERE i.vehicle_id = ?1 ORDER BY i.id"
);

/// The record that retracts a vehicle. From then on no reference resolves
/// to the vehicle and default queries leave it out; everything recorded
/// about it stays in history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleRetraction {
    pub id: i64,
    pub vehicle_id: i64,
    pub vehicle_label: Option<Identifier>,
    pub reason: String,
    pub provenance: Provenance,
}

/// One vehicle anchor with every record row ever written about it,
/// retracted or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleHistory {
    pub id: i64,
    /// The `vehicle` anchor row's provenance.
    pub provenance: Provenance,
    pub label: Option<Identifier>,
    /// `Some` if the vehicle has been retracted.
    pub retraction: Option<VehicleRetraction>,
    /// Every identifier row, including superseded rows and retractions.
    pub identifiers: Vec<Identifier>,
    /// Every service event row, including superseded rows and retractions.
    pub service_events: Vec<ServiceEvent>,
    /// Every note row, including superseded rows and retractions.
    pub notes: Vec<Note>,
}

impl RecordWriter<'_> {
    /// Adds a new head that corrects identifier `target`, which must be a
    /// head on the vehicle `reference` names and not a retraction. The new
    /// head keeps the target's kind. Refused if the new value equals
    /// another vehicle's live alias or VIN.
    pub fn supersede_identifier(
        &mut self,
        reference: &str,
        target: i64,
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
        check_correctable(&tx, "vehicle_ident", vehicle_id, target, false)?;
        let kind = load_ident(&tx, target)?.kind;
        let id = insert_ident(&tx, &stamp, vehicle_id, kind, value, Some(target))?;
        let ident = load_ident(&tx, id)?;
        tx.commit()?;
        Ok(Outcome::Resolved(ident))
    }

    /// Drops identifier `target`: it no longer resolves and is hidden from
    /// default queries, and stays in history. Refused for the vehicle's
    /// last live identifier, which would leave nothing to refer to the
    /// vehicle by; retract the vehicle instead.
    pub fn retract_identifier(
        &mut self,
        reference: &str,
        target: i64,
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
        check_correctable(&tx, "vehicle_ident", vehicle_id, target, true)?;
        let another_live: bool = tx.query_row(
            concat!(
                "SELECT EXISTS (SELECT 1 FROM vehicle_ident i
                  WHERE i.vehicle_id = ?1 AND i.id <> ?2 AND ",
                live!("vehicle_ident", "i"),
                ")"
            ),
            [vehicle_id, target],
            |r| r.get(0),
        )?;
        if !another_live {
            return Err(Error::LastIdentifier(target));
        }
        tx.execute(
            "INSERT INTO vehicle_ident (vehicle_id, kind, value, value_norm, supersedes_id,
                 retracted, source, recorded_at, diagwire_rev)
             SELECT vehicle_id, kind, value, value_norm, id, 1, ?2, ?3, ?4
               FROM vehicle_ident WHERE id = ?1",
            params![target, stamp.source, stamp.recorded_at, REVISION],
        )?;
        let ident = load_ident(&tx, tx.last_insert_rowid())?;
        tx.commit()?;
        Ok(Outcome::Resolved(ident))
    }

    /// Retracts the vehicle `reference` names. A vehicle is retracted at
    /// most once, and a retraction is final (DR-0002 A1): afterwards no
    /// reference resolves to it, so a second retraction finds no vehicle. A
    /// replacement is a new vehicle.
    pub fn retract_vehicle(
        &mut self,
        reference: &str,
        reason: &str,
    ) -> Result<Outcome<VehicleRetraction>> {
        let stamp = self.stamp();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(Error::EmptyReason);
        }
        tx.execute(
            "INSERT INTO vehicle_retraction (vehicle_id, reason, source, recorded_at, diagwire_rev)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                vehicle_id,
                reason,
                stamp.source,
                stamp.recorded_at,
                REVISION
            ],
        )?;
        let retraction =
            load_retraction(&tx, vehicle_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        tx.commit()?;
        Ok(Outcome::Resolved(retraction))
    }
}

/// Reads. Each runs in one read transaction, so it sees one snapshot.
impl Garage {
    /// Every vehicle that has not been retracted, with its live
    /// identifiers, in the order the vehicles were added.
    pub fn vehicles(&self) -> Result<Vec<Vehicle>> {
        let tx = self.conn.unchecked_transaction()?;
        let ids = vehicle_ids(
            &tx,
            "SELECT v.id FROM vehicle v
              WHERE NOT EXISTS (SELECT 1 FROM vehicle_retraction r WHERE r.vehicle_id = v.id)
              ORDER BY v.id",
        )?;
        ids.into_iter().map(|id| load_vehicle(&tx, id)).collect()
    }

    /// Every identifier row of the vehicle `reference` names, including
    /// superseded rows and retractions.
    pub fn identifier_history(&self, reference: &str) -> Result<Outcome<Vec<Identifier>>> {
        let tx = self.conn.unchecked_transaction()?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        Ok(Outcome::Resolved(ident_history(&tx, vehicle_id)?))
    }

    /// Every vehicle ever added, retracted ones included, each with all of
    /// its record rows, in the order the vehicles were added. This is where
    /// a retracted vehicle and what was recorded about it can be read.
    pub fn vehicle_history(&self) -> Result<Vec<VehicleHistory>> {
        let tx = self.conn.unchecked_transaction()?;
        let ids = vehicle_ids(&tx, "SELECT v.id FROM vehicle v ORDER BY v.id")?;
        ids.into_iter()
            .map(|id| {
                Ok(VehicleHistory {
                    id,
                    provenance: vehicle_provenance(&tx, id)?,
                    label: live_label(&tx, id)?,
                    retraction: load_retraction(&tx, id)?,
                    identifiers: ident_history(&tx, id)?,
                    service_events: event_history(&tx, id)?,
                    notes: note_history(&tx, id)?,
                })
            })
            .collect()
    }
}

fn vehicle_ids(conn: &Connection, sql: &str) -> rusqlite::Result<Vec<i64>> {
    conn.prepare(sql)?.query_map([], |r| r.get(0))?.collect()
}

fn ident_history(conn: &Connection, vehicle_id: i64) -> rusqlite::Result<Vec<Identifier>> {
    conn.prepare(IDENT_HISTORY_SQL)?
        .query_map([vehicle_id], ident_from_row)?
        .collect()
}

fn load_retraction(conn: &Connection, vehicle_id: i64) -> Result<Option<VehicleRetraction>> {
    let label = live_label(conn, vehicle_id)?;
    Ok(conn
        .query_row(
            concat!(
                "SELECT ",
                retraction_cols!(),
                " FROM vehicle_retraction r WHERE r.vehicle_id = ?1"
            ),
            [vehicle_id],
            |r| retraction_from_row(r, label),
        )
        .optional()?)
}

fn retraction_from_row(
    r: &Row<'_>,
    vehicle_label: Option<Identifier>,
) -> rusqlite::Result<VehicleRetraction> {
    Ok(VehicleRetraction {
        id: r.get(0)?,
        vehicle_id: r.get(1)?,
        vehicle_label,
        reason: r.get(2)?,
        provenance: provenance_from_row(r, 3)?,
    })
}
