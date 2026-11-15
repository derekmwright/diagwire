# DR-0003: Community fault-code reference

**Status:** author filled 2026-11-15, judge SETTLED in round 3 (2026-11-15; rounds required 8, 2, then 0 changes; verdicts in `docs/notes/`). Awaiting the maintainer's answers to Q1-Q6. Not accepted. No code or data until it is. The maintainer put this ahead of battery registration and clear-faults, which become DR-0004.
**Depends on:** DR-0001 (D4 provenance, D5 deferred definition format, D8 no BMW data, security addendum A1).
**Author fill-in:** round 2, 2026-11-15, revising against judge round 1 (`docs/notes/2026-11-15-dr0003-judge-r1.md`) and the architect's rulings, merged onto the architect's on-disk edits. No reference code or data exists, so everything below is a requirement, not a demonstrated protection. Existing code is cited only where the reference must agree with it: `uds:N` is `crates/uds/src/lib.rs` line N at `431432f`. Outside sources carry a tag (legend under *Authority register*). Anything said about BMW's code format, copyright or licences is **unverified** unless a tag follows it. Nothing here is legal advice; legal points are written as risks the maintainer accepts or declines. Each concept is defined once and referenced elsewhere: key and code form (C2), entry and claim (C3), level (C5), stamp, export handling and redaction check (C6), storage location (C7 and Q2), lookup and rendering (C8), contribution statement (C9).

## Governing rule

The reference publishes which codes real cars reported and what they mean in our own words. Every entry shows how well its meaning is supported, and no text is copied from BMW's data or from any other tool.

## Decisions (one line each; the author fills in reasons and verifies them)

