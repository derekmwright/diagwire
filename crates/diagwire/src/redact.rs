//! The outgoing VIN filter (DR-0002 G8, Q5).
//!
//! Every string bound for the AI client or an export goes through one
//! [`Redactor`]: results, labels, aliases, ambiguity candidates, write echoes
//! and error messages. It replaces each stored VIN wherever it appears, and
//! any other token of VIN shape, with a placeholder (`vehicle-1`, ...).
//!
//! This is best effort (G8's residual). A VIN written with separators,
//! truncated or mistyped passes, and so do plates, names and places.

/// Characters a VIN is made of: `A`-`Z` and `0`-`9` without `I`, `O` and `Q`
/// (ISO 3779 per DR-0002 G8, unverified there).
fn is_vin_char(c: char) -> bool {
    c.is_ascii_alphanumeric() && !matches!(c.to_ascii_uppercase(), 'I' | 'O' | 'Q')
}

const VIN_LEN: usize = 17;

/// Redacts the strings of one response. Use a new one for each response:
/// within it one VIN always gets the same placeholder, numbered in order of
/// first appearance, and the numbers mean nothing in the next response.
pub struct Redactor {
    /// Stored VINs, ASCII upper case.
    stored: Vec<String>,
    /// What has been replaced so far, ASCII upper case; position + 1 is the
    /// placeholder's number.
    seen: Vec<String>,
}

impl Redactor {
    /// `stored_vins` is every VIN the garage holds, whether live, superseded
    /// or retracted.
    pub fn new<I, S>(stored_vins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut stored: Vec<String> = stored_vins
            .into_iter()
            .map(|vin| vin.as_ref().trim().to_ascii_uppercase())
            .filter(|vin| !vin.is_empty())
            .collect();
        // Longest first, so a VIN that contains another is matched whole.
        stored.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        stored.dedup();
        Self { stored, seen: Vec::new() }
    }

    /// Returns `text` with every stored VIN and every VIN-shaped token
    /// replaced.
    pub fn redact(&mut self, text: &str) -> String {
        let mut hits = self.stored_hits(text);
        hits.extend(shape_hits(text));
        // Earliest first; at one start, the longest.
        hits.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut hits = hits.into_iter().peekable();
        while let Some(hit) = hits.next() {
            // Hits that overlap are replaced together, as the first of them.
            let mut end = hit.end;
            while let Some(next) = hits.next_if(|next| next.start < end) {
                end = end.max(next.end);
            }
            out.push_str(&text[copied..hit.start]);
            out.push_str(&self.placeholder(&text[hit.start..hit.end]));
            copied = end;
        }
        out.push_str(&text[copied..]);
        out
    }

    /// Every occurrence of a stored VIN, ignoring ASCII case, whatever
    /// surrounds it.
    fn stored_hits(&self, text: &str) -> Vec<Hit> {
        // ASCII upper-casing keeps every byte offset where it was.
        let upper = text.to_ascii_uppercase();
        let mut hits = Vec::new();
        for vin in &self.stored {
            let mut from = 0;
            while let Some(at) = upper[from..].find(vin.as_str()) {
                let start = from + at;
                hits.push(Hit {
                    start,
                    end: start + vin.len(),
                });
                from = start + vin.len();
            }
        }
        hits
    }

    fn placeholder(&mut self, matched: &str) -> String {
        let key = matched.to_ascii_uppercase();
        let number = match self.seen.iter().position(|seen| *seen == key) {
            Some(index) => index + 1,
            None => {
                self.seen.push(key);
                self.seen.len()
            }
        };
        format!("vehicle-{number}")
    }
}

/// A byte range of the text to replace.
struct Hit {
    start: usize,
    end: usize,
}

