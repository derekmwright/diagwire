# DR-0001: Architecture, authorities and safety boundary

**Status:** **Accepted** (maintainer, 2026-11-14). Judge settled in round 3 (rounds required 10 → 2 → 0 changes; verdicts in `docs/notes/`). All open questions answered.
**Spike:** everything under `crates/` at this date is a spike written before this record. It is disposable and needs an explicit productionization decision.
**Author fill-in:** round 2, 2026-11-14. This round revises against judge round 1 (`docs/notes/2026-11-14-dr0001-judge-r1.md`) and the architect's rulings. The spike's provenance is commit `2abb6e0` (github.com/derekmwright/diagwire). Citation legend: `hsfz:N`, `uds:N`, `ds2:N` and `diagwire:N` mean `crates/<crate>/src/lib.rs` line N at `2abb6e0`. `cli:N` means `crates/cli/src/main.rs`. `INI:N` means the maintainer's local `C:\EDIABAS\Bin\EDIABAS.INI` (EDIABAS 7.3.0), which I read but did not change. D4–D7 carry r2 revisions approved by the architect; D10 is new in r2.

## Governing rule

diagwire may only claim a protocol behavior that is checked against an authority outside diagwire, and it never changes vehicle state without the maintainer's explicit per-action consent.

## Decisions (one line each; the author fills in reasons and verifies each against code)

- **D1 Layering:** transports (`hsfz`, later `kline`) / pure protocol codecs with no I/O (`uds`, `ds2`, later `kwp`) / operations (`diagwire`) / front ends (`cli`, `mcp`).
  - *Reason:* With no I/O, a codec can be tested byte for byte against fixtures, without hardware or a runtime. UDS is written once for every transport. One operations layer stops the CLI and MCP drifting apart.
  - *Alternatives:* One crate per car generation (UDS would be duplicated). A single crate with modules (nothing would check that codecs stay free of I/O).
  - *Consequence:* Operations reach transports through a seam that a fake can fill (D7). Front ends reach only operations (D6).
  - *Spike:* **Partial match.** `uds` and `ds2` depend only on `thiserror`. Mismatches: `Session` holds a concrete `hsfz::Connection` and wraps `hsfz::Error` (`diagwire:20,31-33`). The CLI calls `hsfz::discover` directly (`cli:47,84`). HSFZ framing (`hsfz:56-96`, pure) lives in the transport crate. No crate uses `ds2`, and cargo warns "unused workspace dependency".
  - *Reopen if:* a transport needs protocol state the codec boundary can't express.

- **D2 Runtime:** tokio async in transports, operations and front ends, because the MCP server (rmcp) is tokio-based. Codec crates never depend on tokio.
  - *Reason:* A single runtime means no thread bridge. Async timeouts make reads cancellable. "rmcp is tokio-based" is the skeleton's claim, and it is unverified here because rmcp isn't a dependency yet.
  - *Alternatives:* Blocking threads, bridged only in `mcp`. A sans-IO core with per-runtime adapters. smol.
  - *Consequence:* K-line needs an async serial crate or `spawn_blocking`. Deterministic timing tests need every deadline on `tokio::time`, or an injected clock.
  - *Spike:* **Partial match.** `hsfz` and the CLI use tokio (`hsfz:17-19`, `cli:38`). `scan_dtcs` measures time with `std::time::Instant` (`diagwire:5,75`), so it cannot use tokio's paused clock unchanged.
  - *Reopen if:* rmcp or the serial crate forces another runtime.
  > Author dissent (open from r1): read "throughout" as transports, operations and front ends. The codec crates stay runtime-free, as they already are in the spike.
  > Architect ruling (2026-11-14): accepted; D2 reworded.

