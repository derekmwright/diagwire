# Garage slice 1: worker result note

**Date:** 2026-11-14. **Record:** DR-0002, First slice. **Branch:** `worktree-agent-a66a61805c1f22b57`. **Code commits:** `0dfb48497f198b2a570d69023eb27f9a69cd9767` (slice), `c830699` (review round 1 fixes). Windows 11, Rust 1.100.0. No stop condition was hit.

*Corrected after review round 1 (see the last section).* The descriptions below match the code at `c830699`. Claims the reviewer refuted are marked "(r1)" where they were fixed in the code, or reworded where the note was wrong.

## What was built

New crate `crates/garage` (package `diagwire-garage`). No other crate was changed.

| File | Lines | Contents |
|---|---|---|
| `Cargo.toml` (workspace) | | added `caseless = "0.2"` (r1), `jiff = "0.2"`, `rusqlite = { version = "=0.42.0", features = ["bundled"] }`, `tempfile = "3"` to `[workspace.dependencies]` |
| `Cargo.lock` | | caseless 0.2.2 (r1), jiff 0.2.44, rusqlite 0.42.0, libsqlite3-sys 0.40.0, tempfile 3.27.0 and their dependencies |
| `crates/garage/Cargo.toml` | | version/edition/license inherited from the workspace |
| `crates/garage/src/lib.rs` | 298 | `Garage::create`/`open`, `Options` (range-checked busy timeout), `Error`, SQLite floor check, sidecar entry check, connection configuration |
| `crates/garage/src/clock.rs` | 25 | `Clock` trait (`now()` UTC, `today()` local date) and `SystemClock` |
| `crates/garage/src/schema.rs` | 104 | migration runner and migration v1 (`vehicle`, `vehicle_ident`, `service_event`, append-only triggers) |
| `crates/garage/src/records.rs` | 661 | `RecordWriter`, reference resolution (`VehicleRef`), reads with provenance |
| `crates/garage/src/tests.rs` | 714 | tests 1–7 plus four review-round-1 tests (13 test functions) |

Line counts are total lines (`(Get-Content f).Count`) at `c830699`. The round-0 counts came from `Measure-Object -Line`, which skips blank lines, so they were wrong even then.

**`create(path, clock, options)`** first range-checks `options.busy_timeout` (r1), before touching the filesystem. It then refuses if a directory entry exists at the path or at its `-wal`, `-shm` or `-journal`. The check uses `symlink_metadata`, so a dangling symlink counts, and inspection errors propagate (r1). It creates a missing parent directory (0700 on Unix), creates the file exclusively (`create_new`, 0600 on Unix), opens it without `SQLITE_OPEN_CREATE`, checks the SQLite floor, sets and reads back the pragmas outside any transaction, then writes `application_id`, the v1 schema and `user_version` in one `BEGIN IMMEDIATE` transaction.

**`open(path, clock, options)`** range-checks the busy timeout (r1), returns `Missing` if there is no file (other metadata errors propagate, r1), and opens with `READ_WRITE | NO_MUTEX` (no `CREATE`). In order, before any configuration change: SQLite version ≥ 3.51.3, `application_id` = `0x64776772` ("dwgr", 0 is refused), `user_version` not above 1 (`TooNew`), and exactly 1 (`UnknownVersion` otherwise). Only then does it set `busy_timeout`, `foreign_keys=ON`, `synchronous=FULL` and `journal_mode=WAL`, reading each back and failing on a mismatch. At `0dfb484` this was false for `busy_timeout`, which was only set. It has been read back since `c830699` (r1).

