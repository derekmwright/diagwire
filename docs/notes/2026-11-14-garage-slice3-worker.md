# Garage slice 3: worker result note

**Record:** DR-0002, slice 3: notes, identifier correction and vehicle retraction, on the record-writer side of `crates/garage`, including the paragraph "A1 tests owed by the slice that adds identifier correction and vehicle retraction". **Branch:** `worktree-agent-a3a5cf9851bc0c837`. **Code commit:** `d27640352c8e8965093d60d0bac6be3ddab3fc24`. Windows 11, Rust 1.100.0. Work started 2026-11-14; the negative controls, the gates and this note were done on 2026-11-15 after the session was interrupted.

No test failed in the worktree at any point. The first run of the suite with the new tests passed 44 of 44. One `cargo clippy` run before the commit failed on two lints in a new test file (`cloned_ref_to_slice_refs`, `tests_notes.rs`); both were fixed before the commit. No import entry point was added.

## What was built

Only `crates/garage/src` changed. No dependency was added; `Cargo.toml` and `Cargo.lock` are untouched.

| File | Lines | Change |
|---|---|---|
| `schema.rs` | 312 | `LATEST` = 3; migration v3 |
| `notes.rs` | 189 | new: `Note`, `NoteInput`, note writes on `RecordWriter`, `Garage::notes`, `Garage::note_history` |
| `corrections.rs` | 255 | new: identifier supersede and retract, `retract_vehicle`, `VehicleRetraction`, `VehicleHistory`, `Garage::vehicles`, `Garage::identifier_history`, `Garage::vehicle_history` |
| `records.rs` | 784 | resolution and the uniqueness check ignore retracted vehicles; `insert_ident` takes the row it supersedes; shared `check_correctable`; helpers made `pub(crate)` |
| `lib.rs` | 353 | modules, exports, `Error::EmptyNote`, `Error::EmptyReason`, `Error::LastIdentifier` |
| `tests_notes.rs` | 433 | new: 3 tests |
| `tests_corrections.rs` | 1041 | new: 9 tests |
| `tests_migration.rs` | 523 | 1 new test; the v1 fixture is now SQL; version numbers updated |
| `tests_observations.rs` | 783 | 1 new test; both replay 3 tests extended |
| `tests.rs` | 747 | two helpers made `pub(crate)`; version numbers in two tests updated |

Line counts are `(Get-Content f).Count` at `d276403`.

**Migration v3** (`schema.rs`, `V3`). Two STRICT tables, each with `BEFORE UPDATE` and `BEFORE DELETE` triggers that `RAISE(ABORT)`, and `source` limited by CHECK to the two record kinds:
- `note`: `vehicle_id` NOT NULL FK, `body`, `about_date` NOT NULL, `supersedes_id` UNIQUE FK to `note`, `retracted`, `source`, `recorded_at`, `diagwire_rev`, `CHECK (retracted = 0 OR supersedes_id IS NOT NULL)`, and a `BEFORE INSERT` trigger that keeps a superseding row on its target's vehicle.
- `vehicle_retraction`: `vehicle_id` NOT NULL UNIQUE FK, `reason` NOT NULL, `source`, `recorded_at`, `diagwire_rev`.

An existing v2 (or v1) garage reaches v3 through `Garage::open` and the unchanged `schema::upgrade` path: backup first, then the pending steps in one `BEGIN IMMEDIATE` transaction.

**Notes** (`RecordWriter`): `add_note(reference, &NoteInput { about_date: Option<Date>, body })`, `supersede_note(reference, target, &NoteInput)`, `retract_note(reference, target)`. Each is one `BEGIN IMMEDIATE` transaction that resolves the reference first; `Ambiguous` and `NotFound` write nothing. The source is the writer's private `RECORD_SOURCE` (`maintainer-entry`); no method takes a source. A missing date is the injected clock's local date. The body is trimmed; an empty one is `EmptyNote`. A retraction row repeats its target's date and body with `retracted = 1`. `Garage::notes` returns live rows; `Garage::note_history` returns every row.