- **D3 Authorities:** UDS → ISO 14229-1. HSFZ → traffic captured from a canonical client (E-Sys or EDIABAS) against the real X5, with EdiabasLib as an independent reader. DS2 → EDIABAS `api32.dll` on the M3, used as a differential oracle via its trace.
  - *Reason:* Each protocol gets the strongest outside oracle available. The author knows of no public HSFZ specification. The M3's EDIABAS setup works today.
  - *Alternatives:* Open-source implementations alone (they may share our misreadings, so they are only an independent reader). Simulators (see D4).
  - *Consequence:* No HSFZ claim before the X5 capture. ISO 14229-1 is paywalled, so a UDS claim needs a clause check against the maintainer's copy. The DS2 oracle depends on what the trace contains. `INI:16-17` has `ApiTrace`/`IfhTrace` = 0, and `INI:188-195` documents `IfhTrace` 2 as "User trace + interface". Whether either shows raw telegrams, including the echo, is **unverified**. If it doesn't, a passive serial tap is needed.
  - *Spike:* **No authority exercised.** The spike's doc comment attributes the HSFZ constants to "public reverse-engineering notes" (`hsfz:7-9`). Per the architect's brief, they were recalled rather than captured, which code can't establish. Open discrepancy: the spike sends UDP discovery to 6811 (`hsfz:22,233-246`), but `INI:626-630` calls 6811 a "control channel of TCP connection to ZGW". Neither settles the other.
  - *Reopen if:* a public HSFZ specification appears, or the capture tooling can't see the ENET link.

- **D4 Fixtures (revised r2):** captures from real cars only, each stamped with provenance (car, tool, date, revision). The general loader rejects unstamped fixtures. A separate conformance gate admits only canonical-client fixtures. Nothing diagwire produced about itself counts as correctness evidence.
  - *Reason:* Goldens a program produced for itself prove only self-consistency (playbook §1). A stamp lets a reviewer tell evidence from belief.
  - *Alternatives:* Hand-written vectors (the spike's approach: they characterize belief). EDIABAS simulation files (`INI:9,12`), which are hand-written too.
  - *Consequence:* No conformance test before the first capture. Before any capture is committed it needs a provenance stamp (including the diagwire revision, or "n/a" for a client-only capture), a redaction review, and a list of the transformations applied. The original stays private, referenced by hash. Characterization fixtures can still load but are ineligible for conformance.
  - *Spike:* **Does not match.** No fixtures, stamp or loader. All 16 tests use hand-written bytes, for example invented DTCs (`uds:222`) and an invented ident/VIN (`hsfz:334`).
  - *Reopen if:* a path can't practically be captured (for example, gateway error replies). It would need a labelled class of reviewed examples.
  > Author dissent (open from r1, narrowed): please rule explicitly that a capture where diagwire is the tester and the car answers is characterization, not conformance. The first slice assumes that.
  > Architect ruling (2026-11-14): accepted. A capture with diagwire as the tester is characterization only and can never back a conformance claim, even though the car's replies are real.

- **D5 Job definitions (revised r2):** the declarative job-file mechanism is deferred until a verified BMW-specific job needs it. No BEST/2 interpreter in v0.
  - *Reason:* The first slice needs only the standard ISO service 0x19/0x02. A format designed before any verified job exists would be guesswork.
  - *Alternatives:* A BEST/2 interpreter over the user's SGBDs (large, out of scope, and it only repeats what the SGBD says). ODX (proprietary data, ruled out by D8). Hand-coding each job in Rust (acceptable for the first few).
  - *Consequence:* No job loader in the first slice. When the format arrives, every job's scaling and text needs an authority (an EDIABAS result, or the client's display).
  - *Spike:* **Matches.** Requests are hard-coded builders (`uds:29-41`), with no job files and no interpreter.
  - *Reopen if:* the first verified BMW-specific DID or DS2 job lands.

