//! Forward-only migrations, embedded in the binary. Each table is created by
//! the migration that lands its first writer (G3).

use rusqlite::{Connection, TransactionBehavior};

use crate::backup::BackupFile;
use crate::{APPLICATION_ID, Error, Result};

pub(crate) struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

pub(crate) const LATEST: i64 = 3;

/// How many backups one migration takes before giving up, when other
/// connections keep committing between a backup and the migration's lock.
pub(crate) const MAX_BACKUPS: u32 = 3;

pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: V1,
    },
    Migration {
        version: 2,
        sql: V2,
    },
    Migration {
        version: 3,
        sql: V3,
    },
];

/// Applies every step newer than the file's `user_version`, and the version
/// bump, in one `BEGIN IMMEDIATE` transaction. Version 0 is a new file, which
/// also gets its `application_id` in that transaction. Takes no backup, so
/// it is for a new file only; an existing garage goes through [`upgrade`].
pub(crate) fn migrate(conn: &mut Connection, steps: &[Migration]) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-read under the lock: another process may have migrated already.
    let mut version = user_version(&tx)?;
    if version == 0 {
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    }
    let from = version;
    for step in steps.iter().filter(|s| s.version > from) {
        tx.execute_batch(step.sql)?;
        tx.pragma_update(None, "user_version", step.version)?;
        version = step.version;
    }
    tx.commit()?;
    Ok(version)
}

/// Migrates an existing garage to the last of `steps` (G3). `backup` must
/// produce a consistent backup of the garage as it stands (it is given the
/// version being migrated from); if it fails, nothing has been changed and
/// nothing is. The steps then run in one `BEGIN IMMEDIATE` transaction, but
/// only if no other connection committed since before the backup's
/// snapshot; if one did, the lock is released, that backup is discarded and
/// a new one is taken, up to [`MAX_BACKUPS`] in all, after which the
/// migration gives up with [`Error::BackupOutrun`]. A backup is kept only
/// when the migration commits.
pub(crate) fn upgrade(
    conn: &mut Connection,
    steps: &[Migration],
    backup: &mut dyn FnMut(&Connection, i64) -> Result<BackupFile>,
) -> Result<i64> {
    let target = steps.last().map_or(0, |s| s.version);
    for _ in 0..MAX_BACKUPS {
        // Read before the backup starts, so a commit that the backup's
        // snapshot misses always changes this value.
        let seen = data_version(conn)?;
        let from = user_version(conn)?;
        if !needs_steps(from, target)? {
            return Ok(from);
        }
        let taken = backup(conn, from).map_err(|e| Error::Backup(Box::new(e)))?;
        match apply_if_unchanged(conn, steps, target, seen) {
            Ok(Some(version)) => return Ok(version),
            Ok(None) => taken.discard()?,
            Err(e) => {
                // Nothing was migrated, so the backup is of no use. If it
                // can't be removed it stays: a valid copy, and harmless.
                let _ = taken.discard();
                return Err(e);
            }
        }
    }
    Err(Error::BackupOutrun(MAX_BACKUPS))
}

/// `None` means another connection committed after `seen` was read, so the
/// backup may be stale; nothing was applied.
fn apply_if_unchanged(
    conn: &mut Connection,
    steps: &[Migration],
    target: i64,
    seen: i64,
) -> Result<Option<i64>> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if data_version(&tx)? != seen {
        return Ok(None);
    }
    // Re-read under the lock: another process may have migrated already.
    let mut version = user_version(&tx)?;
    if !needs_steps(version, target)? {
        return Ok(Some(version));
    }
    let from = version;
    for step in steps.iter().filter(|s| s.version > from) {
        tx.execute_batch(step.sql)?;
        tx.pragma_update(None, "user_version", step.version)?;
        version = step.version;
    }
    tx.commit()?;
    Ok(Some(version))
}

/// Whether a garage at `version` is behind `target`. One ahead of it was
/// written by a newer binary and is refused (replay 4).
fn needs_steps(version: i64, target: i64) -> Result<bool> {
    if version > target {
        return Err(Error::TooNew {
            found: version,
            supported: target,
        });
    }
    Ok(version < target)
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
}

/// Differs between two reads on one connection exactly when another
/// connection committed in between; this connection's own commits don't
/// change it (sqlite.org/pragma.html#pragma_data_version).
fn data_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.pragma_query_value(None, "data_version", |r| r.get(0))
}

