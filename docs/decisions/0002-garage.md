# DR-0002: Garage — vehicles, history and service log

**Status:** skeleton (architect, 2026-11-14). Not accepted. Battery registration and clear-faults move to DR-0003 (v0 is read-only per DR-0001 Q4).
**Depends on:** DR-0001 (operation tiers D6, development-agent boundary D7).

## Governing rule

The garage records what was observed and what was done, with where each fact came from. It never presents a note as a measurement, and it never leaves the maintainer's machine unless the maintainer exports it.

## Decisions (one line each; the author fills in reasons and verifies them)

- **G0 Product, not our setup:** the garage is a feature every diagwire user gets, with nothing specific to this maintainer's fleet. Anything we do for our own cars (for example importing our notes) goes through the same public features any user would use.
- **G1 Storage:** one SQLite file via `rusqlite` (bundled build), in the OS per-user local data directory (`%LOCALAPPDATA%\diagwire\` on Windows, `~/.local/share/diagwire/` on Linux, `~/Library/Application Support/diagwire/` on macOS), with the path overridable. Never in the repo.
- **G2 Crate:** a new `garage` crate with no transport or car I/O. The operations layer writes observations into it; front ends read through it.
- **G3 Schema:** vehicles, aliases, sessions, fault snapshots (with per-DTC rows), live-data samples, service events and notes. The schema is versioned, with forward-only migrations embedded in the binary.
- **G4 Provenance:** every observation carries its source (`canonical-capture:<tool>`, `diagwire-read`, `maintainer-entry` or `imported-note`), a UTC timestamp, and the diagwire revision. Queries return provenance with the data.
- **G5 Append-only:** nothing the AI writes ever edits or deletes an existing record. A correction is a new record that supersedes the old one. Hard deletion is a maintainer-only CLI action.
- **G6 Garage write tier:** through MCP, the AI may add `maintainer-entry` records (service events, notes) when the maintainer states them. This tier changes the maintainer's records, never the car. Car-state changes stay under DR-0001 D6.
- **G7 Vehicle identity:** the VIN is the key when known, and maintainer-defined aliases ("03 E46", "the M3", "son's E90") resolve to it. If a reference matches more than one vehicle, the AI must ask and must not write.
- **G8 Privacy:** the garage is never committed, never sent to any service, and export redacts VINs unless the maintainer explicitly asks otherwise.
- **G9 Test boundary:** tests use temporary databases only. Development agents never open the maintainer's garage file (this extends DR-0001 D7).
- **G10 Time and units:** relative dates ("today") are resolved to an absolute date at write time and stored as dates. Odometer readings are stored with their unit.

## Explicitly out (v0)

Cloud sync, multi-user access, GUI, photo or file attachments, parts pricing, service reminders and scheduling.

## Replays the judge will run

1. "Changed oil on the E46 today" with two E46s in the garage: no write happens, and the AI asks which car.
2. The AI tries to edit or delete an existing service record.
3. An imported note's fault list is queried as "the car's current faults".
4. An older binary opens a garage written by a newer schema version.
5. The process dies partway through writing a fault snapshot.
6. Discovery reports a stored VIN with chassis data that differs from the stored record.
7. The garage is exported without an explicit request to include VINs.
8. The test suite runs on a machine that has a real garage at the default path.
9. Two MCP server instances write to the same garage concurrently.

## Open questions for the maintainer (answerable in one line; cost given)

1. **Note import:** ship a general import feature (any user can bring in prior notes or records, labelled `imported-note`), which we then use for our own five cars? (Per G0 it's a product feature, not a one-off script.)
2. **Automatic session logging:** store every MCP read as a snapshot automatically, or only when asked? (Automatic makes "what's new since last time" work with no extra step.)
3. **Odometer unit:** miles only (US fleet), or a unit per record? (Per-record costs one column and supports any car later.)
4. **Garage location:** default OS data directory, or a path you choose? (Either way it can be overridden.) **Answered (maintainer, 2026-11-14): the user's local app-data directory**, per OS (G1).