**Identifier correction** (`RecordWriter`): `supersede_identifier(reference, target, value)` and `retract_identifier(reference, target)`, for any kind (label, alias, VIN, chassis). The new head keeps the target's kind. The value check is the one `add_identifier` uses (`insert_ident`), run inside the write transaction: a value equal to another vehicle's live alias or VIN is `IdentifierTaken`. A retraction row repeats its target with `retracted = 1`. `Garage::identifier_history(reference)` returns every identifier row of a vehicle.

**The single correction check** (`records::check_correctable`), used by notes and identifiers: the target must be a row of that table on the resolved vehicle (`RecordNotFound`), with nothing superseding it (`AlreadySuperseded`); a retracted head can be neither superseded (`RetractionIsFinal`) nor retracted again (`AlreadyRetracted`). Service events keep their slice 1 code (`head_event` and the checks in `write_event`), which was not touched.

**Vehicle retraction** (`RecordWriter::retract_vehicle(reference, reason)`): inserts one `vehicle_retraction` row and returns it as `VehicleRetraction { id, vehicle_id, vehicle_label, reason, provenance }`. No other row is written or changed.

**Reads made consistent.**
- `records::resolve` and the uniqueness check in `insert_ident` use a new predicate, `live_ident!`: the identifier row is live and its vehicle has no `vehicle_retraction` row. So no reference resolves to a retracted vehicle, every read and write that takes a reference returns `NotFound` for it, and a retracted vehicle's alias or VIN no longer blocks another vehicle.
- `Garage::vehicles()` lists the vehicles that are not retracted, with live identifiers.
- `Garage::vehicle_history()` lists every anchor, retracted or not, each with its retraction (if any) and every identifier, service event and note row. This is the only read that reaches a retracted vehicle.
- The slice 2 identity-conflict comparison (`live_ident_disagrees`) already filtered on live rows; its code is unchanged apart from the doc comment. The missing test was added.
- Every new read result carries a `Provenance`: `Note`, `VehicleRetraction`, `VehicleHistory` (the anchor's, and each row's own), and `Vehicle` from `vehicles()`.

## Tests

All use `tempfile` directories and the injected `FixedClock` or `SteppingClock`. Garage has 44 tests: 30 from slices 1 and 2, 14 new.