**`RecordWriter`** (from `Garage::record_writer`): `add_vehicle(label)`, `add_identifier(reference, kind, value)`, `add_service_event(reference, input)`, `supersede_service_event(reference, target, input)` and `retract_service_event(reference, target)`. Each is one `BEGIN IMMEDIATE` transaction that resolves the reference inside it. `Ambiguous` and `NotFound` return before any insert. Source (`RECORD_SOURCE`, private const), `recorded_at` (from `clock.now()`, stored as fixed-width `YYYY-MM-DDTHH:MM:SS.ffffffZ`) and `diagwire_rev` are set inside the writer. No write method takes a source. A missing date uses `clock.today()`. `SQLITE_BUSY` maps to `Error::Busy` and is never retried.

**Reads** on `Garage`: `resolve`, `vehicle`, `service_events` (live heads only), `service_event_history` (all rows). Every result carries a `Provenance { source, recorded_at, diagwire_rev }`. At `0dfb484` this was false: `resolve` returned a bare id, and ambiguity candidates and labels were plain strings. Since `c830699` (r1), resolution returns a `VehicleRef` (anchor provenance, plus label and matched identifiers as `Identifier`s with their own provenance). Candidates are `VehicleRef`s, and `Vehicle.label` and `ServiceEvent.vehicle_label` are `Identifier`s.

**Schema v1** uses STRICT tables. Each table has `BEFORE UPDATE`/`BEFORE DELETE` triggers that `RAISE(ABORT)`. `supersedes_id` is UNIQUE with a self-FK. There is a `CHECK (retracted = 0 OR supersedes_id IS NOT NULL)`, an odometer pair CHECK (`(odo_value IS NULL) = (odo_unit IS NULL)`, unit ∈ `mi`/`km`), and `source` is limited to `maintainer-entry`/`imported-note` as a backstop. A `BEFORE INSERT` trigger refuses a supersede that crosses vehicles.

## Tests (all in `crates/garage/src/tests.rs`, temp dirs via `tempfile`, `FixedClock`)

The clock returns UTC now = `2026-11-14T03:30:00Z` and local today = `2026-11-13`. The two dates differ, so a test can tell which one a default date came from.

