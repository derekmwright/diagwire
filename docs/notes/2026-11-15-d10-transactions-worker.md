# D10 transactions: worker result note

**Record:** DR-0001 D10, with the injectable stream D7 asks for; epic #1, "D10 transactions in production code". First-slice steps 4 and 7. **Date:** 2026-11-15. Linux (cloud container), Rust 1.97.0. Not reviewed by the judge.

**No hardware I/O.** Nothing here connected, discovered or ran the CLI against a link (D7). Every test runs on `tokio::io::duplex` with tokio's paused clock. Every byte in the tests is hand-written, so they characterize what diagwire believes the protocol to be (D4). **None of this is conformance evidence and no register row changes.**

This was built before the X5 capture, out of the order the epic lists. If the capture contradicts a constant, the first slice's stop condition applies to this code as well.

## What was built

| File | Change |
|---|---|
| `crates/hsfz/src/lib.rs` | `Connection<S>` is generic over the stream, with `from_stream`. `send`/`recv` and the `pending` queue are gone. New: `Connection::begin` → `Transaction`, `Transaction::response`, `Transaction::finish`, `Connection::is_disposed`, `Error::Disposed` |
| `crates/diagwire/src/lib.rs` | `Session<S>` with `from_stream`; `read_dtcs` returns `Result<Outcome<DtcReport>>`; private `transact`; private `Timing`. Removed: `scan_dtcs`, `FUNCTIONAL_ALL`, the public `Session::request`, the `Negative`, `NoResponse` and `Decode` error variants |
| `crates/diagwire/src/tests.rs` | new: 20 scenarios |
| `crates/cli/src/main.rs` | `dtc` requires `--ecu`; a read that is not complete prints why and exits non-zero |
| `README.md` | usage shows the single-ECU read only |
| `crates/diagwire/Cargo.toml`, `Cargo.lock` | `tokio` and `tracing` added to `diagwire` (both already in the workspace; no new crate), `test-util` for tests |

**How disposal works.** The stream and its receive buffer are one value. `begin` takes it out of the connection, and only `finish` puts it back. Every other exit drops it, which closes the stream and discards the buffer: an error, a timeout, and a future dropped part-way (cancellation). So closing is what happens unless the code says otherwise, and there is no path that has to remember to close.

**D10 line by line:**

| D10 | Where |
|---|---|
| One outstanding transaction | `begin` borrows the connection mutably for the life of the `Transaction` |
| Deadline fixed at the start, covering the write, ack wait, alive replies and 0x78 | `transact` computes `deadline` once. `begin` wraps the write and the ack wait in one `timeout_at`. Each response wait is `deadline.min(now + wait)`. Alive-check replies are written inside those same timeouts |
| A response counts only if source = addressed ECU and target = tester | `Io::response`; anything else is skipped |
| Positive: expected SID and subfunction echo. Negative or pending: `7F` + requested service | `uds::classify` and `uds::parse_dtc_report`, unchanged |
| Closed after a timeout once the write has begun, cancellation, an interrupted write, malformed framing | see "How disposal works" |
| The five outcomes; nothing salvaged from a malformed response | `Outcome<T>`; only `Complete` has a payload |
| Functional scanning disabled | `scan_dtcs` and its constant are deleted, and the CLI no longer has a default that used them |

Kept after `Complete` and `Negative`. Closed after everything else.

## Points the record leaves open (for the architect)

Each is a choice I made. D10 does not say.

