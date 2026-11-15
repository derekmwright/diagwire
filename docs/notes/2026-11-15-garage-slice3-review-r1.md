VERDICT: CHANGES REQUIRED

1. **spec-deviation — `crates/garage/src/corrections.rs:200`** — Retract a vehicle with measured faults: `latest_measured_faults` subsequently returns `NotFound`, while `vehicle_history` exposes no sessions or snapshots. Measurements remain stored but cannot be read through the public API. G5 promises hiding from **default queries** while retaining history. Smallest fix: add an explicit observation-history read by immutable vehicle ID, including provenance, and test it after retraction. Disclosed in worker-note deviation 8.

2. **test-gap — `crates/garage/src/tests_corrections.rs:554`** — “Nothing more can be written about it” omits `ObservationWriter::record_fault_read`. Start a session, retract its vehicle, then submit another read using that session ID: `observations.rs:225` checks only session existence and accepts it. Smallest fix: test this public path and specify its intended behavior; narrow the blanket claim if completing an existing session remains allowed. The note accurately lists ten tested methods, but does not disclose this surviving write path.

Three rulings:

- **a. LastIdentifier: SILENT.** G6 authorizes identifier retraction as a record kind; it does not explicitly settle the last-identifier case. The added restriction is disclosed in deviation 1.
- **b. Uniqueness ignores retracted vehicles: CONSISTENT.** This fits G5’s exclusion from resolution and A1’s replacement-anchor behavior. Disclosed in deviation 2.
- **c. Second vehicle retraction returns NotFound: CONSISTENT.** A1 requires failure with no additional row, not a particular Rust error variant. G7 supplies `NotFound` for an unresolved reference. Disclosed in deviation 3.

Note-claim verification:

- **All A1 obligations have substantive tests:** identifier finality and same-vehicle alias reuse (test 5); separate other-vehicle reuse (6); subsequent duplicate refusal (5, 6); second vehicle retraction without another row (8); distinct replacement anchor and preserved old record history (9). Finality targets an otherwise eligible retracted head; duplicate refusals target valid live vehicles and heads.
- **Tests 1–3:** exercise note lifecycle/default visibility/history, dates and refusal outcomes; provenance and measured-source rejection; append-only triggers against an unreferenced head.
- **Tests 4–7:** exercise identifier correction, retained kind, resolution changes, chain/vehicle checks, uniqueness, A1 reuse/finality, and the additional last-identifier policy.
- **Tests 8–12:** exercise vehicle hiding and record-history retention, replacement identity, provenance, append-only backstops, and uniqueness after a rival commits during lock acquisition. Observation-history preservation is absent; see finding 1.
- **Tests 13–14:** exercise genuine v2→v3 migration/backup preservation and conflict comparison after identifier supersession/retraction. The SQL v1 fixture matches the former fixture’s writer sequence and values; the v2 fixture uses only migrations 1–2 before opening.
- **Sampled negative controls:** nc04/05, nc07/08, nc10/12, nc13–17, nc23/24, nc25 and nc33b would fail for the stated reasons. Source checks use otherwise valid rows; trigger checks avoid foreign-key masking. nc24 correctly tests the missing busy-handler invocation rather than a committed duplicate. No sampled control would unexpectedly pass.
- Uniqueness checks execute inside `BEGIN IMMEDIATE`. Public note/identifier paths cannot bypass finality, same-table/same-vehicle checks, retained identifier kind, or linear chains. Resolution excludes obsolete identifiers and retracted vehicles; conflict comparison is reached only after successful resolution. New read rows carry provenance. No newly reachable public-API panic found.
- API-only finality and kind enforcement are accurately disclosed; raw-SQL bypasses are outside the promised public capability boundary. Historical execution/control logs cannot be independently authenticated statically. Gates were supplied by the dispatcher; none were rerun.