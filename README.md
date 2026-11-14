# diagwire

Open-source, cross-platform BMW diagnostics in Rust, built to be driven by an AI over MCP.

## Layout

| Crate | What it does |
|---|---|
| `crates/hsfz` | HSFZ transport for F/G-series over an ENET cable: TCP 6801, UDP 6811 discovery |
| `crates/uds` | UDS (ISO 14229) request builders and response parsers, no I/O |
| `crates/ds2` | DS2 telegram framing for pre-CAN K-line modules (E36 etc.) |
| `crates/diagwire` | High-level operations (read faults, scan all ECUs) shared by the front ends |
| `crates/cli` | `diagwire` command-line tool |

## Usage

```
diagwire discover
diagwire dtc              # every ECU, via the functional address
diagwire dtc --ecu 12     # just the DME
```

Set `RUST_LOG=debug` to log every frame in hex.

## Status

**Everything in `crates/` is a spike** written before [DR-0001](docs/decisions/0001-architecture.md). It is disposable until that record is accepted.

- HSFZ and UDS code is written from public protocol notes and still needs checking against a real car.
- DS2 is framing only. The serial transport comes after capturing real traffic through EDIABAS.
- Next: an MCP server crate (read-only tools first, `clear_faults` gated behind confirmation, no coding or flashing).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