- **D6 Product boundary (revised r2, maintainer 2026-11-14):** the product's AI user reaches hardware only through an MCP server the maintainer starts with the cable attached. Starting the server is the consent for reads. It exposes only operations on a reviewed read allowlist. Discovery is live I/O. **v0 is read-only** (Q4): there is no state-changing tier. Coding and flashing are not exposed.
  - *Read tier:* An allowlist entry names the operation, transport, exact request template (service, subfunction, permitted parameter values), target ECUs, response decoder, the register rows it depends on, and the reviewer and date. Entries come from review, never from the model, and an operation qualifies because it was reviewed, not because it is called a read. The v0 candidates are `discover` and single-ECU `read_dtcs` (0x19 0x02 mask). There is no functional scan (D10), no generic raw-request tool, and no per-DID read until that DID has its own entry.
  - *Assumptions (unverified, see register):* that these reads don't change ECU state, that a clear erases freeze-frame data, and what 0xDF fans out to. Whatever rests on them must be checked against the authority before its allowlist entry is approved.
  - *Enforcement:* Front ends (MCP **and** CLI, security addendum A3) link only the allowlisted operations API. The re-exported raw transport (`pub use diagwire_hsfz as hsfz`, `diagwire:7`; `Connection::send`, `hsfz:143`) and the raw `Session::request` (`diagwire:44-61`) must not be reachable from any front end. The server opens live transport only when the maintainer launches it (development agents never do, D7).
  - *State-changing tier:* none in v0 (Q4). No front end has any state-changing operation. `clear_faults` and battery registration, with their approval mechanism, belong to DR-0003.
  - *Alternatives:* All tools free (breaks the governing rule). A session-level unlock for writes (not per-action).
  - *Spike:* **Does not match.** There is no MCP crate. The raw transport is public through `diagwire:7`, and `Session::request` is public and panics on an empty request (`diagwire:45`). The CLI reaches `hsfz` directly. `uds::clear_all_dtcs` (`uds:34-36`) has no caller.
  - *Reopen if:* MCP gains a standard human-approval primitive, or DR-0003 is accepted.

- **D7 Development-agent boundary (revised r2, maintainer 2026-11-14):** development agents never initiate hardware I/O: no discovery, no connect, no CLI or server run against a real link. Tests use injectable fake streams only. Hardware runs are maintainer-run.
  - *Reason:* An agent's mistake on a live bus can change vehicle state. Fakes make splits, timeouts and alive checks deterministic.
  - *Enforcement:* Transports accept an injected stream, and discovery accepts an injected socket. Tests construct only fakes, and review rejects any test or script that calls real connect or discovery. Residual risk: an agent with a shell can still run the binary. The code doesn't stop it.
  - *Alternatives:* EDIABAS simulation (hand-written files, D4). Read-only hardware access for agents (nothing guarantees it stays read-only).
  - *Spike:* **Does not match.** No transport has a fake. `Connection` has no injectable stream: `connect` builds its own `TcpStream` (`hsfz:119-140`). `Session` wraps that concrete type (`diagwire:31-40`). `discover` sends real broadcasts (`hsfz:242-246`), and `diagwire dtc` without `--ip` runs it implicitly (`cli:56-59`).
  - *Reopen if:* never for the boundary. The fake's design is revisited if fakes diverge from captures.

