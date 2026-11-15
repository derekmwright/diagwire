//! Measurements diagwire took from a car: sessions and fault snapshots.
//! Written only by [`ObservationWriter`], which sets the source itself.
//! Inserts only (G5).

use jiff::Timestamp;
use rusqlite::{Connection, Row, TransactionBehavior, params};

use crate::records::{
    Outcome, Provenance, Source, bad_value, format_timestamp, parse_col, provenance_from_row,
    resolve,
};
use crate::{Clock, Error, Garage, REVISION, Result};

/// The source every [`ObservationWriter`] row carries. No write method takes
/// one.
const OBSERVATION_SOURCE: Source = Source::DiagwireRead;

macro_rules! snapshot_cols {
    () => {
        "f.id, f.session_id, f.ecu_address, f.outcome, f.nrc, f.read_at, \
         s.source, s.recorded_at, s.diagwire_rev"
    };
}

/// What discovery reported about the car at the start of a session. Stored
/// as given; the vehicle's identifiers are never changed by it (G7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionInput {
    pub reported_vin: Option<String>,
    pub reported_chassis: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub vehicle_id: Option<i64>,
    pub reported_vin: Option<String>,
    pub reported_chassis: Option<String>,
    pub provenance: Provenance,
}

/// One stored fault: the raw code and status byte. No text (DR-0001 D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dtc {
    pub code: u32,
    pub status: u8,
}

/// How one fault-memory read of one ECU ended, mirroring DR-0001 D10's
/// outcomes. Only `Complete` has a fault list: `Complete` with no faults
/// means the ECU reported none, and every other outcome means unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultRead {
    Complete(Vec<Dtc>),
    /// The ECU refused, with this negative response code.
    Negative(u8),
    PendingExpired,
    NoResponse,
    Malformed,
}

impl FaultRead {
    fn outcome(&self) -> &'static str {
        match self {
            FaultRead::Complete(_) => "complete",
            FaultRead::Negative(_) => "negative",
            FaultRead::PendingExpired => "pending-expired",
            FaultRead::NoResponse => "no-response",
            FaultRead::Malformed => "malformed",
        }
    }

    fn nrc(&self) -> Option<u8> {
        match self {
            FaultRead::Negative(nrc) => Some(*nrc),
            _ => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(self, FaultRead::Complete(_))
    }
}

/// One read attempt of one ECU's fault memory, with the provenance of the
/// session it was taken in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultSnapshot {
    pub id: i64,
    pub session_id: i64,
    pub ecu_address: u8,
    /// UTC.
    pub read_at: Timestamp,
    pub read: FaultRead,
    pub provenance: Provenance,
}

/// The latest measured faults of one ECU. Measured at `read_at`, which may
/// be long ago: this is never the car's current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcuFaults {
    pub ecu_address: u8,
    /// The newest complete read, or `None` if no read of this ECU ever
    /// completed, in which case its faults are unknown.
    pub latest_complete: Option<FaultSnapshot>,
    /// Attempts written after `latest_complete` (all of them, if it is
    /// `None`), oldest first. None of them is complete.
    pub newer_attempts: Vec<FaultSnapshot>,
}

/// Adds sessions and fault snapshots. Every write is one `BEGIN IMMEDIATE`
/// transaction.
pub struct ObservationWriter<'g> {
    conn: &'g mut Connection,
    clock: &'g dyn Clock,
}

impl<'g> ObservationWriter<'g> {
    pub(crate) fn new(conn: &'g mut Connection, clock: &'g dyn Clock) -> Self {
        Self { conn, clock }
    }

    /// Starts a session on the vehicle `reference` names, resolved in the
    /// same transaction as the insert. `Ambiguous` and `NotFound` write
    /// nothing.
    pub fn start_session(
        &mut self,
        reference: &str,
        input: &SessionInput,
    ) -> Result<Outcome<Session>> {
        let recorded_at = format_timestamp(self.clock.now());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        tx.execute(
            "INSERT INTO session (vehicle_id, reported_vin, reported_chassis,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                vehicle_id,
                input.reported_vin,
                input.reported_chassis,
                OBSERVATION_SOURCE.as_str(),
                recorded_at,
                REVISION,
            ],
        )?;
        let session = tx.query_row(
            "SELECT id, vehicle_id, reported_vin, reported_chassis,
                    source, recorded_at, diagwire_rev
             FROM session WHERE id = ?1",
            [tx.last_insert_rowid()],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    vehicle_id: r.get(1)?,
                    reported_vin: r.get(2)?,
                    reported_chassis: r.get(3)?,
                    provenance: provenance_from_row(r, 4)?,
                })
            },
        )?;
        tx.commit()?;
        Ok(Outcome::Resolved(session))
    }

    /// Stores one read attempt of the ECU at `ecu_address`, taken in
    /// `session_id`. The snapshot and its fault rows are one transaction:
    /// either all of it is stored or none. A failed read stores its failure
    /// kind and no faults.
    pub fn record_fault_read(
        &mut self,
        session_id: i64,
        ecu_address: u8,
        read: &FaultRead,
    ) -> Result<FaultSnapshot> {
        let read_at = format_timestamp(self.clock.now());
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let known: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM session WHERE id = ?1)",
            [session_id],
            |r| r.get(0),
        )?;
        if !known {
            return Err(Error::SessionNotFound(session_id));
        }
        tx.execute(
            "INSERT INTO fault_snapshot (session_id, ecu_address, outcome, nrc, read_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, ecu_address, read.outcome(), read.nrc(), read_at],
        )?;
        let id = tx.last_insert_rowid();
        if let FaultRead::Complete(dtcs) = read {
            let mut insert = tx
                .prepare("INSERT INTO fault_dtc (snapshot_id, code, status) VALUES (?1, ?2, ?3)")?;
            for dtc in dtcs {
                insert.execute(params![id, dtc.code, dtc.status])?;
            }
        }
        let snapshot = tx.query_row(
            concat!(
                "SELECT ",
                snapshot_cols!(),
                " FROM fault_snapshot f JOIN session s ON s.id = f.session_id WHERE f.id = ?1"
            ),
            [id],
            stored_from_row,
        )?;
        let snapshot = snapshot.load(&tx)?;
        tx.commit()?;
        Ok(snapshot)
    }
}