// Record tables: `source` is limited to record kinds (backstop to G2's
// writer split). Every table is append-only by trigger (G5).
const V1: &str = "
CREATE TABLE vehicle (
    id           INTEGER PRIMARY KEY,
    source       TEXT NOT NULL CHECK (source IN ('maintainer-entry', 'imported-note')),
    recorded_at  TEXT NOT NULL,
    diagwire_rev TEXT NOT NULL
) STRICT;

CREATE TABLE vehicle_ident (
    id            INTEGER PRIMARY KEY,
    vehicle_id    INTEGER NOT NULL REFERENCES vehicle(id),
    kind          TEXT NOT NULL CHECK (kind IN ('label', 'alias', 'vin', 'chassis')),
    value         TEXT NOT NULL,
    value_norm    TEXT NOT NULL CHECK (value_norm <> ''),
    supersedes_id INTEGER UNIQUE REFERENCES vehicle_ident(id),
    retracted     INTEGER NOT NULL DEFAULT 0 CHECK (retracted IN (0, 1)),
    source        TEXT NOT NULL CHECK (source IN ('maintainer-entry', 'imported-note')),
    recorded_at   TEXT NOT NULL,
    diagwire_rev  TEXT NOT NULL,
    CHECK (retracted = 0 OR supersedes_id IS NOT NULL)
) STRICT;
CREATE INDEX vehicle_ident_norm ON vehicle_ident(value_norm);

CREATE TABLE service_event (
    id            INTEGER PRIMARY KEY,
    vehicle_id    INTEGER NOT NULL REFERENCES vehicle(id),
    event_date    TEXT NOT NULL,
    summary       TEXT NOT NULL,
    odo_value     INTEGER CHECK (odo_value >= 0),
    odo_unit      TEXT CHECK (odo_unit IN ('mi', 'km')),
    supersedes_id INTEGER UNIQUE REFERENCES service_event(id),
    retracted     INTEGER NOT NULL DEFAULT 0 CHECK (retracted IN (0, 1)),
    source        TEXT NOT NULL CHECK (source IN ('maintainer-entry', 'imported-note')),
    recorded_at   TEXT NOT NULL,
    diagwire_rev  TEXT NOT NULL,
    CHECK ((odo_value IS NULL) = (odo_unit IS NULL)),
    CHECK (retracted = 0 OR supersedes_id IS NOT NULL)
) STRICT;
CREATE INDEX service_event_vehicle ON service_event(vehicle_id);

CREATE TRIGGER vehicle_no_update BEFORE UPDATE ON vehicle
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER vehicle_no_delete BEFORE DELETE ON vehicle
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER vehicle_ident_no_update BEFORE UPDATE ON vehicle_ident
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER vehicle_ident_no_delete BEFORE DELETE ON vehicle_ident
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER service_event_no_update BEFORE UPDATE ON service_event
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER service_event_no_delete BEFORE DELETE ON service_event
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;

-- A correction stays on its target's vehicle.
CREATE TRIGGER vehicle_ident_same_vehicle BEFORE INSERT ON vehicle_ident
WHEN NEW.supersedes_id IS NOT NULL
 AND (SELECT vehicle_id FROM vehicle_ident WHERE id = NEW.supersedes_id) IS NOT NEW.vehicle_id
BEGIN SELECT RAISE(ABORT, 'supersede must stay on the same vehicle'); END;
CREATE TRIGGER service_event_same_vehicle BEFORE INSERT ON service_event
WHEN NEW.supersedes_id IS NOT NULL
 AND (SELECT vehicle_id FROM service_event WHERE id = NEW.supersedes_id) IS NOT NEW.vehicle_id
BEGIN SELECT RAISE(ABORT, 'supersede must stay on the same vehicle'); END;
";

// Observation tables. A session's `source` is limited to the measured kind
// this binary writes (backstop to G2's writer split); a snapshot and its
// DTC rows carry provenance through the NOT NULL link to their session
// (G4). A session records at insert whether its reported VIN or chassis
// disagreed with its vehicle's (G7). `vehicle_id` is nullable for a session
// bound to no vehicle, which no writer creates yet.
// `outcome` mirrors DR-0001 D10. Append-only by trigger (G5).
const V2: &str = "
CREATE TABLE session (
    id               INTEGER PRIMARY KEY,
    vehicle_id       INTEGER REFERENCES vehicle(id),
    reported_vin     TEXT,
    reported_chassis TEXT,
    vin_conflict     INTEGER NOT NULL CHECK (vin_conflict IN (0, 1)),
    chassis_conflict INTEGER NOT NULL CHECK (chassis_conflict IN (0, 1)),
    source           TEXT NOT NULL CHECK (source = 'diagwire-read'),
    recorded_at      TEXT NOT NULL,
    diagwire_rev     TEXT NOT NULL
) STRICT;
CREATE INDEX session_vehicle ON session(vehicle_id);