- **D8 BMW data:** no SGBD, ODX or other BMW-proprietary data in the repo. A local tool may read the user's own copies.
  - *Reason:* BMW's data is proprietary and licensed.
  - *Consequence:* Decoded diagnostic evidence from the maintainer's own car is **allowed**: captured bytes, the client's decoded DTC list, after D4's redaction review. Tables copied from SGBD/ODX/ISTA files are **prohibited** (DTC texts, scaling tables, ECU variant lists). This includes content read from the user's `EcuPath` (`INI:11`) by a local tool.
  - *Spike:* **Matches.** At `2abb6e0` the repo holds `.gitattributes`, `.gitignore`, `Cargo.toml`/`Cargo.lock`, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md`, five crate manifests and sources (`cli`, `diagwire`, `ds2`, `hsfz`, `uds`), this record and the r1 author note. No BMW data files. The spike's ECU name table (`diagwire:103-111`) is four entries with no stated source (see register).
  - *Reopen if:* BMW publishes the data under a compatible license.

- **D9 Output channels:** logs go to stderr. Stdout belongs to the MCP protocol and CLI output.
  - *Reason:* MCP stdio carries protocol messages on stdout, so one stray line corrupts the stream.
  - *Spike:* **Matches.** The CLI logs to stderr (`cli:40-43`) and prints results with `println!` (`cli:49,52,68,73,105,107`). Libraries use only `tracing` (`hsfz:144,178,244,265`). No test asserts this.
  - *Reopen if:* MCP moves off stdio.

- **D10 Transactions (new r2):** one outstanding transaction per connection, with an absolute deadline per operation. Each response is validated. The connection is disposed of after any ambiguity. Functional (0xDF) scanning is disabled in v0.
  - *Rule:* The deadline is fixed when the operation starts. It covers the write, ack wait, alive-check replies and 0x78 extensions, and no extension may pass it. A response counts only if its source is the addressed ECU and its target is the tester, and then: a **positive** response has the expected positive SID and, for services that take one, echoes the request's subfunction; a **negative or pending** response is `0x7F` followed by the requested service (the NRC follows; there is no subfunction field). The byte layouts are subject to D3's ISO clause check. The connection is closed (no resync in v0) after: a timeout once the write has begun, cancellation, an interrupted write, or malformed framing.
  - *Outcomes:* `Complete(report)`, `Negative(nrc)`, `PendingExpired` (the ECU sent 0x78 but no final reply arrived before the deadline), `NoResponse`, `Malformed`. A malformed response is an error. Salvaged records are never presented as a complete fault list.
  - *Reason:* The spike's stale-answer, unbounded-wait and poisoned-buffer paths (replays 1–3) all come from shared connection state that outlives a failed transaction.
  - *Alternatives:* Several transactions in flight, matched by address (needs per-ECU completion rules we don't have). Resynchronising on bad framing (needs an authority for frame boundaries).
  - *Consequence:* "All ECUs" becomes sequential single-ECU reads over an address list that a capture has confirmed. Each ambiguous failure costs a reconnect.
  - *Spike:* **Does not match.** `recv` restarts its deadline on every call (`hsfz:168`). `write_all` is unbounded and the ack deadline starts after it (`hsfz:146-150`), and the same goes for alive replies (`hsfz:195`). Responses are filtered by source only (`diagwire:52`). Queued and buffered bytes survive a timeout (`hsfz:124,209-218`). `scan_dtcs` exists and is the CLI default (`cli:65-66`).
  - *Reopen if:* a per-ECU completion policy for functional requests is written and verified against a capture.

## Explicitly out (v0)

Coding, flashing, BEST/2 interpreter, ICOM, MOST, DoIP (TCP 13400), Bluetooth/ELM327 adapters, GUI.

## Replays the judge will run

1. The gateway's ack never arrives after a send.
   - **Partial.** `Timeout("gateway ack")` comes after 2 s (`hsfz:30,148-150`), but the deadline starts after an unbounded `write_all` (`hsfz:146`). The ack's payload isn't checked (`hsfz:152`), and the connection stays in use after the timeout. D10 requires a bounded write and disposal.
2. A functional request (0xDF) gets interleaved answers from two ECUs, one of them answering 0x78 first.
   - **Mishandled.** A 0x78 is skipped and the window never extends (`diagwire:87`, `cli:66`), so a late ECU silently disappears. A second reply overwrites the first (`diagwire:95`). Late replies can satisfy a later single-ECU `request` (`diagwire:52-56`), and chatter restarts its wait (`hsfz:168`). Under D10, functional scanning is disabled in v0.
3. A frame is split across TCP reads and a timeout fires mid-read.
   - **Bytes preserved, recovery mishandled.** Bytes are buffered only after `read` completes (`hsfz:205-219`). A bad length leaves `Malformed` stuck in the buffer forever (`hsfz:209`). D10 requires closing the connection. First-slice scenario 7 tests this.
4. An alive check arrives while a send is waiting for its ack.
   - **Partial.** It is answered and the wait continues (`hsfz:156`). The reply's `write_all` is unbounded (`hsfz:195`), so it can stall past the deadline, and its format (`hsfz:188-197`) is unverified.
5. The K-line echo of our request comes back with one corrupted byte.
   - **Unknown: no K-line transport, no echo handling** (the plan exists only as a doc comment, `ds2:5-7`). `ds2::decode` rejects telegrams with a bad checksum (`ds2:49-69`). It knows nothing about echoes. EDIABAS's behavior is the authority.
6. An MCP client calls `clear_faults` without confirmation, and with a forged confirmation.
   - **Not applicable yet.** There is no MCP and no clear operation, and v0 has none (Q4: read-only). The replay belongs to DR-0003. The raw transport must be unreachable regardless (D6).
7. An unstamped fixture, or one captured from diagwire itself, is loaded.
   - **Unknown: no loader or gate** (D4).
8. A DTC response has a record count that isn't a multiple of 4.
   - **Handled for one response, tested** (`uds:153-155`, test at `uds:237`). Limitation: in a scan, a later reply from the same ECU overwrites the error (`diagwire:95`). D10 makes this an error and never shows salvaged records as complete. The record layout itself is unverified.

## Authority register

Status values are **verified**, **unverified** (an authority is named but nothing has been compared) and **no authority yet**. Nothing is verified. Where the local `EDIABAS.INI` agrees with a constant, that is a hint from BMW's configuration, not a capture. The origin of the constants is described under D3. "Assumption" marks semantics the decisions rely on that are not yet checked.

| Contract (spike location) | Authority | Evidence to compare | Status |
|---|---|---|---|
| HSFZ header: u32 BE length + u16 BE control (`hsfz:4,70-95`) | Canonical-client capture; EdiabasLib reader | Raw TCP bytes | unverified |
| Control words 0x01/0x02/0x11/0x12; the spike treats any value ≥0x40 as an error (`hsfz:32-40,157,182`) | Capture; EdiabasLib | Control words seen | unverified |
| Ack payload semantics (`hsfz:152`) | Capture | Ack vs request | no authority yet |
| Alive-check request/reply (`hsfz:188-197`) | Capture of an idle client session | Control word, payload, timing | unverified |
| Tester address 0xF4 (`hsfz:24`) | Capture | Request source byte | unverified; `INI:67` lists `F4,F5` |
| TCP 6801 (`hsfz:21`) | Capture | Destination port | unverified; `INI:69,633-637` agrees |
| Discovery: UDP 6811, empty probe, `BMWVIN` ident (`hsfz:22,232-280`) | Capture | Transport, payload, reply | unverified; conflicts with `INI:626-630` |
| Functional address 0xDF and its fan-out scope (`diagwire:11`) | Capture | Target byte, which ECUs answer | unverified (assumption in D6) |
| ECU names 0x10/0x12/0x18/0x29 (`diagwire:103-111`) | Capture + the client's ECU list | Address → module | unverified |
| UDS requests 0x19 02 mask, 0x14 FFFFFF, 0x22 DID (`uds:29-41`) | ISO 14229-1 clause check; capture | Bytes, clause | unverified |
| Reads (0x19, 0x22) don't change ECU state | ISO 14229-1 clause check | Service definition | unverified (assumption in D6) |
| Clear (0x14) erases DTCs and freeze frames (`uds:34-36`) | ISO 14229-1; the client's behavior | Service definition | unverified (assumption in D6) |
| Reply classification, NRC 0x78, NRC names (`uds:53-72,169-190`) | ISO 14229-1 | Values | unverified |
| DTC record 3+1 bytes, status-bit names (`uds:78-87,146-167`) | ISO 14229-1; the client's decoded list | Codes, statuses | unverified |
| DTC shown as 6 hex digits (`uds:131-135`) | The client's display | String form | unverified |
| P2 2 s / P2* 5 s (`diagwire:13-15`); ack 2 s / connect 5 s (`hsfz:29-30`) | ISO 14229 (part to confirm); capture timing | Durations | unverified; no authority yet (ack/connect values were chosen) |
| DS2 framing `[addr][len][data][xor]` (`ds2:1-3,31-70`) | EDIABAS trace on the M3 | Telegram bytes | unverified (trace content also unverified) |
| DS2 status 0xA0 (`ds2:10`); 9600 8E1 and echo (`ds2:5-7`, comment only) | EDIABAS trace; serial tap | Bytes, line settings | unverified |
| Read allowlist and consent (D6) | Maintainer-approved scenarios | Allowed/denied calls | no authority yet (no code) |
| MCP protocol | MCP specification; rmcp | Message conformance | no authority yet (no code) |

## Test evidence

At `2abb6e0`, `cargo test --workspace` (Rust 1.100.0, Windows) gave **16 passed, 0 failed**: `ds2` 5 (`ds2:76-111`), `hsfz` 6 (`hsfz:297-338`), `uds` 5 (`uds:196-246`). `diagwire`, `cli` and the doctests have 0. Cargo warns about the unused `diagwire-ds2`. Process: the judge's sandbox can't run `cargo test` (its r1 attempt hit access denied on `target/debug/.cargo-build-lock`), so it reviews statically. The dispatcher runs the tests.

Covered: HSFZ frame codec, partial and trailing input, the length guard, VIN extraction; UDS request bytes, reply classification, DTC parsing including the bad record count, status flags; DS2 codec, checksum and length limits.

Gaps: **no negative controls** recorded. **No fakes**: `Connection`, `discover`, `Session`, `scan_dtcs` and the CLI are untested, and so is every replay except 8. **No fixtures with provenance**: the vectors characterize belief (D4). No async tests. `scan_dtcs` runs on the wall clock. No second platform, no CI.

## First slice (after acceptance)

Mode: feature. Outcome: single-ECU `read_dtcs`, matched against a canonical-client capture of the X5, with exchanges asserted and a negative control. No job loader (D5).

1. **Maintainer-run, 2026-11-16:** capture the ENET NIC while a canonical client (ISTA/EDIABAS with `Interface = ENET`, or E-Sys) discovers the car, reads the DME's fault memory, and idles long enough to show alive checks. Save the client's decoded DTC list. No agent touches the link (D7).
2. **Before commit (D4/D8):** stamp it (car, client and version, capture tool, date, host, diagwire revision or n/a). The maintainer reviews redaction (VIN, MACs, IPs), the transformations are listed in the stamp, and the original stays private, referenced by hash.
3. A general loader (rejects unstamped fixtures) and a separate conformance gate (admits only canonical-client fixtures), each with tests and negative controls (replay 7).
4. An injectable stream for `hsfz` (D7) and the D10 transaction rules for single-ECU reads.
5. A fake gateway over `tokio::io::duplex` that replays the fixture **and asserts** that diagwire's outgoing bytes and the exchange order (request → ack → response) match the capture.
6. A conformance test: `read_dtcs` returns exactly the client's list. **Negative control:** flip one record byte in a scratch copy, run in a fresh `CARGO_TARGET_DIR`, see the expected failure, restore.
7. A fake-stream acceptance scenario on tokio's paused clock: a frame split across reads, with the deadline firing mid-frame, gives `NoResponse` and a disposed connection. No later call sees stale bytes.
8. Update only the register rows exercised (header, DIAG/ACK framing as seen, tester address, the 0x19 02 request as the client sent it, and alive check if one was observed). UDS semantic rows stay unverified until the ISO clause check.

Out: scanning, MCP, DS2, clear. Stop conditions: the capture contradicts a spike constant (amend the register before writing code). No canonical client reaches the X5 (park the slice and record why).

## Open questions for the maintainer (answerable in one line; cost given)

1. **Judge:** which other vendor's model, and how is it invoked? **Answered (maintainer, 2026-11-14): Codex CLI**, run as `codex exec --sandbox read-only`.
2. **License:** MIT/Apache-2.0 dual (Rust norm), or other? **Answered (maintainer, 2026-11-14): MIT OR Apache-2.0**, applied in `LICENSE-MIT`, `LICENSE-APACHE` and `workspace.package.license`.
3. **Public repo:** push to GitHub now, or after DR-0001 is accepted? **Answered (maintainer, 2026-11-14): now.** Public at github.com/derekmwright/diagwire from `2abb6e0`.
4. **`clear_faults` (rewritten r2):** (a) exclude clear from v0 and defer it to its own record together with battery registration (DR-0003; DR-0002 is the garage), or (b) allow it with single-use human approval bound to vehicle, ECU, operation and parameters, consumed once, on every hardware entry point? ((a) costs nothing now. (b) needs an approval channel the model can't forge. Making clear CLI-only does not remove replay 6 unless an agent cannot invoke that CLI.) **Answered (maintainer, 2026-11-14): (a), v0 is read-only.** Clear-faults moves to DR-0003 with battery registration.
5. **Name:** keep `diagwire`? **Answered (maintainer, 2026-11-14): yes.**

**Answered 2026-11-14, hardware I/O by role:** development agents never initiate hardware I/O, including discovery, connect and CLI runs, and tests use injectable fake streams only. The product's AI user may run reviewed, allowlisted reads only through an MCP server the maintainer starts with the cable attached, and starting it is the consent for reads. Discovery counts as live I/O. Recorded in D6 and D7.