`tests_notes.rs`:
1. **`notes_add_supersede_and_retract`.** Two vehicles. A dated note is returned whole (vehicle, label, date, trimmed body, no chain, provenance). An undated note gets the clock's local date (2026-11-13, not the UTC date 2026-11-14), also checked in the stored column. Supersede adds a head and the default query hides the old row. A second supersede of the same row, and a retract of it, give `AlreadySuperseded`; a raw insert with the same `supersedes_id` fails with `UNIQUE constraint failed: note.supersedes_id`. A supersede or retract through the other vehicle's reference gives `RecordNotFound`; a raw insert on the other vehicle pointing at a head fails with the same-vehicle trigger's message. An empty body, an unknown id, an ambiguous reference (add, supersede, retract) and an unknown reference write nothing (row count compared). Retraction: the returned row repeats its target, the default query hides the chain, history lists all four rows with `(id, supersedes_id, retracted)`. A1: superseding the retracted head gives `RetractionIsFinal`, retracting it again `AlreadyRetracted`, superseding its target `AlreadySuperseded`, row count unchanged; a fresh note is live and history still has the retraction. The other vehicle's notes are unchanged.
2. **`notes_carry_maintainer_entry_provenance`.** Add at T1, supersede and add at T2, retract at T3. Provenance from the writer, from `notes` (and the label's), from `note_history`, and from every stored row. A raw insert of a note with `diagwire-read` and all other columns valid fails with `CHECK constraint failed: source`.
3. **`note_table_is_append_only`.** A superseded row, its head, and a lone head that nothing refers to. `DELETE` and `UPDATE` of the lone row each fail with an error containing `append-only`; three unfiltered statements fail with a constraint violation; the full dump is unchanged.

`tests_corrections.rs`:
4. **`identifier_supersede_corrects_what_resolves`.** A label is superseded: the returned row is compared whole (same vehicle, kind `label`, trimmed value, `supersedes_id`), the old label stops resolving, the new one resolves, `vehicle` shows the new label and the live ids, history has all three rows. A superseded alias keeps kind `alias`. Second supersede and retract of a superseded row: `AlreadySuperseded`; raw insert: `UNIQUE constraint failed: vehicle_ident.supersedes_id`. Through the other vehicle's reference: `RecordNotFound`; raw insert on the other vehicle pointing at a head: the same-vehicle trigger's message. A corrected value equal to another vehicle's live alias: `IdentifierTaken`, for an alias and for a label. Empty value, unknown id, ambiguous and unknown references: nothing written (row count), and the other vehicle's identifiers are unchanged.
5. **`a1_retracted_identifier_is_final_and_fresh_alias_works_on_same_vehicle`** (A1). Retracting an alias returns a row that repeats it; the alias stops resolving, is gone from the vehicle's live identifiers, and history has the retraction. Superseding the retraction: `RetractionIsFinal`; retracting it: `AlreadyRetracted`; superseding its target: `AlreadySuperseded`; nothing written. A fresh identifier with the same normalised alias on the same vehicle succeeds and resolves; history keeps the retraction. A subsequent live duplicate on another vehicle is refused, as a new identifier and as a correction, with nothing written.
6. **`a1_retracted_alias_can_be_added_to_another_vehicle`** (A1, "separately"). While the alias is live on the first vehicle, the second is refused it. After retraction the same normalised alias is added to the second vehicle and resolves there; the first vehicle's history keeps the retraction. Adding it back to the first vehicle is then refused with nothing written.
7. **`last_live_identifier_cannot_be_retracted`.** See deviation 1. A vehicle's only identifier cannot be retracted (`LastIdentifier`, nothing written, still resolves). With an alias added the label can be retracted (the vehicle then has no label and resolves by alias); the alias is then the last and is refused.
8. **`vehicle_retraction_hides_the_vehicle_and_is_final`** (A1). A vehicle with four identifiers, a service event, a note and a measured read, and a second vehicle sharing its chassis. Before: the chassis is ambiguous. An empty reason, an ambiguous reference and an unknown reference write nothing. The retraction is returned whole (vehicle, label, trimmed reason, provenance at the retraction's time). Dumps of the other seven tables are unchanged. Afterwards: label, alias and VIN resolve to nothing; the shared chassis resolves to the other vehicle; `vehicles()` lists only the other vehicle; `vehicle`, `service_events`, `service_event_history`, `notes`, `note_history`, `identifier_history` and `latest_measured_faults` return `NotFound`; ten write methods (nine on `RecordWriter`, plus `start_session`) return `NotFound` with all eight table counts unchanged. A second retraction through each of three references returns `NotFound`, and a raw second row fails with `UNIQUE constraint failed: vehicle_retraction.vehicle_id`; the table still has one row. `vehicle_history()` equals the history read before the retraction with the retraction added.
9. **`a1_replacement_vehicle_is_a_new_anchor`** (A1). The old vehicle has an alias, a VIN, four service event rows (with a supersede and a retraction) and two note rows. While it is live another vehicle is refused its VIN. After its retraction a new vehicle is added with the same label; it has a new id and the vehicle count is 3; it takes the old alias and VIN; all three references resolve to it; it has no service events or notes; a new event goes to it; the VIN is again refused on the third vehicle. The old vehicle's entry in `vehicle_history()` equals what was read before, plus the retraction, and every row stored before is still stored unchanged (dump prefix per record table).
10. **`corrections_carry_maintainer_entry_provenance`.** Times T1 to T5. Provenance of the superseding row, the retraction row and the vehicle retraction from the writer; of every row from `identifier_history`, `vehicles()` and `vehicle_history()`; and of every stored `vehicle_ident` and `vehicle_retraction` row. A raw `vehicle_retraction` row with `diagwire-read`, for a vehicle with no retraction, fails with `CHECK constraint failed: source`.
11. **`vehicle_retraction_and_vehicle_anchor_are_append_only`.** `DELETE` and `UPDATE` of a retraction row (no table refers to it) and `DELETE` of a vehicle anchor made by raw SQL that nothing refers to each fail with an error containing `append-only`; dumps unchanged. The anchor case closes a gap in slice 1's test, where `DELETE FROM vehicle` is also refused by the foreign key from `vehicle_ident`.
12. **`duplicate_is_refused_against_a_commit_made_while_waiting_for_the_lock`** (G6, "inside the write transaction"). Another connection holds `BEGIN IMMEDIATE` with an uncommitted alias on another vehicle. The garage connection's busy handler commits that transaction when the garage's write meets the lock. The write (once as a correction, once as a new identifier) must return `IdentifierTaken`; the test also asserts that the handler ran. Only the rival's rows are added.

`tests_migration.rs`:
13. **`v2_garage_migrates_to_v3_and_leaves_a_v2_backup`.** A populated v2 file (2 vehicles, 4 identifiers, 4 service events, 2 sessions, 3 snapshots, 2 fault rows; made by the real v1 and v2 migrations and closed cleanly). `Garage::open` gives version 3, the same dumps of all six tables, and both new tables present and empty. The backup `garage.db.v2-20261114T033000Z.backup` exists, has the garage `application_id`, version 2, `integrity_check` `ok`, the same six dumps and neither new table. Through the API the migrated garage returns the stored faults and service event, takes a note and a vehicle retraction, and the six old dumps are still unchanged. After close the directory holds the garage and the one backup; a second open makes no second backup.

`tests_observations.rs`:
14. **`identity_conflict_ignores_superseded_and_retracted_identifiers`** (the test the slice 2 reviewer noted was missing). VIN A superseded by VIN B, the only chassis retracted. Reporting A is a VIN conflict; reporting B is not; reporting a chassis (different from, or equal to, the retracted one) is not a conflict. With B retracted too, no reported VIN conflicts. With a fresh VIN C, reporting B conflicts and reporting C does not.

**Existing tests changed.**
- `replay3_record_writer_creates_no_observation` now also adds, supersedes and retracts a note whose text is a fault list, supersedes and retracts an identifier, and retracts the vehicle; the three observation tables stay empty.
- `replay3_record_writer_source_names_no_observation_table` now checks `notes.rs` and `corrections.rs` as well as `records.rs`.
- `LATEST` is 3: `open_refuses_newer_version` and `upgrade_without_pending_steps_takes_no_backup` expect `TooNew { found: 4, supported: 3 }`; `failed_migration_rolls_back_table_and_version` uses a step at version 4 and expects `(3, 0)`; the migration tests expect version 3 and check the v3 tables too.
- **The v1 fixture is SQL.** Slice 2 built it with the real `RecordWriter` on a v1 schema. This binary's writer queries `vehicle_retraction`, which a v1 or v2 file lacks, so the fixture rows are now inserted by SQL (`V1_ROWS`, and `V2_ROWS` for the v2 file). Checked: in scratch copies of `fc284b8` (before this slice) and `d276403`, a temporary test printed the dumps of `v1_garage()`; the 10 rows are identical.

## Negative controls

Method: for each control, `git archive` of `d27640352c8e8965093d60d0bac6be3ddab3fc24` (`Cargo.toml`, `Cargo.lock`, `crates`) into its own directory under the session scratchpad, edits by exact text replacement (the script aborts if the text is missing or not unique), its own new `CARGO_TARGET_DIR` (the script aborts if it exists), then `cargo test --manifest-path <copy>\Cargo.toml -p diagwire-garage --locked -- <tests>`. The worktree was never edited for a control. `nc00`, the same procedure with no edit and no filter, passed 44 of 44 (exit 0).

Things that went wrong in the harness, none of them a test result: the first two launches ran nothing (argument passing, then a variable-name clash in the script); after `nc01` the script failed to delete its target directory (the test output was already saved). `nc20` and `nc33` did not build: `LINK : fatal error LNK1104: cannot open file '…\libfallible_streaming_iterator-372c2f2a068dd6bf.rlib'`, a path over the Windows length limit under those long directory names. No test ran in them. They were run again from new copies in new, shorter directories (`nc20b`, `nc33b`); those results are the ones below.

All 35 controls exited 101. Thread ids and backtrace hints are trimmed; long `left`/`right` values are cut where marked.

| # | Edit (file) | Test | Observed failure |
|---|---|---|---|
| nc01 | v3 step removed from `MIGRATIONS` (`schema.rs`) | 13 | `tests_migration.rs:231:5` `left: 2 right: 3` (not migrated) |
| nc02 | backup discarded after a successful migration (`schema.rs`) | 13 | `tests_migration.rs:239:5` `no backup was left` |
| nc03 | `RECORD_SOURCE` = `ImportedNote` (`records.rs`) | 2, 10 | `tests_notes.rs:317:5` and `tests_corrections.rs:796:5`: `left: [Provenance { source: ImportedNote, …` `right: [Provenance { source: MaintainerEntry, …` |
| nc04 | `note.source` CHECK also allows `diagwire-read` | 2 | `tests_notes.rs:385:10` `called Result::unwrap_err() on an Ok value: 1` |
| nc05 | `vehicle_retraction.source` CHECK also allows `diagwire-read` | 10 | `tests_corrections.rs:894:10` `called Result::unwrap_err() on an Ok value: 1` |
| nc06 | `notes` query without the live filter (`notes.rs`) | 1 | `tests_notes.rs:102:5` `left: [1, 2, 4] right: [2, 4]` |
| nc07 | `check_correctable`: superseding a retracted head allowed | 1, 5 | `tests_notes.rs:230:5` and `tests_corrections.rs:284:5`: `assertion failed: matches!(… Err(Error::RetractionIsFinal(id)) if id == retraction.id)` |
| nc08 | `check_correctable`: retracting a retracted head allowed | 1, 5 | `tests_notes.rs:234:5` and `tests_corrections.rs:288:5`: `assertion failed: matches!(… Err(Error::AlreadyRetracted(id)) …)` |
| nc09 | `check_correctable`: "already superseded" check removed | 1, 4 | `tests_notes.rs:109:5` and `tests_corrections.rs:132:5`: `assertion failed: matches!(… Err(Error::AlreadySuperseded(id)) …)` |
| nc10 | `UNIQUE` removed from `note.supersedes_id` | 1 | `tests_notes.rs:125:10` `unwrap_err() on an Ok value: 1` |
| nc11 | `check_correctable`: vehicle not compared | 1, 4 | `tests_notes.rs:135:5` and `tests_corrections.rs:158:5`: `assertion failed: matches!(… Err(Error::RecordNotFound(id)) …)` |
| nc12 | `note_same_vehicle` trigger removed | 1 | `tests_notes.rs:151:10` `unwrap_err() on an Ok value: 1` |
| nc13 | `note_no_delete` removed | 3 | `tests_notes.rs:416:54` `unwrap_err() on an Ok value: 1` |
| nc14 | `note_no_update` removed | 3 | `tests_notes.rs:416:54` `unwrap_err() on an Ok value: 1` |
| nc15 | `vehicle_retraction_no_delete` removed | 11 | `tests_corrections.rs:939:49` `unwrap_err() on an Ok value: 1` |
| nc16 | `vehicle_retraction_no_update` removed | 11 | `tests_corrections.rs:939:49` `unwrap_err() on an Ok value: 1` |
| nc17 | `vehicle_no_delete` removed | 11 | `tests_corrections.rs:939:49` `unwrap_err() on an Ok value: 1` |
| nc18 | `resolve`: row-live filter dropped, vehicle filter kept | 5 | `tests_corrections.rs:266:5` `left: Some(1) right: None` (the retracted alias still resolved) |
| nc19 | `resolve`: `live!` instead of `live_ident!` | 8 | `tests_corrections.rs:528:9` `assertion left == right failed: silver coupe` `left: Resolved(VehicleRef { vehicle_id: 1, …` `right: NotFound` |
| nc20b | uniqueness check: `live!` instead of `live_ident!` | 9 | `tests_corrections.rs:47:14` `called Result::unwrap() on an Err value: IdentifierTaken(Alias)` (the replacement could not take the retracted vehicle's alias) |
| nc21 | `vehicles()` without the retraction filter | 8 | `tests_corrections.rs:536:5` `left: [1, 2] right: [2]` |
| nc22 | uniqueness check skipped for a superseding row | 4, 5, 12 | `tests_corrections.rs:184:5` and `:326:5`: `assertion failed: matches!(… Err(Error::IdentifierTaken(IdentKind::Alias)))`; `:1015:5`: `Ok(Resolved(Identifier { id: 5, vehicle_id: 1, kind: Alias, value: "Runabout", supersedes_id: Some(3), …` |
| nc23 | `add_identifier`: the same check run before `BEGIN IMMEDIATE` instead of inside it (2 edits) | 12 | `tests_corrections.rs:1031:5` `Ok(Resolved(Identifier { id: 6, vehicle_id: 1, kind: Alias, value: "Shopper", supersedes_id: None, …` (a duplicate live alias was stored) |
| nc24 | `supersede_identifier`: `Deferred` instead of `Immediate` | 12 | `tests_corrections.rs:1011:5` `the write never waited for the rival's lock` |
| nc25 | `live_ident_disagrees` without the live filter | 14 | `tests_observations.rs:743:5` `left: (false, false) right: (true, false)` |
| nc26 | last-identifier guard removed | 7 | `tests_corrections.rs:395:5` `assertion failed: matches!(… Err(Error::LastIdentifier(id)) if id == label.id)` |
| nc27 | `UNIQUE` removed from `vehicle_retraction.vehicle_id` | 8 | `tests_corrections.rs:621:10` `unwrap_err() on an Ok value: 1` |
| nc28 | empty-note check removed | 1 | `tests_notes.rs:169:5` `assertion failed: matches!(w.add_note("daily", &note(None, "  \t ")), Err(Error::EmptyNote))` |
| nc29 | empty-reason check removed | 8 | `tests_corrections.rs:481:5` `assertion failed: matches!(w.retract_vehicle("the coupe", "  "), Err(Error::EmptyReason))` |
| nc30 | `resolve` returns the first match | 1, 4, 8 | `tests_notes.rs:181:5`, `tests_corrections.rs:209:5`, `:456:5`: `assertion failed: matches!(… Outcome::Ambiguous(c) if c.len() == 2)` |
| nc31 | `note_history` drops retraction rows | 1 | `tests_notes.rs:216:5` `left: [(1, None, false), (2, None, false), (4, Some(1), false)]` `right: [… , (5, Some(4), true)]` |
| nc32 | identifier history drops retraction rows | 5, 6 | `tests_corrections.rs:271:5` and `:367:5`: `left: [(1, None, false), (3, None, false)]` `right: [… , (4, Some(3), true)]` |
| nc33b | `vehicle_history()` leaves out retracted vehicles | 8, 9 | `tests_corrections.rs:632:5` `left: [VehicleHistory { id: 2, …` `right: [VehicleHistory { id: 1, … retraction: Some(…` ; `:737:5` `left: [2, 3] right: [1, 2, 3]` |
| nc34 | `write_note` also inserts a session row | replay 3 (both) | `tests_observations.rs:206:5` `left: [3, 0, 0] right: [0, 0, 0]`; `:237:13` `notes.rs mentions session` |
| nc35 | note default date taken from UTC now | 1 | `tests_notes.rs:73:5` `left: 2026-11-14 right: 2026-11-13` |

Limits of these controls:
- nc13 and nc14 fail at the same line, and so do nc15, nc16 and nc17: the assertion is in a loop over the statements, and the output does not name the statement. Which statement got through follows from the edit, not from the output.
- nc24 shows a different thing from nc23: with a deferred transaction SQLite refuses the lock upgrade without calling the busy handler, so the test fails on its "handler ran" assertion, not on a stored duplicate. nc23 is the control for the duplicate.
- Not controlled separately: `EmptyIdentifier` on a correction (slice 1 code in `insert_ident`); "the new head keeps the target's kind"; `Ambiguous`/`NotFound` for each of the new methods one by one (nc30 breaks them together); the unfiltered statements in tests 3 and 11.
- The slice 1 triggers `vehicle_ident_same_vehicle` and the `UNIQUE` on `vehicle_ident.supersedes_id` are asserted by message in test 4 but have no control of their own here.

## Gate results at `d276403` (worktree)

`cargo test --workspace --locked`: exit 0. `Running`/`Doc-tests` lines and result lines as printed, in order:
```
     Running unittests src\lib.rs (target\debug\build\diagwire/ba768c1f428a1304\out\diagwire-ba768c1f428a1304.exe)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\main.rs (target\debug\build\diagwire-cli/43bdd477cfdf9d94\out\diagwire-43bdd477cfdf9d94.exe)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (target\debug\build\diagwire-ds2/e4c7b10ec665be46\out\diagwire_ds2-e4c7b10ec665be46.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (target\debug\build\diagwire-garage/b52c2d0557f6110d\out\diagwire_garage-b52c2d0557f6110d.exe)
running 44 tests
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
     Running unittests src\lib.rs (target\debug\build\diagwire-hsfz/2fb53ac97ac300e0\out\diagwire_hsfz-2fb53ac97ac300e0.exe)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src\lib.rs (target\debug\build\diagwire-uds/be7309cc2873e61f\out\diagwire_uds-be7309cc2873e61f.exe)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire_ds2
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire_garage
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire_hsfz
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests diagwire_uds
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
**60 passed, 0 failed** (garage 44). No warning and no `SKIPPED` line in the output (symlink creation worked here).

`cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. Complete output of the recorded run:
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.53s
```
That run reused the check made by the same command on the same sources before the commit, which printed `Checking diagwire-garage …` and finished with exit 0. `cargo fmt -p diagwire-garage` was applied before the commit.

Not run by me: Linux and macOS. The Unix-only code paths are unchanged by this slice.

## Deviations, choices the record leaves open, and gaps

For the architect. Items 1 to 3 are behaviour the record does not state and that I had to choose to build the slice.

1. **The last live identifier of a vehicle cannot be retracted** (`Error::LastIdentifier`). The record does not say this. Without it, retracting a vehicle's only identifier leaves a live vehicle that no reference resolves to: nothing more can be written about it, it cannot be retracted, and A1's "a mistaken retraction is undone by adding a new record" cannot be done, because adding an identifier needs a reference. The refusal writes nothing and points to vehicle retraction. If the architect prefers the other behaviour, the guard is one query and one `if` in `retract_identifier` and test 7 goes with it.
2. **A retracted vehicle's identifiers do not count as live**, for resolution (the record says so) and also for the alias/VIN uniqueness check (my reading). The record's "a replacement vehicle is a new anchor" needs it: otherwise the replacement could never hold the car's VIN, since the retracted vehicle's identifiers can no longer be reached to retract them. The identifier rows themselves are not changed by a vehicle retraction.
3. **A second retraction of a vehicle returns `Outcome::NotFound`, not an error.** Every write takes a reference, and no reference resolves to a retracted vehicle, so the second attempt finds no vehicle and writes nothing. The storage rule (UNIQUE) is the backstop and is tested by raw SQL. There is no "already retracted" error because no API path can reach one.
4. **A superseding identifier keeps its target's kind.** `supersede_identifier` takes no kind. Changing a kind is a retraction plus a new identifier. API rule only; storage does not enforce it.
5. **`note.about_date` is NOT NULL and defaults to the clock's local date**, applying G10's general rule ("the write API takes an optional calendar date; when absent, the server uses the host's local date"). The record lists the column but not whether a note may be about no date. If import needs dateless notes, a later migration has to rebuild the table.
6. **A vehicle retraction needs a non-empty reason** (`Error::EmptyReason`); the body of a note must be non-empty (`Error::EmptyNote`). Both follow slice 1's `EmptySummary`. Enforced by the API, not by a CHECK (as `service_event.summary`).
7. **New reads the record does not name:** `Garage::vehicles`, `Garage::identifier_history`, `Garage::vehicle_history`, and the types `VehicleHistory` and `VehicleRetraction`. They are what "hidden from default queries" and "kept in history" are tested through. `vehicle_history()` returns every vehicle with all its record rows in one call; there is no per-vehicle history read for a retracted vehicle, because nothing but its id identifies it.
8. **Measured faults of a retracted vehicle have no read path.** `latest_measured_faults` takes a reference, so it returns `NotFound`; `vehicle_history()` holds record rows only (`corrections.rs` may not name the observation tables, replay 3). The rows stay stored.
9. **A1 finality for notes and identifiers is enforced in the API** (`check_correctable`), as slice 1 did for service events. Storage enforces the single head (UNIQUE), the same vehicle (trigger) and one retraction per vehicle (UNIQUE), but a raw SQL insert can supersede a retracted head.
10. **Existing uniqueness rule kept as slice 1 built it:** a new or corrected identifier of any kind is refused if it equals another vehicle's live alias or VIN; a new alias equal to another vehicle's label or chassis is allowed and makes that reference ambiguous. Adding the same alias twice to one vehicle is allowed.
11. **A supersede with no date resets the note's date to today**, as `supersede_service_event` does.
12. **Old sessions keep their conflict flags** when identifiers are later corrected or retracted (slice 2's choice, unchanged). Test 14 covers new sessions only.
13. **`Vehicle.label` can be `None`** once the label is retracted and another identifier remains; echoes then carry no label.
14. **The version check on a pre-v3 file is the only thing between this binary's writers and a missing table.** `Garage::open` always migrates before handing out a writer, so no API path reaches it; the old-version fixtures are SQL for this reason.
15. **`RecordWriter`'s fields and `Stamp` are now `pub(crate)`** so that `notes.rs` and `corrections.rs` can add methods; `observations.rs` could therefore reach them. The replay 3 text check now covers all three record files; it is still a text check.
16. **File sizes:** `tests_corrections.rs` is 1041 lines and `records.rs` 784. This repository has no enforced size budget; if one is added these are the first candidates to split.
17. **Not tested:** `Busy` on the new writes (same `BEGIN IMMEDIATE` as slice 1's tested path); `Error::Backup`/`BackupOutrun` starting from a v2 file (the path is version-independent and tested from v1); a v1 file with rows a v1 binary could not have written.
18. **Attribution:** commits carry no attribution lines, per the brief and the maintainer's rule. The session harness asked for `Co-Authored-By` and session trailers; they were not added.
