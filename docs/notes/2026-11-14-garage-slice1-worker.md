# Garage slice 1: worker result note

**Date:** 2026-11-14. **Record:** DR-0002, First slice. **Branch:** `worktree-agent-a66a61805c1f22b57`. **Code commit:** `0dfb48497f198b2a570d69023eb27f9a69cd9767`. Windows 11, Rust 1.100.0. No stop condition was hit.

## What was built

New crate `crates/garage` (package `diagwire-garage`). No other crate was changed.

| File | Lines | Contents |
|---|---|---|
| `Cargo.toml` (workspace) | | added `jiff = "0.2"`, `rusqlite = { version = "=0.42.0", features = ["bundled"] }`, `tempfile = "3"` to `[workspace.dependencies]` |
| `Cargo.lock` | | jiff 0.2.44, rusqlite 0.42.0, libsqlite3-sys 0.40.0, tempfile 3.27.0 and their dependencies |
| `crates/garage/Cargo.toml` | | version/edition/license inherited from the workspace |
| `crates/garage/src/lib.rs` | 239 | `Garage::create`/`open`, `Options` (busy timeout), `Error`, SQLite floor check, connection configuration |
| `crates/garage/src/clock.rs` | 21 | `Clock` trait (`now()` UTC, `today()` local date) and `SystemClock` |
| `crates/garage/src/schema.rs` | 93 | migration runner and migration v1 (`vehicle`, `vehicle_ident`, `service_event`, append-only triggers) |
| `crates/garage/src/records.rs` | 580 | `RecordWriter`, reference resolution, reads with provenance |
| `crates/garage/src/tests.rs` | 461 | tests 1–7 (9 test functions) |

**`create(path, clock, options)`** refuses if the path or its `-wal`, `-shm` or `-journal` exists. It creates a missing parent directory (0700 on Unix), creates the file exclusively (`create_new`, 0600 on Unix), opens it without `SQLITE_OPEN_CREATE`, checks the SQLite floor, sets and reads back the pragmas outside any transaction, then writes `application_id`, the v1 schema and `user_version` in one `BEGIN IMMEDIATE` transaction.

**`open(path, clock, options)`** returns `Missing` if there is no file and opens with `READ_WRITE | NO_MUTEX` (no `CREATE`). In order, before any configuration change: SQLite version ≥ 3.51.3, `application_id` = `0x64776772` ("dwgr", 0 is refused), `user_version` not above 1 (`TooNew`), and exactly 1 (`UnknownVersion` otherwise). Only then does it set `busy_timeout`, `foreign_keys=ON`, `synchronous=FULL` and `journal_mode=WAL`, reading each back and failing on a mismatch.

**`RecordWriter`** (from `Garage::record_writer`): `add_vehicle(label)`, `add_identifier(reference, kind, value)`, `add_service_event(reference, input)`, `supersede_service_event(reference, target, input)` and `retract_service_event(reference, target)`. Each is one `BEGIN IMMEDIATE` transaction that resolves the reference inside it. `Ambiguous` and `NotFound` return before any insert. Source (`RECORD_SOURCE`, private const), `recorded_at` (from `clock.now()`, stored as fixed-width `YYYY-MM-DDTHH:MM:SS.ffffffZ`) and `diagwire_rev` are set inside the writer. No write method takes a source. A missing date uses `clock.today()`. `SQLITE_BUSY` maps to `Error::Busy` and is never retried.

**Reads** on `Garage`: `resolve`, `vehicle`, `service_events` (live heads only), `service_event_history` (all rows). Every result carries a `Provenance { source, recorded_at, diagwire_rev }`.

**Schema v1** uses STRICT tables. Each table has `BEFORE UPDATE`/`BEFORE DELETE` triggers that `RAISE(ABORT)`. `supersedes_id` is UNIQUE with a self-FK. There is a `CHECK (retracted = 0 OR supersedes_id IS NOT NULL)`, an odometer pair CHECK (`(odo_value IS NULL) = (odo_unit IS NULL)`, unit ∈ `mi`/`km`), and `source` is limited to `maintainer-entry`/`imported-note` as a backstop. A `BEFORE INSERT` trigger refuses a supersede that crosses vehicles.

