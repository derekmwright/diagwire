//! Maintainer notes about a vehicle. Inserts only; a correction supersedes,
//! a mistake is retracted (G5). A note is a record, never a measurement.

use jiff::civil::Date;
use rusqlite::{Connection, Row, TransactionBehavior, params};

use crate::records::{
    Identifier, Outcome, Provenance, RecordWriter, check_correctable, live, live_label, parse_col,
    provenance_from_row, resolve,
};
use crate::{Error, Garage, REVISION, Result};

macro_rules! note_cols {
    () => {
        "n.id, n.vehicle_id, n.about_date, n.body, n.supersedes_id, n.retracted, \
         n.source, n.recorded_at, n.diagwire_rev"
    };
}

const NOTE_HISTORY_SQL: &str = concat!(
    "SELECT ",
    note_cols!(),
    " FROM note n WHERE n.vehicle_id = ?1 ORDER BY n.id"
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub vehicle_id: i64,
    pub vehicle_label: Option<Identifier>,
    /// The local date the note is about.
    pub about_date: Date,
    pub body: String,
    pub supersedes_id: Option<i64>,
    pub retracted: bool,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInput {
    /// `None` means today, per the garage's clock.
    pub about_date: Option<Date>,
    pub body: String,
}

impl RecordWriter<'_> {
    pub fn add_note(&mut self, reference: &str, input: &NoteInput) -> Result<Outcome<Note>> {
        self.write_note(reference, None, Some(input))
    }

    /// Adds a new head that corrects `target`, which must be a head on the
    /// same vehicle and not a retraction.
    pub fn supersede_note(
        &mut self,
        reference: &str,
        target: i64,
        input: &NoteInput,
    ) -> Result<Outcome<Note>> {
        self.write_note(reference, Some(target), Some(input))
    }

    /// Hides `target` from default queries; it stays in history.
    pub fn retract_note(&mut self, reference: &str, target: i64) -> Result<Outcome<Note>> {
        self.write_note(reference, Some(target), None)
    }

    /// `input` is `None` for a retraction, which copies the target's fields.
    fn write_note(
        &mut self,
        reference: &str,
        target: Option<i64>,
        input: Option<&NoteInput>,
    ) -> Result<Outcome<Note>> {
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
        if let Some(t) = target {
            check_correctable(&tx, "note", vehicle_id, t, input.is_none())?;
        }
        let (about_date, body, retracted) = match (input, target) {
            (Some(i), _) => {
                let body = i.body.trim();
                if body.is_empty() {
                    return Err(Error::EmptyNote);
                }
                (i.about_date.unwrap_or(today), body.to_owned(), false)
            }
            (None, Some(t)) => {
                let previous = load_note(&tx, t)?;
                (previous.about_date, previous.body, true)
            }
            (None, None) => unreachable!("a retraction always has a target"),
        };
        tx.execute(
            "INSERT INTO note (vehicle_id, about_date, body, supersedes_id, retracted,
                 source, recorded_at, diagwire_rev)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                vehicle_id,
                about_date.to_string(),
                body,
                target,
                retracted,
                stamp.source,
                stamp.recorded_at,
                REVISION,
            ],
        )?;
        let note = load_note(&tx, tx.last_insert_rowid())?;
        tx.commit()?;
        Ok(Outcome::Resolved(note))
    }
}

/// Reads. Each runs in one read transaction, so it sees one snapshot.
impl Garage {
    /// Live notes: superseded rows and retracted heads are hidden.
    pub fn notes(&self, reference: &str) -> Result<Outcome<Vec<Note>>> {
        self.note_query(
            reference,
            concat!(
                "SELECT ",
                note_cols!(),
                " FROM note n WHERE n.vehicle_id = ?1 AND ",
                live!("note", "n"),
                " ORDER BY n.about_date, n.id"
            ),
        )
    }

    /// Every note row, including superseded rows and retractions.
    pub fn note_history(&self, reference: &str) -> Result<Outcome<Vec<Note>>> {
        self.note_query(reference, NOTE_HISTORY_SQL)
    }

    fn note_query(&self, reference: &str, sql: &str) -> Result<Outcome<Vec<Note>>> {
        let tx = self.conn.unchecked_transaction()?;
        let vehicle_id = match resolve(&tx, reference)? {
            Outcome::Resolved(r) => r.vehicle_id,
            Outcome::Ambiguous(c) => return Ok(Outcome::Ambiguous(c)),
            Outcome::NotFound => return Ok(Outcome::NotFound),
        };
        Ok(Outcome::Resolved(note_rows(&tx, vehicle_id, sql)?))
    }
}

/// Every note row of one vehicle, by id.
pub(crate) fn note_history(conn: &Connection, vehicle_id: i64) -> Result<Vec<Note>> {
    note_rows(conn, vehicle_id, NOTE_HISTORY_SQL)
}

fn note_rows(conn: &Connection, vehicle_id: i64, sql: &str) -> Result<Vec<Note>> {
    let label = live_label(conn, vehicle_id)?;
    let mut stmt = conn.prepare(sql)?;
    let notes = stmt
        .query_map([vehicle_id], |r| note_from_row(r, label.clone()))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(notes)
}

fn load_note(conn: &Connection, id: i64) -> Result<Note> {
    let mut note = conn.query_row(
        concat!("SELECT ", note_cols!(), " FROM note n WHERE n.id = ?1"),
        [id],
        |r| note_from_row(r, None),
    )?;
    note.vehicle_label = live_label(conn, note.vehicle_id)?;
    Ok(note)
}

fn note_from_row(r: &Row<'_>, vehicle_label: Option<Identifier>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get(0)?,
        vehicle_id: r.get(1)?,
        vehicle_label,
        about_date: parse_col(r, 2, |s| s.parse().ok())?,
        body: r.get(3)?,
        supersedes_id: r.get(4)?,
        retracted: r.get(5)?,
        provenance: provenance_from_row(r, 6)?,
    })
}