1. **`replay1_ambiguous_reference_writes_nothing`.** Two vehicles share chassis `E46`. `add_service_event("e46")` returns `Ambiguous` listing both, with ids and labels, and the `service_event` count is unchanged. The unique alias `"  The   COUPE "` (normalisation: trim, collapse spaces, Unicode full case folding since r1) resolves to the coupe and writes one row. `"e90"` returns `NotFound`.
2. **`provenance_set_by_writer`.** It writes a vehicle, a VIN, a dated event, an undated event, a supersede and a retraction. Every returned `Provenance` equals `{MaintainerEntry, NOW, REVISION}`. Every row in all three tables has `source='maintainer-entry'`, `recorded_at='2026-11-14T03:30:00.000000Z'` and `diagwire_rev=REVISION`. The undated event and the supersede take `event_date = 2026-11-13` (the clock's local today), and the dated one keeps its date. "No public write method accepts a source" is a property of the API's shape, checked by reading the signatures, not at runtime.
3. **`append_only`.** Raw `UPDATE`/`DELETE` on `service_event`, `vehicle_ident` and `vehicle` each fail with `ConstraintViolation`, and full row dumps of all three tables are unchanged afterwards. At `0dfb484` only `service_event` and `vehicle_ident` were dumped; the `vehicle` dump was added in `c830699` (r1). A supersede adds a head (the default list shows only the new head). A second supersede of the same row fails in the API (`AlreadySuperseded`) and in storage (a raw insert hits UNIQUE), with the count unchanged. A retracted head is hidden by default and shown in history as `[(first,false),(second,false),(retraction,true)]`. Since A1 (`797b430`): superseding the retracted head returns `RetractionIsFinal` and the row count is unchanged; a fresh service event then succeeds and is the only live row; history shows `[…, (retraction,true), (fresh,false)]`. A duplicate live alias on another vehicle (`" Commuter "` vs `commuter`) is refused (`IdentifierTaken(Alias)`), with the count unchanged. An odometer value without a unit is refused by the storage CHECK (raw insert); the API's `Odometer` type can't express one.
4. **`open_refuses_newer_version`, `open_refuses_foreign_and_empty_files`, `open_missing_creates_nothing_and_create_refuses_existing`.** Each refusal is checked on a cleanly closed file (no sidecars beforehand). Afterwards the main file is **byte-identical** (a full byte comparison, stronger than a hash) and no sidecar exists. Cases: `user_version=2` gives `TooNew{2,1}`; `application_id=0x12345678` gives `Foreign`; a database with a header but no application ID gives `Foreign(0)`; a zero-length file gives `Foreign(0)`. `open` on a missing path gives `Missing`, and the directory stays empty. `create` on an existing file gives `Exists` and leaves the contents unchanged. `create` where only a `-wal` exists gives `Exists` and makes no main file.
5. **`failed_migration_rolls_back_table_and_version`.** A test-only step (version 2) runs `CREATE TABLE probe; PRAGMA user_version = 2; INSERT INTO no_such_table …` through the real runner `schema::migrate`. The step errors, and afterwards `(user_version, probe tables)` = `(1, 0)` on the same connection and on a fresh connection after close. **`user_version` reverts with the transaction**, so the record's "unverified (test 5)" row is now settled for SQLite 3.54.1 and the stop condition is not hit.
6. **`busy_write_fails_without_writing`.** B opens with `busy_timeout = 0`. A raw connection A holds `BEGIN IMMEDIATE`. B's `add_vehicle` returns `Error::Busy`. After A rolls back, the vehicle count is 0. "Not retried" follows from the code (no retry loop) and the immediate return; it is not separately observable.
7. **`durable_connection_and_sqlite_floor`.** After `open`: `synchronous` = 2, `foreign_keys` = 1, `journal_mode` = `wal`, `busy_timeout` = 1234 for an `Options` of 1234 ms (r1), and the runtime `sqlite_version()` = **3.54.1** (printed by the test), which is ≥ 3.51.3. It also tests the version comparator: 3.51.2 refused, 3.51.3 accepted, 3.54.1 accepted, 3.50.7 refused, 4.0 accepted, `x` refused.

## Negative controls

Each NC was run on a scratch copy (`Cargo.toml`, `Cargo.lock`, `crates/` copied into the session scratchpad) with the guarded code broken (one edit for NC1 and NC2; **NC4 and NC5 each needed several lines changed**, as described under each), run with `cargo test --manifest-path <copy>/Cargo.toml -p diagwire-garage <test>`, and each had a **fresh** `CARGO_TARGET_DIR` (`nc1-target` … `nc5-target`, each checked as non-existent first, so each built SQLite from scratch). The worktree itself was never modified, so there was nothing to restore. `git status` was clean afterwards. Observed output:

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

## Full results at `0dfb484` (round 0; worktree, default target dir; superseded by the round 1 results below)

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
- ~~**Normalisation** uses `str::to_lowercase`, not full Unicode case folding.~~ Fixed in r1: it now uses full default case folding (`caseless`).
- **Retraction semantics:** a retraction row copies its target's fields. Retracting an already-retracted head returns `AlreadyRetracted`. ~~Superseding a retracted head is allowed and makes the new row live (an un-retract); the record doesn't address this.~~ Resolved by DR-0002 amendment A1 (a retraction is final). Since `797b430`, superseding a retracted service-event head returns `RetractionIsFinal`; see the A1 section.
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

## Review round 1 response

Verdict: `docs/notes/2026-11-14-garage-slice1-review-r1.md` (CHANGES REQUIRED, four findings). Fixes are in commit `c830699`. Superseding a retracted service event is out of this round, per the dispatcher, and is unchanged.

### Finding 1: busy_timeout panic (`lib.rs:239` at `0dfb484`)

**Change.** `Options::busy_timeout_ms()` converts the timeout to whole milliseconds exactly as rusqlite 0.42.0's `busy_timeout` does (`as_secs * 1000 + subsec_millis`, then `i32::try_from`). It returns `Error::BusyTimeoutOutOfRange(Duration)` instead of reaching rusqlite's `.expect("too big")`. `create` calls it as its **first statement**, before the sidecar checks, the directory creation and the file creation. `open` calls it before anything else too. `configure` takes the validated `i32`, sets it, and reads `PRAGMA busy_timeout` back (`expect_pragma`, which now compares integers). A mismatch is `Error::Pragma`, whose `expected` field is now a `String`.

**Tests.**
- New: `busy_timeout_out_of_range_refused_before_any_file`. `create` with `Duration::MAX` and with `i32::MAX + 1` ms, aimed at a path under a missing subdirectory, returns `BusyTimeoutOutOfRange(d)` with the same `d`. The temp directory stays empty, so not even the parent directory is made. `create` with exactly `i32::MAX` ms succeeds. `open` of that garage with `Duration::MAX` returns `BusyTimeoutOutOfRange`, leaves the file byte-identical and leaves no sidecar.
- Extended: test 7 opens with a 1234 ms timeout and asserts `busy_timeout` reads back `1234`.

**NC R1a: no validation, raw `Duration` to rusqlite (the original code path; 2 edits in `create`).** I removed the `busy_timeout_ms()?` line and replaced `configure(&conn, busy_ms)?` with `conn.busy_timeout(options.busy_timeout)?; configure(&conn, 5000)?;`.
```
test tests::busy_timeout_out_of_range_refused_before_any_file ... FAILED
thread 'tests::busy_timeout_out_of_range_refused_before_any_file' (18200) panicked at C:\Users\User\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\rusqlite-0.42.0\src\busy.rs:30:14:
too big
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.01s
```

**NC R1b: validation moved after the file is created (2 edits: remove it at the top, re-insert it just before `Connection::open_with_flags`).**
```
test tests::busy_timeout_out_of_range_refused_before_any_file ... FAILED
thread 'tests::busy_timeout_out_of_range_refused_before_any_file' (20248) panicked at crates\garage\src\tests.rs:527:9:
  left: 1
 right: 0
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.01s
```
The error was still returned, but the directory had already been created (1 entry, expected 0). The test therefore pins "before any filesystem mutation", not only "no panic".

**NC R1c: busy_timeout never set (1 edit: the `conn.busy_timeout(...)` line in `configure` deleted).** This shows the readback guard catches it.
```
test tests::durable_connection_and_sqlite_floor ... FAILED
thread 'tests::durable_connection_and_sqlite_floor' (20720) panicked at crates\garage\src\tests.rs:476:52:
called `Result::unwrap()` on an `Err` value: Pragma { name: "busy_timeout", got: "5000", expected: "1234" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.02s
```
(5000 is rusqlite's own default.)

### Finding 2: case folding (`records.rs:208` at `0dfb484`)

**Change.** `normalize` now trims, collapses whitespace and applies `caseless::default_case_fold_str`: Unicode **full** default case folding (CaseFolding.txt statuses C+F), so `ß` becomes `ss` and `ς` becomes `σ`. The one function is used for stored `value_norm` (in `insert_ident`) and for incoming references (in `resolve`), so the two cannot drift apart. No existing garages hold old-style `value_norm` values (this crate is unreleased), so no migration is needed.

**Crate choice: `caseless` 0.2.2** (`caseless = "0.2"` in the workspace).
- std has no case folding (`to_lowercase` is lowercase mapping, not folding).
- `caseless` is from the unicode-rs organisation (which also maintains `unicode-normalization`). Its tables are generated from Unicode **16.0** CaseFolding.txt (checked: `UNICODE_VERSION = (16, 0, 0)` in its `case_folding_data.rs`). It has one dependency, `unicode-normalization` (+ `tinyvec`), and it produces a folded `String`, which an SQL equality on `value_norm` needs.
- Rejected: `focaccia` (only comparison functions, no folded string to store), and ICU4X `icu_casemap` 2.4 (Unicode-maintained and newer, but about 25 crates: zerovec, yoke, icu_provider and others, which is far from small).
- Caveat: `caseless` releases are infrequent. Folding data changes little between Unicode versions, but a future Unicode version could change a fold, and that would change `value_norm` for new rows only. Noted, not mitigated.

**Test (new): `unicode_case_folding_for_identifiers_and_references`.** It covers the `Straße`/`STRASSE` regression:
- A vehicle labelled `Straße` resolves from `STRASSE`, `strasse`, `Straße` and `  STRAßE `.
- `ΣΊΣΥΦΟΣ` resolves from `σίσυφος`, so final sigma folds.
- The duplicate-alias check uses the same fold: an alias `Weiß` on one vehicle blocks `WEISS` on another (`IdentifierTaken(Alias)`, with the count unchanged).

**NC R2: lowercase instead of folding (1 edit in `normalize`).**
```
test tests::unicode_case_folding_for_identifiers_and_references ... FAILED
thread 'tests::unicode_case_folding_for_identifiers_and_references' (14264) panicked at crates\garage\src\tests.rs:70:18:
expected Resolved, got NotFound
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.02s
```
(`tests.rs:70` is the `resolved()` helper. The first reference, `STRASSE`, did not match `Straße`.)

### Finding 3: provenance on read results (`records.rs:157,160` at `0dfb484`)

**Change.** `Candidate` was replaced by `VehicleRef { vehicle_id, provenance, label: Option<Identifier>, matched: Vec<Identifier> }`.
- `provenance` is the `vehicle` anchor row's provenance.
- `label` is the newest live label as a full `Identifier`, carrying its own provenance.
- `matched` holds the live identifiers equal to the reference, each with its provenance.

`Resolution` is now `Outcome<VehicleRef>`, and `Outcome::Ambiguous` holds `Vec<VehicleRef>`, so a resolved identity and every candidate carry provenance. For consistency, `Vehicle.label` and `ServiceEvent.vehicle_label` changed from `Option<String>` to `Option<Identifier>`. Every label in any read result now carries provenance. This is a public API change, but the crate has no dependents yet.

**Test (new): `resolution_carries_provenance`.** A `SteppingClock` gives each write a distinct `recorded_at`: vehicle Coupe at t1, alias "the coupe" at t2, vehicle Sedan at t3, chassis E46 on both at t4. The checks:
- `resolve("The Coupe")` gives anchor provenance t1, label provenance t1, and `matched` = [(Alias, "the coupe", t2)].
- `resolve("e46")` gives `Ambiguous` with [(coupe, t1, label t1, matched [t4]), (sedan, t3, label t3, matched [t4])].

The distinct times show each provenance comes from the right row.

**NC R3: anchor provenance taken from the matched identifier (1 edit in `vehicle_ref`).**
```
test tests::resolution_carries_provenance ... FAILED
thread 'tests::resolution_carries_provenance' (20776) panicked at crates\garage\src\tests.rs:634:5:
  left: Provenance { source: MaintainerEntry, recorded_at: 2026-02-01T00:00:00Z, diagwire_rev: "0.1.0" }
 right: Provenance { source: MaintainerEntry, recorded_at: 2026-01-01T00:00:00Z, diagwire_rev: "0.1.0" }
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.02s
```

### Finding 4: dangling-symlink sidecars (`lib.rs:118` at `0dfb484`)

**Change.** The new `entry_exists(path)` uses `fs::symlink_metadata`: an entry, even a dangling symlink, is `true`; `NotFound` is `false`; any other error propagates as `Error::Io`. `create` uses it for the main path and all three sidecars, and for the parent-directory check. `open`'s missing-file check now uses `fs::metadata` and propagates errors other than `NotFound`, instead of `Path::exists()`, which swallows them. The test helper `sidecars_absent` uses `entry_exists` too. No test forces an inspection error (for example, a permission-denied parent). Propagation rests on the code.

**Test (new): `create_refuses_dangling_symlink_entries`.** For each of `""` (the main path), `-wal`, `-shm` and `-journal`, in a fresh temp dir:
- create a symlink at that entry pointing to a missing `nowhere`;
- assert `link.exists()` is false, so the link really dangles;
- assert `create` returns `Exists`;
- assert the directory still holds only the link.

If the symlink can't be created, the test prints `SKIPPED create_refuses_dangling_symlink_entries: cannot create a symlink here (<error>); on Windows this needs Developer Mode or SeCreateSymbolicLinkPrivilege` and returns. The message is written with `writeln!(std::io::stderr(), …)`, which libtest does **not** capture, so it shows in normal `cargo test` output. I verified this in a scratch crate: a direct-stderr line appeared in a passing test's output and an `eprintln!` line did not. A skip still counts as "ok" in libtest's tally, because Rust has no runtime-skip status. The visible line is the signal. **On this machine symlink creation succeeded** (no SKIPPED line in any run), so the test really ran and passed here. The Linux CI path uses `std::os::unix::fs::symlink`, which was not compiled here.

**NC R4: entry check follows symlinks again (1 edit: the `entry_exists` body became `Ok(path.exists())`).**
```
test tests::create_refuses_dangling_symlink_entries ... FAILED
thread 'tests::create_refuses_dangling_symlink_entries' (2900) panicked at crates\garage\src\tests.rs:701:9:
"-wal"
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.02s
```
The `""` case passed even under the NC, because `create_new` refuses to create through an existing symlink entry on its own. The `-wal` case is the one the old check missed: `create` went ahead despite the dangling `-wal`.

### NC method (round 1)

The method was the same as round 0. Each NC used its own scratch copy (`Cargo.toml`, `Cargo.lock`, `crates/`) of the committed `c830699` state, in the session scratchpad (`r1a`, `r1b`, `r1c`, `r2`, `r3`, `r4`). Each had a fresh `CARGO_TARGET_DIR` (`<name>-target`), confirmed non-existent before the run, and was run with `cargo test --manifest-path <copy>/Cargo.toml -p diagwire-garage <test>`. The worktree was never edited for an NC, and `git status` showed only the reviewer's untracked note afterwards. Edit counts per NC are stated above.

### Note corrections

- §What was built: the line counts were stale and also miscounted (`Measure-Object -Line` skips blank lines). They now show total lines at `c830699`.
- "every configured pragma is read back": false at `0dfb484` for `busy_timeout`, true since `c830699`. Marked inline.
- "every read carries provenance": false at `0dfb484`, true since `c830699`. Marked inline.
- Test 3, "full row dumps unchanged" for all three tables: at `0dfb484` the test dumped only `service_event` and `vehicle_ident`. **Test 3** (`append_only`, not test 4) now also dumps `vehicle`, so the claim holds rather than being edited down. The dispatcher's message said "test 4", but the dump claim and the reviewer's citation (`tests.rs:235–251`) are test 3.
- Negative controls, round 0: "exactly one guarded line" was wrong. NC4 deleted the `user_version` read plus both version checks (11 lines). NC5 replaced the transaction with the bare connection and removed `commit` (3 changes). Corrected inline.
- Normalisation deviation: struck, now fixed.

### Results after round 1 (worktree, default target dir)

`cargo test --workspace`: exit 0. Per binary: diagwire 0, diagwire-cli 0, diagwire-ds2 **5 passed**, diagwire-garage **13 passed**, diagwire-hsfz **6 passed**, diagwire-uds **5 passed**, all doc-tests 0. That is **29 passed, 0 failed**, and no SKIPPED line. The garage binary printed:
```
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
```
Only warning: `unused workspace dependency diagwire-ds2` (pre-existing).

`cargo clippy --workspace --all-targets`: exit 0. The same two pre-existing warnings outside garage: `unused workspace dependency diagwire-ds2`, and `clippy::chunks_exact_to_as_chunks` at `crates\uds\src\lib.rs:157` (lib and lib test, 1 duplicate). **Zero warnings from `diagwire-garage`.**

No failed test was rerun, and no stop condition was hit. Linux remains unverified, as before.

## A1: a retraction is final

The maintainer accepted DR-0002 amendment A1 (main `e68237d`): G5 now says a retracted head cannot be superseded, and a mistaken retraction is undone by adding a new record, while history keeps the retraction. First-slice test 3 was extended to match. Round 2 code review had approved the slice with 0 findings before this change. Implemented for **service events only**, per the dispatcher. The A1 tests for identifiers and vehicles are owed by the slice that adds identifier correction and vehicle retraction.

**Change (`797b430bbb757443c99985007fbb62ffc64434d2`).**
- New error variant `Error::RetractionIsFinal(i64)`: "record {id} is retracted, and a retraction is final; add a new record instead".
- In `RecordWriter::write_event`, after the target head is loaded inside the `BEGIN IMMEDIATE` transaction, a supersede (`input` present) of a head with `retracted = 1` returns `RetractionIsFinal` before any insert. The transaction is dropped, so nothing is written.
- Retracting an already-retracted head still returns `AlreadyRetracted`.
- Adding a fresh service event is unaffected.
- No schema change: the rule is enforced in the API, like the existing head checks.

**Test (extended): `append_only` (test 3).** After the existing retraction steps:
- `supersede_service_event("daily", retraction.id, …)` returns `RetractionIsFinal(retraction.id)`, and the `service_event` count is unchanged;
- `add_service_event("daily", …)` succeeds with `supersedes_id = None`;
- the default list is exactly `[fresh]`;
- history is `[(first,false), (second,false), (retraction,true), (fresh,false)]`, so the retraction stays visible.

The final row-count assertion moved from `before + 1` to `before + 2` to account for the fresh row.

**NC A1: finality check removed (1 edit: the six-line `RetractionIsFinal` block in `write_event` deleted).** Scratch copy `a1` of `797b430`, fresh `CARGO_TARGET_DIR` `a1-target` (confirmed non-existent), run with `cargo test --manifest-path <copy>/Cargo.toml -p diagwire-garage append_only`. My first attempt to launch it never ran cargo: a PowerShell variable-name collision (`$s` and `$S` are the same variable) garbled the target path, and `cmd` refused with "The command line is too long". No build happened, no test ran, and no target directory or log was created; I checked before relaunching. So the launch was repeated, but no test was rerun. Observed:
```
thread 'tests::append_only' (5304) panicked at crates\garage\src\tests.rs:309:5:
assertion failed: matches!(g.record_writer().supersede_service_event("daily", retraction.id,
    &oil(None)), Err(Error::RetractionIsFinal(id)) if id == retraction.id)
test tests::append_only ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.02s
```
Without the check, superseding the retracted head is accepted (the old un-retract behaviour).

**Results after A1 (worktree, default target dir).**

`cargo test --workspace`: exit 0. Per binary: diagwire 0, diagwire-cli 0, diagwire-ds2 **5 passed**, diagwire-garage **13 passed**, diagwire-hsfz **6 passed**, diagwire-uds **5 passed**, all doc-tests 0. That is **29 passed, 0 failed**, and no SKIPPED line. The garage binary printed:
```
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
```
Only warning: `unused workspace dependency diagwire-ds2` (pre-existing).

`cargo clippy --workspace --all-targets`: exit 0. The same two pre-existing warnings outside garage: `unused workspace dependency diagwire-ds2`, and `clippy::chunks_exact_to_as_chunks` at `crates\uds\src\lib.rs:157` (lib and lib test, 1 duplicate). **Zero warnings from `diagwire-garage`.**

No failed test was rerun, and no stop condition was hit.