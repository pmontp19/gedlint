//! Baseline (RFC 014 section 5): adopt gedlint on a legacy tree and ratchet
//! down. A baseline records the findings of a first run keyed by
//! `(code, message fingerprint)` plus a COUNT, never by line
//! number, so inserting lines cannot invalidate it. A run fails only on
//! findings beyond the recorded counts; recorded findings that disappeared
//! are "resolved" and `--write-baseline` prunes them.
//!
//! A baseline carries **no text from the file it was generated on** (issue
//! 61). The fingerprint is a digest, not the message: a baseline is a file
//! users are told to commit, and rule messages quote surnames, note bodies
//! and places. Matching only ever needs equality, and nothing reconstructs a
//! message from a baseline, so a digest costs nothing and closes the leak
//! for every rule at once, including rules not written yet.
//!
//! Everything here is pure: text in, structures out. All file I/O lives in
//! `src/main.rs`. The JSON reader is hand-rolled and accepts exactly the
//! shape `baseline_to_json` writes, nothing more; zero dependencies.

use std::collections::HashMap;

use crate::diag::{escape_json, Diag, Report};
use crate::hash::sha256_hex;

/// The file format version this module reads and writes. Version 2 is the
/// digest fingerprint; version 1 stored the normalized message text, so it
/// is refused rather than read, with a message saying to regenerate.
const FORMAT_VERSION: u32 = 2;

/// Digest algorithm tag on every fingerprint. Versioned separately from the
/// file format so a future algorithm is recognizable inside a file that has
/// not otherwise changed shape.
const FP_ALG: &str = "h1:";

/// Digest bytes kept, rendered as `2 * FP_BYTES` hex characters. 64 bits
/// puts the collision risk for a tree with a few thousand distinct findings
/// far below the odds of anything else in this program going wrong, and a
/// collision only ever merges two counters, which the surplus check still
/// reports.
const FP_BYTES: usize = 8;

/// Domain separation: what is hashed is this tag plus the normalized
/// message, so a fingerprint cannot be compared against a bare SHA-256 of
/// some candidate string computed elsewhere without knowing the recipe.
const FP_DOMAIN: &str = "gedlint-baseline-fingerprint-v1\u{1f}";

/// One recorded finding shape: rule code plus normalized message
/// fingerprint, and how many times it occurred when the baseline was
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEntry {
    pub code: String,
    pub fingerprint: String,
    pub count: u32,
}

/// A parsed baseline file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baseline {
    pub entries: Vec<BaselineEntry>,
}

/// Result of matching one run against a baseline.
#[derive(Debug, Clone, Default)]
pub struct BaselineOutcome {
    /// Findings the baseline could not absorb (counts exhausted or keys
    /// unknown). These alone drive the exit code.
    pub new_diags: Vec<Diag>,
    /// Findings absorbed by the baseline. Never affect the exit code.
    pub baselined: usize,
    /// Baseline entries this run did not hit at all: the findings were
    /// fixed. Reported as resolved; `--write-baseline` prunes them.
    pub resolved: Vec<BaselineEntry>,
    /// One bool per diagnostic of the matched report, in report order:
    /// true when the baseline absorbed it. `baselined` is the number of
    /// trues. The web viewer renders its new/seen split from this, so the
    /// browser can never disagree with the CLI about absorption.
    pub known_flags: Vec<bool>,
}

/// The fingerprint recorded for a message: `h1:` plus a truncated SHA-256
/// of its normalized form.
///
/// Two findings share a fingerprint exactly when [`normalize`] maps their
/// messages to the same string, which is what makes the baseline survive
/// line shifts. The digest is what keeps record content out of the file:
/// the input to it is built from the message, the output is not.
pub fn fingerprint(msg: &str) -> String {
    let mut input = String::with_capacity(FP_DOMAIN.len() + msg.len());
    input.push_str(FP_DOMAIN);
    input.push_str(&normalize(msg));
    let mut out = String::with_capacity(FP_ALG.len() + FP_BYTES * 2);
    out.push_str(FP_ALG);
    out.push_str(&sha256_hex(input.as_bytes(), FP_BYTES));
    out
}

