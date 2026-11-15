VERDICT: APPROVE

Findings: None.

- Finality check is inside `BEGIN IMMEDIATE`, before any insert (`records.rs:330–346`); refusal writes nothing.
- Extended test 3 covers refusal, unchanged row count, fresh recovery, and preserved retraction history. Deleting the check would accept the supersede and fail `tests.rs:309`, as stated.
- No regression to round 2 approval identified.

Static review only; no tests run, files changed, hardware touched, or databases opened.