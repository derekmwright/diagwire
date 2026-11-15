# DR-0002: Garage — vehicles, history and service log

**Status:** judge SETTLED (2026-11-14) in a fresh pass after an architectural review (rounds 8 → 4 → 2 → review → 0; verdicts and review in `docs/notes/`). **Accepted** (maintainer, 2026-11-14). All open questions answered. **Amendment A1 accepted (maintainer, 2026-11-14; judge settled in round 2):** a retraction is final (G5). It came from garage slice 1, whose code left superseding a retracted head undefined. Battery registration and clear-faults move to DR-0003 (v0 is read-only per DR-0001 Q4).
**Depends on:** DR-0001 (operation tiers D6, development-agent boundary D7).
**Author fill-in:** round 2, 2026-11-14, revising against judge round 1 (`docs/notes/2026-11-14-dr0002-judge-r1.md`) and the architect's round-2 rulings, merged onto the architect's on-disk edits. No garage code exists, so nothing below cites code; every requirement is a requirement, not a demonstrated protection. Claims about SQLite and `rusqlite` are tagged with the source checked (legend under *Authorities*) or marked **unverified**. "The maintainer" in the G-lines means whoever owns that garage, which per G0 is any user.

## Governing rule

The garage records what was observed and what was done, with where each fact came from. It never presents a note as a measurement. It leaves the user's machine only through an export the user asks for, or through results of the MCP server the user starts, which go to that user's AI client.

## Decisions (one line each; the author fills in reasons and verifies them)

- **G0 Product, not our setup:** the garage is a feature every diagwire user gets, with nothing specific to this maintainer's fleet. Anything we do for our own cars (for example importing our notes) goes through the same public features any user would use.
  - *Reason:* A one-off path for the first user's cars is untested for everyone else and would drift. Using the public path for our own data makes the first user the first acceptance test.
  - *Consequence:* No VIN, chassis code, alias or unit is hard-coded or seeded. Tests use invented vehicles. Chassis codes are free text, not a BMW enum, so nothing assumes one make.
  - *Reopen if:* never for the principle.

