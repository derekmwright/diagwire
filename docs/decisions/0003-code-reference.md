# DR-0003: Community fault-code reference

**Status:** skeleton (architect, 2026-11-15). Not accepted. No code or data until it is. The maintainer put this ahead of battery registration and clear-faults, which become DR-0004.
**Depends on:** DR-0001 (D4 provenance, D5 deferred definition format, D8 no BMW data, security addendum A1).

## Governing rule

The reference publishes which codes real cars reported and what they mean in our own words. Every entry shows how well its meaning is supported, and no text is copied from BMW's data or from any other tool.

## Decisions (one line each; the author fills in reasons and verifies them)

- **C1 Scope:** fault codes (DTCs) per control unit, with a description. Not live-data definitions, job definitions, freeze-frame layouts or repair procedures (D5 covers those later).
- **C2 Entry key:** platform (for example `G05`), control-unit diagnostic address, control-unit variant when known, and the code as the car reports it. The same code under a different variant is a different entry.
- **C3 Entry fields:** the key, a short description, an evidence level (C5), and the list of evidence stamps (C6) that support it. Nothing else in v0.
- **C4 Descriptions are original:** a contributor writes the description in their own words. Text is never copied or machine-translated from ISTA, SGBD or ODX data, or from another app's output. Review rejects a description that matches a known vendor string.
- **C5 Evidence levels, defined once here:** `observed` (a tool read this code from a real car; the description is unverified), `corroborated` (two independent sources agree on the meaning), `confirmed` (the meaning is tied to a documented diagnosis or repair after which the fault stopped). Every display of an entry shows its level.
- **C6 Evidence stamps:** each stamp records the tool and version, the date, the platform, and the contributor. It never holds a VIN, MAC or IP address. Raw exports stay with the contributor; the repo holds the stamp and a hash of the export.
- **C7 Storage:** plain text files a person can read and diff, one per platform and control unit, kept in this repo. The file format is decided in this record's first slice, not by D5's later job format.
- **C8 How diagwire uses it:** a code found in the reference is shown with its description and evidence level. A code not found is shown raw. An AI's interpretation of a code is labelled as interpretation and is never written into the reference automatically.
- **C9 Contribution path:** a pull request with a DCO sign-off. CI checks the schema, duplicate keys and redaction. Contributed text is untrusted input to any automation that reads it.
- **C10 No bulk seeding:** entries come from cars people actually read, one observation at a time. The reference is never filled by extracting tables from proprietary files (D8).
- **C11 Disagreement:** when contributors give different meanings for one key, both claims are kept with their evidence until one reaches a higher level. Git history is the audit trail.

## Explicitly out (v0)

Live-data and job definitions, freeze-frame decoding, repair instructions, translations, other makes, descriptions generated without a contributor standing behind them.

## Replays the judge will run

1. A contributor pastes another tool's description word for word.
2. One pull request adds thousands of entries taken from an SGBD file.
3. A submitted evidence file contains a VIN.
4. The same code means different things on two variants of the unit at one address.
5. Two contributors give conflicting meanings for one key.
6. An `observed` entry is shown to a user as though its description were established.
7. A description written by an AI is submitted with no evidence behind it.
8. Pull-request text contains instructions aimed at an agent that triages contributions.
9. One tool shows a code as six hex digits and another splits it into a code and a fault-type byte.

## Open questions for the maintainer (answerable in one line; cost given)

1. **Data license:** CC0 (anyone can use it with no conditions), CC-BY (credit required), or the same MIT/Apache terms as the code? (Hard to change once others have contributed.)
2. **Location:** a folder in this repo, or a separate repo? (In-repo is simpler now; a separate repo is easier for other tools to consume.)
3. **Reading other tools:** may a contributor write a description after reading another tool's text, as long as the wording is their own? (Allowing it grows the list much faster; it carries some legal uncertainty this record can't settle.)
4. **First entries:** seed the list with codes read from the maintainer's own cars at `observed` level? (Gives the format real data from day one.)
5. **Shipping:** built into the binary, or read from files at run time? (Built-in works offline with no setup; files let users add entries without rebuilding.)