/// Every token of VIN shape: exactly 17 VIN characters with no letter or
/// digit on either side.
fn shape_hits(text: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    // The run of letters and digits being read: where it started, how many
    // characters it has, and whether all of them are VIN characters.
    let mut run: Option<(usize, usize, bool)> = None;
    let mut close = |run: &mut Option<(usize, usize, bool)>, end: usize| {
        if let Some((start, VIN_LEN, true)) = run.take() {
            hits.push(Hit { start, end });
        }
    };
    for (at, c) in text.char_indices() {
        if c.is_alphanumeric() {
            let (start, len, all_vin) = run.unwrap_or((at, 0, true));
            run = Some((start, len + 1, all_vin && is_vin_char(c)));
        } else {
            close(&mut run, at);
        }
    }
    close(&mut run, text.len());
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invented VINs (DR-0002 G0): the right shape, no real car.
    const VIN_A: &str = "WBAXX00000T000001";
    const VIN_B: &str = "ZZZ99999999999992";
    /// VIN shape, but not in the garage.
    const STRANGER: &str = "JH4KA7650MC000003";

    fn garage() -> Redactor {
        Redactor::new([VIN_A, VIN_B])
    }

    #[test]
    fn stored_vin_is_replaced_wherever_it_appears() {
        let mut r = garage();
        assert_eq!(r.redact(VIN_A), "vehicle-1");
        assert_eq!(r.redact("VIN WBAXX00000T000001, the wagon"), "VIN vehicle-1, the wagon");
        // Case does not hide it.
        assert_eq!(r.redact("wbaxx00000t000001"), "vehicle-1");
        assert_eq!(r.redact("WbaXX00000t000001"), "vehicle-1");
        // Nor does being glued into a label, alias or note.
        assert_eq!(r.redact("wagon-WBAXX00000T000001-2019"), "wagon-vehicle-1-2019");
        assert_eq!(r.redact("vinWBAXX00000T000001x"), "vinvehicle-1x");
        // Glued in and in another case: the shape rule cannot see this one,
        // only the stored value can.
        assert_eq!(r.redact("vinwbaxx00000t000001x"), "vinvehicle-1x");
        assert_eq!(r.redact("WBAXX00000T000001WBAXX00000T000001"), "vehicle-1vehicle-1");
    }

    #[test]
    fn one_vin_keeps_one_placeholder_within_a_response() {
        let mut r = garage();
        assert_eq!(
            r.redact("ZZZ99999999999992 then WBAXX00000T000001 then zzz99999999999992"),
            "vehicle-1 then vehicle-2 then vehicle-1"
        );
        // A later string of the same response uses the same numbers.
        assert_eq!(r.redact("about WBAXX00000T000001"), "about vehicle-2");
        assert_eq!(r.redact(&format!("{STRANGER} and {VIN_B}")), "vehicle-3 and vehicle-1");

        // A new response starts over.
        let mut next = garage();
        assert_eq!(next.redact("about WBAXX00000T000001"), "about vehicle-1");
    }

    #[test]
    fn unknown_token_of_vin_shape_is_replaced() {
        let mut r = Redactor::new([] as [&str; 0]);
        assert_eq!(r.redact(STRANGER), "vehicle-1");
        assert_eq!(r.redact("(jh4ka7650mc000003)"), "(vehicle-1)");
        assert_eq!(r.redact("vin:JH4KA7650MC000003, ok"), "vin:vehicle-1, ok");
        assert_eq!(r.redact("12345678901234567"), "vehicle-2");
        assert_eq!(r.redact("a JH4KA7650MC000003\nb 12345678901234567"), "a vehicle-1\nb vehicle-2");
    }

    #[test]
    fn other_tokens_are_left_alone() {
        let mut r = Redactor::new([] as [&str; 0]);
        for text in [
            "",
            "changed the oil at 84,000 mi",
            // 16 and 18 characters.
            "JH4KA7650MC00000",
            "JH4KA7650MC0000033",
            // 17 characters, but I, O and Q are not VIN characters.
            "JH4KA7650MC00000I",
            "OH4KA7650MC000003",
            "JH4KA7650MQ000003",
            // A letter or digit on either side, ASCII or not.
            "xJH4KA7650MC000003",
            "JH4KA7650MC000003é",
            "éJH4KA7650MC000003",
            // An export hash (DR-0003 C6).
            "0000000000000000000000000000000000000000000000000000000000000000",
        ] {
            assert_eq!(r.redact(text), text);
        }
    }

    #[test]
    fn text_around_a_replacement_is_kept_byte_for_byte() {
        let mut r = garage();
        assert_eq!(
            r.redact("Ölwechsel für WBAXX00000T000001 – erledigt ✓"),
            "Ölwechsel für vehicle-1 – erledigt ✓"
        );
        assert_eq!(r.redact("ééWBAXX00000T000001éé"), "éévehicle-1éé");
    }

    #[test]
    fn stored_value_of_any_length_is_replaced() {
        // What the garage holds as a VIN need not have VIN shape.
        let mut r = Redactor::new(["  ab12345  ", "", "   "]);
        assert_eq!(r.redact("chassis AB12345, xAB12345y"), "chassis vehicle-1, xvehicle-1y");
        assert_eq!(r.redact("chassis ab12345"), "chassis vehicle-1");
        assert_eq!(r.redact("nothing here"), "nothing here");
    }

    #[test]
    fn overlapping_stored_values_are_replaced_together() {
        let mut r = Redactor::new(["ABCDE", "CDEFG", "ABCDEFGH"]);
        // The longest match at a position wins.
        assert_eq!(r.redact("x ABCDEFGH y"), "x vehicle-1 y");
        // Two stored values that overlap leave no part of either behind.
        assert_eq!(r.redact("x ABCDEFG y"), "x vehicle-2 y");
    }

    /// G8's residual, pinned so that a change to it is deliberate: these
    /// forms pass the filter.
    #[test]
    fn residual_forms_pass() {
        let mut r = garage();
        for text in ["WBAXX-00000-T000001", "WBAXX 00000 T000001", "last seven T000001"] {
            assert_eq!(r.redact(text), text);
        }
        // A mistyped VIN is caught only because it still has VIN shape.
        assert_eq!(r.redact("WBAXX00000T000007"), "vehicle-1");
        assert_eq!(r.redact("WBAXX00000T00007"), "WBAXX00000T00007");
    }
}
