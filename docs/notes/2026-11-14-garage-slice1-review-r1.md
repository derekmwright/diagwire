VERDICT: CHANGES REQUIRED

1. **bug — `crates/garage/src/lib.rs:239`:** Public `Options.busy_timeout` can panic: `Duration::MAX` reaches rusqlite 0.42.0’s `busy_timeout`, whose conversion ends in `.expect("too big")`. `create` has already created the file. Validate the timeout against the supported integer-millisecond range before filesystem mutation; return an error.

2. **spec-deviation — `crates/garage/src/records.rs:208`:** Lowercasing does not implement G7’s case folding. An identifier `Straße` does not resolve from `STRASSE`, potentially allowing duplicate identities. Implement Unicode case folding consistently for stored identifiers and references; add this regression case. Disclosed in the note, but still contrary to the accepted requirement.

3. **spec-deviation — `crates/garage/src/records.rs:157`:** `resolve` returns an integer, and ambiguity candidates contain labels without provenance (`:160`). This contradicts G4’s “Every read result carries a provenance struct” and the note’s equivalent claim. Return provenance with resolved identities and candidates. **Undisclosed deviation.**

4. **bug — `crates/garage/src/lib.rs:118`:** `Path::exists()` follows symlinks, so a dangling `garage.db-wal` symlink is treated as absent. `create` then creates the main file despite G3’s requirement to refuse when any sidecar exists. Check directory-entry existence with `symlink_metadata`, propagate inspection errors, and test dangling sidecars. **Undisclosed deviation.**

Note-claim verification:

- **Tests 1–7:** Confirmed against their stated first-slice requirements. Fixtures contain actual rows; refusal checks inspect outcomes and persisted state; rollback checks both the current and reopened connection. None of the required checks is vacuous.
- **NC1/NC2:** Confirmed statically: first-match resolution fails the ambiguity assertion; `ImportedNote` passes the schema CHECK and fails the provenance assertion.
- **NC4/NC5:** Confirmed statically: removing both version guards permits opening; removing the transaction persists the probe table and version before failure. These involve multiple edits, contradicting the note’s “exactly one guarded line” wording.
- **Execution claims:** Scratch-copy isolation, fresh target directories, historical failure output, platform results, and dependency comparisons cannot be independently confirmed from committed source. Tests/clippy were not rerun.
- **Implementation claims:** Confirmed transaction scope, explicit noncreating open flags, validation-before-configuration ordering, internal source assignment, append-only triggers, self-table UNIQUE foreign keys, same-vehicle checks, SQLite floor, and FULL readback. The internal `unreachable!` is unreachable through the supplied public write methods.
- **Refuted:** Note `:22` says every configured pragma is read back; `busy_timeout` is only set (`lib.rs:239`). Note `:26` claims every read carries provenance; see finding 3. Note `:36` claims unchanged full dumps for all three tables; tests dump only `service_event` and `vehicle_ident` (`tests.rs:235–251`).
- **Disclosures:** Confirmed missing identifier corrections, unknown-version refusal, partial-file retention, backport refusal, missing Unix directory warning, and untested `SystemClock`. Linux validation remains unverified as disclosed. File line counts are stale.