- **G1 Storage:** one SQLite file via `rusqlite` (bundled build), in the OS per-user local data directory (`%LOCALAPPDATA%\diagwire\` on Windows, `~/.local/share/diagwire/` on Linux, `~/Library/Application Support/diagwire/` on macOS), with the path overridable. Never in the repo.
  - *Reason:* No server, atomic transactions (S-atomic, which also lists the storage misbehaviour that breaks this), and a format users can inspect with standard tools. Windows uses Local, not Roaming, AppData because roaming profiles may sync off the machine (Windows behaviour **unverified**). On Linux the directory is `$XDG_DATA_HOME/diagwire`, defaulting to `~/.local/share` only when that variable is unset or empty (XDG).
  - *Alternatives:* JSON/TOML files (atomic replace and lock files are possible, but per-record transactions and concurrent writers would be ours to build and test). An embedded key-value store such as redb (no ad-hoc query tool for users). A server database (a service for a single-user tool).
  - *Consequence:* "One file" means one logical database: the main file plus its `-wal` and `-shm` sidecars. The WAL is persistent state, and separating it from the main file can lose committed transactions or corrupt the database (S-wal), so nothing copies or moves the main file alone. Journal mode WAL, so readers do not block the writer (S-wal); it needs every process on one host and does not work on a network filesystem (S-wal), so an override on a network share is unsupported. **Durability:** every write connection sets `synchronous=FULL` and reads it back, failing the open if it isn't 2; FULL is ACID in WAL mode, while NORMAL can roll back a committed transaction after power loss (S-sync). **Version:** the WAL-reset corruption bug affects SQLite 3.7.0–3.51.2 except the fixed backports 3.44.6 and 3.50.7, and is fixed in 3.51.3 (S-walreset). rusqlite 0.42.0 depends on libsqlite3-sys 0.40.0, whose bundled `sqlite3.h` at tag `v0.42.0` reads 3.54.1 (R-tag). The slice pins it, and open refuses any runtime `sqlite_version()` below 3.51.3. That catches an older runtime, not the loss of `bundled` as such: a system SQLite at or above the floor would pass. The default-path resolver lives in the front ends, not in `garage` (G9).
  - *Reopen if:* multi-host access or sync is accepted (both out of v0), or write latency under FULL is a measured problem.

- **G2 Crate:** a new `garage` crate with no transport or car I/O. The operations layer writes observations into it; front ends read it only through operations (DR-0001 D1; reworded per architect ruling 1, r2).
  - *Reason:* Storage tests then need no transport fakes (D7) and no runtime. Without car I/O, the garage can never become a path to hardware.
  - *Consequence:* Package `diagwire-garage`. It is synchronous; operations call it via `spawn_blocking` (D2). It never exposes `rusqlite::Connection` or raw SQL. It hands out two capabilities: `RecordWriter` (vehicles, identifiers, service events, notes, imports; it sets the source to `maintainer-entry` or `imported-note` itself) and `ObservationWriter` (sessions and fault snapshots; it sets the source to `diagwire-read` itself). No write method takes a source parameter. In operations, the MCP write tier and the import path hold only a `RecordWriter`; only the allowlisted live-read operation (D6) holds an `ObservationWriter`. Front ends hold neither (D1).
  - *Reopen if:* an operation needs storage and car I/O in one atomic step.

- **G3 Schema:** vehicles, aliases, sessions, fault snapshots (with per-DTC rows), live-data samples, service events and notes. The schema is versioned, with forward-only migrations embedded in the binary. Each table is created by the migration that lands its first writer.
  - *Reason:* A database outlives any one binary, so its version must be in the file. SQLite keeps an integer at header offset 60 that it never uses itself (`user_version`), and a 32-bit application ID at offset 68 meant to identify the file format (`application_id`) (S-pragma).
  - *Consequence, create vs open:* `create(path)` refuses if the path or any sidecar exists. It makes the directory user-private (G8), then sets and verifies the connection pragmas outside any transaction (journal mode can't change inside one, and `foreign_keys` is a no-op inside one, S-pragma), then writes `application_id`, the schema and `user_version` in one transaction. `open(path)` never creates: rusqlite's default `open` includes `SQLITE_OPEN_CREATE` (R-conn), so `open` passes explicit flags without it. Before changing any configuration, `open` reads `application_id`, `user_version` and the SQLite version, and refuses a foreign file. That includes an application ID of 0, so an existing but unrecognised database, even an empty one, is never initialised. It also refuses a version newer than the binary knows (replay 4). Only after that does it set `busy_timeout`, `foreign_keys` (off by default and per connection, S-fk), `synchronous` and the journal mode.
  - *Consequence, migrations:* To migrate an existing garage, a SQLite-consistent backup of the logical database (never a copy of the main file) must succeed first, or the migration aborts with nothing changed. The backup's coverage is defined at its own snapshot boundary (the API is deferred; the online backup API and `VACUUM INTO` are candidates, **unverified**). The migration then takes `BEGIN IMMEDIATE` and checks that no other connection committed after that snapshot (for example via `PRAGMA data_version`, **unverified**); if one did, it releases the lock and backs up again, so the backup holds every commit before the migration. It then re-reads `user_version` under that lock (another process may have migrated already), and applies the remaining steps and the version bump in that one transaction. Whether a `user_version` write rolls back with its transaction is **unverified**; first-slice test 5 settles it. v1 is creation only, so the first slice needs no backup.
  - *Alternatives:* An external migration tool (users have none). A schema-version table (works, but the header integer is readable before any table exists).
  - *Reopen if:* users need to open one garage from two binary versions in turn.
  > Author dissent: create each table in the migration that adds its first writer, not all at once. Live-data samples have no writer in v0, because D6 allows no per-DID read until that DID has its own allowlist entry. An unwritten table is schema nobody tests, and forward-only migrations make adding it later cheap.
  > Architect ruling (2026-11-14): accepted; G3 extended.
  > Author dissent (r2, on ruling 5's "byte-identical = main file plus sidecars"): this is achievable only for a cleanly closed garage. Closing the last connection checkpoints and deletes the WAL (S-wal), so a refusing open of a garage left with a `-wal` by a crash may fold that WAL into the main file. Proposed definition: for a cleanly closed garage (no sidecars), a refusal leaves the main file byte-identical and no sidecar behind; with a leftover WAL, the logical contents and header values are unchanged. Test 4 uses the clean case.
  > Architect ruling (2026-11-14): accepted; this definition replaces ruling 5's "byte-identical" wording.

- **G4 Provenance:** every observation carries its source (`canonical-capture:<tool>`, `diagwire-read`, `maintainer-entry` or `imported-note`), a UTC timestamp, and the diagwire revision. Queries return provenance with the data.
  - *Reason:* The governing rule: a reader must be able to tell a measurement from a note. Same rationale as D4.
  - *Consequence:* The protection is G2's capability split. Source is assigned inside `garage` by the writer used, and the import and MCP paths cannot reach session or snapshot writers. A CHECK limiting `session.source` to measured values is only a backstop. An imported fault list is stored as an `imported-note` note. Observations carry provenance through a NOT NULL link to their session; records carry it on the row. The revision is the writing binary's version string fixed at build time. v0 has no `canonical-capture` writer. If one is added, each row must reference its capture's D4 stamp (fixture hash), not just a tool name. Every read result carries a provenance struct.
  - *Alternatives:* Provenance per table. A free-text source. Caller-supplied source plus a CHECK (the judge's round-1 objection: any caller could claim `diagwire-read`).
  - *Reopen if:* a fifth source kind is needed.

- **G5 Append-only:** nothing the AI writes ever edits or deletes an existing record. A correction is a new record that supersedes the old one; a mistake is retracted by a retraction record. There is no hard deletion in v0 through any front end (erasure belongs with DR-0003's approval mechanism).
  - *Reason:* History is the product's value, and a model that mis-hears can otherwise destroy it. Supersession keeps the wrong entry visible as wrong.
  - *Consequence:* Every table the AI can write is append-only, including `vehicle` and `vehicle_ident` (ruling 2), along with `service_event`, `note` and the observation tables. (1) API: inserts only; a correction is an insert with `supersedes_id`, checked to be the same table and vehicle. (2) Storage: `BEFORE UPDATE`/`BEFORE DELETE` triggers `RAISE(ABORT, …)`, which ends the statement with `SQLITE_CONSTRAINT` (S-trig). `supersedes_id` is UNIQUE, so the chain stays linear. **Retraction (defined once, here):** always an append-only record that hides its target from default queries (and from alias resolution, for vehicles) and keeps it in history. For chain-bearing records (identifiers, service events, notes) it is a superseding row with `retracted = 1`; for a vehicle, which has no chain, it is a `vehicle_retraction` row. **A retraction is final (amendment A1):** a retracted head cannot be superseded, and a vehicle is retracted at most once. A mistaken retraction is undone by adding a new record, which leaves the retraction visible in history. Changing a label, adding a VIN learned later or dropping an alias is a supersede or retraction of a `vehicle_ident` row; the `vehicle` row is an immutable anchor that history points at. Residual, as in D7: an agent with a shell can edit the file with any SQLite tool.
  - *Alternatives:* Soft-delete flags (still an edit). Event sourcing for everything.
  - *Reopen if:* growth from superseded rows becomes a measured problem.
  > Author dissent: drop hard deletion from v0. DR-0001's judge established that "CLI-only" does not stop an AI user that has a shell (DR-0001 Q4), so a CLI delete reopens replay 2 for any such user. Retraction covers mistakes; true erasure needs an approval channel the model can't forge, which is DR-0003's problem. Without hard delete, nothing ever lifts the triggers.
  > Architect ruling (2026-11-14): accepted; G5 reworded.

- **G6 Garage write tier:** through MCP, the AI may add `maintainer-entry` records (vehicles, aliases, service events, notes) when the maintainer states them. Front ends reach the garage only through the operations layer (consistent with DR-0001 D1; no amendment needed). This tier changes the maintainer's records, never the car. Car-state changes stay under DR-0001 D6.
  - *Reason:* Logging "changed the oil" by talking is the main use, and a new user should never need the CLI. Records can be superseded (G5); car state can't be undone.
  - *Consequence:* The MCP tier's write rule: it may add, and supersede or retract (G5), any record kind a `RecordWriter` holds (vehicles, which are retracted but never superseded; identifiers such as label, alias, VIN and chassis; service events; notes), plus read queries. No raw SQL, no observation writer. A new live identifier that equals another vehicle's live alias or VIN is refused inside the write transaction. Every write returns the stored row (label, absolute date, odometer and unit), redacted per G8, so the user sees what was written. The server cannot verify the user actually said it. Adding a VIN-less car twice can't be detected. The duplicate is retracted (G5); the `vehicle` anchor itself never changes.
  - *Alternatives:* No AI writes (loses the main use). Per-write confirmation (no forge-proof channel yet, DR-0003). Vehicle management CLI-only (rejected by ruling 2).
  - *Reopen if:* MCP gains a standard human-approval primitive.

- **G7 Vehicle identity:** the VIN is the key when known, and maintainer-defined aliases ("03 E46", "the M3", "son's E90") resolve to it. If a reference matches more than one vehicle, the AI must ask and must not write. If discovery reports a known VIN with data that conflicts with the stored record, the conflict is recorded and flagged; the stored record is never silently overwritten.
  - *Reason:* A VIN is unique but not always known (no cable, older cars). People name cars by nickname.
  - *Consequence:* Resolution normalises the reference (trim, case-fold, collapse spaces) and collects the distinct vehicles with a live identifier (label, alias, VIN or chassis) equal to it: one → `Resolved`; several → `Ambiguous(candidates)`; none → `NotFound`. No fuzzy matching. Every write about an existing vehicle takes a reference and resolves it in the same `BEGIN IMMEDIATE` transaction as the insert, so a concurrent addition cannot slip past. `Ambiguous` and `NotFound` write nothing. Residual: the garage refuses ambiguous references it receives; it cannot stop a model from choosing one alias itself. The G6 echo is the mitigation. A discovery conflict is stored on the session and returned flagged; the vehicle's identifiers are unchanged.
  - *Alternatives:* VIN-only (unusable without a cable). Most-recently-used match (silently wrong on replay 1).
  - *Reopen if:* users need fuzzy matching; it must still never auto-pick.

- **G8 Privacy:** the garage is never committed and never sent anywhere except through a user-requested export or the user-started MCP server (starting it is the user's consent to disclose garage data to their AI client, as DR-0001 D6 treats consent for reads). Export and MCP results redact VINs by default, referring to vehicles by label (Q5, answered 2026-11-14).
  - *Reason:* A VIN identifies a car and, through registration records, often its owner. Service history is personal. "The VIN isn't needed at the provider edge" (Q5).
  - *Consequence:* One outgoing filter in operations covers **every** string bound for the AI client or an export: results, labels, aliases, ambiguity candidates, write echoes and error messages. It replaces each stored VIN wherever it appears, case-insensitively, including inside a label, alias or note. It also replaces any 17-character token of VIN shape (alphabet per ISO 3779, **unverified** here). Each is replaced with a per-response placeholder (`vehicle-1`, …). Including VINs is a user-side switch (an export flag or a server launch flag), never a tool parameter the model can set. **Residual, stated once and meant everywhere:** this is best effort. A VIN written with separators, truncated (for example the last 7 characters) or mistyped can pass, and free text can carry plates, names and places. Logs record operation names, row ids, counts and outcomes, never garage payloads. On Unix the garage directory is created 0700, so the database, sidecars and backups all sit inside it, and files are created 0600. On Windows the per-user `%LOCALAPPDATA%` ACL is relied on (**unverified**) and never widened. A directory the user supplies is used as-is, with a warning on Unix if group or others can access it.
  - *Reopen if:* sync or sharing is accepted.
  > Author dissent: through MCP, garage contents go to whatever AI provider the user's client uses. That is "sent to a service", and it contradicts both this line and the governing rule as written. Proposed: treat MCP read results as a disclosure the user makes by starting the server (as D6 treats consent for reads), and redact VINs in MCP results by default exactly as in export, referring to vehicles by label.
  > Architect ruling (2026-11-14): accepted; governing rule and G8 reworded. The VIN-redaction default is user-visible, so it goes to the maintainer as Q5.

- **G9 Test boundary:** tests use temporary databases only. Development agents never open the maintainer's garage file (this extends DR-0001 D7).
  - *Consequence:* `create`/`open` require an explicit path, and `garage` has no default-path function or dependency, so no `garage` test reaches the real file without a literal path in review. Tests use `tempfile` directories. Front-end tests that resolve paths set the override to a temp directory. Residual, as in D7: an agent with a shell can run the binary against the default path.
  - *Reopen if:* never for the boundary.

- **G10 Time and units:** relative dates ("today") are resolved to an absolute date at write time and stored as dates. Odometer readings are stored with their unit.
  - *Reason:* "Today" stored as text is wrong tomorrow, and a model's idea of today can be wrong, so the server owns the clock.
  - *Consequence:* The write API takes an optional calendar date; when absent, the server uses the host's local date from an injected clock (no wall-clock tests). `recorded_at` is UTC (G4); `event_date` is the local date meant. Odometer: value plus `mi`/`km`, both or neither (CHECK). A date the model computed itself can't be checked; the G6 echo shows it. G10 already decides per-record units, so Q3 = "miles only" would change this line.
  - *Reopen if:* engine-hours meters are needed.

## Explicitly out (v0)

Cloud sync, multi-user access, GUI, photo or file attachments, parts pricing, service reminders and scheduling.

## Schema sketch (key columns only)

Timestamps UTC ISO-8601; dates `YYYY-MM-DD`. `prov` = `source`, `recorded_at`, `diagwire_rev` (NOT NULL, set by the writer). `chain` = `supersedes_id` NULL UNIQUE, `retracted`. Every table has the append-only triggers.

| Table | Key columns | Notes |
|---|---|---|
| `vehicle` | `id`, `prov` | immutable anchor |
| `vehicle_ident` | `vehicle_id`, `kind` (label/alias/vin/chassis), `value`, `value_norm`, `chain`, `prov` | live alias/VIN uniqueness checked in the write txn |
| `vehicle_retraction` | `vehicle_id` (unique), `reason`, `prov` | a vehicle's retraction record (G5) |
| `service_event` | `vehicle_id`, `event_date`, `summary`, `odo_value`, `odo_unit`, `chain`, `prov` | odo pair CHECK |
| `note` | `vehicle_id`, `body`, `about_date`, `chain`, `prov` | |
| `session` *(operations slice)* | `vehicle_id` NULL, `reported_vin`, `reported_chassis`, `prov` | measured-source CHECK (backstop) |
| `fault_snapshot` *(operations slice)* | `session_id`, `ecu_address`, `outcome`, `read_at` | `outcome` ∈ D10 outcomes |
| `fault_dtc` *(operations slice)* | `snapshot_id`, `code`, `status` | only under `outcome = complete` |

A complete snapshot with no DTC rows means "no faults"; any other outcome means unknown. **Latest measured faults**, per vehicle and ECU, is the newest complete snapshot, shown together with any newer non-complete attempts (outcome and time), and never labelled "current". DTC text is not stored (D8).

## Replays the judge will run

1. "Changed oil on the E46 today" with two E46s in the garage: no write happens, and the AI asks which car.
   - **Prevented by G7 once built** (first-slice test 1): "E46" matches both chassis identifiers, giving `Ambiguous`, and nothing is written. Residual: a model that picks "03 E46" itself bypasses the refusal; the G6 echo is the mitigation.
2. The AI tries to edit or delete an existing service record.
   - **Prevented for diagwire's paths by G5/G6:** no edit or delete operation exists, and triggers reject UPDATE/DELETE from any code using `garage` (test 3). **Only described** for an AI with a shell using an SQLite tool directly. There is no hard delete in v0.
3. An imported note's fault list is queried as "the car's current faults".
   - **Prevented by G2/G4's entry-point boundary:** the import path holds only a `RecordWriter`, which cannot create sessions or snapshots and takes no source, so an imported fault list can only become an `imported-note` note. The faults query returns latest measured faults (schema sketch), never "current". **Negative test (operations slice):** import a record containing a fault list; session and snapshot counts stay 0 and the faults query for that vehicle reports no measurement. *NC:* route the import's fault list to the observation writer.
4. An older binary opens a garage written by a newer schema version.
   - **Prevented by G3's open protocol** (test 4): refused before any configuration change, and the file is unchanged as defined in the G3 r2 dissent. The version is rechecked under the migration lock. SQLite itself does not enforce this (S-pragma).
5. The process dies partway through writing a fault snapshot.
   - **Prevented by one transaction per snapshot plus SQLite's atomic commit and `synchronous=FULL`** (S-atomic, S-sync, given storage that honours sync). Only `Complete` writes DTC rows (D10). The operations-slice test injects a failure between rows and checks the snapshot count is unchanged. That tests our transaction scoping; crash recovery itself rests on S-atomic and is not tested by us.
6. Discovery reports a stored VIN with chassis data that differs from the stored record.
   - **Overwrite prevented, settlement only described:** G7 records the conflict on the session and flags it; identifiers are unchanged. The user settles it later by superseding an identifier.
7. The garage is exported without an explicit request to include VINs.
   - **Prevented for stored VINs and VIN-shaped tokens in every outgoing string by G8; best effort beyond that** (G8 residual). The same filter covers MCP results.
8. The test suite runs on a machine that has a real garage at the default path.
   - **Prevented structurally for `garage`** (G9). **Only described** for front ends and agent shells.
9. Two MCP server instances write to the same garage concurrently.
   - **Prevented by G7's transaction rule:** one write transaction at a time (S-txn, S-wal). Every write is `BEGIN IMMEDIATE`, which takes the lock up front or fails with `SQLITE_BUSY` (S-txn). The busy timeout is set explicitly, because rusqlite's 5000 ms default may change (R-conn). A busy write fails and is not retried (test 6).

## Authorities

| Contract | Authority | Status |
|---|---|---|
| User-visible behaviour (replays, G6 echo, G7 refusal, G8 filter) | Maintainer-approved acceptance scenarios (the replays, once approved) | no authority yet |
| `user_version`, `application_id` | S-pragma | verified (doc read) |
| Atomic commit; `synchronous` FULL vs NORMAL in WAL | S-atomic, S-sync | verified (doc read); our use untested |
| One writer, `BEGIN IMMEDIATE`, `SQLITE_BUSY`; WAL as persistent state, checkpoint and delete on last close | S-txn, S-wal | verified (doc read) |
| WAL-reset bug fixed in 3.51.3; rusqlite 0.42.0 bundles 3.54.1 | S-walreset, R-tag | verified (doc and source read); runtime check in test 7 |
| `RAISE(ABORT)`; foreign keys per connection | S-trig, S-fk | verified (doc read) |
| `Connection::open` creates by default; `busy_timeout`; `Immediate` | R-conn, R-txb | verified (doc read) |
| `user_version` write rolls back with its transaction | none found | **unverified** (test 5) |
| Consistent backup API; Windows AppData ACL and roaming; VIN alphabet | none checked | **unverified** |
| Linux data directory | XDG | verified (doc read) |

An acceptance scenario is a requirement, not an independent implementation (playbook §2). With no outside oracle for garage semantics, the tests check stated properties.

Sources (fetched 2026-11-14): S-pragma https://www.sqlite.org/pragma.html · S-sync https://www.sqlite.org/pragma.html#pragma_synchronous · S-wal https://www.sqlite.org/wal.html · S-walreset https://www.sqlite.org/wal.html#walreset · S-txn https://www.sqlite.org/lang_transaction.html · S-atomic https://www.sqlite.org/atomiccommit.html · S-trig https://www.sqlite.org/lang_createtrigger.html · S-fk https://www.sqlite.org/foreignkeys.html · R-conn https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html · R-txb https://docs.rs/rusqlite/latest/rusqlite/enum.TransactionBehavior.html (docs.rs "latest" = 0.42.0) · R-tag https://raw.githubusercontent.com/rusqlite/rusqlite/v0.42.0/libsqlite3-sys/sqlite3/sqlite3.h and …/libsqlite3-sys/Cargo.toml · XDG https://specifications.freedesktop.org/basedir-spec/latest/

## First slice (after acceptance)

Mode: feature. Outcome: `diagwire-garage` with `create`/`open`, migration v1 (`vehicle`, `vehicle_ident`, `service_event`), and a `RecordWriter` that adds vehicles and identifiers, refuses ambiguous references, and adds, supersedes and retracts service events, all on temp databases (G9). No operations, MCP, CLI, sessions, notes, export or import. Dependencies: `rusqlite =0.42.0` (bundled), a date crate, `tempfile` (dev). Negative controls (break the guarded line in a scratch copy, run in a fresh `CARGO_TARGET_DIR`, see the stated failure, restore) are required for the refusal, rollback and provenance boundaries (tests 1, 2, 4, 5).

1. **Replay 1:** two vehicles with chassis `E46`; a service event for "e46" → `Ambiguous` listing both, row count unchanged; a unique alias resolves and writes. *NC:* resolver returns the first match.
2. **Provenance:** every row the `RecordWriter` writes has `source = maintainer-entry`, the build revision, and timestamps and the default date from the injected clock; no public write method accepts a source. *NC:* change the writer's internal source constant.
3. **Append-only:** raw UPDATE/DELETE on `service_event` and `vehicle_ident` fail and rows are unchanged. A supersede adds a head; a retracted head is hidden by default and shown in history; a second supersede of one row fails; superseding a retracted service event fails, and a fresh service event added afterwards succeeds while history still shows the retraction (A1); a duplicate live alias is refused; an odometer value without a unit is refused.
4. **Open refusal (replay 4):** on cleanly closed temp files, `open` refuses a newer `user_version`, a foreign `application_id`, and an empty SQLite file with no application ID; each leaves the main file's hash unchanged and no sidecar behind. `open` on a missing path creates nothing; `create` on an existing path refuses. *NC:* remove the version check.
5. **Migration rollback:** a test-only migration creates a table, sets `user_version`, then fails; both the table and the version revert. If the version does not revert, stop. *NC:* commit before the injected failure.
6. **Lock (replay 9):** connection A holds `BEGIN IMMEDIATE`; B, with busy timeout 0, gets `Busy` and no row appears.
7. **Durability and version:** after `open`, the write connection reports `synchronous` = 2 and `sqlite_version()` ≥ 3.51.3.

Stop conditions: the bundled build fails on Windows or Linux; test 5's version escapes rollback; the pinned rusqlite's runtime SQLite is below 3.51.3; Q3 is answered "miles only" (G10 changes first). Next slice: the observation writer, sessions and fault snapshots, with replays 3 and 5.

**A1 tests owed by the slice that adds identifier correction and vehicle retraction:** a retracted identifier cannot be superseded; a fresh identifier with the same normalised alias succeeds on the same vehicle and, separately, on another vehicle, with history keeping the retraction; a subsequent live duplicate across vehicles is refused; a second retraction of one vehicle fails and adds no row; a replacement vehicle is a new anchor, and the old one's history is preserved.

## Open questions for the maintainer (answerable in one line; cost given)

1. **Note import:** ship a general import feature (any user can bring in prior notes or records, labelled `imported-note`), which we then use for our own five cars? (Per G0 it's a product feature, not a one-off script.) **Answered (maintainer, 2026-11-14): yes**, a general import feature, always labelled `imported-note`.
2. **Automatic session logging:** store every MCP read as a snapshot automatically, or only when asked? (Automatic makes "what's new since last time" work with no extra step.) **Answered (maintainer, 2026-11-14): automatic.**
3. **Odometer unit:** miles only (US fleet), or a unit per record? (Per-record costs one column and supports any car later.) **Answered (maintainer, 2026-11-14): a unit per record** (as G10 already states).
4. **Garage location:** default OS data directory, or a path you choose? (Either way it can be overridden.) **Answered (maintainer, 2026-11-14): the user's local app-data directory**, per OS (G1).
5. **VINs in MCP results:** redact by default and refer to vehicles by label (VINs are filtered from what the AI provider receives, best effort per G8), or include them? (Redaction costs a little convenience. Including them sends VINs to the AI provider on every garage query.) **Answered (maintainer, 2026-11-14): redacted by default in everything sent to the AI client** ("the VIN isn't needed at the provider edge").
