# Garage default path: worker result note

**Record:** DR-0002 G1, G9 and Q4; epic #2, "Default path: the per-OS default path resolver (Q4) in the front ends". **Date:** 2026-11-15. Linux (cloud container), Rust 1.97.0. Not reviewed by the judge. No car, garage file or real environment variable was read by any test.

## What was built

| File | Change |
|---|---|
| `crates/cli/src/garage_path.rs` | new: `resolve(flag, platform, env)`, `Platform`, `GaragePath`, `ChosenBy`, `Error`, 10 tests |
| `crates/cli/src/main.rs` | new subcommand `garage-path [--garage-dir DIR]`, which prints the resolved file path |
| `crates/cli/Cargo.toml`, `Cargo.lock` | `thiserror` added to the CLI (already in the workspace; no new crate) |

`resolve` only computes a path. It opens no file and creates no directory, and `garage-path` does neither. The environment is a closure argument: `main` passes `std::env::var_os`, tests pass a fixed list. `crates/garage` is untouched and still has no default-path function (G9).

Order: `--garage-dir`, then `DIAGWIRE_GARAGE_DIR`, then the default.

| Platform | Default directory |
|---|---|
| Windows | `%LOCALAPPDATA%\diagwire` |
| macOS | `$HOME/Library/Application Support/diagwire` |
| Linux and other Unix | `$XDG_DATA_HOME/diagwire`; `$HOME/.local/share/diagwire` when that variable is unset or empty |

## Points the record leaves open (for the architect)

Each of these is a choice I made, not something DR-0002 says.

1. **The override names a directory, not a file.** G1 says "the path" is overridable. G8 speaks of "a directory the user supplies" and puts the database, sidecars and backups inside one directory, so I read the override as the directory.
2. **The file is called `garage.sqlite3`.** The record names no file.
3. **The override is `--garage-dir` and `DIAGWIRE_GARAGE_DIR`**, the flag winning. The record names neither.
4. **A relative base directory is refused.** If `XDG_DATA_HOME`, `HOME` or `LOCALAPPDATA` holds a relative path, `resolve` returns an error. G1 says the fallback applies "only when that variable is unset or empty". The XDG specification says an implementation should treat a relative path as invalid and ignore it, which would mean falling back. I did neither silently: using it would put the garage wherever the command ran, and falling back contradicts G1's "only". A directory the user names with the flag or the override variable may be relative.
5. **No default without the variable.** On Windows with `LOCALAPPDATA` unset there is an error, and `APPDATA` (Roaming) is never used. Without `HOME` on macOS or Linux there is an error too. Nothing looks the home directory up another way.
6. **An empty override variable counts as unset.**
7. **A `garage-path` subcommand exists.** Without a caller the resolver is dead code, which the clippy gate rejects, and no garage command exists in the CLI yet. It can go when the first real garage command lands.

## Not done

- G8's file permissions (0700 directory, 0600 files, the warning for a user-supplied directory that group or others can read). That is the privacy-surface slice.
- Nothing opens a garage at the resolved path. No operation or front end uses the garage yet.
- The resolver is in the CLI. The MCP server, when it exists, needs the same function; where the shared copy lives is undecided.
- Run on Linux only. The Windows and macOS rules are tested on Linux through the `platform` argument, with paths compared as built by `PathBuf::join`. `Platform::current()` and Windows path handling on a Windows host were not run; CI covers the three hosts.

## Negative controls

Each ran in a scratch copy with its own new target directory: `cargo test -p diagwire-cli -p diagwire --locked`.

| Control | Guarded line broken | Failing test and message |
|---|---|---|
| p1 | Windows falls back to `APPDATA` | `windows_uses_local_app_data_and_never_roaming`: `left: Ok("C:\\Users\\u\\AppData\\Roaming/diagwire")` `right: Err(Unset("LOCALAPPDATA"))` |
| p2 | absolute-path check removed | `relative_base_directory_is_refused_not_used`: `left: Ok("data/diagwire")` `right: Err(Relative("XDG_DATA_HOME"))` |
| p3 | empty variable treated as set | `linux_falls_back_to_local_share_when_xdg_is_unset_or_empty`: `left: Err(Relative("XDG_DATA_HOME"))` `right: Ok("/home/u/.local/share/diagwire")`; also `flag_beats_the_variable_and_the_variable_beats_the_default` and `no_default_without_a_home` |
| p4 | the variable beats the flag | `flag_beats_the_variable_and_the_variable_beats_the_default`: `left: ("/from/env", Environment)` `right: ("/from/flag", Flag)` |

Not controlled: the macOS branch, the XDG-set branch and the file name, each of which has one test and no deliberately broken run.

## Gate results

See the end of `2026-11-15-d10-transactions-worker.md`: the three slices of this session were gated together on one tree.
