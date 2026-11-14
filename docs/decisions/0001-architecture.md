# DR-0001: Architecture, authorities and safety boundary

**Status:** in judge review, round 1 (architect, 2026-11-14). Not accepted. No production code until it is.
**Spike:** everything under `crates/` at this date is a spike written before this record. It is disposable and needs an explicit productionization decision.
**Author fill-in:** 2026-11-14, checked against the uncommitted spike tree. There are no commits yet, so there is no revision hash. Citations: `hsfz:N`, `uds:N`, `ds2:N` and `diagwire:N` mean `crates/<crate>/src/lib.rs` line N. `cli:N` means `crates/cli/src/main.rs`. `INI:N` means the maintainer's local `C:\EDIABAS\Bin\EDIABAS.INI` (EDIABAS 7.3.0), which I only read.

## Governing rule

diagwire may only claim a protocol behavior that is checked against an authority outside diagwire, and it never changes vehicle state without the maintainer's explicit per-action consent.

## Decisions (one line each; the author fills in reasons and verifies each against code)

- **D1 Layering:** transports (`hsfz`, later `kline`) / pure protocol codecs with no I/O (`uds`, `ds2`, later `kwp`) / operations (`diagwire`) / front ends (`cli`, `mcp`).
  - *Reason:* A codec with no I/O can be tested byte for byte against fixtures without hardware or a runtime. UDS is written once and serves every transport that carries it. Both front ends share one operations layer, so the CLI and the MCP server can't drift apart.
  - *Alternatives:* One crate per car generation (UDS would be duplicated). A single crate with modules (no compile-time check that codecs stay I/O-free).
  - *Consequence:* Operations have to reach transports through a seam, not through a concrete type.
  - *Spike:* **Partial match.** `uds` and `ds2` depend only on `thiserror` (their `Cargo.toml`) and do no I/O. Mismatches: (a) `Session` holds a concrete `hsfz::Connection`, and its error wraps `hsfz::Error` (`diagwire:20,31-33`). There is no transport seam, so K-line can't slot in. (b) The CLI calls `hsfz::discover` directly and bypasses the operations layer (`cli:47,84`). (c) HSFZ framing (`hsfz:56-96`, pure code) sits in the transport crate next to the socket code, while DS2 framing has a codec crate of its own. This is inconsistent but harmless. (d) No crate uses `ds2`, and cargo warns "unused workspace dependency `diagwire-ds2`". `kline`, `kwp` and `mcp` don't exist yet, as planned.
  - *Reopen if:* a transport needs protocol state that the codec boundary can't express, for example DS2 inter-byte timing that is tied to framing.

- **D2 Runtime:** tokio async in transports, operations and front ends, because the MCP server (rmcp) is tokio-based. Codec crates never depend on tokio.
  - *Reason:* With one runtime, nothing has to bridge between threads and async code. HSFZ needs reads that can be cancelled when a timeout fires, and async timeouts provide that. "rmcp is tokio-based" is the skeleton's claim. rmcp isn't a dependency yet, so the claim is unverified in this repo.
  - *Alternatives:* Blocking I/O on threads, with an async bridge only in `mcp`. A sans-IO core with per-runtime adapters. smol.
  - *Consequence:* K-line serial needs either an async serial crate or `spawn_blocking`. Tests can use tokio's paused clock only if every deadline goes through `tokio::time`.
  - *Spike:* **Partial match.** tokio is used in `hsfz` (`hsfz:17-19`) and the CLI (`cli:38`). Mismatch: `scan_dtcs` measures its window with `std::time::Instant` (`diagwire:5,75`), and `diagwire` doesn't depend on tokio. A paused-clock test can't drive that deadline, so testing replay 2 would need a wall-clock test, which the testing rules forbid.
  - *Reopen if:* rmcp or the chosen serial crate forces a different runtime, or blocking K-line I/O proves simpler and keeps timing correct.
  > Author dissent: "throughout" should read "in transports, operations and front ends". D1 keeps the codecs runtime-free, and the spike's codecs are correctly synchronous (`uds`/`ds2` `Cargo.toml`). Please confirm that codec crates never take a tokio dependency.
  > Architect ruling (2026-11-14): accepted; D2 reworded.