/// True for a string shaped like a fingerprint this module writes. The
/// reader enforces it so a file carrying readable message text cannot be
/// passed off as a baseline, whatever wrote it.
fn is_fingerprint(fp: &str) -> bool {
    fp.len() == FP_ALG.len() + FP_BYTES * 2
        && fp.starts_with(FP_ALG)
        && fp[FP_ALG.len()..]
            .bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// Message normalization, the input to the digest: case-folded, whitespace
/// collapsed, every digit run collapsed to `#`. Digit collapsing is what
/// keeps messages that embed line references ("duplicate xref @F1@ (first
/// at line 42)") stable when lines shift; counts do the per-finding work,
/// so collapsing two digit-variants of a message into one key only ever
/// merges counters, and a surplus is still reported.
fn normalize(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut in_digits = false;
    let mut in_space = false;
    for c in msg.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
            in_space = false;
        } else if c.is_whitespace() {
            in_digits = false;
            if !in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = true;
        } else {
            in_digits = false;
            in_space = false;
            out.extend(c.to_lowercase());
        }
    }
    out.trim_end().to_string()
}

/// Build the baseline that covers every finding of this run (the
/// `--write-baseline` payload before serialization). Entries are sorted by
/// (code, fingerprint) so the output is byte-stable across runs.
pub fn baseline_from_report(report: &Report) -> Baseline {
    let mut counts: HashMap<(String, String), u32> = HashMap::new();
    for d in &report.diags {
        *counts
            .entry((d.code.to_string(), fingerprint(&d.msg)))
            .or_insert(0) += 1;
    }
    let mut entries: Vec<BaselineEntry> = counts
        .into_iter()
        .map(|((code, fingerprint), count)| BaselineEntry {
            code,
            fingerprint,
            count,
        })
        .collect();
    entries.sort_by(|a, b| a.code.cmp(&b.code).then(a.fingerprint.cmp(&b.fingerprint)));
    Baseline { entries }
}

/// Match a run against a baseline: consume counts in report order, collect
/// surplus findings as new, and report untouched entries as resolved.
pub fn apply_baseline(report: &Report, baseline: &Baseline) -> BaselineOutcome {
    let mut remaining: Vec<u32> = baseline.entries.iter().map(|e| e.count).collect();
    let mut index: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, e) in baseline.entries.iter().enumerate() {
        index.insert((e.code.as_str(), e.fingerprint.as_str()), i);
    }
    let mut out = BaselineOutcome {
        known_flags: Vec::with_capacity(report.diags.len()),
        ..BaselineOutcome::default()
    };
    for d in &report.diags {
        let fp = fingerprint(&d.msg);
        let mut known = false;
        if let Some(&i) = index.get(&(d.code, fp.as_str())) {
            if remaining[i] > 0 {
                remaining[i] -= 1;
                out.baselined += 1;
                known = true;
            }
        }
        out.known_flags.push(known);
        if !known {
            out.new_diags.push(d.clone());
        }
    }
    for (i, e) in baseline.entries.iter().enumerate() {
        if remaining[i] == e.count {
            out.resolved.push(e.clone());
        }
    }
    out
}

