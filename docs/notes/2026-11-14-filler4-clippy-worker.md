# Issue #4 worker note: make clippy warnings fatal in CI

Date: 2026-11-14

## What changed

- `crates/uds/src/lib.rs`, `parse_dtc_report`: replaced `chunks_exact(4)` with
  `as_chunks::<4>()` (fixes `clippy::chunks_exact_to_as_chunks`). The
  `records.len() % 4 != 0` check became a check that the `as_chunks` remainder
  is non-empty; both return `DecodeError::Truncated` for the same inputs, so
  behavior is unchanged. Tests in the file were not edited.
- `Cargo.toml`: removed the unused `diagwire-ds2 = { path = "crates/ds2" }`
  entry from `[workspace.dependencies]`. `crates/ds2` is still a workspace
  member via `members = ["crates/*"]`; the K-line work re-adds the entry when
  something depends on it.
- `.github/workflows/ci.yml`: Clippy step is now
  `cargo clippy --workspace --all-targets --locked -- -D warnings`. Nothing
  else in the workflow changed; the header comment had no statement about
  clippy being non-fatal, so it needed no edit.
- `Cargo.lock` did not change.

## Results

Each command was run once, on Windows with Rust 1.100. Both exited 0 with zero
warnings in the output.

`cargo test --workspace --locked` (exit 0), final lines:

```
   Doc-tests diagwire_uds

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Test binaries with tests reported 5, 13, 6 and 5 passed, 0 failed.

`cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0),
final lines:

```
    Checking diagwire-hsfz v0.1.0 (C:\Users\User\Projects\diagwire\.claude\worktrees\agent-ac248d7097698a19c\crates\hsfz)
    Checking diagwire v0.1.0 (C:\Users\User\Projects\diagwire\.claude\worktrees\agent-ac248d7097698a19c\crates\diagwire)
    Checking diagwire-cli v0.1.0 (C:\Users\User\Projects\diagwire\.claude\worktrees\agent-ac248d7097698a19c\crates\cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.68s
```