- **D3 Authorities:** UDS → ISO 14229-1. HSFZ → traffic captured from a canonical client (E-Sys or EDIABAS) against the real X5, with EdiabasLib as an independent reader. DS2 → EDIABAS `api32.dll` on the M3, used as a differential oracle via its trace.
  - *Reason:* Each protocol gets the strongest outside oracle it has: the standard for UDS semantics, the OEM's own client for HSFZ framing (the author knows of no public HSFZ specification), and EDIABAS for DS2, because the M3 setup already works.
  - *Alternatives:* Open-source implementations alone. They may share our misreadings, so they serve only as an independent reader, which is how D3 already uses EdiabasLib. Simulators (see D4).
  - *Consequence:* No HSFZ behavior can be claimed until the X5 capture exists. ISO 14229-1 is paywalled, so UDS claims stay unverified until someone checks them against a copy the maintainer holds, citing clause numbers. The DS2 oracle depends on what the EDIABAS trace contains. `INI:16-17` sets `ApiTrace = 0` and `IfhTrace = 0`, and `INI:188-195` documents `IfhTrace` level 2 as "User trace + interface" and level 3 as adding a time stamp. Whether either level shows raw telegram bytes, including the K-line echo, is **unverified**. If it doesn't, a passive serial tap is needed.
  - *Spike:* **No authority exercised.** Every protocol constant comes from memory (see the register). There is one discrepancy for a capture to settle: the spike sends discovery as UDP to port 6811 (`hsfz:22,233-246`), but `INI:626-630` describes 6811 as the "port of control channel of TCP connection to ZGW". This record doesn't resolve it.
  - *Reopen if:* a public HSFZ specification appears, or the capture tooling can't see the ENET link.

- **D4 Fixtures:** captures from real cars only, each stamped with provenance (car, tool, date, revision). The loader rejects unstamped fixtures. Nothing diagwire produced about itself counts as correctness evidence. A conformance fixture requires a canonical client (ISTA, E-Sys, EDIABAS) as the tester. Captures with diagwire as the tester are labelled characterization and can never back a conformance claim.
  - *Reason:* A golden file the code generated for itself only proves self-consistency (playbook §1). Provenance lets a reviewer tell evidence from author belief.
  - *Alternatives:* Hand-written vectors from public notes. That is what the spike does, and it only characterizes what the author believes. EDIABAS simulation files (`INI:9,12`): someone writes them, so they have the same problem.
  - *Consequence:* No conformance test can exist before the first capture. The loader is a gate, so its own tests need negative controls (an unstamped fixture is rejected, and so is one with diagwire as the tester).
  - *Spike:* **Does not match.** There are no fixtures, no stamp format and no loader. All 16 tests use hand-written bytes, for example invented DTCs `130A04` and `480200` (`uds:222`) and an invented ident string and VIN (`hsfz:334`).
  - *Reopen if:* some path can't practically be captured (for example, gateway error replies). That path would need a labelled class of reviewed examples.
  > Author dissent: "captured from diagwire itself" is ambiguous when diagwire is the tester and the car does the answering. The car's replies are real, but the request framing and timing are diagwire's own. Proposal: a conformance fixture requires a canonical-client tester. Captures with diagwire as the tester may be kept, labelled as characterization only. The first slice below assumes this ruling.
  > Architect ruling (2026-11-14): accepted; D4 extended.