/// Serialize a baseline with the same hand-rolled style as `Report::to_json`
/// (shared `escape_json`). Deterministic: entries in stored order, one per
/// line.
pub fn baseline_to_json(b: &Baseline) -> String {
    let mut out = String::with_capacity(64 + b.entries.len() * 96);
    out.push_str("{\n  \"gedlint-baseline\": ");
    out.push_str(&FORMAT_VERSION.to_string());
    out.push_str(",\n  \"entries\": [");
    if b.entries.is_empty() {
        out.push_str("]\n}\n");
        return out;
    }
    out.push('\n');
    for (i, e) in b.entries.iter().enumerate() {
        out.push_str("    {\"code\": \"");
        out.push_str(&escape_json(&e.code));
        out.push_str("\", \"fingerprint\": \"");
        out.push_str(&escape_json(&e.fingerprint));
        out.push_str("\", \"count\": ");
        out.push_str(&e.count.to_string());
        out.push('}');
        if i + 1 < b.entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

/// Parse a baseline file. Accepts exactly the shape `baseline_to_json`
/// writes (whitespace anywhere between tokens): one object with
/// `"gedlint-baseline": 1` and an `"entries"` array of
/// `{code, fingerprint, count}` objects. Anything else is a clear error.
pub fn parse_baseline(text: &str) -> Result<Baseline, String> {
    let mut p = Parser {
        s: text,
        b: text.as_bytes(),
        i: 0,
    };
    p.ws();
    p.expect(b'{')?;
    let mut got_version = false;
    let mut got_entries = false;
    let mut entries: Vec<BaselineEntry> = Vec::new();
    p.ws();
    if !p.eat(b'}') {
        loop {
            p.ws();
            let key = p.string()?;
            p.ws();
            p.expect(b':')?;
            p.ws();
            match key.as_str() {
                "gedlint-baseline" => {
                    if got_version {
                        return Err(p.err("duplicate key \"gedlint-baseline\""));
                    }
                    let v = p.uint()?;
                    if v != FORMAT_VERSION {
                        // Version 1 recorded message text; it is not read,
                        // both because the keys differ and because the file
                        // it describes is one the user should replace.
                        let advice = if v < FORMAT_VERSION {
                            "regenerate it with --write-baseline (\"Save baseline\" in the \
                             web viewer)"
                        } else {
                            "it was written by a newer gedlint; upgrade gedlint"
                        };
                        return Err(p.err(&format!(
                            "unsupported baseline version {} (this build reads {}): {}",
                            v, FORMAT_VERSION, advice
                        )));
                    }
                    got_version = true;
                }
                "entries" => {
                    if got_entries {
                        return Err(p.err("duplicate key \"entries\""));
                    }
                    p.entries_array(&mut entries)?;
                    got_entries = true;
                }
                k => return Err(p.err(&format!("unexpected key \"{}\" in baseline file", k))),
            }
            p.ws();
            if p.eat(b',') {
                continue;
            }
            p.expect(b'}')?;
            break;
        }
    }
    p.ws();
    if p.i != p.b.len() {
        return Err(p.err("trailing content after the baseline object"));
    }
    if !got_version {
        return Err(p.err("missing key \"gedlint-baseline\""));
    }
    if !got_entries {
        return Err(p.err("missing key \"entries\""));
    }
    Ok(Baseline { entries })
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, m: &str) -> String {
        format!("baseline file: {} (at byte {})", m, self.i)
    }

    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", c as char)))
        }
    }

    fn uint(&mut self) -> Result<u32, String> {
        let start = self.i;
        while matches!(self.b.get(self.i), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
        }
        if start == self.i {
            return Err(self.err("expected a number"));
        }
        self.s[start..self.i]
            .parse::<u32>()
            .map_err(|_| self.err("number out of range"))
    }

    /// A JSON string literal with the escapes the serializer emits (plus the
    /// standard set). Input is a `&str`, so raw bytes are valid UTF-8.
    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let start = self.i;
        let mut escaped = false;
        loop {
            match self.b.get(self.i) {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => break,
                Some(b'\\') => {
                    escaped = true;
                    self.i += 2;
                }
                Some(c) if *c < 0x20 => return Err(self.err("control character in string")),
                Some(_) => self.i += 1,
            }
        }
        let raw = &self.s[start..self.i];
        self.i += 1; // closing quote
        if !escaped {
            return Ok(raw.to_string());
        }
        let mut out = String::with_capacity(raw.len());
        let mut ch = raw.chars();
        while let Some(c) = ch.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match ch.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('u') => out.push(self.unicode_escape(&mut ch)?),
                _ => return Err(self.err("invalid escape sequence")),
            }
        }
        Ok(out)
    }

    /// `\uXXXX`, including surrogate pairs; lone surrogates are an error.
    fn unicode_escape(&self, ch: &mut impl Iterator<Item = char>) -> Result<char, String> {
        let hi = self.hex4(ch)?;
        if (0xDC00..0xE000).contains(&hi) {
            return Err("baseline file: lone low surrogate in \\u escape".to_string());
        }
        if !(0xD800..0xDC00).contains(&hi) {
            return char::from_u32(hi as u32)
                .ok_or_else(|| "baseline file: invalid \\u escape".to_string());
        }
        if ch.next() != Some('\\') || ch.next() != Some('u') {
            return Err("baseline file: missing low surrogate".to_string());
        }
        let lo = self.hex4(ch)?;
        if !(0xDC00..0xE000).contains(&lo) {
            return Err("baseline file: missing low surrogate".to_string());
        }
        let c = 0x10000 + ((hi as u32 - 0xD800) << 10) + (lo as u32 - 0xDC00);
        char::from_u32(c).ok_or_else(|| "baseline file: invalid surrogate pair".to_string())
    }

    fn hex4(&self, ch: &mut impl Iterator<Item = char>) -> Result<u16, String> {
        let mut v: u16 = 0;
        for _ in 0..4 {
            let d = ch
                .next()
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| "baseline file: invalid \\u escape".to_string())?;
            v = v * 16 + d as u16;
        }
        Ok(v)
    }

    fn entries_array(&mut self, entries: &mut Vec<BaselineEntry>) -> Result<(), String> {
        self.expect(b'[')?;
        self.ws();
        if self.eat(b']') {
            return Ok(());
        }
        loop {
            self.ws();
            self.expect(b'{')?;
            let (mut code, mut fp) = (String::new(), String::new());
            let (mut count, mut have) = (0u32, (false, false, false));
            self.ws();
            if !self.eat(b'}') {
                loop {
                    self.ws();
                    let key = self.string()?;
                    self.ws();
                    self.expect(b':')?;
                    self.ws();
                    match key.as_str() {
                        "code" => {
                            if have.0 {
                                return Err(self.err("duplicate key \"code\""));
                            }
                            code = self.string()?;
                            have.0 = true;
                        }
                        "fingerprint" => {
                            if have.1 {
                                return Err(self.err("duplicate key \"fingerprint\""));
                            }
                            fp = self.string()?;
                            have.1 = true;
                        }
                        "count" => {
                            if have.2 {
                                return Err(self.err("duplicate key \"count\""));
                            }
                            count = self.uint()?;
                            have.2 = true;
                        }
                        k => {
                            return Err(
                                self.err(&format!("unexpected key \"{}\" in baseline entry", k))
                            )
                        }
                    }
                    self.ws();
                    if self.eat(b',') {
                        continue;
                    }
                    self.expect(b'}')?;
                    break;
                }
            }
            if !have.0 {
                return Err(self.err("baseline entry missing \"code\""));
            }
            if !have.1 {
                return Err(self.err("baseline entry missing \"fingerprint\""));
            }
            if !have.2 {
                return Err(self.err("baseline entry missing \"count\""));
            }
            if code.is_empty() {
                return Err(self.err("baseline entry has an empty \"code\""));
            }
            if fp.is_empty() {
                return Err(self.err("baseline entry has an empty \"fingerprint\""));
            }
            if !is_fingerprint(&fp) {
                return Err(self.err(
                    "baseline entry \"fingerprint\" is not a digest (expected \"h1:\" and 16 hex digits): regenerate the baseline with --write-baseline",
                ));
            }
            if count == 0 {
                return Err(self.err("baseline entry \"count\" must be at least 1"));
            }
            // Merge repeated keys by summing: a hand-edited file with two
            // identical entries behaves like one with the added count.
            if let Some(e) = entries
                .iter_mut()
                .find(|e| e.code == code && e.fingerprint == fp)
            {
                e.count += count;
            } else {
                entries.push(BaselineEntry {
                    code,
                    fingerprint: fp,
                    count,
                });
            }
            self.ws();
            if self.eat(b',') {
                continue;
            }
            self.expect(b']')?;
            break;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- normalization -----

    #[test]
    fn normalize_folds_case_whitespace_and_digits() {
        assert_eq!(normalize("Duplicate XREF @F1@"), "duplicate xref @f#@");
        assert_eq!(normalize("  a   b\t\tc\n"), "a b c");
        // Line references collapse away: shifting lines cannot change it.
        assert_eq!(
            normalize("first at line 42"),
            normalize("first at line 108")
        );
        assert_eq!(normalize("5.5.1"), "#.#.#");
        assert_eq!(normalize(""), "");
        // Non-ASCII survives (Catalan fixtures are real data).
        assert_eq!(normalize("Giron\u{e8} 2"), "giron\u{e8} #");
    }

    // ----- fingerprint -----

    #[test]
    fn fingerprint_is_a_digest_of_the_normalized_message() {
        let fp = fingerprint("all-caps surname: FERRER");
        assert!(is_fingerprint(&fp), "{}", fp);
        assert_eq!(fp.len(), "h1:".len() + 16);
        // Same normalization, same fingerprint: case, spacing and the line
        // numbers a message embeds cannot break a match.
        assert_eq!(fingerprint("SEX value Q"), fingerprint("sex   value  q"));
        assert_eq!(
            fingerprint("duplicate xref @F1@ (first at line 42)"),
            fingerprint("duplicate xref @F1@ (first at line 108)")
        );
        // Distinct findings keep distinct entries, so counts never collapse.
        assert_ne!(
            fingerprint("all-caps surname: FERRER"),
            fingerprint("all-caps surname: FERRES")
        );
        assert_ne!(fingerprint(""), fingerprint("a"));
        // Stable across runs: the file is committed and must not churn.
        assert_eq!(fingerprint("SEX value Q"), fingerprint("SEX value Q"));
    }

    /// Issue 61: rule messages quote surnames, note bodies and places, and a
    /// baseline is a file users commit. Nothing recognizable may survive.
    #[test]
    fn fingerprint_keeps_no_message_text() {
        for (msg, secret) in [
            ("all-caps surname: FERRER I PUIG", "ferrer"),
            (
                "NOTE with HTML (exporter quirk): <br>Maria was born in Olot",
                "maria",
            ),
            ("@I7@: invalid SEX value \"home\"", "home"),
            ("PLAC with URL (MyHeritage quirk): Sant Feliu", "feliu"),
        ] {
            let fp = fingerprint(msg).to_lowercase();
            assert!(
                !fp.contains(secret),
                "{} leaked {} into {}",
                msg,
                secret,
                fp
            );
            // Not a weaker claim than it looks: the digest is hex, so any
            // run of letters from the message would have to survive whole.
            assert!(is_fingerprint(&fp), "{}", fp);
        }
    }

    #[test]
    fn is_fingerprint_rejects_message_text() {
        assert!(is_fingerprint("h1:0123456789abcdef"));
        for bad in [
            "",
            "famc @f#@ points nowhere",
            "h1:",
            "h1:0123456789abcde",   // one hex digit short
            "h1:0123456789abcdef0", // one too many
            "h1:0123456789ABCDEF",  // uppercase hex is not what we write
            "h1:0123456789abcdeg",  // not hex
            "h2:0123456789abcdef",  // unknown algorithm
            "0123456789abcdef",     // no algorithm tag
        ] {
            assert!(!is_fingerprint(bad), "{} accepted", bad);
        }
    }

    // ----- JSON writer + reader -----
    //
    // These tests construct Baselines directly. Matching tests, which need
    // synthetic diagnostics, live in tests/baseline.rs: this module must
    // stay free of diagnostic construction so the registry completeness
    // scan (tests/registry.rs) sees only real engine emission sites.

    #[test]
    fn json_round_trip() {
        let b = Baseline {
            entries: vec![
                BaselineEntry {
                    code: "E201".into(),
                    fingerprint: fingerprint("FAMC @F9@ points nowhere"),
                    count: 2,
                },
                BaselineEntry {
                    code: "W302".into(),
                    fingerprint: fingerprint("possible duplicate (b. 1899)"),
                    count: 1,
                },
            ],
        };
        let parsed = parse_baseline(&baseline_to_json(&b)).unwrap();
        assert_eq!(parsed, b);
        // Equal content serializes byte-identically.
        let again = Baseline {
            entries: b.entries.clone(),
        };
        assert_eq!(baseline_to_json(&again), baseline_to_json(&b));
    }

    #[test]
    fn json_empty_baseline() {
        let b = Baseline::default();
        let parsed = parse_baseline(&baseline_to_json(&b)).unwrap();
        assert_eq!(parsed, b);
        assert!(baseline_to_json(&b).contains("\"entries\": []"));
    }

    // Escapes are exercised on "code", the one field with no shape rule:
    // a fingerprint is hex, so it can only carry \u escapes.
    #[test]
    fn json_reader_accepts_whitespace_and_escapes() {
        let text = "{\n  \"gedlint-baseline\" : 2 ,\n  \"entries\" : [\n    { \"code\" : \"quote \\\" back\\\\slash tab \\t \\u00e8\" , \"fingerprint\" : \"\\u0068\\u0031:0123456789abcdef\" , \"count\" : 3 }\n  ]\n}\n";
        let b = parse_baseline(text).unwrap();
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].code, "quote \" back\\slash tab \t \u{e8}");
        assert_eq!(b.entries[0].fingerprint, "h1:0123456789abcdef");
        assert_eq!(b.entries[0].count, 3);
    }

    #[test]
    fn json_reader_merges_duplicate_entries() {
        let text = "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":2},{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":3}]}";
        let b = parse_baseline(text).unwrap();
        assert_eq!(b.entries.len(), 1);
        assert_eq!(b.entries[0].count, 5);
    }

    #[test]
    fn json_reader_surrogate_pair() {
        let text = "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"a \\ud83d\\ude00 b\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1}]}";
        let b = parse_baseline(text).unwrap();
        assert_eq!(b.entries[0].code, "a \u{1f600} b");
    }

    /// Issue 61, acceptance: a version 1 file (readable message text) is
    /// refused with an instruction, never silently mismatched.
    #[test]
    fn json_reader_refuses_the_old_readable_format() {
        let v1 = "{\"gedlint-baseline\":1,\"entries\":[{\"code\":\"W702\",\"fingerprint\":\"all-caps surname: ferrer\",\"count\":1}]}";
        let err = parse_baseline(v1).unwrap_err();
        assert!(err.contains("unsupported baseline version 1"), "{}", err);
        assert!(err.contains("--write-baseline"), "{}", err);
        // A version 2 file carrying message text is refused too: the digest
        // shape is what the format promises, not just the version number.
        let faked = v1.replace("\"gedlint-baseline\":1", "\"gedlint-baseline\":2");
        let err = parse_baseline(&faked).unwrap_err();
        assert!(err.contains("is not a digest"), "{}", err);
        assert!(err.contains("--write-baseline"), "{}", err);
    }

    #[test]
    fn json_reader_rejects_garbage() {
        let good = |entries: &str| format!("{{\"gedlint-baseline\":2,\"entries\":[{}]}}", entries);
        let entry = |body: &str| {
            format!(
                "{{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1,{}}}",
                body
            )
        };
        let cases: Vec<(String, &str)> = vec![
            ("not json".into(), "expected '{'"),
            ("".into(), "expected '{'"),
            ("[]".into(), "expected '{'"),
            ("{}".into(), "missing key"),
            ("{\"entries\":[]}".into(), "missing key"),
            ("{\"gedlint-baseline\":1,\"entries\":[]}".into(), "unsupported baseline version"),
            ("{\"gedlint-baseline\":3,\"entries\":[]}".into(), "upgrade gedlint"),
            ("{\"gedlint-baseline\":2}".into(), "missing key"),
            ("{\"gedlint-baseline\":2,\"entries\":{}}".into(), "expected '['"),
            (good("[1]"), "expected '{'"),
            (good("{\"code\":\"E001\"}"), "missing"),
            (good("{\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1}"), "missing \"code\""),
            (good("{\"code\":\"E001\",\"count\":1}"), "missing \"fingerprint\""),
            (good("{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\"}"), "missing \"count\""),
            (good("{\"code\":\"\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1}"), "empty"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"\",\"count\":1}"), "empty"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"x\",\"count\":1}"), "is not a digest"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":0}"), "at least 1"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":-1}"), "expected a number"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":\"1\"}"), "expected a number"),
            (good("{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1,\"nope\":2}"), "unexpected key"),
            (good(&entry("\"zzz\":1")), "unexpected key"),
            (good(&entry("\"count\":2")), "duplicate key"),
            ("{\"gedlint-baseline\":2,\"gedlint-baseline\":2,\"entries\":[]}".to_string(), "duplicate key"),
            ("{\"gedlint-baseline\":2,\"entries\":[]} trailing".to_string(), "trailing content"),
            ("{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1}".into(), "expected ']'"),
            ("{\"gedlint-baseline\":2,\"entries\":[[{\"code\":\"E001\",\"fingerprint\":\"h1:0123456789abcdef\",\"count\":1}]}".into(), "expected '{'"),
        ];
        for (text, needle) in cases {
            let err = parse_baseline(&text).unwrap_err();
            assert!(err.contains("baseline file:"), "{} -> {}", text, err);
            assert!(
                err.contains(needle),
                "{} -> {} (want {})",
                text,
                err,
                needle
            );
        }
    }

    #[test]
    fn json_reader_rejects_broken_strings() {
        for text in [
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":E001,\"fingerprint\":\"x\",\"count\":1}]}",
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"E001,\"fingerprint\":\"x\",\"count\":1}]}",
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"a\\q\",\"fingerprint\":\"x\",\"count\":1}]}",
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"\\u00\",\"fingerprint\":\"x\",\"count\":1}]}",
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"\\ud83d\",\"fingerprint\":\"x\",\"count\":1}]}",
            "{\"gedlint-baseline\":2,\"entries\":[{\"code\":\"\\ude00\\ud83d\",\"fingerprint\":\"x\",\"count\":1}]}",
        ] {
            let err = parse_baseline(text).unwrap_err();
            assert!(err.contains("baseline file:"), "{} -> {}", text, err);
        }
    }
}