- **C1 Scope:** fault codes (DTCs) per control unit, with a description. Not live-data definitions, job definitions, freeze-frame layouts or repair procedures (D5 covers those later).
  - *Reason:* A code and one sentence is the smallest thing that is useful without BMW's data, and it needs no definition format. Everything that needs scaling or a layout waits for D5.
  - *Alternatives:* Include live-data names (needs D5's format). Ship no text and leave meaning to the AI (the unlabelled guess C8 exists to avoid).
  - *Consequence:* A description says what the control unit is reporting. It never contains a value to scale, a procedure or a part to replace.
  - *Reopen if:* D5 is reopened.

- **C2 Entry key:** platform (for example `G05`), control-unit diagnostic address, control-unit variant when known, and the code. In v0 the code is written as exactly six upper-case hex digits: the three DTC bytes of a UDS read, never split. The same code under a different variant is a different entry. Platforms whose codes have another shape (the DS2 and KWP cars) get entries only after an amendment defines their written form.
  - *Reason:* Nothing checked says a code means the same thing in two control units, on two platforms or under two variants, so the key assumes it doesn't. That costs duplicate entries and keeps known variants apart. It does not hold for an entry with no variant recorded, which can combine observations from different variants (replay 4).
  - *Consequence (key form, defined here):* Platform is free text, upper case (as DR-0002 G0 treats chassis codes). Address is two upper-case hex digits. **Code is exactly six upper-case hex digits: the three DTC bytes of a UDS `0x19 02` record in the order received, the form `uds` already prints (`uds:127-133,160`).** It is never split, so the key is the same whether the third byte belongs to the code or is a fault type (**unverified**, register). Variant is a free-text label or absent; absent means "not recorded" and is its own key value. What identifies a variant is **unverified**, and so is whether a variant name shown by a tool counts as BMW data under D8 ("ECU variant lists"). diagwire cannot read a variant in v0 (D6 allows no per-DID read), so the lookup never knows one (C8).
  - *Alternatives:* The code alone (merges meanings across units). A base code plus a fault-type field (needs the format verified first). A number (loses leading zeros).
  - *Reopen if:* diagwire reads a platform whose codes are not three UDS bytes, or a reviewed read identifies the variant.
  > Author dissent: "the code as the car reports it" does not fix one written form, and without one, replay 9 gives two keys for one fault. The form above narrows v0 to codes read as three UDS bytes. Cars whose codes have another shape (the DS2 platforms) get no entries until an amendment defines their form. If that is too narrow, the alternative is a per-platform code form, which is more format for cars diagwire cannot read yet.
  > Architect ruling (2026-11-15): accepted; C2 reworded. The amendment for DS2 and KWP code forms is owed with the K-line work, which is the project's priority, so it follows soon.

- **C3 Entry fields:** the key and one or more claims. A claim is a short description, an evidence level (C5), and the list of evidence stamps (C6) that support it. Nothing else in v0.
  - *Reason:* Every field is something to validate, redact, review and license.
  - *Consequence (entry and claim, defined here):* An **entry** is one key with one or more **claims**. A claim is a description, a level and at least one stamp. One claim is the normal case; more than one is a disagreement (C11). A description is one line of at most 200 Unicode scalar values (a chosen limit), printable characters only, no URL. The limit also bounds what hostile text can carry (C9). The loader rejects unknown fields.
  - *Alternatives:* Severity, component, links (each is more to verify, and links carry text we don't control).
  - *Reopen if:* a second consumer needs a field the first slice left out.
  > Author dissent: C3 gives an entry one description and one level, C9 rejects duplicate keys, and C11 keeps two meanings for one key. All three cannot hold. Proposed and drafted above: the description, level and stamps belong to a claim, and an entry holds one or more claims. The duplicate-key check then stays absolute.
  > Architect ruling (2026-11-15): accepted; C3 reworded.

- **C4 Descriptions are original:** a contributor writes the description in their own words. Text is never copied or machine-translated from ISTA, SGBD or ODX data, or from another app's output. Review rejects a description that matches a known vendor string.
  - *Reason:* The governing rule. Whether a one-line factual description is protected, and how close a paraphrase may be, is **unverified** and not settled here. S-cfr and S-ccdata give general positions (short phrases, facts) and both say nothing about this case. Writing our own words is the one choice that does not depend on the answer.
  - *Consequence:* This is a certification (C9), not a detector. The repo can hold no list of vendor strings to compare against (D8), and whether hashes of them would be BMW data is **unverified**, so CI has no such check. "Review rejects" means a reviewer who recognises the text, or who compares privately against their own copies (D8 allows a local tool to read them). A match found after merge is removed by an ordinary pull request and stays in git history.
  - *Alternatives:* An automated similarity check (needs the corpus we refuse to hold). Quoting with credit (breaks the governing rule).
  - *Reopen if:* BMW publishes its texts under a compatible licence (as D8).

- **C5 Evidence levels, defined once here:** `observed` (a tool read this code from a real car; the description is unverified), `corroborated` (two independent sources agree on the meaning), `confirmed` (the meaning is tied to a documented diagnosis or repair after which the fault stopped). Every display of an entry shows its level. v0 accepts `observed` only: `corroborated` is not accepted until Q3's answer yields a reviewable independence rule and its attestation, and `confirmed` is rejected until an amendment decides how a repair is recorded.
  - *Reason:* The governing rule: a reader must be able to tell a sighting from an established meaning.
  - *Consequence (what the loader enforces):* a claim needs at least one stamp, its level must be one v0 accepts, and no two stamps in a claim share an export hash. That is a check that stamps are distinct and nothing more: two handles and two hashes show neither two people nor two sources of meaning. A stamp attests that a code was read, not what it means; a contributor who adds a stamp to a claim asserts, under the contribution statement, that they agree with its description. Display is C8's.
  - *Alternatives:* A numeric confidence (false precision). One "verified" flag (hides the difference between seen and understood).
  - *Reopen if:* users are shown to misread a level.
  > Author dissent (C5 and C6): `confirmed` needs "a documented diagnosis or repair", and neither C3 nor C6 gives that document a place. Options: (a) one optional free-text `repair` field on a stamp, written in the contributor's words and covered by the redaction check; (b) leave `confirmed` defined but have the v0 loader reject it until an amendment decides the repair record. Drafted as (b), because no v0 entry can need it yet and an optional field is cheap to add later.
  > Architect ruling (2026-11-15): option (b) accepted; C5 extended.

- **C6 Evidence stamps:** each stamp records the tool and version, the date, the platform, and the contributor. It never holds a VIN, MAC or IP address. Raw exports stay with the contributor; the repo holds the stamp and a hash of the export.
  - *Reason:* Same idea as D4: a stamp lets a reader tell evidence from belief. It is weaker than a D4 fixture stamp, because the export is not in the repo, and inspecting it is neither required nor guaranteed (the voluntary private path is below).
  - *Consequence (stamp, defined here):* Fields are `tool`, `tool_version`, `read_on` (date), `contributor` and `export_sha256`. The platform is the file's (C7) and is not repeated per stamp. `tool`, `tool_version` and `contributor` are non-empty, `read_on` is a valid `YYYY-MM-DD` date, and the hash is 64 lower-case hex digits. `contributor` is a handle the contributor chooses; it is published for good (S-dco, clause d). The hash is a commitment nobody can check without the export.
  - *Consequence (exports and linkage, defined here):* An export is never requested in public. A reviewer may ask to see one privately; agreeing is voluntary, and without it the reviewer decides on what the repo shows. A reviewer who is shown an export checks it against the hash, records only that it matched, and does not keep it. Nothing enforces that. A stamp names no car, but it can link observations: one handle ties together everything a contributor submitted, and one hash ties together every code from one reading, with its platform and date.
  - ***Redaction check* (defined here):** C6's line prohibits these identifiers in any form; the check detects only the three shapes below, and the rest rests on the contribution statement and review. It runs over the raw text of every reference file, comments included, and again over every decoded string, because TOML strings allow Unicode escapes (S-toml). Matching ignores case. **VIN shape:** exactly 17 characters from `A`–`Z` and `0`–`9` without `I`, `O` and `Q`, with no letter or digit on either side (DR-0002 G8's shape; the alphabet is unverified there). **MAC shape:** six pairs of hex digits joined by `:` or `-`, with no hex digit or separator on either side. **IPv4 shape:** four decimal numbers from 0 to 255 joined by dots, with no digit or dot on either side. *Not detected:* IPv6 in any form, a MAC written without separators or in dotted groups, an IPv4 address in another notation, host names, and the forms in G8's residual. Known false positives: a four-part tool version reads as an IP address, and a 17-character handle or word can read as a VIN. **It detects and cannot unpublish:** a pull request is public when pushed, before CI runs (host behaviour **unverified**). The only prevention is running the same check locally first, which the contribution guide asks for and nothing enforces.
  - *Alternatives:* Commit redacted exports (other tools' exports contain their description text, which C4 forbids). No hash (nothing to check a later dispute against).
  - *Reopen if:* the check's misses or false positives are measured to matter, or `confirmed` needs the export.

- **C7 Storage:** plain text files a person can read and diff, one per platform and control unit, kept where Q2 decides. The file format is decided in this record's first slice, not by D5's later job format.
  - *Reason:* A reviewer reads the diff, and another project can copy a directory without building anything.
  - *Consequence:* Proposed for the slice: TOML 1.0, one file named `<PLATFORM>/<ADDRESS>.toml` under a reference directory whose location is Q2's, with a `format = 1` line (sketch below). The loader takes the text plus the platform and address its caller expects, and never a path. In TOML a string must be quoted, so the loader can reject a code written as a number. The specification makes a repeated key invalid (S-toml), but entries are an array, so the duplicate-entry check is ours. The parser crate is not chosen and its behaviour is **unverified** until the slice's tests run.
  - *Alternatives:* JSON (every language reads it; no comments, noisier diffs). CSV (a list of stamps does not fit a row). SQLite (not diffable).
  - *Reopen if:* another tool needs JSON. Generate an export then; keep one source format.

- **C8 How diagwire uses it:** a code found in the reference is shown with its description and evidence level. A code not found is shown raw. An AI's interpretation of a code is labelled as interpretation and is never written into the reference automatically.
  - *Reason:* The governing rule, and DR-0002 G4's: a reader must be able to tell what was measured from what was said about it.
  - *Consequence (lookup and rendering, defined here):* Lookup takes platform, address and code. It returns every entry with those three, whatever its variant, and never chooses among entries or claims. One rendering function puts the description, the level in words, the variant label and, with more than one claim, the word "disputed" into one string. The API has no description-only accessor. A miss returns the raw code and says there is no reference entry, so any meaning offered is interpretation. The reference crate has no write API, and no operation or MCP tool writes reference files; the only way in is C9. Descriptions are joined at display time and never stored in the garage (DR-0002's schema stores no DTC text). Strings bound for the AI client pass DR-0002 G8's outgoing filter like any other. In every outgoing result, reference text sits in a field of its own that is identified as untrusted reference data, apart from instructions and apart from measured provenance (DR-0002 G4). The CLI/MCP integration slice owns that check.
  - *Residual:* diagwire cannot label what an AI client composes, and cannot stop it restating an `observed` description as fact. It controls only its own output, and the marking above does not prevent injection on the client side (C9).
  - *Alternatives:* Hide `observed` entries (hides most of the list). Store the text with each snapshot (goes stale, and changes DR-0002).
  - *Reopen if:* MCP gains a standard way to mark a result's trust level.

- **C9 Contribution path:** a pull request with a DCO sign-off. CI checks the schema, duplicate keys and redaction. Contributed text is untrusted input to any automation that reads it.
  - *Reason:* A sign-off is the lightest certification in common use. Security addendum A1 covers the rest.
  - *Consequence, certification:* The DCO certifies the right to submit under "the open source license indicated in the file" and says nothing about whose car was read or whose words were used; its text may not be changed (S-dco). So each reference file names its licence in a comment line (Q1), and the project adds a ***contribution statement*** (defined here; its text goes in the contribution guide once Q1 and Q3 are answered): the contributor read the code from a car they own or were allowed to read; whoever owns the car has authorised publishing the observation (code, control unit, platform, date, tool and handle); the description is their own words under C4; everyone named on a stamp of a claim agrees with that claim's description; the submission holds no vehicle or network identifier in any form, including forms the redaction check (C6) does not detect; and they keep the export the hash names. Whether a sign-off can carry a statement beyond the DCO's own text is **unverified**: a risk for the maintainer.
  - *Consequence, checks:* "CI" is the existing workflow (`.github/workflows/ci.yml` at `431432f`: `pull_request`, read-only token, no secrets; repository settings **unverified**) running a workspace test that loads every reference file through the loader and the redaction check. If Q2 puts the files elsewhere, that repository needs the same gate. Owed by a later slice, and a reviewer's rule until then: a pull request that touches reference files touches nothing else; a sign-off is present.
  - *Consequence, untrusted text:* In v0 no agent triages, reviews or merges an outside contribution on its own; the maintainer merges (pending Q6). Giving an agent that job is a new decision under A1 (text through a data channel, enumerated read-only tools, no merge right). A description also reaches the user's AI client through C8. v0 is vehicle-read-only (D6), which protects the car and nothing else: hostile text can still aim at garage writes (DR-0002 G6) and at whatever other tools the client holds. C3's limits, human review and C8's marking reduce that exposure; they do not prevent it.
  - *Alternatives:* A contributor licence agreement (heavier; whether it is needed is **unverified**). Issue-form submissions turned into commits by a bot (an automation with write access reading strangers' text, the A1 escape).
  - *Reopen if:* maintainer-only merging becomes the bottleneck.

- **C10 No bulk seeding:** entries come from cars people actually read, one observation at a time. The reference is never filled by extracting tables from proprietary files (D8).
  - *Reason:* D8, and the governing rule's "which codes real cars reported". S-ccfaq also describes a separate database right, mainly in the EU, against extracting a substantial part of a database. Whether it would apply is **unverified**; C10 stays clear of it either way.
  - *Consequence:* Every claim needs a stamp (C5), so an entry with no stamp fails the loader. A later slice adds a cap of 100 new claims per pull request (a chosen number, not measured). A cap slows extraction; it does not identify it, and an invented stamp looks like a real one. The maintainer's own entries use the same path as anyone's (DR-0002 G0's principle).
  - *Alternatives:* A local tool that imports the user's own SGBD texts into the published reference (D8 prohibits proprietary tables in the repo; it does not prohibit reading them locally).
  - *Reopen if:* as D8.

- **C11 Disagreement:** when contributors give different meanings for one key, both claims are kept with their evidence until one reaches a higher level. Git history is the audit trail.
  - *Reason:* Picking a winner by merge order would hide that neither meaning is established.
  - *Consequence:* A disagreement is an entry with two or more claims (C3). While v0 accepts one level (C5), every claim in an entry is at that level and nothing is dropped. Once a higher level can be accepted, the pull request that raises one claim removes the others, history keeps them, and a new meaning that conflicts with an already higher claim is not merged. Whether two descriptions are one meaning in different words is a reviewer's judgment. A pull request that rewrites an existing description in place passes CI; only review catches it.
  - *Alternatives:* Last writer wins. Voting (counts contributors, not evidence).
  - *Reopen if:* a dropped lower-level claim turns out to be right.

## Entry and file-format sketch (C3, C6, C7; every value invented)

```toml
# SPDX-License-Identifier: <per Q1>
format = 1
platform = "G05"
address = "12"

[[entry]]
code = "0A1B2C"
# variant = "<label>"   (omitted = not recorded)

[[entry.claim]]
description = "Invented example: the left widget sensor reads out of range."
level = "observed"

[[entry.claim.stamp]]
tool = "exampletool"
tool_version = "0.0.0"
read_on = "2026-01-01"
contributor = "example-contributor"
export_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
```

The header must equal the platform and address the loader's caller expects (C7). A reading that found several codes repeats its stamp under each; that is accepted until files become hard to read.

## Explicitly out (v0)

Live-data and job definitions, freeze-frame decoding, repair instructions, translations, other makes, descriptions generated without a contributor standing behind them.

## Replays the judge will run

"Prevented" below means prevented once the named check is built. Nothing is built.

1. A contributor pastes another tool's description word for word.
   - **Described, not prevented.** C4 forbids it and the contribution statement makes the contributor certify otherwise. Nothing compares text. *Gap:* detection depends on a reviewer recognising the wording, and removal leaves it in history.
2. One pull request adds thousands of entries taken from an SGBD file.
   - **Prevented if the claims have no stamps** (C5, slice test 1) **and, once the cap exists, as a single pull request** (C10). **Described** when the stamps are invented or the work is split up. *Gap:* stamps cannot be verified, and a private look at an export is voluntary (C6).
3. A submitted evidence file contains a VIN.
   - **Merge prevented, publication only described.** The repository gate fails on any file in the reference directory that is not a reference file, and the redaction check rejects the VIN shape in raw text and in decoded strings (C6, slice tests 3 and 5). *Gap:* the push is already public (C6); the forms C6 lists as not detected pass; a file elsewhere in the pull request is unchecked until the paths rule exists (C9).
4. The same code means different things on two variants of the unit at one address.
   - **Wrong merge prevented; the right pick is only described.** The variant is part of the key (C2, slice test 2), and lookup returns both, labelled, without choosing (C8). *Gap:* diagwire cannot read the variant, so the user sees two meanings; an entry recorded without a variant can hide that its meaning is variant-specific.
5. Two contributors give conflicting meanings for one key.
   - **Prevented for a second entry:** the duplicate-key check forces the second meaning into the same entry as a second claim, shown as disputed (C3, C8, C11). **Described** for an in-place rewrite (C11).
6. An `observed` entry is shown to a user as though its description were established.
   - **Prevented in diagwire's own output** by C8's single rendering (slice test 4). **Described** for what an AI client says next and for any other tool that reads the files.
7. A description written by an AI is submitted with no evidence behind it.
   - **Prevented when it has no stamp** (C5, slice test 1). With a real stamp it is an `observed` claim a contributor stands behind, which the out list allows, and its description is unverified by definition. **Described** for an invented stamp, and for AI text that repeats a vendor's wording (replay 1).
8. Pull-request text contains instructions aimed at an agent that triages contributions.
   - **Prevented by absence:** v0 has no such agent, and CI parses files as data (C9). **Described** for the rule itself, which is process and not enforced (Q6), and for description text that reaches the user's AI client: C8 marks it as untrusted data, which does not stop a client acting on it (C9).
9. One tool shows a code as six hex digits and another splits it into a code and a fault-type byte.
   - **Prevented in the files:** one code form, and the loader rejects any other (C2, slice test 1). **Described** for a contributor who joins a split display by hand. Whether joining gives the three bytes in order for that tool is **unverified**. The export is not in the repo, and C6's voluntary private look, when it happens, only shows that the hash matches; it does not verify byte order. Until a register row for a tool's display is verified, reviewers decline stamps from tools that do not show the six-digit form; nothing enforces that.

## Authority register

Status values as in DR-0001. Nothing is verified against a car or a canonical client. "Doc read" means the cited page says so, not that it applies to this project's case.

| Contract | Authority | Status |
|---|---|---|
| A code is the three DTC bytes of a `0x19 02` record, shown as six hex digits (C2) | DR-0001 register rows "DTC record 3+1 bytes" and "DTC shown as 6 hex digits" | unverified there, so unverified here |
| Whether the third byte is a fault type, and whether tools that split a code split it there (replay 9) | ISO 14229-1; SAE J2012 / ISO 15031-6. Paywalled; not read. The ISO catalogue page refused an automated fetch | **unverified** |
| What identifies a control-unit variant; platform names such as `G05` | none named | no authority yet |
| VIN shape used by the redaction check | DR-0002 G8 (ISO 3779, unverified there) | unverified |
| MAC and IPv4 shapes (C6); description limits (C3); the 100-claim cap (C10) | our own definitions | no authority yet (chosen values) |
| TOML: quoted strings, Unicode escapes in strings, repeated key or table invalid | S-toml | verified (doc read); parser behaviour unverified |
| What a DCO sign-off certifies; that its text may not be changed; that the record is kept indefinitely | S-dco | verified (doc read) |
| A sign-off can carry the contribution statement; the DCO's "open source license" wording fits a CC-licensed data file | none checked | **unverified** (maintainer's risk) |
| CC0 text: a waiver of the affirmer's own rights with a fallback licence; patent and trademark rights unaffected; no warranty; the affirmer disclaims clearing other persons' rights | S-cc0, S-cc0code (sections 2–4) | verified (doc read); whether and how it applies here unverified |
| CC BY 4.0 deed: appropriate credit, a link to the licence and an indication if changes were made, in any reasonable manner; not required for public-domain elements or uses under an exception; the licensor cannot revoke while the terms are followed | S-ccby (the deed, which says it is not the licence) | verified (doc read); whether and how it applies here unverified |
| CC advises against CC licences for software, calls CC0 acceptable for software, and says CC licences and CC0 can be used on databases | S-ccfaq | verified (doc read) |
| MIT and Apache-2.0 both require their notice to travel with copies | `LICENSE-MIT`, `LICENSE-APACHE` in this repo | verified (file read); fit for a data collection unverified |
| A one-line description is or is not protected; how close a paraphrase may be (C4, Q3) | S-cfr and S-ccdata state general positions only | **unverified**; not decided by this record |
| CI runs without secrets on pull requests | `.github/workflows/ci.yml` | verified (file read); repository settings unverified |
| A pushed pull request is public before CI; what the host offers to remove it | none checked | **unverified** |
| User-visible behaviour (rendering, miss message, disputed marking) | Maintainer-approved acceptance scenarios (the replays, once approved) | no authority yet |

Sources (fetched 2026-11-15): S-dco https://developercertificate.org/ · S-cc0 https://creativecommons.org/publicdomain/zero/1.0/ · S-cc0code https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt · S-ccby https://creativecommons.org/licenses/by/4.0/ · S-ccfaq and S-ccdata https://creativecommons.org/faq/ (general and data sections) · S-toml https://toml.io/en/v1.0.0 · S-cfr https://www.ecfr.gov/current/title-37/section-202.1 (US regulation; fetched through the site's renderer API)

## First slice (after acceptance)

Mode: feature. Outcome: a new crate `diagwire-reference` whose loader takes a file's text and the expected platform and address (C7), with no file, car or network I/O and no dependency on another diagwire crate, plus the redaction check and a lookup that nothing calls yet. All fixtures are invented. Negative controls are kept to the refusal, redaction and rendering boundaries (tests 1, 3, 4), run as in DR-0002's first slice.

1. **Schema (replays 2, 7, 9):** the sketch loads. Each of these is rejected: an unknown field; a `format` other than 1; a code in lower case, with five digits, or written as a number; a claim with no stamp; a level v0 does not accept (C5); two stamps in a claim with one hash; an empty tool, version or contributor; an impossible date; a hash that is not 64 lower-case hex digits; a header that differs from the expected platform or address; a description over the limit or with a line break. *NC:* remove the stamp-count check.
2. **Keys (replays 4, 5):** the same code and variant twice is rejected; the same code under two variants loads as two entries; two claims in one entry load.
3. **Redaction (replay 3):** a VIN shape built from one repeated character is rejected in a comment, and again when written with Unicode escapes inside a string; so are a MAC shape and an IPv4 shape; the sketch, with its 64-digit hash, passes. *NC:* scan raw text only, then decoded strings only.
4. **Lookup (replays 4, 5, 6):** a hit returns every claim rendered with its level; two variants return both, labelled; two claims are marked disputed; a miss says so. *NC:* render without the level.
5. **Repository gate (awaits Q2; the only part that depends on location):** a test walks the reference directory, takes each file's expected platform and address from its path, loads it through 1–3 and fails on any other file there. It reports the count, and zero files is reported as zero, not as agreement.

Out: display in the CLI or MCP, the cap, the paths rule, the sign-off check, the contribution guide (waits for Q1 and Q3), any real entry (waits for Q1 and Q4). Stop conditions: the chosen parser accepts a repeated key or hands back a numeric code as a string; a real tool's version trips the IPv4 shape (decide the rule first); Q5's answer needs file access inside the crate.

## Open questions for the maintainer (answerable in one line; cost given)

1. **Data license:** CC0 (anyone can use it with no conditions), CC-BY (credit required), or the same MIT/Apache terms as the code? (Hard to change once others have contributed.)
   - *Considerations (author; not an answer):* These report what the texts say, not whether or how they apply here. **CC0:** the deed allows copying, changing and distributing without asking permission, so its text sets no condition for a downstream tool to carry, the closest fit to "grab and run with"; it covers only the affirmer's own copyright and related rights, leaves patent and trademark rights alone, and disclaims clearing anyone else's rights (S-cc0, S-cc0code); contributors get no credit they can require. **CC-BY:** the deed asks for appropriate credit, a link to the licence and an indication if changes were made, in any reasonable manner, and not for public-domain elements or uses under an exception (S-ccby); for a list with many contributors that is still something each small tool, and our own binary if Q5 is "built in", must work out; CC says its licences are not compatible with the major software licences (S-ccfaq). **MIT/Apache:** one licence for the whole repo and nothing new to explain; both require notices to travel with copies; whether software licences suit a data list is unverified. For either CC choice, whether the DCO's "open source license" wording fits is unverified (register). Risk: a different licence later may need further permission from past contributors, and it cannot take back grants already made (CC0's waiver is written as irrevocable, S-cc0code; the CC BY deed says the licensor cannot revoke, S-ccby).
2. **Location:** a folder in this repo, or a separate repo? (In-repo is simpler now; a separate repo is easier for other tools to consume.)
   - *Considerations:* **In-repo:** format, loader and data change in one commit and the existing CI covers them; data contributors send pull requests to a code repo, so the paths rule (C9) matters; a non-code licence from Q1 makes the repo multi-licence. **Separate:** a clean licence boundary and nothing but data for a consumer to clone; the checker lives here and the data there, so their versions can drift, and a second repo needs its own CI and settings. Only the repository gate depends on the answer (slice test 5).
3. **Reading other tools:** may a contributor write a description after reading another tool's text, as long as the wording is their own? (Allowing it grows the list much faster; it carries some legal uncertainty this record can't settle.)
   - *Considerations:* **Yes:** most contributors only know a meaning because a tool told them, so this is where most entries would come from; reviewers must then judge how close a paraphrase is, with no rule to judge by (C4); two contributors who read the same tool are not independent, so the answer has to come with the independence rule C5 is waiting for. **No:** descriptions come only from a contributor's own diagnosis, so growth is slow and many seen codes get no entry, since C3 requires a description; it cannot be checked, only certified (C9). Machine translation stays banned either way (C4).
4. **First entries:** seed the list with codes read from the maintainer's own cars at `observed` level? (Gives the format real data from day one.)
   - *Considerations:* **Yes:** real files exercise the format and the gate early; it needs Q1 answered first and, for the descriptions, Q3; it publishes that these cars had these faults, under the maintainer's handle; development agents cannot do the reading (D7); a stamp whose tool is diagwire rests on DR-0001 register rows that are still unverified. **No:** the slice loses nothing, because its fixtures are invented either way; the format meets real data later, when changing it costs more.
5. **Shipping:** built into the binary, or read from files at run time? (Built-in works offline with no setup; files let users add entries without rebuilding.)
   - *Considerations:* **Built in:** a bad file fails the build, not the user; entries are as old as the binary; the data's licence terms travel with every binary (Q1). **Files:** needs a search path (resolved in the front ends, as DR-0002 G9 does), a rule for a malformed local file, and a visible mark separating local entries from the project's, or a locally written guess looks like reference (C8); an AI with a shell can write such files (the same residual as D7). Doing both costs both. The slice's parse-from-a-string loader serves either answer.
6. **Who merges outside contributions:** only the maintainer (C9 as drafted), or an agent once the gates pass, as for the project's own work? (Maintainer-only is the safer start, since contributed text comes from strangers; it makes the maintainer the bottleneck as the list grows.)
