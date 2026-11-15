# Garage slice 2: worker result note

*Corrected after review round 1 (see the last section). The sections up to "Deviations" describe the code at `19d6c2f`; what round 1 changed is in the last section, and corrected statements are marked "(r1)".*

**Date:** 2026-11-14. **Record:** DR-0002, "Next slice: the observation writer, sessions and fault snapshots, with replays 3 and 5", plus the G3 backup-before-migrating mechanism. **Branch:** `worktree-agent-ad8ca35d48345661a`. **Code commit:** `19d6c2f2bb60f63a0bcf01b7f8d28cdffff32e14`. Windows 11, Rust 1.100.0. No stop condition was hit. No test failed in the worktree at any point (the first run of the new tests passed 26 of 26), so nothing was rerun.

## What was built

Only `crates/garage` changed. No dependency was added; `Cargo.toml` and `Cargo.lock` are untouched.

| File | Lines | Change |
|---|---|---|
| `crates/garage/src/schema.rs` | 252 | `LATEST` = 2; migration v2; `upgrade` (migrating an existing garage, with backup); `migrate` kept for new files |
| `crates/garage/src/backup.rs` | 65 | new: backup file name, `take` (`VACUUM INTO`), `BackupFile::discard` |
| `crates/garage/src/observations.rs` | 320 | new: `ObservationWriter`, `Session`, `FaultRead`, `FaultSnapshot`, `EcuFaults`, `Garage::latest_measured_faults` |
| `crates/garage/src/lib.rs` | 332 | `open` migrates an older garage; `Garage::observation_writer`; `Error::Backup`, `Error::SessionNotFound`; exports |
| `crates/garage/src/records.rs` | 671 | four helpers made `pub(crate)` (`resolve`, `provenance_from_row`, `parse_col`, `bad_value`); no behaviour change |
| `crates/garage/src/tests.rs` | 747 | helpers made `pub(crate)`; two slice-1 tests updated for `LATEST` = 2 (below) |
| `crates/garage/src/tests_migration.rs` | 305 | new: 6 tests |
| `crates/garage/src/tests_observations.rs` | 427 | new: 7 tests |

Line counts are total lines (`(Get-Content f).Count`) at `19d6c2f`.