- **D5 Job definitions:** declarative files (request bytes, reply layout, scaling, units, English text). No BEST/2 interpreter in v0.
  - *Reason:* BMW's job logic lives in SGBD bytecode, which D8 keeps out of the repo. Declarative files keep each job reviewable as data and testable against a fixture.
  - *Alternatives:* A BEST/2 interpreter over the user's SGBDs (large, out of scope for v0, and its output is only whatever the SGBD says). Hand-coding each job in Rust (the spike's approach). ODX (proprietary data, ruled out by D8).
  - *Consequence:* Each job's scaling and text needs an authority: the EDIABAS result for DS2, and the canonical client's display for UDS. English text that we write is itself a translation claim.
  - *Spike:* **Not started; no conflict.** There are no job files. Requests are hard-coded builders (`uds:29-41`), and there is no DTC text or scaling at all. There's no interpreter either, which matches. Standard ISO services such as 0x19/0x02 may never need a job file. BMW-specific DIDs and DS2 jobs will.
  - *Reopen if:* a needed job requires control flow the format can't express, such as multiple steps, seed/key, or conditional parsing.

- **D6 Operation tiers:** read-only operations are free; state-changing operations (`clear_faults`) need explicit confirmation on every call; coding and flashing are not exposed. The tier check lives in the operations layer, so every front end (CLI and MCP) passes through it. No public API sends arbitrary request bytes.
  - *Reason:* An AI client may call tools repeatedly or misread intent. Reads don't change the car. A clear erases the fault and freeze-frame evidence the X5 drivetrain diagnosis depends on. Coding and flashing can disable a module.
  - *Alternatives:* All tools free (breaks the governing rule). Clearing from the CLI only (open question 4). A session-level unlock (one consent would cover many actions, which isn't per-action).
  - *Consequence:* Confirmation has to come from the human, outside the model's control: for example an MCP elicitation, or a token the server issues that only the maintainer sees. A boolean argument the model sets is not consent (replay 6, forged confirmation). A per-action consent names its ECU. A functional clear to 0xDF is one action that reaches every ECU, so it needs its own wording.
  - *Spike:* **No MCP yet.** `uds::clear_all_dtcs` exists (`uds:34-36`), but nothing calls it, so the spike has no path that changes vehicle state today. Gap: `Session::request` is public and sends any bytes (`diagwire:44-61`). A future front end could send 0x14, 0x2E, 0x31 or 0x11 without passing any tier check. It also panics on an empty request (`diagwire:45`).
  - *Reopen if:* MCP gains a standard human-confirmation primitive, or the maintainer picks CLI-only.
  > Author dissent: D6 names only MCP, but the governing rule covers every front end (security addendum A3: every door is a policy boundary). Proposal: put the tier check in the operations layer, so the CLI and MCP both pass through it, and stop exposing the raw `request` door publicly.
  > Architect ruling (2026-11-14): accepted; D6 renamed and extended.

- **D7 Hardware boundary:** agents never open the serial port or the ENET link. Every transport has a fake for tests, and hardware runs are maintainer-run.
  - *Reason:* An agent's mistake on a live bus can change vehicle state or drain the battery. Fakes make splits, timeouts and alive checks deterministic and testable.
  - *Alternatives:* EDIABAS simulation (someone has to write the files, as in D4). Read-only hardware access for agents (nothing mechanically guarantees read-only).
  - *Consequence:* Every transport needs a seam a fake can drive. Hardware evidence moves at the maintainer's pace (the X5 from Monday 2026-11-16).
  - *Spike:* **Does not match.** No transport has a fake. `Connection::connect` opens a real `TcpStream` on the fixed port 6801 (`hsfz:128-140`), so a loopback fake would have to own port 6801 and tests couldn't run in parallel. `Session` wraps the concrete type (`diagwire:31-40`). `discover` sends real broadcasts to 255.255.255.255 and 169.254.255.255 (`hsfz:242-246`), and `diagwire dtc` without `--ip` runs discovery implicitly (`cli:56-59`). The rule exists only as prose: nothing mechanical stops an agent from running the CLI.
  - *Reopen if:* never for the agent boundary. The fake's design gets revisited if fakes diverge from captures.

- **D8 BMW data:** no SGBD, ODX or other BMW-proprietary data in the repo. A local tool may read the user's own copies.
  - *Reason:* BMW's data is proprietary and licensed. Shipping it would put the repository at risk.
  - *Alternatives:* Ship extracted tables (legal risk). Depend on the user's SGBDs for everything (useless without ISTA).
  - *Consequence:* Everything diagwire knows has to come from captures or public knowledge. A local reader of the user's files (EDIABAS `EcuPath` = `C:\Program Files (x86)\Rheingold\Ecu`, `INI:11`) must not copy their content into committed fixtures or logs. Captures contain the VIN (as the ident does, `hsfz:275-280`), so fixtures need a redaction policy before any public push.
  - *Spike:* **Matches.** The repo holds only Rust sources, manifests, the README and this record. The ECU name table (`diagwire:103-111`) and the protocol constants are the author's recollection, not copied data.
  - *Reopen if:* BMW publishes the data under a compatible license.

- **D9 Output channels:** logs go to stderr. Stdout belongs to the MCP protocol and CLI output.
  - *Reason:* The MCP stdio transport carries its protocol messages on stdout, so one stray log line corrupts the stream.
  - *Alternatives:* Logging to a file only. MCP over HTTP (frees stdout, but most clients are set up for stdio).
  - *Consequence:* Libraries never print. Only front ends write to stdout.
  - *Spike:* **Matches.** The CLI logs to stderr (`cli:40-43`) and prints results with `println!` (`cli:49,52,68,73,105,107`). Libraries log only through `tracing` (`hsfz:144,178,244,265`), and no library crate contains a `println!`. No test asserts any of this.
  - *Reopen if:* MCP moves off stdio.

## Explicitly out (v0)

Coding, flashing, BEST/2 interpreter, ICOM, MOST, DoIP (TCP 13400), Bluetooth/ELM327 adapters, GUI.

## Replays the judge will run

1. The gateway's ack never arrives after a send.
   - **Handled, untested.** `send` waits until a 2 s deadline (`hsfz:30,148-150`) and returns `Timeout("gateway ack")`. Gaps: the ack's payload is never compared with the request (`hsfz:152`), and whether it should be is unknown until a capture shows it. The connection's state after the timeout is undefined: a late reply stays queued and may be taken as the answer to the next request (see 2).
2. A functional request (0xDF) gets interleaved answers from two ECUs, one of them answering 0x78 first.
   - **Mishandled.** `scan_dtcs` skips 0x78 replies (`diagwire:87`) and never extends its fixed window (`cli:66`, 5 s). If the ECU that sent 0x78 answers after the window, it is silently missing, so "pending" looks the same as "absent". A second reply from the same ECU overwrites the first (`diagwire:95`). Answers that arrive late stay in the connection, and a later `request` to that ECU filters only by source and service (`diagwire:52-56`), so it would accept a stale 0x59 from the scan as its own answer. In `request`, each `recv` starts a fresh deadline (`hsfz:168`), so chatter from other ECUs can extend the wait without bound. Replies that arrive while `send` waits for its ack are kept (`hsfz:153-155`), which is correct.
3. A frame is split across TCP reads and a timeout fires mid-read.
   - **Handled by construction, untested.** Bytes are buffered only after `read` completes (`hsfz:205-219`). This relies on tokio's `read` being cancel-safe. Only the codec's partial-frame case is tested (`hsfz:311-315`). Gap: an oversized length field returns `Malformed` and leaves the bad bytes in `rx` (`hsfz:209`), so every later read fails the same way. The connection never resynchronises.
4. An alive check arrives while a send is waiting for its ack.
   - **Structurally handled, correctness unknown.** `send` answers it and keeps waiting under the same deadline (`hsfz:156`). The reply's control word and payload (`[0x00, tester]`, `hsfz:188-197`) are unverified, and the code says so itself.
5. The K-line echo of our request comes back with one corrupted byte.
   - **Unknown: no K-line transport.** Echo stripping is only mentioned in a doc comment (`ds2:5-7`). `ds2::decode` rejects a bad checksum (`ds2:60-64`) but has no notion of an echo. The expected behavior (abort, retry, or report) needs EDIABAS's behavior as its authority.
6. An MCP client calls `clear_faults` without confirmation, and with a forged confirmation.
   - **Not applicable yet.** There is no MCP crate and no clear operation. The latent risk is the public raw `Session::request` (see D6).
7. An unstamped fixture, or one captured from diagwire itself, is loaded.
   - **Unknown: no loader or fixtures** (D4 mismatch).
8. A DTC response has a record count that isn't a multiple of 4.
   - **Handled and tested.** `parse_dtc_report` returns `Truncated` (`uds:153-155`, test at `uds:237`). In a scan, the error stays attached to that one ECU (`diagwire:86`) and is printed (`cli:73`). Still open: whether to reject the whole response or salvage the complete records is a contract question for ISO 14229-1, and the 3+1-byte record layout itself is unverified. No negative control is recorded.

## Authority register

Status values are **verified**, **unverified** (an authority is named but nothing has been compared) and **no authority yet**. **Nothing is verified today.** Where the local `EDIABAS.INI` agrees with a constant, that counts as a corroborating hint from BMW's configuration, not as a capture.

| Contract (spike location) | Authority | Evidence to compare | Status |
|---|---|---|---|
| HSFZ header: u32 BE length + u16 BE control word (`hsfz:4,70-95`) | Canonical-client capture vs X5; EdiabasLib reader | Raw TCP bytes | unverified |
| HSFZ control words 0x0001/0x0002/0x0011/0x0012, errors ≥0x0040 (`hsfz:32-40`). The spike treats *any* control word ≥0x40 as an error (`hsfz:157,182`), not just 0x40-0xFF | Capture; EdiabasLib | Control words seen, including any gateway error | unverified (from memory) |
| Ack semantics: payload echoes request? (`hsfz:152`) | Capture | Ack payload vs request | no authority yet (not modelled) |
| Alive-check request/reply format (`hsfz:188-197`) | Capture of an idle canonical session | Control word, payload, timing | unverified (from memory) |
| Tester address 0xF4 (`hsfz:24`) | Capture | Source byte of client requests | unverified (from memory); `INI:67` lists `TesterAddress = F4,F5` |
| Diagnostic port TCP 6801 (`hsfz:21`) | Capture | TCP destination port | unverified; `INI:69,633-637` agrees |
| Discovery: UDP 6811 broadcast, empty payload, ident reply with `BMWVIN` marker (`hsfz:22,232-280`) | Capture | Transport, payload, reply layout | unverified; **conflicts** with `INI:626-630` ("control channel of TCP connection") |
| Functional address 0xDF (`diagwire:11`) | Capture of a client's functional read | Target byte, which ECUs answer, source bytes | unverified (from memory) |
| ECU address names 0x10/0x12/0x18/0x29 (`diagwire:103-111`) | Capture plus the client's ECU list for this X5 | Address → module | unverified (from memory) |
| UDS request bytes 0x19 02 mask, 0x14 FFFFFF, 0x22 DID (`uds:29-41`) | ISO 14229-1 (maintainer's copy); capture | Bytes, clause cited | unverified |
| UDS reply classification, NRC 0x78, NRC names (`uds:53-72,169-190`) | ISO 14229-1 | Values, edge cases | unverified |
| DTC record 3-byte code + status, status-bit names (`uds:78-87,146-167`) | ISO 14229-1; client's decoded DTC list | Codes and statuses match client output | unverified |
| DTC display as 6 hex digits (`uds:131-135`) | Canonical client's display | String form | unverified |
| Timing: P2 2 s, P2* 5 s (`diagwire:13-15`); ack 2 s, connect 5 s (`hsfz:29-30`) | ISO 14229 (part to confirm); capture timing | Durations | unverified (P2/P2*); no authority yet (ack/connect: chosen values) |
| DS2 framing `[addr][len][data][xor]` (`ds2:1-3,31-70`) | EDIABAS trace on M3 | Telegram bytes | unverified (trace content itself unverified, see D3) |
| DS2 status 0xA0 (`ds2:10`); 9600 8E1 and echo (`ds2:5-7`, comment only) | EDIABAS trace; serial tap | Bytes, line settings | unverified |
| Consent and tool tiers (D6) | Maintainer-approved acceptance scenarios (replay 6) | Allowed/denied calls | no authority yet (no code) |
| MCP protocol | MCP specification; rmcp | Message conformance | no authority yet (no code) |

## Test evidence

`cargo test --workspace` (Rust 1.100.0, Windows only): **16 passed, 0 failed**. There are 5 in `ds2` (`ds2:76-111`), 6 in `hsfz` (`hsfz:297-338`) and 5 in `uds` (`uds:196-246`), plus 0 in `diagwire`, 0 in `cli` and 0 doctests. Cargo also prints a warning: unused workspace dependency `diagwire-ds2`.

Covered: HSFZ frame encode/decode, partial and trailing input, the oversized-length guard, VIN extraction; UDS request bytes, reply classification, DTC parsing including the bad record count, status flags; DS2 encode/decode, checksum, the length limits.

Gaps against the testing rules:
- **No negative controls** recorded for any test. I didn't run any, because this task may not edit `crates/`.
- **No fakes**: no test exercises `Connection`, `discover`, `Session`, `scan_dtcs` or the CLI. Every I/O path and every judge replay except 8 is untested.
- **No fixtures with provenance**: every vector is hand-written, so these tests characterize the author's beliefs. They are not conformance evidence (D4).
- No async tests, and `scan_dtcs` uses the wall clock (D2 mismatch). No second-platform run. No CI.

## First slice (after acceptance)

Mode: feature. Outcome: `read_dtcs` on one ECU, proven against a canonical-client capture of the X5 and a test that fails when the bytes are wrong.

1. **Maintainer-run, Monday 2026-11-16:** capture packets on the ENET NIC while a canonical client (ISTA/EDIABAS with `Interface = ENET`, or E-Sys) does three things: discovers the car, reads the DME's fault memory, and idles long enough to show alive checks. Save the client's own decoded DTC list as text. No agent touches the link.
2. Stamp the fixture (car, tool and version, date, capture host, redaction applied) and commit it with the client's decoded output as the expected result.
3. A fixture loader that rejects unstamped fixtures and diagwire-tester fixtures, with tests for both (replay 7).
4. A transport seam: `Connection` generic over an async stream, so a fake gateway can replay the fixture over `tokio::io::duplex`. This needs the D1/D7 productionization decision for `hsfz`.
5. A conformance test: `read_dtcs` against the fake returns exactly the client's DTC list. **Negative control:** flip one record byte in a scratch copy of the fixture, run in a fresh `CARGO_TARGET_DIR`, watch the test fail on the DTC mismatch, then restore.
6. Update the register rows that the capture settles: header, DIAG/ACK, tester address, alive check, port and discovery.

Out of this slice: the `scan_dtcs` fix (replay 2), MCP, DS2, clear. Stop conditions: the capture contradicts a spike constant (amend the register before writing code); no canonical client can reach the X5 over ENET (park the slice and record why).

## Open questions for the maintainer (answerable in one line; cost given)

1. **Judge:** which other vendor's model, and how is it invoked? (Blocks the judge loop.)
2. **License:** MIT/Apache-2.0 dual (Rust norm), or other? (Free to change before the first public push, costly after.) **Answered (maintainer, 2026-11-14): MIT OR Apache-2.0**, applied in `LICENSE-MIT`, `LICENSE-APACHE` and `workspace.package.license`.
3. **Public repo:** push to GitHub now, or after DR-0001 is accepted? (Tracking issues per record need a remote.)
4. **`clear_faults` over MCP:** allowed with per-call confirmation (D6), or CLI-only? (CLI-only removes replay 6 entirely.)
5. **Name:** keep `diagwire`? (Free now, costly after publishing crates.)
