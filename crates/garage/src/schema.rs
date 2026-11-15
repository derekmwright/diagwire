//! Forward-only migrations, embedded in the binary. Each table is created by
//! the migration that lands its first writer (G3).

use rusqlite::{Connection, TransactionBehavior};

use crate::{APPLICATION_ID, Result};

pub(crate) struct Migration {
    pub version: i64,
    pub sql: &'static str,
}

pub(crate) const LATEST: i64 = 1;

pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: V1,
}];

/// Applies every step newer than the file's `user_version`, and the version
/// bump, in one `BEGIN IMMEDIATE` transaction. Version 0 is a new file, which
/// also gets its `application_id` in that transaction.
pub(crate) fn migrate(conn: &mut Connection, steps: &[Migration]) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Re-read under the lock: another process may have migrated already.
    let mut version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
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