CREATE TABLE fault_snapshot (
    id          INTEGER PRIMARY KEY,
    session_id  INTEGER NOT NULL REFERENCES session(id),
    ecu_address INTEGER NOT NULL CHECK (ecu_address BETWEEN 0 AND 255),
    outcome     TEXT NOT NULL CHECK (outcome IN
                    ('complete', 'negative', 'pending-expired', 'no-response', 'malformed')),
    nrc         INTEGER CHECK (nrc BETWEEN 0 AND 255),
    read_at     TEXT NOT NULL,
    CHECK ((outcome = 'negative') = (nrc IS NOT NULL))
) STRICT;
CREATE INDEX fault_snapshot_session ON fault_snapshot(session_id);

CREATE TABLE fault_dtc (
    id          INTEGER PRIMARY KEY,
    snapshot_id INTEGER NOT NULL REFERENCES fault_snapshot(id),
    code        INTEGER NOT NULL CHECK (code BETWEEN 0 AND 4294967295),
    status      INTEGER NOT NULL CHECK (status BETWEEN 0 AND 255)
) STRICT;
CREATE INDEX fault_dtc_snapshot ON fault_dtc(snapshot_id);

CREATE TRIGGER session_no_update BEFORE UPDATE ON session
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER session_no_delete BEFORE DELETE ON session
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER fault_snapshot_no_update BEFORE UPDATE ON fault_snapshot
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER fault_snapshot_no_delete BEFORE DELETE ON fault_snapshot
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER fault_dtc_no_update BEFORE UPDATE ON fault_dtc
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER fault_dtc_no_delete BEFORE DELETE ON fault_dtc
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;

-- Only a complete read has a fault list.
CREATE TRIGGER fault_dtc_only_complete BEFORE INSERT ON fault_dtc
WHEN (SELECT outcome FROM fault_snapshot WHERE id = NEW.snapshot_id) IS NOT 'complete'
BEGIN SELECT RAISE(ABORT, 'only a complete read stores faults'); END;
";

// Notes and vehicle retractions (G5). A note is a chain-bearing record like
// a service event. A vehicle has no chain, so its retraction is a row of
// its own, at most one per vehicle. Both carry provenance on the row (G4),
// limited to record kinds, and are append-only by trigger.
const V3: &str = "
CREATE TABLE note (
    id            INTEGER PRIMARY KEY,
    vehicle_id    INTEGER NOT NULL REFERENCES vehicle(id),
    body          TEXT NOT NULL,
    about_date    TEXT NOT NULL,
    supersedes_id INTEGER UNIQUE REFERENCES note(id),
    retracted     INTEGER NOT NULL DEFAULT 0 CHECK (retracted IN (0, 1)),
    source        TEXT NOT NULL CHECK (source IN ('maintainer-entry', 'imported-note')),
    recorded_at   TEXT NOT NULL,
    diagwire_rev  TEXT NOT NULL,
    CHECK (retracted = 0 OR supersedes_id IS NOT NULL)
) STRICT;
CREATE INDEX note_vehicle ON note(vehicle_id);

CREATE TABLE vehicle_retraction (
    id           INTEGER PRIMARY KEY,
    vehicle_id   INTEGER NOT NULL UNIQUE REFERENCES vehicle(id),
    reason       TEXT NOT NULL,
    source       TEXT NOT NULL CHECK (source IN ('maintainer-entry', 'imported-note')),
    recorded_at  TEXT NOT NULL,
    diagwire_rev TEXT NOT NULL
) STRICT;

CREATE TRIGGER note_no_update BEFORE UPDATE ON note
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER note_no_delete BEFORE DELETE ON note
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER vehicle_retraction_no_update BEFORE UPDATE ON vehicle_retraction
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;
CREATE TRIGGER vehicle_retraction_no_delete BEFORE DELETE ON vehicle_retraction
BEGIN SELECT RAISE(ABORT, 'garage records are append-only'); END;

-- A correction stays on its target's vehicle.
CREATE TRIGGER note_same_vehicle BEFORE INSERT ON note
WHEN NEW.supersedes_id IS NOT NULL
 AND (SELECT vehicle_id FROM note WHERE id = NEW.supersedes_id) IS NOT NEW.vehicle_id
BEGIN SELECT RAISE(ABORT, 'supersede must stay on the same vehicle'); END;
";
