# Escape: architect edits to DR-0001 lost in the author's round-2 rewrite

**Date:** 2026-11-14. **Found by:** the architect, checking the author's round-2 hand-back against the record.

1. **Observed failure:** after round 1, the architect edited `docs/decisions/0001-architecture.md` directly: the status line, the D2 rewording, rulings on the D2 and D4 dissents, and the license answer on Q2. The author's round-2 revision rewrote the file from its own earlier copy, and all of those edits disappeared. Its hand-back reported both dissents as "still open" and Q2 as unanswered.
2. **Mechanism:** two writers on one record. The author agent was resumed holding the file contents in its context, and it wrote its revision over the file instead of merging with what was on disk.
3. **Why nothing caught it:** no rule assigned a single writer per record, and the hand-back check compared the author's summary with the brief, not with the architect's own prior edits.
4. **Smallest reproduction:** edit a record while an author still holds it, then resume the author with a revision brief.
5. **Rule adopted:** while an author holds a record, the architect makes changes only through the author's brief, never directly in the file. Rulings recorded in the file after a hand-back must be re-checked after the next author round. The dispatcher's hand-back check now includes diffing the record for lost rulings and answers.
6. **Recovery:** restored on 2026-11-14 before judge round 2. No information was lost, because the rulings were also recorded in the conversation.
