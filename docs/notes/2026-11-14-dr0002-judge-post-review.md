VERDICT: SETTLED

Line references: `docs/decisions/0002-garage.md`.

| Round-3 item | Status | Evidence |
|---|---|---|
| G5 universally required chain-based retraction | RESOLVED | L48 scopes `retracted = 1` superseding rows to identifiers, service events and notes; vehicles use `vehicle_retraction` and have no chain. |
| G6 omitted vehicle retraction from MCP capabilities | RESOLVED | L56 permits retraction of any `RecordWriter` record kind and explicitly includes vehicles, which are never superseded. |

**Whole-record class search:** No conflicting restatement found. Checked capability boundaries (L26, L42), G5/G6, schema (L88–94), replays (L104–122), and first-slice scope/tests (L145–149). The schema still restates vehicle-retraction behavior at L94, so literal “define once, reference elsewhere” consolidation is incomplete; that restatement agrees with G5.

**Required changes:** None. No new contradiction or unsupported claim introduced by these edits found. Previously passed matters were not reopened. Static review only; nothing modified, no hardware or databases touched.