## Tests (all in `crates/garage/src/tests.rs`, temp dirs via `tempfile`, `FixedClock`)

The clock returns UTC now = `2026-11-14T03:30:00Z` and local today = `2026-11-13`. The two dates differ, so a test can tell which one a default date came from.

1. **`replay1_ambiguous_reference_writes_nothing`.** Two vehicles share chassis `E46`. `add_service_event("e46")` returns `Ambiguous` listing both, with ids and labels, and the `service_event` count is unchanged. The unique alias `"  The   COUPE "` (normalisation: trim, case-fold, collapse spaces) resolves to the coupe and writes one row. `"e90"` returns `NotFound`.
2. **`provenance_set_by_writer`.** It writes a vehicle, a VIN, a dated event, an undated event, a supersede and a retraction. Every returned `Provenance` equals `{MaintainerEntry, NOW, REVISION}`. Every row in all three tables has `source='maintainer-entry'`, `recorded_at='2026-11-14T03:30:00.000000Z'` and `diagwire_rev=REVISION`. The undated event and the supersede take `event_date = 2026-11-13` (the clock's local today), and the dated one keeps its date. "No public write method accepts a source" is a property of the API's shape, checked by reading the signatures, not at runtime.
3. **`append_only`.** Raw `UPDATE`/`DELETE` on `service_event`, `vehicle_ident` and `vehicle` each fail with `ConstraintViolation`, and full row dumps are unchanged afterwards. A supersede adds a head (the default list shows only the new head). A second supersede of the same row fails in the API (`AlreadySuperseded`) and in storage (a raw insert hits UNIQUE), with the count unchanged. A retracted head is hidden by default and shown in history as `[(first,false),(second,false),(retraction,true)]`. A duplicate live alias on another vehicle (`" Commuter "` vs `commuter`) is refused (`IdentifierTaken(Alias)`), with the count unchanged. An odometer value without a unit is refused by the storage CHECK (raw insert); the API's `Odometer` type can't express one.
4. **`open_refuses_newer_version`, `open_refuses_foreign_and_empty_files`, `open_missing_creates_nothing_and_create_refuses_existing`.** Each refusal is checked on a cleanly closed file (no sidecars beforehand). Afterwards the main file is **byte-identical** (a full byte comparison, stronger than a hash) and no sidecar exists. Cases: `user_version=2` gives `TooNew{2,1}`; `application_id=0x12345678` gives `Foreign`; a database with a header but no application ID gives `Foreign(0)`; a zero-length file gives `Foreign(0)`. `open` on a missing path gives `Missing`, and the directory stays empty. `create` on an existing file gives `Exists` and leaves the contents unchanged. `create` where only a `-wal` exists gives `Exists` and makes no main file.
5. **`failed_migration_rolls_back_table_and_version`.** A test-only step (version 2) runs `CREATE TABLE probe; PRAGMA user_version = 2; INSERT INTO no_such_table …` through the real runner `schema::migrate`. The step errors, and afterwards `(user_version, probe tables)` = `(1, 0)` on the same connection and on a fresh connection after close. **`user_version` reverts with the transaction**, so the record's "unverified (test 5)" row is now settled for SQLite 3.54.1 and the stop condition is not hit.
6. **`busy_write_fails_without_writing`.** B opens with `busy_timeout = 0`. A raw connection A holds `BEGIN IMMEDIATE`. B's `add_vehicle` returns `Error::Busy`. After A rolls back, the vehicle count is 0. "Not retried" follows from the code (no retry loop) and the immediate return; it is not separately observable.
7. **`durable_connection_and_sqlite_floor`.** After `open`: `synchronous` = 2, `foreign_keys` = 1, `journal_mode` = `wal`, and the runtime `sqlite_version()` = **3.54.1** (printed by the test), which is ≥ 3.51.3. It also tests the version comparator: 3.51.2 refused, 3.51.3 accepted, 3.54.1 accepted, 3.50.7 refused, 4.0 accepted, `x` refused.

## Negative controls

Each NC was run on a scratch copy (`Cargo.toml`, `Cargo.lock`, `crates/` copied into the session scratchpad) with exactly one guarded line changed, run with `cargo test --manifest-path <copy>/Cargo.toml -p diagwire-garage <test>`, and each had a **fresh** `CARGO_TARGET_DIR` (`nc1-target` … `nc5-target`, each checked as non-existent first, so each built SQLite from scratch). The worktree itself was never modified, so there was nothing to restore. `git status` was clean afterwards. Observed output:

**NC1 (test 1): the resolver returns the first match.** `[id] => Outcome::Resolved(*id),` → `[id, ..] => Outcome::Resolved(*id),` in `records.rs`.
```
test tests::replay1_ambiguous_reference_writes_nothing ... FAILED
thread 'tests::replay1_ambiguous_reference_writes_nothing' (17184) panicked at crates\garage\src\tests.rs:123:5:
  left: Resolved(ServiceEvent { id: 1, vehicle_id: 1, vehicle_label: Some("Silver coupe"), event_date: 2026-11-13, summary: "Oil and filter", odometer: Some(Odometer { value: 120000, unit: Mi }), supersedes_id: None, retracted: false, provenance: Provenance { source: MaintainerEntry, recorded_at: 2026-11-14T03:30:00Z, diagwire_rev: "0.1.0" } })
 right: Ambiguous([Candidate { vehicle_id: 1, label: Some("Silver coupe") }, Candidate { vehicle_id: 2, label: Some("Blue sedan") }])
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.03s
```
The right reason: "e46" silently picked vehicle 1 and wrote.

**NC2 (test 2): change the writer's internal source constant.** `Source::MaintainerEntry` → `Source::ImportedNote` in `RECORD_SOURCE`. `imported-note` was chosen deliberately: it passes the schema's source CHECK, so the failure comes from the test's assertion and not from the backstop.
```
test tests::provenance_set_by_writer ... FAILED
thread 'tests::provenance_set_by_writer' (16036) panicked at crates\garage\src\tests.rs:189:9:
  left: Provenance { source: ImportedNote, recorded_at: 2026-11-14T03:30:00Z, diagwire_rev: "0.1.0" }
 right: Provenance { source: MaintainerEntry, recorded_at: 2026-11-14T03:30:00Z, diagwire_rev: "0.1.0" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.02s
```

**NC4 (test 4): remove the version check.** The whole `user_version` read and both checks (`> LATEST` → `TooNew`, `!= LATEST` → `UnknownVersion`) were deleted from `open`. Removing only the `> LATEST` check would still refuse, through `UnknownVersion`, so it would not show the guard missing.
```
test tests::open_refuses_newer_version ... FAILED
thread 'tests::open_refuses_newer_version' (1432) panicked at crates\garage\src\tests.rs:325:18:
open accepted a file it must refuse
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.03s
```

**NC5 (test 5): commit before the injected failure.** In `schema::migrate`, the `BEGIN IMMEDIATE` transaction was replaced by the bare connection (`let tx = &*conn;`) and the `commit` was removed, so each statement autocommits before the failing `INSERT`.
```
test tests::failed_migration_rolls_back_table_and_version ... FAILED
thread 'tests::failed_migration_rolls_back_table_and_version' (10856) panicked at crates\garage\src\tests.rs:431:5:
  left: (2, 1)
 right: (1, 0)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.02s
```
Without the transaction, both the version (2) and the table (1) persist. With it (the real code), both revert.

## Full results (worktree, default target dir)

`cargo test --workspace`: exit 0. Per binary: diagwire 0, diagwire-cli 0, diagwire-ds2 **5 passed**, diagwire-garage **9 passed**, diagwire-hsfz **6 passed**, diagwire-uds **5 passed**, all doc-tests 0. That is **25 passed, 0 failed**. The garage binary printed:
```
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```
Only warning: `unused workspace dependency diagwire-ds2` (pre-existing).

`cargo clippy --workspace --all-targets`: exit 0. Two warnings, both pre-existing and outside garage: `unused workspace dependency diagwire-ds2`, and `clippy::chunks_exact_to_as_chunks` at `crates\uds\src\lib.rs:157` (lib and lib test, 1 duplicate). **Zero warnings from `diagwire-garage`.**

## Date crate: `jiff` 0.2 (locked 0.2.44), default features

It is maintained by Andrew Gallant (BurntSushi), with active releases. It gives a plain calendar `civil::Date` (exactly what `event_date` means), a UTC `Timestamp`, and correct **local-date** lookup on Windows, Linux and macOS via `Zoned::now()`. The last one decided it. `time` 0.3 is smaller, but its local offset (`now_local`) is refused on Unix in multi-threaded processes for soundness reasons, and the tokio front ends that will call the garage are multi-threaded, so G10's "host's local date" would fail there. `chrono` works but is larger, and its local time zone handling is older. The garage uses jiff only behind the `Clock` trait and in stored-value parsing.

## Deviations from the record, and choices it left open

- **`diagwire_rev`** is `env!("CARGO_PKG_VERSION")` (currently `0.1.0`), exposed as `garage::REVISION`. The record says "the writing binary's version string fixed at build time". A git hash would need a build script, which I judged out of scope. If a finer revision is wanted, change it in one place.
- **Workspace entry for `diagwire-garage` not added.** Nothing depends on it yet, and cargo warns about unused workspace dependencies. Add it with the first dependent (the operations slice).
- **Identifier supersede and retract were not implemented.** The slice outcome lists only *adding* identifiers. The schema (chain columns, triggers, same-vehicle trigger) already supports them.
- **Duplicate-identifier rule, implemented literally from G6:** a new identifier is refused if it equals another vehicle's **live alias or VIN**. Labels and chassis may be shared; test 1 needs a shared chassis. A new alias equal to another vehicle's label is therefore allowed, and that reference then resolves as `Ambiguous`, which is safe.
- **Normalisation** uses `str::to_lowercase`, not full Unicode case folding (for example, `ß` is not folded to `ss`).
- **Retraction semantics:** a retraction row copies its target's fields. Retracting an already-retracted head returns `AlreadyRetracted`. Superseding a retracted head is allowed and makes the new row live (an un-retract); the record doesn't address this.
- **`open` with a matching application ID but `user_version` 0 (or any version other than 1)** is refused as `UnknownVersion`, since v1 is creation only and there is nothing to migrate from. This is not tested. Test 4 covers newer, foreign and unmarked files.
- **`create` failure after the file exists** leaves the partial file in place (documented on `create`). Nothing in garage deletes files. `open` refuses such a file, because its application ID is 0 or its version is 0.
- **The SQLite floor refuses the fixed backports 3.44.6 and 3.50.7**, as the record specifies ("refuses any runtime `sqlite_version()` below 3.51.3").
- **Unix-only code is unverified:** creating a missing directory with 0700, the file with 0600, and the `cfg(unix)` branches in general. They were not compiled here, because there is no Linux environment (no WSL distro). G8's warning for a user-supplied directory readable by group or others is **not implemented**, because `garage` has no logging dependency. It belongs with the front end's path resolver, or it needs `tracing`.
- **`SystemClock` is untested** by design (wall clock).

## Stop conditions

- Bundled build on Windows: **builds** (libsqlite3-sys 0.40.0 compiled with MSVC). On Linux: **not verified**, because no Linux environment was available. This is not a hit, but it is open: the record requires both platforms, and so does the playbook's "pinned values proven on a second platform".
- Test 5's `user_version`: **reverts**. Not hit.
- Runtime SQLite: **3.54.1** ≥ 3.51.3. Not hit.
- Q3: answered "a unit per record". Not hit.
