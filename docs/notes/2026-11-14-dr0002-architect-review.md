# DR-0002 architectural review after a third failed judge round

**Date:** 2026-11-14. **Trigger:** the round-loop rule. Judge round 3 returned CHANGES REQUIRED (2 items, after 8 and then 4).

## Findings
- Round 3's two items (G5 still defined retraction as only `retracted = 1`; G6's operation list omitted "retract vehicle") were **introduced by the architect's own round-2 edits**. Round 1's G6 inconsistency had the same origin: the architect's ruling changed G6's decision line without updating its consequence.
- **Class:** a concept restated in several places, then edited in one. Patching each restatement leaves the class open, so the next edit reintroduces it.

## Simpler rule adopted (removes the class)
1. **Define once, reference elsewhere.** Retraction is defined in one place (G5), with its mechanism per record kind. Every other mention references G5 instead of restating it.
2. **Rules over enumerations.** G6's list of MCP operations is replaced by a rule: MCP may add, and supersede or retract, any record kind a `RecordWriter` holds.

## Process rule (escape record)
Before an architect edits a record directly, grep the record for every mention of the concept being changed, and change or point all of them at one definition. An architect edit that touches a concept defined in more than one place first consolidates it into one.

## Next
A fresh judge pass on the consolidated text. This is a new loop after the review, not a fourth round of the old one.
