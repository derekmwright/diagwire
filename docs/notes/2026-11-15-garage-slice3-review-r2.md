VERDICT: CHANGES REQUIRED

| Round-1 finding | Status | Verification |
|---|---|---|
| Retracted measurements unreadable | RESOLVED | `observation_history` reads by immutable ID before/after retraction, returns session/snapshot provenance, orders sessions/reads by ID, and errors on unknown IDs. No new panic path found. |
| Late session write untested; blanket claim inaccurate | RESOLVED | New test verifies the late snapshot/DTC is stored and appears in history, excluded from default reads—including through shared chassis—and new sessions are refused. Claims narrowed. |

Both new tests exercise the intended behavior. Negative controls r1a–r1d would fail at the stated assertions. Session ordering across multiple sessions lacks direct test coverage; explicit SQL ordering is correct.

New finding:

- **Spec deviation — `docs/notes/2026-11-15-garage-slice3-architect-rulings.md:9`; `crates/garage/src/observations.rs:220`.** Ruling 5 contradicts **DR-0002:62 (G7)**: “Every write about an existing vehicle takes a reference and resolves it in the same `BEGIN IMMEDIATE` transaction as the insert.” After retraction, `record_fault_read(session_id, …)` still inserts without reference resolution. The behavior predates this commit, but the ruling newly endorses an exception the accepted record does not grant. **Smallest fix:** obtain an accepted G7 amendment explicitly exempting reads appended to existing sessions; otherwise withdraw ruling 5 and enforce G7.

Rulings **1–5 are implemented as written**. No contradiction found for **1–4**. No new implementation bug found in `72e8db8`.

This round found **fewer problems: one versus two**. Static review only; dispatcher-supplied gates accepted.