1. **A diagnostic message before the ack closes the connection (`Malformed`).** The spike queued such messages and returned them later, which is how a stale answer reached the next request (replay 2). Nothing is outstanding until the request is acked, so I treat it as ambiguity. This assumes the order request → ack → response, which the capture has not confirmed. **If a real gateway can deliver the response before the ack, every read fails as `Malformed` and this rule must change.**
2. **A missing ack is `NoResponse`.** D10's outcomes have no "gateway did not ack". The log line says which wait timed out.
3. **Timeouts.** Ack 2 s, first response 2 s, after each pending 5 s (the spike's values), and a new **operation budget of 30 s**, which is a number I chose. D10 requires the deadline and gives no value. All four are unverified.
4. **A gateway error frame (control word ≥ 0x40) and a closed link are errors, not outcomes** (`Err(Transport(Rejected))`, `Err(Transport(Io))`), and the connection is closed. Neither fits one of the five outcomes.
5. **A wrong-SID, wrong-service or undecodable reply from the right ECU is `Malformed` and closes the connection**, under "disposed of after any ambiguity". D10's list of closing conditions names malformed *framing* only.
6. **Unknown control words below 0x40 are still skipped**, as in the spike.
7. **The ack's payload is still not checked** (register: no authority yet).
8. **Why a reply was malformed goes to the log** (`tracing::warn`, stderr per D9), not into the outcome, which D10 defines without a payload.

## Deviations and limits

- **Step 5 of the first slice is not done.** The fake here is a script inside each test. It asserts diagwire's outgoing bytes against hand-written expectations, not against a capture, and there is no fixture replay.
- **The operations boundary is not done.** `pub use diagwire_hsfz as hsfz` remains, the CLI still calls `hsfz::discover` directly, `hsfz::Connection::begin` is public, and there is no allowlist. That is the epic's next slice. `Session::request` is gone as a public method only because `transact` replaced it.
- **`discover` is unchanged** and still has no injectable socket (D7).
- **Garage wiring is not done.** `diagwire::Outcome` has the same five cases as `garage::FaultRead` but nothing converts one to the other, and no read is logged.
- **A regression can hang instead of fail.** With the paused clock, a wait with no timer behind it never ends. Controls nc1, nc2 and nc4 below stopped at the 240 s limit I put on them, not at an assertion. In CI the job's 30-minute limit is the only backstop.
- **Cancellation has no control of its own.** It follows from the take-out structure and cannot be broken by changing one line. `cancelling_a_read_closes_the_connection` covers it in both waits.
- The `hsfz` crate's own tests are still the 6 codec tests. The transaction code in `hsfz` is exercised only through the `diagwire` scenarios.
- Run on Linux only.

## Scenarios (`crates/diagwire/src/tests.rs`)

| Test | Replay or step | Asserts |
|---|---|---|
| `complete_read_keeps_the_connection` | | request bytes, the fault list, a second read on the same connection |
| `no_faults_is_a_complete_read` | | `Complete` with an empty list |
| `missing_ack_is_no_response_and_closes` | replay 1 | `NoResponse` at exactly 2 s, closed |
| `blocked_write_is_bounded_by_the_deadline` | replay 1 | a 4-byte link nobody reads: `NoResponse` at 2 s, 4 request bytes on the wire, closed |
| `silence_after_the_ack_is_no_response` | | `NoResponse` at 2 s, closed |
| `pending_extends_the_wait_for_the_final_reply` | | `Complete` at 5 s |
| `pending_then_silence_is_pending_expired` | | `PendingExpired` at 5 s, closed |
| `endless_pending_stops_at_the_operation_deadline` | | a pending reply every 4 s: `PendingExpired` at exactly 30 s, closed |
| `deadline_inside_a_split_frame_closes_the_connection` | replay 3, step 7 | 9 bytes of a frame, deadline, then the rest: `NoResponse` at 2 s, the late write fails, closed |
| `impossible_frame_length_is_malformed_and_closes` | replay 3 | `Malformed`, closed |
| `diagnostic_frame_without_addresses_is_malformed` | | `Malformed`, closed |
| `replies_from_or_to_anyone_else_do_not_count` | | another source and another target are skipped: `NoResponse`; with the real answer after them, that answer |
| `invalid_responses_are_malformed_and_close` | replay 8 | seven invalid replies, each `Malformed` and closed, including a record count that is not a multiple of 4 |
| `negative_response_keeps_the_connection` | | `Negative(0x31)`, then a second read |
| `alive_checks_are_answered_inside_a_transaction` | replay 4 | answered before the ack and before the response; reply bytes asserted (format unverified) |
| `diagnostic_message_before_the_ack_is_malformed` | open point 1 | `Malformed`, closed |
| `leftover_reply_never_answers_the_next_request` | replay 2 | a second reply to a finished request does not answer the next one: `Malformed`, closed |
| `cancelling_a_read_closes_the_connection` | | dropped while waiting for the response, and for the ack: closed |
| `gateway_error_frame_is_an_error_and_closes` | | `Rejected` in place of the ack and of the response, closed |
| `gateway_hanging_up_is_an_error` | | `Io` error, closed |

"Closed" means: `is_disposed()` is true, the gateway's end reads end-of-file with no further bytes, and a second read fails with `Disposed`.

Two of these failed on their first run, both through the test script and not the code under test: `endless_pending…` measured the time of the whole join (32 s, the gateway's last sleep) instead of the read (30 s), and `leftover_reply…` had the gateway write an ack to a link the tester had already closed.

## Negative controls

Each ran in a scratch copy with its own new target directory: `cargo test -p diagwire -p diagwire-hsfz -p diagwire-uds --locked`, limited to 240 s. nc0 is the unmodified copy: 20, 6 and 5 passed. A first attempt shared one target directory between copies; the copies kept their files' timestamps, cargo reused stale builds, and those results were discarded.

| Control | Guarded line broken | Result |
|---|---|---|
| nc1 | the request is written outside the ack timeout | no assertion failed; the run hung and was stopped at 240 s (exit 124). Expected: `blocked_write_is_bounded_by_the_deadline` has no timer left to end it |
| nc2 to nc8 | connection kept after a timeout; source filter and target filter each removed; pending wait not capped by the deadline; message before the ack skipped; connection kept after an invalid reply; subfunction check removed; ack wait not capped at 2 s | **still running when this note was committed. Results follow in an update to this note.** |

## Gate results

All three slices of this session (this one, the garage default path and the VIN filter) on one tree:

`cargo test --workspace --locked` at `6ce8944`: exit 0, **100 passed, 0 failed** (`diagwire` 28, `diagwire-cli` 10, `diagwire-ds2` 5, `diagwire-garage` 46, `diagwire-hsfz` 6, `diagwire-uds` 5; doctests 0).

`cargo clippy --workspace --all-targets --locked -- -D warnings` at `6ce8944`: exit 0, no warning.

The two earlier commits of the session (`2f46f7e`, `9101248`) were not gated on their own. The tree at `2f46f7e` was tested and linted before the later work was added (20, 5, 46, 6, 5 passed; clippy clean), with the same sources; `9101248` was never built alone.