/// Reads. Each runs in one read transaction, so it sees one snapshot.
impl Garage {
    /// The latest measured faults of the vehicle `reference` names, one
    /// entry per ECU that was ever read, by ECU address: the newest complete
    /// read, and with it every attempt written after it. An empty list
    /// means nothing was ever measured on this vehicle. "Newest" is the
    /// order the reads were written in.
    pub fn latest_measured_faults(&self, reference: &str) -> Result<Outcome<Vec<EcuFaults>>> {
        let tx = self.conn.unchecked_transaction()?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        // Per ECU: its newest complete snapshot, if any, and all later rows.
        let mut stmt = tx.prepare(concat!(
            "SELECT ",
            snapshot_cols!(),
            " FROM fault_snapshot f JOIN session s ON s.id = f.session_id
              WHERE s.vehicle_id = ?1
                AND f.id >= coalesce(
                    (SELECT max(c.id) FROM fault_snapshot c
                       JOIN session cs ON cs.id = c.session_id
                      WHERE cs.vehicle_id = ?1
                        AND c.ecu_address = f.ecu_address
                        AND c.outcome = 'complete'),
                    0)
              ORDER BY f.ecu_address, f.id"
        ))?;
        let stored = stmt
            .query_map([vehicle_id], stored_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut ecus: Vec<EcuFaults> = Vec::new();
        for row in stored {
            let snapshot = row.load(&tx)?;
            if ecus.last().map(|e| e.ecu_address) != Some(snapshot.ecu_address) {
                ecus.push(EcuFaults {
                    ecu_address: snapshot.ecu_address,
                    latest_complete: None,
                    newer_attempts: Vec::new(),
                });
            }
            let ecu = ecus.last_mut().expect("pushed above");
            if snapshot.read.is_complete() {
                ecu.latest_complete = Some(snapshot);
            } else {
                ecu.newer_attempts.push(snapshot);
            }
        }
        Ok(Outcome::Resolved(ecus))
    }
}

/// A `fault_snapshot` row before its fault rows are read.
struct Stored {
    snapshot: FaultSnapshot,
}

impl Stored {
    /// Fills in the fault list of a complete read.
    fn load(mut self, conn: &Connection) -> rusqlite::Result<FaultSnapshot> {
        if let FaultRead::Complete(dtcs) = &mut self.snapshot.read {
            let mut stmt = conn
                .prepare("SELECT code, status FROM fault_dtc WHERE snapshot_id = ?1 ORDER BY id")?;
            *dtcs = stmt
                .query_map([self.snapshot.id], |r| {
                    Ok(Dtc {
                        code: r.get(0)?,
                        status: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(self.snapshot)
    }
}

fn stored_from_row(r: &Row<'_>) -> rusqlite::Result<Stored> {
    let outcome: String = r.get(3)?;
    let nrc: Option<u8> = r.get(4)?;
    let read = match (outcome.as_str(), nrc) {
        ("complete", None) => FaultRead::Complete(Vec::new()),
        ("negative", Some(nrc)) => FaultRead::Negative(nrc),
        ("pending-expired", None) => FaultRead::PendingExpired,
        ("no-response", None) => FaultRead::NoResponse,
        ("malformed", None) => FaultRead::Malformed,
        _ => return Err(bad_value(3, outcome)),
    };
    Ok(Stored {
        snapshot: FaultSnapshot {
            id: r.get(0)?,
            session_id: r.get(1)?,
            ecu_address: r.get(2)?,
            read_at: parse_col(r, 5, |s| s.parse().ok())?,
            read,
            provenance: provenance_from_row(r, 6)?,
        },
    })
}
