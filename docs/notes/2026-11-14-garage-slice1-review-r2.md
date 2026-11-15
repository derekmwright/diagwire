VERDICT: APPROVE

| Round-1 finding | Status | Code and regression-test verification |
|---|---|---|
| Busy-timeout panic / filesystem mutation | RESOLVED | Checked conversion is first in both `create` and `open`. Test covers overflow, maximum accepted value, and unchanged filesystem. NC R1a panics; R1b fails the empty-directory assertion; R1c fails pragma readback. |
| Lowercasing instead of case folding | RESOLVED | Stored values, duplicate checks, and references share `normalize` using `caseless`. Test covers Straße/STRASSE, sigma, and duplicate aliases. Lowercase NC fails resolution. |
| Resolution lacks provenance | RESOLVED | `VehicleRef` loads anchor provenance by vehicle ID; labels and matched identifiers retain their own row provenance. Distinct-timestamp test covers resolved and ambiguous results. Matched-row-as-anchor NC fails. |
| Dangling sidecars missed | RESOLVED | `symlink_metadata` counts dangling entries; non-`NotFound` errors propagate. `open` likewise propagates metadata inspection errors. Test covers main path and all three sidecars; `exists()` NC fails at `-wal`, provided symlink creation succeeds. |

Previously refuted note claims now check out:

- Busy timeout is set and read back.
- Read records, resolution candidates, and labels carry appropriate provenance.
- Append-only test compares full dumps of all three tables.
- NC wording correctly acknowledges multiple edits for NC4/NC5.

**New findings:** None. Metadata-error propagation is verified statically; no dedicated test covers it, as disclosed.

This round found **0 problems versus 4 last round**. Static review only; no files changed, tests run, hardware touched, or databases opened.