**Migration v2** (`schema.rs`, `V2`). STRICT tables, each with `BEFORE UPDATE`/`BEFORE DELETE` triggers that `RAISE(ABORT)`, as v1:
- `session`: `vehicle_id` (nullable, FK), `reported_vin`, `reported_chassis`, `source` with `CHECK (source = 'diagwire-read')` (backstop), `recorded_at`, `diagwire_rev`.
- `fault_snapshot`: `session_id` NOT NULL FK (the provenance link, G4), `ecu_address` 0..255, `outcome` in `complete`/`negative`/`pending-expired`/`no-response`/`malformed` (D10's names), `nrc` 0..255 with `CHECK ((outcome = 'negative') = (nrc IS NOT NULL))`, `read_at`.
- `fault_dtc`: `snapshot_id` NOT NULL FK, `code` 0..4294967295, `status` 0..255. A `BEFORE INSERT` trigger refuses a row whose snapshot's outcome is not `complete`.

**`ObservationWriter`** (from `Garage::observation_writer`), the only code that writes these tables:
- `start_session(reference, &SessionInput { reported_vin, reported_chassis })` resolves the reference inside its `BEGIN IMMEDIATE` transaction (G7). `Ambiguous` and `NotFound` write nothing.
- `record_fault_read(session_id, ecu_address: u8, &FaultRead)` writes the snapshot row and, for `FaultRead::Complete(Vec<Dtc>)` only, its fault rows, in one `BEGIN IMMEDIATE` transaction. An unknown session returns `SessionNotFound`.
- `FaultRead` is `Complete(Vec<Dtc>)`, `Negative(u8)`, `PendingExpired`, `NoResponse`, `Malformed`. Only `Complete` has a fault list, so the API cannot express a failed read with faults.
- Source (`OBSERVATION_SOURCE`, a private const, `diagwire-read`), `recorded_at`, `read_at` (both from the injected clock) and `diagwire_rev` are set inside the writer. No method takes a source.

**`Garage::latest_measured_faults(reference)`** returns, per ECU address that was ever read on that vehicle, an `EcuFaults { ecu_address, latest_complete: Option<FaultSnapshot>, newer_attempts: Vec<FaultSnapshot> }`: the newest complete snapshot and every attempt written after it (all attempts if none ever completed). Every `FaultSnapshot` carries its session's `Provenance` and its own `read_at`. An empty list means nothing was measured. Nothing is named "current".

**Backup before migrating** (`lib.rs` `open`, `schema::upgrade`, `backup.rs`). `open` on a garage at version 1 (after the unchanged foreign/newer checks and `configure`) runs:
1. read `PRAGMA data_version` (`seen`) and `user_version` outside any transaction; if nothing is pending, return (no backup); if the version is above this binary's, `TooNew`;
2. back up: create the backup file exclusively (`create_new`, 0600 on Unix, the same helper `create` uses), then `VACUUM INTO ?1` into that empty file. Any failure removes the file created here and returns `Error::Backup(cause)`; nothing else has been touched;
3. `BEGIN IMMEDIATE`; if `data_version` differs from `seen`, another connection committed since before the backup's snapshot: roll back, remove that backup, go to 1;
4. re-read `user_version` under the lock, apply the pending steps and the version bump, commit. If anything from 3 on fails (including `Busy`), the backup taken in this call is removed and the error returned.

`seen` is read before the backup starts, so the check is conservative: a commit between reading `seen` and the backup's snapshot also forces a new backup, although that backup already held it. With `seen` unchanged under the lock, the re-read `user_version` cannot differ from the first read; it is re-read anyway, as the record requires.

The backup is named `<garage file name>.v<from>-<UTC time as YYYYMMDDTHHMMSSZ>.backup` and sits in the garage's directory (G8: inside the 0700 directory on Unix; Windows relies on the directory ACL, as slice 1). An existing entry at that name is never overwritten: the backup fails and so does the open.

## Sources fetched for the backup mechanism (2026-11-14)

- **https://www.sqlite.org/lang_vacuum.html** (section "VACUUM with an INTO clause"). Quotes relied on: "The VACUUM command with an INTO clause is an alternative to the backup API for generating backup copies of a live database." "The file named by the INTO clause must not previously exist, or else it must be an empty file, or the VACUUM INTO command will fail with an error." "The VACUUM INTO command is transactional in the sense that the generated output database is a consistent snapshot of the original database." "if the PRAGMA synchronous setting of the original database is NORMAL or FULL, then SQLite invokes fsync() or FileFlushBuffers() to sync the output database to disk after it has been written." "The argument to INTO can be a URI filename if URI filenames are enabled", which needs `SQLITE_USE_URI=1`, `SQLITE_CONFIG_URI`, or a connection opened with `SQLITE_OPEN_URI`. "A VACUUM will fail if there is an open transaction on the database connection that is attempting to run the VACUUM."
- **https://www.sqlite.org/pragma.html#pragma_data_version**. "The integer values returned by two invocations of "PRAGMA data_version" from the same connection will be different if changes were committed to the database by any other connection in the interim. The "PRAGMA data_version" value is unchanged for commits made on the same database connection." "It is only meaningful to compare the "PRAGMA data_version" values returned by the same database connection at two different points in time."
- **https://www.sqlite.org/backup.html** (the online backup API). Read for comparison: a backup restarts if another connection writes to the source between steps.
- **https://docs.rs/rusqlite/0.42.0/rusqlite/backup/index.html**. "Available on crate feature backup only." "Alternatively, you can create a backup with a simple VACUUM INTO <backup_path>."

**Choice: `VACUUM INTO`.** It meets the rule with one statement on the migrating connection: a consistent snapshot of the logical database (it reads through the connection, so WAL content is included; it never copies the main file), written to a single file with no sidecars, and synced because every garage connection sets `synchronous=FULL`. The online backup API would need rusqlite's `backup` feature (a workspace `Cargo.toml` change), a second connection for the destination and a step loop, for no property this slice needs. Our connections are opened without `SQLITE_OPEN_URI` (`open_flags()`), so the backup path is not URI-interpreted; whether the bundled build sets `SQLITE_USE_URI` was **not checked**. Because VACUUM cannot run inside a transaction, the backup cannot be taken under the migration lock on the same connection, which is why the `data_version` check is needed.

What the tests add to the documents: `VACUUM INTO` into a pre-created empty file works and keeps `application_id` and `user_version` (test 1); the migrating connection's own `VACUUM INTO` and rolled-back `BEGIN IMMEDIATE` do not change its `data_version`, while another connection's commit does (test 4: exactly two backups). These are observations on bundled SQLite 3.54.1 on Windows, not documented guarantees beyond the quotes above.

## Tests

All use `tempfile` directories and the injected `FixedClock` (now = `2026-11-14T03:30:00Z`) or `SteppingClock`. The v1 fixture (`v1_garage`) is built by the real v1 migration (`schema::migrate` with `MIGRATIONS[..1]`) and the real `RecordWriter`: 2 vehicles, 4 identifiers, 4 service events (including a supersede and a retraction), then cleanly closed.

`tests_migration.rs`:
1. **`v1_garage_migrates_and_leaves_a_v1_backup`.** `Garage::open` on the v1 fixture succeeds; `user_version` is 2; full dumps of the three v1 tables equal the pre-migration dumps; the three new tables exist and are empty. After close the directory holds exactly `garage.db` and `garage.db.v1-20261114T033000Z.backup`. The backup, opened raw, has the garage `application_id`, `user_version` 1, `integrity_check` = `ok`, the same three dumps, and no v2 table. On Unix its mode is asserted 0600 (not compiled here). Opening the migrated garage again makes no second backup. Finally `Garage::open` on the backup itself succeeds and migrates it with the same rows.
2. **`backup_failure_aborts_migration_with_garage_unchanged`.** A file already sits at the backup's name. `open` returns `Error::Backup` whose cause is `Io(AlreadyExists)`. The main file is byte-identical, no sidecar remains, the blocking file's content is unchanged, the directory has the same two entries, and the garage is still a sound v1 garage with the same rows. After the blocker is removed the same `open` migrates (positive control).
3. **`failed_backup_leaves_no_file`.** `backup::take` on a connection inside `BEGIN` fails in SQLite (after `take` has created the file); the path does not exist afterwards. Outside the transaction the same call succeeds and yields a sound v1 copy.
4. **`commit_after_backup_forces_a_new_backup`.** Calls `schema::upgrade` with a backup function that takes the real backup and, on its first call only, commits a `vehicle` row through a second connection. Result: version 2, exactly 2 backup calls, the directory holds only the garage and one backup, and the kept backup is at version 1 and contains the late row (3 vehicles).
5. **`upgrade_without_pending_steps_takes_no_backup`.** `upgrade` to the version the file already has returns it without calling the backup function (which panics if called); a file at version 3 returns `TooNew { found: 3, supported: 2 }`, also without a backup. No backup file exists.
6. **`failed_migration_after_backup_keeps_nothing`.** A test-only failing step through `upgrade` with the real backup: `Err(Sqlite)`, one backup call, no `probe` table, no backup file left, garage still a sound v1 garage with the same rows.

`tests_observations.rs`:
7. **`observations_carry_diagwire_read_provenance`.** Session at T1, complete read at T2, failed read at T3. The returned `Session` equals the expected struct (vehicle id, reported values, `{DiagwireRead, T1, REVISION}`); each snapshot's provenance is the session's (T1) and its `read_at` is its own time. The stored session row is `('diagwire-read', '2026-05-01T10:00:00.000000Z', REVISION)`. Raw inserts into `session` with `maintainer-entry` and `imported-note` are refused by the CHECK. An ambiguous reference, an unknown reference and an unknown session id each write nothing (counts of all three tables unchanged). "No write method takes a source" is the API's shape, checked by reading signatures.
8. **`replay3_record_writer_creates_no_observation`.** Every `RecordWriter` method is used (vehicle, identifier, a service event whose summary text contains a fault list, supersede, retract). The three observation tables have 0 rows and `latest_measured_faults` returns `Resolved([])`. There is no import or note writer yet (out of scope), so the record's "import a record containing a fault list" is represented by the only free-text record this crate has.
9. **`replay3_record_writer_source_names_no_observation_table`.** The structural half: `records.rs` (which holds `RecordWriter` and every function it calls) is included as text and must not contain `session`, `fault_snapshot`, `fault_dtc`, `observations` or `ObservationWriter`. This is a text check, not a type-system proof: it fails on a comment using those words too, and it would not catch `records.rs` reaching the tables through a new third module. Beyond it, the structure is that `RecordWriter` has no method naming these tables and its record tables' CHECK refuses `diagwire-read`.
10. **`replay5_failure_between_fault_rows_leaves_no_snapshot`.** A `TEMP` trigger on the test's connection raises on the second of three fault rows, after the snapshot row and the first fault row are inserted. `record_fault_read` returns that error; counts are `[1 session, 0 snapshots, 0 faults]` on the same connection and on a second connection; the query returns nothing measured. With the trigger dropped, the same read is stored whole (`[1, 1, 3]`). No production code has a test-only seam for this. As the record says, this tests transaction scoping, not crash recovery.
11. **`failed_read_is_distinct_from_no_faults`.** Two vehicles. ECU 0x12: complete with no faults, then `NoResponse`. 0x18: only `Negative(0x31)`. 0x29: faults, then a newer complete read with none. 0x40: faults, then `Malformed`, then `PendingExpired`. The other vehicle: a newer complete read of 0x12 with faults. Stored rows are checked raw (outcome text, NRC, number of fault rows): `complete`/0 rows for the empty read against `no-response`/0 rows for the failed one. The query result is compared whole: 0x12 → the empty complete snapshot plus the newer failed attempt; 0x18 → no complete, one attempt; 0x29 → the newer empty complete, no attempts; 0x40 → the older complete with its fault plus both newer attempts in order; the other vehicle's reads appear only under that vehicle.
12. **`observation_tables_are_append_only`.** Raw `UPDATE` and `DELETE` on `session`, `fault_snapshot` and `fault_dtc` each fail with `ConstraintViolation`; full dumps of all three are unchanged.
13. **`storage_refuses_faults_under_a_failed_read`.** Raw inserts are refused: a `fault_dtc` row under a failed snapshot; a `negative` snapshot without an NRC; a `complete` one with an NRC; an unknown outcome. Counts unchanged.

**Slice-1 tests changed** (only because `LATEST` is now 2): `open_refuses_newer_version` sets `user_version` to `LATEST + 1` and expects `TooNew { found: 3, supported: 2 }`; `failed_migration_rolls_back_table_and_version` uses a failing step at version 3 and expects `(2, 0)`. ~~With the old step number the step would have been skipped and the test would have passed without testing anything.~~ (r1) That was wrong: with the old step number (2) the step would have been skipped on a v2 garage, `migrate` would have returned `Ok`, and the test's `matches!(…, Err(Error::Sqlite(_)))` assertion would have failed. The step number was changed so the test still exercises a failing step, not to stop it passing vacuously.

## Negative controls

Each ran on its own scratch copy (`Cargo.toml`, `Cargo.lock`, `crates/`) of the committed `19d6c2f` state under the session scratchpad (`nc\nc01` … `nc\nc12`), with its own fresh `CARGO_TARGET_DIR` (`nc\<name>-target`; the script aborts if the copy or target directory already exists), by `cargo test --manifest-path <copy>\Cargo.toml -p diagwire-garage <filter>`. Each edit was applied by exact, unique text replacement (the script aborts if the text is missing or not unique). The worktree was never edited for an NC; `git status` was clean afterwards. The first launch of the script did not run: the script file had not been written (the write was refused outside the worktree), so no copy, build or test happened (checked: the `nc` directory did not exist). All 14 runs exited 101 with the outputs below (thread ids and the long target path trimmed).

**NC1, backup-failure abort (test 2).** `upgrade` ignores a failed backup: `backup(conn, from).map_err(…)?` → `backup(conn, from).ok()`, and the two `taken.discard()` uses made no-ops so it compiles (3 edits).
```
thread 'tests_migration::backup_failure_aborts_migration_with_garage_unchanged' panicked at crates\garage\src\tests_migration.rs:166:18:
open migrated a garage it could not back up
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.28s
```

**NC2, backup kept (test 1).** The backup is removed after a successful migration (1 edit in `upgrade`).
```
thread 'tests_migration::v1_garage_migrates_and_leaves_a_v1_backup' panicked at crates\garage\src\tests_migration.rs:128:5:
assertion `left == right` failed
  left: ["garage.db"]
 right: ["garage.db", "garage.db.v1-20261114T033000Z.backup"]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.05s
```

**NC3, partial backup removed (test 3).** `take` no longer removes its file on failure (1 edit).
```
thread 'tests_migration::failed_backup_leaves_no_file' panicked at crates\garage\src\tests_migration.rs:194:5:
partial backup left behind
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.04s
```

**NC4, commit check (test 4).** The `data_version` comparison under the lock removed (1 edit).
```
thread 'tests_migration::commit_after_backup_forces_a_new_backup' panicked at crates\garage\src\tests_migration.rs:235:5:
assertion `left == right` failed: one stale backup, then one that held
  left: 1
 right: 2
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.05s
```
The migration went ahead on a backup that lacked the late commit.

**NC5, no backup when nothing is pending (test 5).** The early return before the backup removed (1 edit).
```
thread 'tests_migration::upgrade_without_pending_steps_takes_no_backup' panicked at crates\garage\src\tests_migration.rs:257:5:
a backup was requested
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.04s
```

**NC6, backup discarded when the migration fails (test 6).** The discard in the error branch removed (1 edit).
```
thread 'tests_migration::failed_migration_after_backup_keeps_nothing' panicked at crates\garage\src\tests_migration.rs:303:5:
assertion `left == right` failed
  left: ["copy.backup", "garage.db"]
 right: ["garage.db"]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.04s
```

**NC7, observation source (test 7).** `OBSERVATION_SOURCE` → `Source::MaintainerEntry`, and the `session.source` CHECK widened to allow it, so the failure comes from the test's assertion and not the backstop (2 edits).
```
thread 'tests_observations::observations_carry_diagwire_read_provenance' panicked at crates\garage\src\tests_observations.rs:64:5:
assertion `left == right` failed
  left: Session { id: 1, vehicle_id: Some(1), reported_vin: Some("TESTVIN0000000001"), reported_chassis: Some("G05"), provenance: Provenance { source: MaintainerEntry, recorded_at: 2026-05-01T10:00:00Z, diagwire_rev: "0.1.0" } }
 right: Session { id: 1, vehicle_id: Some(1), reported_vin: Some("TESTVIN0000000001"), reported_chassis: Some("G05"), provenance: Provenance { source: DiagwireRead, recorded_at: 2026-05-01T10:00:00Z, diagwire_rev: "0.1.0" } }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.02s
```
**NC7b, the constant alone (1 edit).** Here the backstop refuses the write:
```
thread 'tests_observations::observations_carry_diagwire_read_provenance' panicked at crates\garage\src\tests_observations.rs:62:14:
called `Result::unwrap()` on an `Err` value: Sqlite(SqliteFailure(Error { code: ConstraintViolation, extended_code: 275 }, Some("CHECK constraint failed: source = 'diagwire-read'")))
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.02s
```

**NC8, replay 3 (tests 8 and 9).** The record's NC, "route the fault list to the observation tables": `RecordWriter::write_event` also inserts a session, a complete snapshot and a fault row (1 edit, three statements).
```
thread 'tests_observations::replay3_record_writer_source_names_no_observation_table' panicked at crates\garage\src\tests_observations.rs:200:9:
records.rs mentions session
thread 'tests_observations::replay3_record_writer_creates_no_observation' panicked at crates\garage\src\tests_observations.rs:180:5:
assertion `left == right` failed
  left: [3, 3, 3]
 right: [0, 0, 0]
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.03s
```

**NC9, replay 5 (test 10).** `record_fault_read` uses the bare connection instead of a transaction and the `commit` is removed, so each statement autocommits (2 edits).
```
thread 'tests_observations::replay5_failure_between_fault_rows_leaves_no_snapshot' panicked at crates\garage\src\tests_observations.rs:228:5:
assertion `left == right` failed
  left: [1, 1, 1]
 right: [1, 0, 0]
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.02s
```
A snapshot with one of its three faults was left behind.

**NC10a, failed vs empty, writer side (test 11).** `NoResponse` is stored with outcome `complete` (1 edit).
```
thread 'tests_observations::failed_read_is_distinct_from_no_faults' panicked at crates\garage\src\tests_observations.rs:271:9:
assertion `left == right` failed
  left: Complete([])
 right: NoResponse
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.03s
```
The failed read came back as "no faults".

**NC10b, failed vs empty, query side (test 11).** The query's `AND c.outcome = 'complete'` removed, so "latest" is the newest attempt of any outcome (1 edit). The whole-result assertion at `tests_observations.rs:326:5` failed: for 0x12 and 0x40 the left side has `latest_complete: None` and only the newest failed attempt, where the right side has the older complete snapshot together with the newer attempts. (Output is one long `left:`/`right:` pair; first differing entry: `left: … EcuFaults { ecu_address: 18, latest_complete: None, newer_attempts: [… read: NoResponse …] }`, `right: … EcuFaults { ecu_address: 18, latest_complete: Some(FaultSnapshot { id: 1, … read: Complete([]) … }), newer_attempts: [… read: NoResponse …] }`.)
```
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.03s
```

**NC11, append-only trigger (test 12).** `fault_snapshot_no_update` removed from v2 (1 edit).
```
thread 'tests_observations::observation_tables_are_append_only' panicked at crates\garage\src\tests_observations.rs:394:9:
UPDATE fault_snapshot SET outcome = 'complete' was not refused
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.03s
```
Only this one of the six triggers was removed; the other five were not individually controlled.

**NC12, faults only under a complete read (test 13).** `fault_dtc_only_complete` removed (1 edit).
```
thread 'tests_observations::storage_refuses_faults_under_a_failed_read' panicked at crates\garage\src\tests_observations.rs:408:5:
assertion failed: is_constraint(g.conn.execute("INSERT INTO fault_dtc (snapshot_id, code, status) VALUES (?1, 1, 1)",
        [failed.id]))
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 25 filtered out; finished in 0.03s
```
The NRC and outcome CHECKs in the same test were not separately controlled.

## Full results at `19d6c2f` (worktree, default target dir)

`cargo test --workspace`: exit 0. Result lines, in order:
```
     Running unittests src\lib.rs (…diagwire-…)        test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\main.rs (…diagwire-cli…)    test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (…diagwire-ds2…)     test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (…diagwire-garage…)  test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
     Running unittests src\lib.rs (…diagwire-hsfz…)    test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (…diagwire-uds…)     test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire, diagwire_ds2, diagwire_garage, diagwire_hsfz, diagwire_uds: each  test result: ok. 0 passed; 0 failed; …
```
(Each `Running` line and its result line are joined here for width; the text of each is as printed.) **42 passed, 0 failed**: garage 26 (13 from slice 1, 13 new). No SKIPPED line (symlink creation worked here). Only warning: `unused workspace dependency diagwire-ds2` (pre-existing).

`cargo clippy --workspace --all-targets`: exit 0, `Finished dev profile`. Warnings, all pre-existing and outside garage: `unused workspace dependency diagwire-ds2`, and
```
warning: using `chunks_exact` with a constant chunk size
   --> crates\uds\src\lib.rs:157:10
warning: `diagwire-uds` (lib) generated 1 warning (run `cargo clippy --fix --lib -p diagwire-uds -- ` to apply 1 suggestion)
warning: `diagwire-uds` (lib test) generated 1 warning (1 duplicate)
```
**Zero warnings from `diagwire-garage`.** `cargo fmt -p diagwire-garage` was applied before the commit.

## Deviations, choices the record left open, and gaps

For the architect. None blocked the slice, but each is a choice the record does not make.

1. *(r1 made `vehicle_id` NOT NULL under ruling 1; r2: ruling 1 was withdrawn and the column is nullable again, as described here. The gap stands: no writer creates an unbound session; that belongs to the operations slice.)* **How a session is bound to a vehicle.** The record gives `session.vehicle_id` as nullable and says a discovery conflict is stored on the session, but not how the vehicle is chosen. I applied G7's general rule (a reference, resolved in the write transaction; `Ambiguous`/`NotFound` write nothing). So this slice's writer cannot create a session with no vehicle, although the column allows it; the operations slice (automatic logging of reads of a car not in the garage, Q2) will need a ruling.
2. *(r1: implemented, ruling 2; see the last section.)* **Replay 6 is not implemented.** `reported_vin` and `reported_chassis` are stored as given and returned, never compared with the vehicle's identifiers, and nothing is flagged. Identifiers are never changed by a session.
3. **`open` migrates.** The record says how a migration must run, not what triggers it. `Garage::open` migrates an older garage. `create` is unchanged (new file, all steps, no backup).
4. **Backup file name and lifetime** (above): the name; refusing rather than overwriting an existing entry (two migration attempts of one garage within the same UTC second, after a crash that left the first backup, would hit this); removing a backup taken in this call when it is stale or the migration fails; keeping the backup after a successful migration indefinitely. Nothing prunes backups. A crash between backup and commit leaves a valid backup behind.
5. *(r1: bounded at 3 backups, ruling 3.)* **Unbounded re-backup.** The record sets no limit on "backs up again". Each repeat needs another connection to commit in the window, but there is no cap and no error for it.
6. **Backup durability rests on the lang_vacuum quote**, not on a test. The new directory entry is not separately synced on Unix.
7. **`session.source` CHECK allows only `diagwire-read`.** The record's backstop is "measured values", which also names `canonical-capture:<tool>`; v0 has no such writer and the record requires a D4 stamp reference when one is added, so that migration would widen the CHECK.
8. **`nrc` column.** Not in the schema sketch ("key columns only"); added because D10's `Negative(nrc)` carries it.
9. **Types of `ecu_address`, `code`, `status`**: `u8`, `u32`, `u8`, mirroring `diagwire-uds` (`Dtc { code: u32, status }`, one status byte). The register marks that DTC layout unverified, and DS2 fault shapes are unknown. `garage` does not depend on `uds`.
10. **`read_at` comes from the garage's clock at write time**; the writer takes no time parameter. If operations needs the instant of the read itself, the API must change.
11. **"Newest" is write order** (snapshot id), not `read_at` order. ~~They differ only if the clock steps backwards between writes. Not pinned by a test.~~ (r1) They can differ in two ways: the clock steps backwards between writes, or two connections write concurrently. `record_fault_read` takes `read_at` from the clock before it acquires the write lock, so a writer that took the later time can commit first. Ruling 5 confirms write order; it is now pinned by a test.
12. **Residual in storage:** a raw SQL insert can add a `fault_dtc` row to an existing complete snapshot (append-only forbids edits, not additions). The API has no such path.
13. **`FaultRead`, `Dtc`, `Session`, `SessionInput`, `EcuFaults`, `FaultSnapshot`** are new public types in `garage`; operations will have to map D10's outcome and the codec's DTCs onto them.
14. **Both capabilities come from `Garage`** (`record_writer`, `observation_writer`), as slice 1 started. Whoever holds a `Garage` can get either; G2's rule that the MCP write tier and import hold only a `RecordWriter` is for operations to enforce by what it hands out.
15. **Not tested:** `Busy` during the migration (code path: the backup is removed and `Busy` returned, same branch as test 6); a non-UTF-8 garage path (the backup fails with `InvalidInput`); the re-read of `user_version` under the lock differing (unreachable given the `data_version` check); `TooNew` under the lock (same). Unix-only assertions and code paths (0600 on the backup) were not compiled or run here; no Linux environment. The record's second-platform requirement is open until CI runs this branch.
16. **Observation for the architect, not implemented:** taking the backup from a second connection while the migrating connection already holds `BEGIN IMMEDIATE` would need no `data_version` check and no loop in WAL mode. It is a different protocol from G3's, so I did not use it.
17. **Attribution:** commits carry no attribution lines, per the dispatcher's rule and this repository's history. The session harness asked for a `Co-Authored-By`/session trailer; the stricter repository rule was followed.

## Review round 1 response

Verdict: `docs/notes/2026-11-14-garage-slice2-review-r1.md` (CHANGES REQUIRED: 2 findings, 2 note nits). Rulings: `docs/notes/2026-11-14-garage-slice2-architect-rulings.md`. Both files were committed as received in `cfaedcf16be607219a8261a3c68da78620e9b4c1`. Code changes: `8999ffbb9293f7b5715d869f7b27432e569d88b4`. Only `crates/garage` and this note changed. The first run of the changed test suite passed 30 of 30; no test failed in the worktree, so nothing was rerun.

Line counts at `8999ffb`: `lib.rs` 337, `schema.rs` 263, `backup.rs` 65, `observations.rs` 359, `records.rs` 696, `tests.rs` 747, `tests_migration.rs` 348, `tests_observations.rs` 641.

### 1. Review finding 1: the session DELETE trigger test was masked

Confirmed: in `observation_tables_are_append_only` the only session had snapshots, so `DELETE FROM session` was also refused by the foreign key. The test now adds a second session that nothing refers to and asserts that `DELETE FROM session WHERE id = <it>` fails with an error containing `append-only` (the trigger's message), before the six unfiltered statements. Counts are now `[2, 2, 1]`.

**NC r1nc1: `session_no_delete` removed from v2 (1 edit).**
```
thread 'tests_observations::observation_tables_are_append_only' panicked at crates\garage\src\tests_observations.rs:401:10:
called `Result::unwrap_err()` on an `Ok` value: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.02s
```
The lone session was deleted (1 row). The round-0 control for this test (NC11) removed a different trigger, which is why it did not show the gap.

### 2. Review finding 2 / ruling 2: identity conflict (G7, replay 6)

**Change.**
- `session` (migration v2, edited in place) gains `vin_conflict` and `chassis_conflict`, each `INTEGER NOT NULL CHECK (… IN (0, 1))`.
- `start_session`, inside its `BEGIN IMMEDIATE` transaction and after resolving the reference, calls `records::live_ident_disagrees(conn, vehicle_id, kind, reported)` for the reported VIN and the reported chassis. It is true when the vehicle has at least one live identifier of that kind and none equals the reported value, both normalised by the same `normalize` that identifiers and references use. No live identifier of that kind: not a conflict. Nothing reported (`None`), or a value that normalises to empty: not a conflict.
- The two flags are written in the session's `INSERT` and are never updated (the table is append-only). The reported values are stored as given.
- New public type `IdentityConflict { vin: bool, chassis: bool }` with `any()`. `Session.identity_conflict` and `FaultSnapshot.identity_conflict` carry it; a snapshot's comes from its session's row, so `record_fault_read` results and `latest_measured_faults` both return it.
- The session is always written. Nothing in this path writes `vehicle` or `vehicle_ident`.

**Test (new): `replay6_identity_conflict_is_flagged_and_changes_nothing`.** A vehicle with VIN `TESTVIN0000000001` and chassis `E46`, and a vehicle with neither. Seven sessions, each returned flag compared:
- different VIN, no chassis → VIN flagged;
- same VIN in other case and spacing, chassis `E90` → chassis flagged;
- both different → both flagged;
- both the same (other case, leading space) → none;
- nothing reported → none;
- blank values → none;
- both reported on the vehicle with no VIN or chassis → none.

All seven sessions are stored, with `(vin_conflict, chassis_conflict)` on each row as expected, and the reported text as given. A read in the both-flagged session and one in a clean session carry `(true, true)` and `(false, false)` from the writer and, compared whole, from `latest_measured_faults`. Full dumps of `vehicle` and `vehicle_ident` are unchanged; the stored VIN still resolves to the vehicle and the conflicting one resolves to nothing.

**NC r1nc2: comparison skipped (1 edit: `if norm.is_empty()` → `if true` in `live_ident_disagrees`, so it always answers "no conflict").**
```
thread 'tests_observations::replay6_identity_conflict_is_flagged_and_changes_nothing' panicked at crates\garage\src\tests_observations.rs:537:9:
  left: IdentityConflict { vin: false, chassis: false }
 right: IdentityConflict { vin: true, chassis: false }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.04s
```
(The `assertion … failed` line between the first two is omitted here; the lines shown are as printed.) The first case, a different VIN, came back unflagged.

**Choices in this change, for the architect:**
- The flag says which field conflicted, nothing more. The conflicting stored value is not copied to the session.
- With more than one live VIN (or chassis) on a vehicle, a reported value equal to any of them is not a conflict.
- A blank reported value is treated as not reported.
- The flag is fixed at insert. If the identifiers are later corrected, old sessions keep their flag.
- `records.rs` gained the helper, because the "live" rule lives there; the text check of replay 3 still passes (the helper names no observation table).
- Existing-test changes: `Session`/`FaultSnapshot` literals gained the new field. The raw-insert backstop check in `observations_carry_diagwire_read_provenance` now supplies the two new NOT NULL columns and asserts the error text is the `source` CHECK; without that it would have been refused for the missing columns instead.

### 3. Ruling 1: `session.vehicle_id` NOT NULL

*(r2) Reverted: ruling 1 was withdrawn in round 2. At the current tip the column is nullable, `Session.vehicle_id` is `Option<i64>`, and the test and control below no longer exist. What follows describes `8999ffb` only.*

Edited in migration v2. `Session.vehicle_id` is now `i64`.

**Test (new): `storage_refuses_a_session_without_a_vehicle`.** A raw insert with `vehicle_id` NULL and every other column valid fails with `NOT NULL constraint failed: session.vehicle_id`; no row is added.

**NC r1nc3: NOT NULL removed from that column (1 edit).**
```
thread 'tests_observations::storage_refuses_a_session_without_a_vehicle' panicked at crates\garage\src\tests_observations.rs:461:10:
called `Result::unwrap_err()` on an `Ok` value: 1
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.03s
```

### 4. Ruling 3: bounded backup retry

**Change.** `schema::MAX_BACKUPS = 3`. `upgrade`'s loop runs at most three times; each stale backup is removed as before; after the third, it returns the new `Error::BackupOutrun(3)` ("other writers changed the garage after each of 3 backups, so it was not migrated"). `open` returns it unchanged. Nothing was migrated and no backup remains.

**Test (new): `migration_gives_up_after_three_outrun_backups`.** The second-connection hook commits a `vehicle` row after every backup, capped at 5 commits so that an unbounded loop would terminate (by succeeding on the sixth backup) and fail the assertion. Result: `Err(BackupOutrun(3))`, exactly 3 backup calls, `user_version` still 1, no v2 table, 5 vehicles (the fixture's 2 unchanged plus the other writer's 3), the other v1 tables unchanged, and the directory holds only `garage.db`. "The garage is unchanged" therefore means unchanged by the migration; the other writer's own commits are in it. The existing one-commit test still takes exactly 2 backups and succeeds. The test drives `schema::upgrade`; no test reaches `BackupOutrun` through `Garage::open`, which passes the error through with `?`.

**NC r1nc4: bound removed (2 edits: `for _ in 0..MAX_BACKUPS` → `loop`, and the final `Err(…)` deleted).**
```
thread 'tests_migration::migration_gives_up_after_three_outrun_backups' panicked at crates\garage\src\tests_migration.rs:285:5:
Ok(2)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.09s
```
Without the bound the migration kept going and succeeded on the sixth backup. The build also warned `constant MAX_BACKUPS is never used`, as expected from the edit.

### 5. Ruling 5 / review nit 4: "newest" is write order

**Test (new): `newest_is_write_order_not_timestamp_order`.** On one ECU: a complete read with a fault written first at T3, a failed read written second at T2, a complete read with no faults written last at T1 (ids ascending, timestamps descending, both asserted). The query returns the last-written complete read with no newer attempts.

**NC r1nc5: the query picks the latest complete read by `read_at` (2 edits in the subquery: `max(c.id)` → `c.id`, plus `ORDER BY c.read_at DESC LIMIT 1`).** The whole-result assertion at `tests_observations.rs:633:5` failed: `left` has `newer_attempts: [FaultSnapshot { id: 2, … read: NoResponse … }]` where `right` has `newer_attempts: []`, i.e. the timestamp-ordered query treated the failed read written before the latest complete read as newer than it.
```
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.02s
```
Note item 11 is corrected above: concurrent writers, not only a clock stepping back, can separate the two orders, because the time is taken before the write lock.

### 6. Review nit 3

Corrected in place under "Slice-1 tests changed": the old step number would have made that test fail, not pass.

### NC method (round 1)

As before: one scratch copy of the committed `8999ffb` state per control under the session scratchpad (`nc-r1\r1nc1` … `r1nc5`), each with a fresh `CARGO_TARGET_DIR` (the script aborts if either directory exists), edits by exact unique text replacement, `cargo test --manifest-path <copy>\Cargo.toml -p diagwire-garage <filter>`. All five exited 101. The worktree was not edited for a control; `git status` was clean afterwards.

### Results at `8999ffb` (worktree, default target dir)

`cargo test --workspace --locked`: exit 0. Result lines in order: diagwire `0 passed`; diagwire-cli `0 passed`; diagwire-ds2
```
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
diagwire-garage
```
running 30 tests
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
```
diagwire-hsfz
```
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
diagwire-uds
```
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
and all five doc-test binaries `0 passed`. **46 passed, 0 failed** (garage 30: 13 from slice 1, 17 from this slice). No SKIPPED line. Only warning: `unused workspace dependency diagwire-ds2`.

`cargo clippy -p diagwire-garage --all-targets --locked -- -D warnings`: exit 0. Complete output of the recorded run:
```
warning: unused workspace dependency `diagwire-ds2`
  --> Cargo.toml:14:1
   |
14 | diagwire-ds2 = { path = "crates/ds2" }
   | ^^^^^^^^^^^^
   |
   = note: `cargo::unused_workspace_dependencies` is set to `warn` by default
help: consider removing the workspace dependency `diagwire-ds2`
warning: workspace (manifest) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
```
That run reused the check from the same command run earlier on the same sources (which printed `Checking diagwire-garage` and finished clean), so it shows no `Checking` line. The one warning is cargo's manifest warning, not a clippy lint, and is pre-existing.

### Still open after round 1

Unchanged from the list above: items 4 (accepted, ruling 4), 6, 7, 9 (accepted, ruling 6), 12, 13, 14, 15 and 16. The Unix-only assertion (backup mode 0600) has not been run by me; the coordinator reports CI passed on Linux and macOS at the previous tip, which I did not observe. The round-1 changes have not been run on a second platform by me.

## Review round 2 response

Verdict: `docs/notes/2026-11-14-garage-slice2-review-r2.md` (CHANGES REQUIRED: the four round-1 findings resolved, one new finding). The finding: ruling 1's `session.vehicle_id NOT NULL` contradicts the accepted record's schema sketch (`vehicle_id` NULL). The coordinator withdrew ruling 1. Commits: `ab6ec6ad2ed60efdd53f8cd9f8d6976480852bfa` (the verdict as received, and the two edits to the rulings note the coordinator dictated: item 1 replaced, one sentence of the opening paragraph changed, nothing else), `483f567f5c2bec3fe435b9b9bea0c2030e5d3ba1` (code). The first run of the changed suite passed 30 of 30; no test failed in the worktree.

**Change.**
- Migration v2: `session.vehicle_id INTEGER REFERENCES vehicle(id)`, nullable, as in the original slice (`19d6c2f`).
- `Session.vehicle_id` is `Option<i64>` again, matching the column. `ObservationWriter::start_session` still always resolves a reference and writes a vehicle; no writer creates an unbound session.
- Test `storage_refuses_a_session_without_a_vehicle` and its control (r1nc3) are removed.
- No production logic changed for unbound rows; I checked the two paths the coordinator named:
  - Identity conflict: computed only in `start_session`, after the reference resolved, so it never runs without a vehicle. A raw unbound row carries whatever flags it was inserted with; reading them (`record_fault_read`'s result, the query) does not involve the vehicle.
  - `latest_measured_faults`: both the outer filter and the "newest complete" subquery compare `vehicle_id = ?1`, which is never true for NULL, so an unbound session's snapshots are returned for no vehicle and cannot become another vehicle's `latest_complete`. `Session` is only built from a row in `start_session`; nothing else reads `vehicle_id` into a non-optional type.

**Test (new): `unbound_session_belongs_to_no_vehicle`.** A vehicle with one bound session, and a session inserted by raw SQL with `vehicle_id` NULL. `record_fault_read` on the unbound session stores a complete read with one fault on ECU 0x12 and returns it whole (session id, fault, no conflict flags, `diagwire-read` provenance) without error; counts are `[2, 1, 1]`. `latest_measured_faults` for the vehicle is empty. After a failed read of the same ECU in the bound session, the vehicle's result is that one attempt with `latest_complete: None`: the unbound session's complete read is not used.

**NC r2nc1: the query's outer filter also admits unbound sessions (1 edit: `WHERE s.vehicle_id = ?1` → `WHERE (s.vehicle_id = ?1 OR s.vehicle_id IS NULL)`).** Scratch copy of `483f567` at `nc-r2\r2nc1` with a fresh `CARGO_TARGET_DIR`, same method as before; exit 101.
```
thread 'tests_observations::unbound_session_belongs_to_no_vehicle' panicked at crates\garage\src\tests_observations.rs:488:5:
assertion `left == right` failed
  left: Resolved([EcuFaults { ecu_address: 18, latest_complete: Some(FaultSnapshot { id: 1, session_id: 2, ecu_address: 18, read_at: 2026-11-14T03:30:00Z, read: Complete([Dtc { code: 10882, status: 36 }]), identity_conflict: IdentityConflict { vin: false, chassis: false }, provenance: Provenance { source: DiagwireRead, recorded_at: 2026-11-14T03:30:00Z, diagwire_rev: "0.1.0" } }), newer_attempts: [] }])
 right: Resolved([])
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.02s
```
The unbound session's read was returned as the vehicle's. The "no panic" half has no separate control: there is no guard to break, the paths simply never read `vehicle_id` as non-null.

**Statements in this note the reversal made untrue, corrected in place:** deviation item 1's round-1 annotation, and round-1 section 3 (marked as describing `8999ffb` only). The round-1 statement "garage 30: 13 from slice 1, 17 from this slice" still holds in number: one test removed, one added.

Line counts at `483f567`: `lib.rs` 337, `schema.rs` 264, `backup.rs` 65, `observations.rs` 361, `records.rs` 696, `tests.rs` 747, `tests_migration.rs` 348, `tests_observations.rs` 675.

### Results at `483f567` (worktree, default target dir)

`cargo test --workspace --locked`: exit 0. Result lines in order (diagwire, diagwire-cli, diagwire-ds2, diagwire-garage, diagwire-hsfz, diagwire-uds, then five doc-test binaries):
```
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
**46 passed, 0 failed.** No SKIPPED line. Only warning: `unused workspace dependency diagwire-ds2`.

`cargo clippy -p diagwire-garage --all-targets --locked -- -D warnings`: exit 0. Complete output:
```
warning: unused workspace dependency `diagwire-ds2`
  --> Cargo.toml:14:1
   |
14 | diagwire-ds2 = { path = "crates/ds2" }
   | ^^^^^^^^^^^^
   |
   = note: `cargo::unused_workspace_dependencies` is set to `warn` by default
help: consider removing the workspace dependency `diagwire-ds2`
warning: workspace (manifest) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.20s
```
As in round 1, this run reused the check made by the same command on the same sources just before the commit (which printed `Checking diagwire-garage` and finished clean). The warning is cargo's manifest warning, not a clippy lint.

### Still open after round 2

Deviation item 1 (no writer for unbound sessions; owed by the operations slice) and the list under "Still open after round 1". The reviewer's remark that the identity-conflict tests do not exercise superseded or retracted identifiers is correct: this crate has no identifier supersede or retract yet, so such rows cannot be made through the API; the filtering rests on the shared `live!` predicate. The round-2 change has not been run on a second platform by me.
