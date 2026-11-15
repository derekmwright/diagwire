# Garage VIN filter: worker result note

**Record:** DR-0002 G8 and Q5; epic #2, "Privacy surface". **This is one part of that slice, not the slice.** **Date:** 2026-11-15. Linux (cloud container), Rust 1.97.0. Not reviewed by the judge. All VINs in the tests are invented (G0).

## What was built

`crates/diagwire/src/redact.rs` (new, `pub mod redact`): `Redactor::new(stored_vins)` and `Redactor::redact(&mut self, text) -> String`, with 8 tests. No other file changed, no dependency added.

- **Stored VINs:** each value passed to `new` is replaced wherever it appears, ignoring ASCII case, whatever surrounds it (inside a label, alias or note, or glued into a longer token).
- **VIN shape:** any run of exactly 17 characters from `A`–`Z` and `0`–`9` without `I`, `O` and `Q`, in either case, with no letter or digit on either side, is replaced even if the garage does not hold it. This is the shape as DR-0003 C6 words it. The alphabet is unverified, as in G8.
- **Placeholders:** `vehicle-1`, `vehicle-2`, … in order of first appearance. One `Redactor` serves one response, so a VIN keeps its placeholder across every string of that response and the numbers start again in the next.

## Not done (the rest of the privacy-surface slice)

- **Nothing calls the filter.** No operation returns garage data yet and there is no MCP server or export, so "every string bound for the AI client" is not enforced anywhere. The slice that adds the first outgoing garage result must route every string through one `Redactor` and test that.
- **Export**, with redaction by default and the user-side switch to include VINs. No export format is decided.
- **Payload-free logs.** Not checked.
- **File permissions** (0700 directory, 0600 files, the Unix warning).
- The user-side switch that turns redaction off (an export flag or a server launch flag).

## Points the record leaves open (for the architect)

1. **Which VINs are "stored".** `new` takes a list and its doc comment says: every VIN the garage holds, whether live, superseded or retracted. G8 says "each stored VIN". A corrected or retracted VIN still identifies a car, so I read it as all of them. The query that produces the list does not exist yet.
2. **Case.** Matching ignores ASCII case only. G8 says "case-insensitively". A stored value with non-ASCII letters is matched in its ASCII-upper-cased form, so `é` and `É` are different. A VIN has no such letters.
3. **Stored values are trimmed** of surrounding white space, and an empty one is ignored. Without that, an empty stored value would match everywhere.
4. **Overlaps.** If two matches overlap, the whole overlapping span becomes the first match's placeholder. This only arises between stored values that share a part.
5. **A placeholder can collide with user text.** A label that is literally `vehicle-1` is indistinguishable from a placeholder in the output. Nothing prevents it.
6. **"Referring to vehicles by label"** (G8, Q5) is not the filter's job as built: it replaces a VIN with a placeholder and does not look up a label.

## Residual, pinned by a test

`residual_forms_pass` asserts that these pass unchanged, as G8's residual says: a VIN written with `-` or spaces inside it, and its last seven characters. A mistyped VIN of 17 VIN characters is caught by the shape rule; one of 16 is not. The test exists so that a change to the residual is deliberate.

Over-redaction: any 17-character run of VIN characters is replaced, including a 17-digit number. A 64-digit hash is not.

## Negative controls

Each ran in a scratch copy with its own new target directory: `cargo test -p diagwire-cli -p diagwire --locked`.

| Control | Guarded line broken | Failing test and message |
|---|---|---|
| r1 | stored match made case-sensitive | **first run: nothing failed.** The shape rule was covering every lower-case case in the tests. Two assertions were added (`vinwbaxx00000t000001x`, and `ab12345` against a stored `AB12345`). Second run: `stored_value_of_any_length_is_replaced`: `left: "chassis ab12345"` `right: "chassis vehicle-1"`; also `stored_vin_is_replaced_wherever_it_appears` |
| r2 | shape rule removed | `residual_forms_pass`: `left: "WBAXX00000T000007"` `right: "vehicle-1"`; also `one_vin_keeps_one_placeholder_within_a_response` and `unknown_token_of_vin_shape_is_replaced` |
| r3 | token boundary uses ASCII letters and digits only | `other_tokens_are_left_alone`: `left: "vehicle-1é"` `right: "JH4KA7650MC000003é"` |
| r4 | `I`, `O`, `Q` allowed in the shape | `other_tokens_are_left_alone`: `left: "vehicle-1"` `right: "JH4KA7650MC00000I"` |
| r5 | every match gets a new number | `one_vin_keeps_one_placeholder_within_a_response`: `left: "vehicle-1 then vehicle-2 then vehicle-3"`; also four other tests |

Not controlled: the trimming and empty-value filter in `new`, the overlap merge, and the longest-first ordering.

## Gate results

See the end of `2026-11-15-d10-transactions-worker.md`: the three slices of this session were gated together on one tree.
