# Read-only probe: worker result note

**Record:** DR-0001 amendment A1, which is a **draft** (not judged, not accepted). The maintainer asked for a read-only probe on 2026-11-15 and the code was built ahead of the amendment. **Code commit:** `60f0303`. Linux (cloud container), Rust 1.97.0. Not reviewed by the judge.

**No hardware I/O.** Fake streams and the paused clock only (D7). All bytes are hand-written: characterization, not conformance (D4). No register row changes.

## What was built

| File | Change |
|---|---|
| `crates/uds/src/lib.rs` | `parse_data_by_id(did, body)`, `DecodeError::UnexpectedIdentifier`, 1 test |
| `crates/diagwire/src/lib.rs` | `Session::read_data(ecu, did)`; `probe(connect, addresses, mask, on_probe)` with `Probe` and `ProbeReport`; `READ_SERVICES`; `is_single_ecu`; `Error::NotARead` and `Error::AddressNotAllowed`; both checks at the top of `transact` |
| `crates/diagwire/src/tests.rs` | 8 new scenarios |
| `crates/cli/src/main.rs` | `probe [--from --to --mask]` and `read --ecu --did` |
| `README.md` | usage lines for both |

- **Read list.** `transact` is the only path to the transport from an operation. It refuses any request whose service is not `0x19` or `0x22`, and any address that is `0xDF` or the tester's, before `begin` is called.
- **`read_data`.** Sends `22 hi lo`. A positive reply must be `62`, echo the identifier, and carry at least one byte. It returns those bytes and nothing else.
- **`probe`.** Checks the whole address list first, then asks each address for its fault memory in turn. After any result that closed the connection it calls `connect` again before the next address. If `connect` fails, it stops and reports what it has, with the error.

## Points the amendment leaves open (for the architect)

1. **The probe request is `19 02` with the caller's mask.** Amendment question 1.
2. **A reply with an identifier and no data is `Malformed`.** Whether ISO 14229-1 allows an empty data record is unverified.
3. **`0xDF` is the only functional address refused.** Whether the gateway treats other addresses as functional is unverified.
4. **A gateway error frame for one address is recorded against that address** and the probe goes on with a new connection. What a real gateway sends for an address nobody holds is unverified.
5. **`uds::clear_all_dtcs` still exists** in the codec crate with no caller. `transact` would refuse it. Removing it belongs to the operations-boundary slice.
6. **The CLI skips `0xDF` and `0xF4` silently** inside a requested range, and prints nothing for a silent address.

## Limits

- `hsfz::Connection::begin` is still public and still reachable from the CLI through `diagwire::hsfz`. The read list guards operations, not the transport. Closing that is the operations-boundary slice, as before.
- Probe results are not written to the garage.
- The CLI commands were only checked for argument parsing. Running them needs a car.
- Linux only.

## Negative controls

Each ran in a scratch copy with its own new target directory: `cargo test -p diagwire -p diagwire-hsfz -p diagwire-uds --locked`.

| Control | Guarded line broken | Failing test and message |
|---|---|---|
| a1 | read-list check removed | `only_read_services_are_ever_sent`: `assertion failed: matches!(result, Err(Error::NotARead(sid)) if sid == request[0])` |
| a2 | address check removed from `transact` | `single_reads_refuse_functional_and_tester_addresses`: `assertion failed: matches!(session.read_dtcs(bad, MASK).await, Err(Error::AddressNotAllowed(a)) …` |
| a3 | address check removed from `probe` | `probe_refuses_functional_and_tester_addresses_before_any_io`: `assertion failed: matches!(result, Err(Error::AddressNotAllowed(a)) if a == bad)` |
| a4 | identifier echo not checked | `read_data_rejects_replies_that_are_not_its_answer`: `another identifier echoed` `left: Complete([65])` `right: Malformed` |
| a5 | probe reuses a closed connection | `probe_asks_each_address_in_turn`, `probe_records_a_gateway_rejection_and_goes_on`, `probe_stops_when_it_cannot_reconnect`; the first two fail in the gateway script (`the tester closed the link early`) |
| a6 | `0xDF` allowed | `probe_refuses_functional_and_tester_addresses_before_any_io` and `single_reads_refuse_functional_and_tester_addresses` |

Not controlled: the "no data after the identifier" rule, the `on_probe` callback order, and the stop-on-failed-reconnect path on its own.

## Gate results at `60f0303`

`cargo test --workspace --locked`: exit 0, **109 passed, 0 failed** (`diagwire` 36, `diagwire-cli` 10, `diagwire-ds2` 5, `diagwire-garage` 46, `diagwire-hsfz` 6, `diagwire-uds` 6).

`cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0, no warning. The first clippy run failed on `type_complexity` in a test; the annotation was shortened before the commit.
