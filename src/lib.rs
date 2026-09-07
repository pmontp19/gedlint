//! gedlint: GEDCOM linter engine (5.5.1 + 7.0).
//!
//! Design: streaming line-by-line parsing (`BufRead`) without loading the
//! whole tree. The core is pure (`&str` in, `Report` out) and therefore
//! compilable to WASM unchanged: the CLI binary is a thin layer (fs + args).
//! Zero dependencies.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::BufRead;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn tag(&self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN ",
            Severity::Info => "INFO ",
        }
    }

    pub fn parse(s: &str) -> Option<Severity> {
        match s.to_ascii_lowercase().as_str() {
            "error" | "errors" | "e" => Some(Severity::Error),
            "warning" | "warnings" | "warn" | "w" => Some(Severity::Warning),
            "info" | "i" => Some(Severity::Info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Correctness,
    Suspicious,
    Style,
    Upgrade,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Correctness => "correctness",
            Category::Suspicious => "suspicious",
            Category::Style => "style",
            Category::Upgrade => "upgrade",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V551,
    V70,
    Unknown,
}

impl Version {
    pub fn as_str(&self) -> &'static str {
        match self {
            Version::V551 => "5.5.1",
            Version::V70 => "7.0",
            Version::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub category: Category,
    pub severity: Severity,
    pub line: usize,
    pub msg: String,
}

impl Diag {
    fn new(code: &'static str, category: Category, severity: Severity, line: usize, msg: String) -> Diag {
        Diag { code, category, severity, line, msg }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub version: Version,
    pub diags: Vec<Diag>,
    pub lines: usize,
    pub individuals: usize,
    pub families: usize,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Warning).count()
    }
    pub fn infos(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Info).count()
    }
    pub fn exit_code(&self) -> i32 {
        if self.errors() > 0 {
            2
        } else if self.warnings() > 0 {
            1
        } else {
            0
        }
    }

    pub fn filtered(&self, min: Severity) -> Vec<&Diag> {
        self.diags.iter().filter(|d| d.severity >= min).collect()
    }

    /// Dependency-free JSON serialization (for CLI --format json and WASM).
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(self.diags.len() * 128);
        out.push_str("{\"version\":\"");
        out.push_str(self.version.as_str());
        out.push_str("\",\"lines\":");
        out.push_str(&self.lines.to_string());
        out.push_str(",\"individuals\":");
        out.push_str(&self.individuals.to_string());
        out.push_str(",\"families\":");
        out.push_str(&self.families.to_string());
        out.push_str(",\"summary\":{\"errors\":");
        out.push_str(&self.errors().to_string());
        out.push_str(",\"warnings\":");
        out.push_str(&self.warnings().to_string());
        out.push_str(",\"infos\":");
        out.push_str(&self.infos().to_string());
        out.push_str("},\"diagnostics\":[");
        for (i, d) in self.diags.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"code\":\"");
            out.push_str(d.code);
            out.push_str("\",\"category\":\"");
            out.push_str(d.category.as_str());
            out.push_str("\",\"severity\":\"");
            out.push_str(d.severity.tag().trim());
            out.push_str("\",\"line\":");
            out.push_str(&d.line.to_string());
            out.push_str(",\"message\":\"");
            out.push_str(&escape_json(&d.msg));
            out.push_str("\"}");
        }
        out.push_str("]}");
        out
    }
}

fn escape_json(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

// ---------------------------------------------------------------------------
// Pure public API (reusable from WASM): text in, report out.
// ---------------------------------------------------------------------------

/// Lint already-read GEDCOM text. Pure core, WASM-suitable.
pub fn lint_str(input: &str) -> Report {
    lint_bytes_split(input.as_bytes(), true)
}

/// Lint raw bytes (detects UTF-8 / BOM / CRLF before decoding).
pub fn lint_bytes(data: &[u8]) -> Report {
    lint_bytes_split(data, false)
}

fn lint_bytes_split(data: &[u8], _already_str: bool) -> Report {
    let data = normalize_newlines(data);
    let text = String::from_utf8_lossy(&data).into_owned();
    let (version, charset) = scan_head(&text);
    let diags: Vec<Diag> = encoding_diags(&data, version, charset.as_deref());

    let mut r = lint_lines(&text);
    // Encoding diags go first (low line numbers), then semantic ones.
    let mut all = diags;
    all.append(&mut r.diags);
    all.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    r.diags = all;
    // The byte-level pre-scan and the line parser must agree on version.
    if r.version == Version::Unknown {
        r.version = version;
    }
    r
}

/// Bare CR is a legal line terminator in 5.5.1 (classic Mac; the original
/// TGC551.ged uses it): normalize lone CR to LF, keep CRLF untouched.
/// Borrows the input when there is nothing to do (the common case).
fn normalize_newlines(data: &[u8]) -> Cow<'_, [u8]> {
    if !data.contains(&b'\r') {
        return Cow::Borrowed(data);
    }
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'\r' if data.get(i + 1) == Some(&b'\n') => {
                out.push(b'\r');
                out.push(b'\n');
                i += 2;
            }
            b'\r' => {
                out.push(b'\n');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// Cheap pre-scan of HEAD for version (HEAD.GEDC.VERS) and declared
/// charset (HEAD.CHAR), so byte-level encoding rules can adapt
/// (ANSEL files are not UTF-8; a BOM is recommended by GEDCOM 7).
fn scan_head(text: &str) -> (Version, Option<String>) {
    let mut version = Version::Unknown;
    let mut charset: Option<String> = None;
    let mut in_head = false;
    let mut in_gedc = false;
    let body = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    for raw in body.lines() {
        let l = parse_line(0, raw);
        let Some(lvl) = l.level else { continue };
        if lvl == 0 {
            in_head = l.tag == "HEAD";
            in_gedc = false;
        } else if in_head && lvl == 1 {
            in_gedc = l.tag == "GEDC";
            if l.tag == "CHAR" {
                charset = Some(l.value.trim().to_ascii_uppercase());
            }
        } else if in_head && in_gedc && lvl == 2 && l.tag == "VERS" {
            let v = l.value.trim();
            if v.starts_with("5.5") {
                version = Version::V551;
            } else if v.starts_with("7") {
                version = Version::V70;
            }
        }
    }
    (version, charset)
}

/// Streaming input: reads line by line without loading everything at once.
/// Useful for 100MB+ exports. Internally it delegates to `lint_lines` for
/// simplicity, but the contract is `BufRead`.
pub fn lint_reader<R: BufRead>(mut reader: R) -> Report {
    let mut buf = Vec::new();
    let mut chunk = Vec::new();
    // Read in chunks and append: O(n) memory in bytes but O(1) in objects.
    loop {
        chunk.clear();
        match reader.read_until(b'\n', &mut chunk) {
            Ok(0) => break,
            Ok(_) => buf.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    lint_bytes(&buf)
}

// ---------------------------------------------------------------------------
// Encoding diags (byte level, before the parser)
// ---------------------------------------------------------------------------

/// No global cap during collection: a real file (516 `_UPD`)
/// must not hide errors behind infos. The limit applies at output time
/// via `--max N` (0 = unlimited). The name is kept to avoid touching
/// every call site.
fn push_capped(dst: &mut Vec<Diag>, mut v: Vec<Diag>) {
    dst.append(&mut v);
}

/// E101/W102: UTF-8, BOM, continuation byte at line start (MyHeritage bug
/// splitting multibyte sequences across CONC lines), mixed CRLF, controls.
/// `charset` is the declared HEAD.CHAR: ANSEL/ASCII files are not UTF-8,
/// so E101 does not apply to them. A BOM is recommended by GEDCOM 7
/// (spec 1.1) and only warned about otherwise.
fn encoding_diags(data: &[u8], version: Version, charset: Option<&str>) -> Vec<Diag> {
    let mut out = Vec::new();
    let non_utf8 = matches!(charset, Some("ANSEL") | Some("ASCII") | Some("IBMPC") | Some("MACINTOSH"));
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) && version != Version::V70 {
        out.push(Diag::new(
            "W102",
            Category::Style,
            Severity::Warning,
            1,
            "UTF-8 BOM at start (GEDCOM 5.5.x tools may choke on it; 7.0 recommends it)".into(),
        ));
    }
    let has_crlf = data.windows(2).any(|w| w == b"\r\n");
    let has_lone_lf = {
        let mut prev_cr = false;
        let mut found = false;
        for &b in data {
            if b == b'\n' && !prev_cr {
                found = true;
                break;
            }
            prev_cr = b == b'\r';
        }
        found
    };
    // `data` arrives CR-normalized, so lone CRs no longer exist here: any
    // lone LF next to a CRLF means the file mixes terminator styles
    // (including a classic-Mac CR section, now normalized to LF).
    if has_crlf && has_lone_lf {
        out.push(Diag::new(
            "W102",
            Category::Style,
            Severity::Warning,
            0,
            "mixed line endings (CRLF and LF; normalize to a single style)".into(),
        ));
    }

    // Lines starting with a UTF-8 continuation byte (0x80..=0xBF):
    // symptom of the MyHeritage bug (character split across CONC).
    // Skipped for declared single-byte encodings (ANSEL et al).
    let mut bad = 0usize;
    let mut first = 0usize;
    if !non_utf8 {
        for (i, line) in data.split(|&b| b == b'\n').enumerate() {
            let l = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
            // Skip the "N CONC ..." header: the useful content starts after it.
            let payload = conc_payload(l);
            if payload.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) {
                bad += 1;
                if first == 0 {
                    first = i + 1;
                }
            }
        }
    }
    if bad > 0 {
        out.push(Diag::new(
            "E101",
            Category::Correctness,
            Severity::Error,
            first,
            format!(
                "{} lines start with a UTF-8 continuation byte (character split across CONC lines, MyHeritage bug; try --fix)",
                bad
            ),
        ));
    }
    if std::str::from_utf8(data).is_err() && bad == 0 && !non_utf8 {
        out.push(Diag::new(
            "E101",
            Category::Correctness,
            Severity::Error,
            0,
            "the file is not valid UTF-8".into(),
        ));
    }
    out
}

/// Returns the payload after "N CONC " if present, else the whole line.
fn conc_payload(line: &[u8]) -> &[u8] {
    // Find " CONC " at byte level.
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        &line[p + pat.len()..]
    } else if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let rest = &line[p + 4..];
        let rest = if rest.first() == Some(&b' ') { &rest[1..] } else { rest };
        rest
    } else {
        line
    }
}

// ---------------------------------------------------------------------------
// GEDCOM 7 enumeration sets (exact spellings from the spec registries).
// Lowercase variants are the 5.5.1 spellings where they differ.
const ROLE: &[&str] = &[
    "CHIL", "CLERGY", "FATH", "FRIEND", "GODP", "HUSB", "MOTH", "MULTIPLE", "NGHBR",
    "OFFICIATOR", "PARENT", "SPOU", "WIFE", "WITN", "OTHER",
];
// 5.5.1 has no ASSO.ROLE; its ROLE belongs to the source citation
// (SOUR.EVEN.ROLE) and has a different, smaller set.
const ROLE551: &[&str] = &["chil", "husb", "wife", "moth", "fath", "spou"];
const PEDI70: &[&str] = &["ADOPTED", "BIRTH", "FOSTER", "SEALING", "OTHER"];
const PEDI551: &[&str] = &["adopted", "birth", "foster", "sealing", "other"];
const QUAY: &[&str] = &["0", "1", "2", "3"];
const RESN70: &[&str] = &["CONFIDENTIAL", "LOCKED", "PRIVACY"];
const RESN551: &[&str] = &["confidential", "locked", "privacy"];
const FAMC_STAT: &[&str] = &["CHALLENGED", "DISPROVEN", "PROVEN"];
const NAME_TYPE70: &[&str] = &["AKA", "BIRTH", "IMMIGRANT", "MAIDEN", "MARRIED", "OTHER", "PROFESSIONAL"];
const NAME_TYPE551: &[&str] = &["aka", "birth", "immigrant", "maiden", "married", "other", "professional"];
const MEDI551: &[&str] = &[
    "AUDIO", "BOOK", "CARD", "ELECTRONIC", "FICHE", "FILM", "MAGAZINE", "MANUSCRIPT",
    "MAP", "NEWSPAPER", "OTHER", "PHOTO", "TOMBSTONE", "VIDEO",
];
// Common individual/family events whose detail singletons are {0:1}.
const EVENT_TAGS: &[&str] = &[
    "BIRT", "CHR", "DEAT", "BURI", "MARR", "DIV", "OCCU", "RESI", "EVEN", "FACT",
    "CENS", "EMIG", "IMMI", "GRAD", "RETI", "BAPM", "CONF",
];
const EVENT_SINGLETONS: &[&str] = &[
    "DATE", "PLAC", "ADDR", "AGNC", "CAUS", "RELI", "RESN", "TYPE", "AGE", "SDATE", "PAGE",
];
// LDS ordinances whose STAT is enumerated (spec TSV has PRE_1970/DNS_CAN;
// the human page shows PRE: accept all three spellings).
const LDS_EVENTS: &[&str] = &["BAPL", "CONL", "ENDL", "SLGC", "SLGS", "INIL"];
const ORD_STAT: &[&str] = &[
    "BIC", "CANCELED", "CHILD", "COMPLETED", "DNS", "DNS_CAN", "EXCLUDED", "INFANT",
    "PRE", "PRE_1970", "STILLBORN", "SUBMITTED", "UNCLEARED",
];
// SOUR.DATA.EVEN payloads (7.0): type-List#Enum of event/attribute tags
// (spec: "a parish register of births, deaths, and marriages would be
// BIRT, DEAT, MARR").
const EVENATTR: &[&str] = &[
    "ADOP", "ANUL", "BAPM", "BARM", "BASM", "BIRT", "BLES", "BURI", "CAST", "CENS", "CHR",
    "CHRA", "CONF", "CREM", "DEAT", "DIV", "DIVF", "DSCR", "EDUC", "EMIG", "ENGA", "EVEN",
    "FACT", "FCOM", "GRAD", "IDNO", "IMMI", "MARB", "MARC", "MARL", "MARR", "MARS", "NATI",
    "NATU", "NCHI", "OCCU", "ORDN", "PROB", "PROP", "RELI", "RESI", "RETI", "SSN", "TITL",
    "WILL",
];
// W307 only watches events that are semantically single: one birth, one
// death. Repeatable attributes (OCCU/RESI/CENS/EVEN/FACT) accumulate new
// instances naturally, so differing dates there are not a merge leftover.
const CONFLICT_EVENTS: &[&str] = &["BIRT", "CHR", "DEAT", "BURI", "MARR", "DIV", "BAPM", "CONF"];

// (record, event, instance) key shared by the E008/E009/W307 trackers.
type EvKey = (String, String, usize);
// (instance, DATE value, line) hit inside one record+event group.
type EvHit = (usize, String, usize);

// Line parser + rules
// ---------------------------------------------------------------------------

struct Line {
    no: usize,
    level: Option<u32>,
    xref: String,
    tag: String,
    value: String,
    raw: String,
}

/// Deepest legal level (5.5.1 ch. 1). A bigger leading number is text, not a
/// level: a biography continuation line may start with a year ("1936 va ...").
const MAX_LEVEL: u32 = 99;

fn parse_line(no: usize, raw: &str) -> Line {
    // Grammar: LEVEL [XREF] TAG [VALUE]. XREF only at level 0.
    let mut it = raw.splitn(3, char::is_whitespace);
    let lvl: Option<u32> = it.next().and_then(|x| x.parse().ok()).filter(|n| *n <= MAX_LEVEL);
    let second = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    let (xref, tag, value) = if second.starts_with('@') && second.ends_with('@') && second.len() >= 3 {
        // "0 @I1@ INDI ...": the tag is the first word of rest.
        let (t, v) = match rest.split_once(' ') {
            Some((t, v)) => (t, v.trim()),
            None => (rest, ""),
        };
        (second.to_string(), t.to_string(), v.to_string())
    } else if second.starts_with('@') {
        // Malformed xref (unclosed): still record it as xref so E004 catches it.
        let (t, v) = match rest.split_once(' ') {
            Some((t, v)) => (t, v.trim()),
            None => (rest, ""),
        };
        (second.to_string(), t.to_string(), v.to_string())
    } else {
        (String::new(), second.to_string(), rest.trim().to_string())
    };
    Line { no, level: lvl, xref, tag, value, raw: raw.to_string() }
}

fn is_pointer(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 3 && t.starts_with('@') && t.ends_with('@') && !t[1..t.len() - 1].contains(char::is_whitespace)
}

fn inner_ptr(s: &str) -> &str {
    s.trim()
}

fn year_of(s: &str) -> Option<i64> {
    // First 3-4 digit year (GEDCOM dates: "12 SEP 1909", "BEF 1900"...).
    let mut best: Option<i64> = None;
    for tok in s.split(|c: char| !c.is_ascii_digit()) {
        if (3..=4).contains(&tok.len()) {
            if let Ok(y) = tok.parse::<i64>() {
                if (100..=2100).contains(&y) {
                    best = Some(y);
                    break;
                }
            }
        }
    }
    best
}

fn norm_name(s: &str) -> String {
    s.to_lowercase().replace('/', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(s: &str, n: usize) -> String {
    // Cut on char boundaries: &s[..n] panics when n splits a multibyte char
    // (e.g. a long Catalan PLAC with a URL near byte 60).
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

/// W306: enumerated values (ROLE/PEDI/QUAY/RESN/STAT/TYPE/MEDI) against the
/// 7.0 registry spellings (lowercase variants under 5.5.1). OTHER values
/// are registered for the sibling-PHRASE check flushed at end of run.
#[allow(clippy::too_many_arguments)]
fn check_enum(
    diags: &mut Vec<Diag>,
    tag: &str,
    value: &str,
    parent_tag: &str,
    record: &str,
    parent_key: Option<(String, usize)>,
    pending_other: &mut Vec<(String, String, usize, String, usize)>,
    version: Version,
    line: usize,
) {
    let v = value.trim();
    if v.is_empty() {
        return;
    }
    // OBJE.FILE.FORM under 7.0 is a media type, not an enum.
    if tag == "FORM" && parent_tag == "FILE" && version == Version::V70 {
        if !v.contains('/') || v.chars().any(char::is_whitespace) {
            let who = if record.is_empty() { format!("line {}", line) } else { record.to_string() };
            push_capped(
                diags,
                vec![Diag::new(
                    "W306",
                    Category::Suspicious,
                    Severity::Warning,
                    line,
                    format!("{}: invalid media type {:?} (use image/jpeg etc.)", who, v),
                )],
            );
        }
        return;
    }
    let set: Option<(&[&str], &str)> = match tag {
        "ROLE" => {
            if version == Version::V70 {
                Some((ROLE, "ASSO.ROLE"))
            } else {
                Some((ROLE551, "ROLE"))
            }
        }
        "PEDI" => Some((
            if version == Version::V70 { PEDI70 } else { PEDI551 },
            "FAMC.PEDI",
        )),
        "QUAY" => Some((QUAY, "SOUR.QUAY")),
        "RESN" => Some((
            if version == Version::V70 { RESN70 } else { RESN551 },
            "RESN",
        )),
        "STAT" if parent_tag == "FAMC" && version != Version::V551 => {
            Some((FAMC_STAT, "FAMC.STAT"))
        }
        "STAT" if LDS_EVENTS.contains(&parent_tag) && version != Version::V551 => {
            Some((ORD_STAT, "LDS.STAT"))
        }
        "EVEN" if parent_tag == "DATA" && version == Version::V70 => {
            Some((EVENATTR, "DATA.EVEN"))
        }
        "TYPE" if parent_tag == "NAME" => Some((
            if version == Version::V70 { NAME_TYPE70 } else { NAME_TYPE551 },
            "NAME.TYPE",
        )),
        "MEDI" if version != Version::V70 => Some((MEDI551, "FILE.FORM.MEDI")),
        _ => None,
    };
    let Some((allowed, what)) = set else { return };
    // 7.0 RESN and SOUR.DATA.EVEN payloads are type-List#Enum: a
    // comma-separated list of enum values, each valid on its own
    // (maximal70.ged: "RESN CONFIDENTIAL, LOCKED", "EVEN BIRT, DEAT").
    if version == Version::V70 && (tag == "RESN" || (tag == "EVEN" && parent_tag == "DATA")) {
        let all_ok = v.split(',').all(|t| {
            // Empty tokens (trailing comma, "A, ,B") are tolerated:
            // exporter quirk, the meaningful tokens still get validated.
            let t = t.trim();
            t.is_empty() || allowed.contains(&t)
        });
        if all_ok {
            return;
        }
        let who = if record.is_empty() { format!("line {}", line) } else { record.to_string() };
        push_capped(
            diags,
            vec![Diag::new(
                "W306",
                Category::Suspicious,
                Severity::Warning,
                line,
                format!("{}: invalid {} value {:?} (expected: {})", who, what, v, allowed.join("|")),
            )],
        );
        return;
    }
    // 5.5.1 enum checks accept any case: the spec spells them lowercase but
    // commercial exporters capitalize ("TYPE Birth", "PEDI ADOPTED").
    let ci = version == Version::V551;
    let hit = if ci {
        allowed.iter().any(|s| s.eq_ignore_ascii_case(v))
    } else {
        allowed.contains(&v)
    };
    if hit {
        let is_other = if ci { v.eq_ignore_ascii_case("other") } else { v == "OTHER" };
        if (tag == "ROLE" || tag == "PEDI" || tag == "TYPE") && is_other {
            if let Some((ptag, pline)) = parent_key {
                pending_other.push((record.to_string(), ptag, pline, tag.to_string(), line));
            }
        }
        return;
    }
    let who = if record.is_empty() { format!("line {}", line) } else { record.to_string() };
    push_capped(
        diags,
        vec![Diag::new(
            "W306",
            Category::Suspicious,
            Severity::Warning,
            line,
            format!("{}: invalid {} value {:?} (expected: {})", who, what, v, allowed.join("|")),
        )],
    );
}

fn lint_lines(text: &str) -> Report {
    // The BOM is not part of the grammar: it is reported in encoding_diags
    // and stripped so "0 HEAD" on the first line is recognized.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut diags: Vec<Diag> = Vec::new();
    let raw_lines: Vec<&str> = text.lines().collect();
    let lines: Vec<Line> = raw_lines.iter().enumerate().map(|(i, l)| parse_line(i + 1, l)).collect();

    // Version state (HEAD.GEDC.VERS).
    let mut version = Version::Unknown;
    let mut in_head = false;
    let mut in_gedc = false;
    for l in &lines {
        if l.level == Some(0) {
            in_head = l.tag == "HEAD";
            in_gedc = false;
        } else if in_head && l.level == Some(1) {
            in_gedc = l.tag == "GEDC";
        } else if in_head && in_gedc && l.level == Some(2) && l.tag == "VERS" {
            let v = l.value.trim();
            if v.starts_with("5.5") {
                version = Version::V551;
            } else if v.starts_with("7") {
                version = Version::V70;
            }
        }
    }

    // Indexes.
    let mut records: HashMap<String, (String, usize)> = HashMap::new(); // xref -> (kind, line)
    let mut indi_birth: HashMap<String, Option<i64>> = HashMap::new();
    let mut indi_death: HashMap<String, Option<i64>> = HashMap::new();
    // Dead with unknown date (DEAT Y without DATE): counts as dead
    // but stays out of W301 (no year, no longevity to check).
    let mut indi_died_unknown: HashSet<String> = HashSet::new();
    let mut indi_name: HashMap<String, String> = HashMap::new();
    let mut indi_sex: HashMap<String, (String, usize)> = HashMap::new();
    let mut indi_famc: HashMap<String, Vec<String>> = HashMap::new();
    let mut fam_chil: HashMap<String, Vec<String>> = HashMap::new();
    let mut fam_husb: HashMap<String, (String, usize)> = HashMap::new();
    let mut fam_wife: HashMap<String, (String, usize)> = HashMap::new();
    let mut fam_marr: HashMap<String, Option<i64>> = HashMap::new();
    let mut pending: Vec<(usize, String, String, String)> = Vec::new(); // (line, from, tag, target)

    let mut cur: Option<(String, String)> = None; // (xref, kind)
    let mut cur_sub = String::new();
    let mut saw_head = false;
    let mut saw_trlr = false;
    let mut first_done = false;
    let mut after_trlr = false;
    let mut prev_level: Option<u32> = None;
    let mut expect_conc_parent = false;
    // Parent stack for context-sensitive rules (enums, OTHER/PHRASE):
    // stack[i] is the tag/line of the nearest preceding line at level i.
    let mut stack: Vec<(String, usize)> = Vec::new();
    // E008 duplicate singletons: SEX/HUSB/WIFE counted via their maps below;
    // HEAD children tracked here.
    let mut saw_gedc_line: Option<usize> = None;
    let mut saw_vers_line: Option<usize> = None;
    let mut in_head_main = false;
    // E008 VERS singletons inside HEAD, keyed by the parent's line: GEDC.VERS
    // (GEDCOM version), SOUR.VERS (product version) and CHAR.VERS are three
    // different {0:1} slots, not one.
    let mut head_vers_seen: HashMap<usize, usize> = HashMap::new();
    // OTHER values awaiting a sibling PHRASE: (record, parent_tag, parent_line, tag, line).
    let mut pending_other: Vec<(String, String, usize, String, usize)> = Vec::new();
    let mut phrased: HashSet<(String, String, usize)> = HashSet::new();
    // E008 event-detail singletons: (record, event, instance, sub) -> first line.
    // The instance matters: INDI.BIRT is {0:M}, so two BIRT blocks may each
    // carry one DATE; only two DATEs under the SAME block are duplicates.
    let mut event_seen: HashMap<(String, String, usize, String), usize> = HashMap::new();
    let mut event_inst: HashMap<(String, String), usize> = HashMap::new();
    // Current event instance + the level it opened at: deeper levels
    // (SOUR.DATA.DATE) belong to other structures, not to the event.
    let mut cur_event: Option<(String, usize, u32)> = None;
    // W307 conflicting duplicate events: EvKey -> (DATE, line).
    let mut event_dates: HashMap<EvKey, (String, usize)> = HashMap::new();
    // DIV and MARR event lines per record: a MARR pair separated by a DIV
    // is a remarriage and a DIV pair separated by a MARR is a serial
    // divorce; the 7.0 spec allows a single FAM to hold multiple MARR/DIV.
    let mut div_lines: HashMap<String, Vec<usize>> = HashMap::new();
    let mut marr_lines: HashMap<String, Vec<usize>> = HashMap::new();
    // W402 slash balance must see the whole NAME value: a surname split
    // across CONC lines (Long26CC) is balanced as a whole but odd per line.
    let mut name_buf: Option<(String, usize, String)> = None;
    // E009 EVEN/FACT without TYPE (7.0 only): EvKey line + typed set.
    let mut ef_inst: HashMap<(String, String), usize> = HashMap::new();
    let mut ef_line: HashMap<EvKey, usize> = HashMap::new();
    let mut ef_typed: HashSet<EvKey> = HashSet::new();
    // E009 LDS STAT without DATE (7.0 only): (record, event, stat line).
    let mut lds_stat: Vec<(String, String, usize)> = Vec::new();

    // W402 check on the accumulated NAME value (NAME + CONC/CONT run).
    let flush_name = |diags: &mut Vec<Diag>,
                      buf: &mut Option<(String, usize, String)>,
                      names: &mut HashMap<String, String>| {
        if let Some((xref, line, val)) = buf.take() {
            names.insert(xref.clone(), val.clone());
            if val.matches('/').count() % 2 != 0 {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "W402",
                        Category::Style,
                        Severity::Warning,
                        line,
                        format!("{}: NAME with unbalanced slashes: {}", xref, truncate(&val, 50)),
                    )],
                );
            }
        }
    };

    let flush_person = |diags: &mut Vec<Diag>, xref: &str, b: Option<i64>, d: Option<i64>, line: usize| {
        if let (Some(bb), Some(dd)) = (b, d) {
            if dd < 10000 && bb < 10000 && dd < bb {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "W301",
                        Category::Suspicious,
                        Severity::Warning,
                        line,
                        format!("{}: died ({}) before being born ({})", xref, dd, bb),
                    )],
                );
            }
            if dd < 10000 && dd - bb > 105 {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "W301",
                        Category::Suspicious,
                        Severity::Warning,
                        line,
                        format!("{}: {} - {} = {} years, please verify", xref, bb, dd, dd - bb),
                    )],
                );
            }
        }
    };

    for l in &lines {
        let Some(lvl) = l.level else {
            // A blank line has no level and no content: not a malformed line
            // (E002 already treats blanks as non-content). prev_level survives
            // so a level jump across a blank line is still caught.
            if l.raw.trim().is_empty() {
                continue;
            }
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E001",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("malformed line (non-numeric level): {}", truncate(&l.raw, 60)),
                )],
            );
            prev_level = None;
            continue;
        };
        // A NAME run ends here: any level <= 1 line closes the CONC/CONT run.
        if lvl <= 1 {
            flush_name(&mut diags, &mut name_buf, &mut indi_name);
        }
        // E002: HEAD must be the first line; nothing may follow TRLR.
        if !l.raw.trim().is_empty() {
            if !first_done {
                first_done = true;
                if !(lvl == 0 && l.tag == "HEAD") {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "E002",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            "HEAD must be the first line".into(),
                        )],
                    );
                }
            } else if after_trlr {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "E002",
                        Category::Correctness,
                        Severity::Error,
                        l.no,
                        format!("content after TRLR: {}", truncate(&l.raw, 50)),
                    )],
                );
            }
        }
        // E001: level jump > +1.
        if let Some(p) = prev_level {
            if lvl > p + 1 {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "E001",
                        Category::Correctness,
                        Severity::Error,
                        l.no,
                        format!("level jump {} -> {} (max +1)", p, lvl),
                    )],
                );
            }
        }
        prev_level = Some(lvl);
        // Parent stack: truncate to the current level, read the parent
        // (nearest preceding line one level up), then push this line.
        while stack.len() > lvl as usize {
            stack.pop();
        }
        let parent: Option<(String, usize)> = stack.last().cloned();
        stack.push((l.tag.clone(), l.no));
        let parent_tag: &str = parent.as_ref().map(|p| p.0.as_str()).unwrap_or("");

        // E004: xref syntax (@id@, no spaces, closed).
        if !l.xref.is_empty() && !is_pointer(&l.xref) {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E004",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("malformed xref: {}", l.xref),
                )],
            );
        }

        // CONT/CONC must hang off a parent level.
        if (l.tag == "CONT" || l.tag == "CONC") && !expect_conc_parent {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E005",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("{} with no parent line (must continue a value)", l.tag),
                )],
            );
        }
        // E007: CONC was removed in 7.0 (spec 1.3, reserved tag): reflow to CONT.
        if version == Version::V70 && l.tag == "CONC" {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E007",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    "CONC is reserved in 7.0 (spec 1.3): split the value into CONT lines".into(),
                )],
            );
        }
        expect_conc_parent = !l.raw.trim().is_empty();

        // ASCII controls (other than \t).
        if l.raw.chars().any(|c| c.is_control() && c != '\t') {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W102",
                    Category::Style,
                    Severity::Warning,
                    l.no,
                    "control character inside line".into(),
                )],
            );
        }

        if lvl == 0 {
            if let Some((xref, _)) = cur.take() {
                let _ = xref;
            }
            // Birth/death resolve at record change via the already stored maps.
            cur_sub.clear();
            cur_event = None;
            // HEAD scope for E008 (GEDC/VERS singletons) and E009.
            in_head_main = l.tag == "HEAD";
            if l.tag == "HEAD" {
                saw_head = true;
            }
            if l.tag == "TRLR" {
                saw_trlr = true;
                after_trlr = true;
            }
            if !l.xref.is_empty() {
                if let Some((_, first_line)) = records.get(&l.xref) {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "E003",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            format!("duplicate xref {} (first at line {})", l.xref, first_line),
                        )],
                    );
                } else {
                    records.insert(l.xref.clone(), (l.tag.clone(), l.no));
                    match l.tag.as_str() {
                        "INDI" => {
                            indi_birth.insert(l.xref.clone(), None);
                            indi_death.insert(l.xref.clone(), None);
                            cur = Some((l.xref.clone(), "INDI".into()));
                        }
                        "FAM" => {
                            cur = Some((l.xref.clone(), "FAM".into()));
                        }
                        _ => {
                            cur = Some((l.xref.clone(), l.tag.clone()));
                        }
                    }
                }
            } else if l.tag != "HEAD" && l.tag != "TRLR" && l.tag != "SUBM" && l.tag != "SUBN" {
                // Level-0 records without xref (except HEAD/TRLR) are suspicious.
                if !l.tag.is_empty() && l.tag.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    // Tag sol al nivell 0: p. ex. "0 @X@ OBJE" ja cobert; altrament ho deixem passar.
                }
            }
            continue;
        }

        if lvl == 1 {
            cur_sub = l.tag.clone();
            cur_event = None;
            // HEAD.CHAR: removed in 7.0 (UTF-8 assumed); 5.5.1 has 4 legal values.
            if in_head_main && l.tag == "CHAR" {
                if version == Version::V70 {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "U501",
                            Category::Upgrade,
                            Severity::Info,
                            l.no,
                            "CHAR removed in 7.0 (UTF-8 is assumed)".into(),
                        )],
                    );
                } else {
                    const CHAR551: &[&str] = &["ANSEL", "ASCII", "UNICODE", "UTF-8"];
                    if !CHAR551.contains(&l.value.trim().to_ascii_uppercase().as_str()) {
                        push_capped(
                            &mut diags,
                            vec![Diag::new(
                                "W306",
                                Category::Suspicious,
                                Severity::Warning,
                                l.no,
                                format!(
                                    "invalid HEAD.CHAR {:?} (expected ANSEL/ASCII/UNICODE/UTF-8)",
                                    l.value.trim()
                                ),
                            )],
                        );
                    }
                }
            }
            // E008: HEAD.GEDC is a {1:1} singleton.
            if in_head_main && l.tag == "GEDC" {
                if let Some(first) = saw_gedc_line {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "E008",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            format!("duplicate HEAD.GEDC (first at line {})", first),
                        )],
                    );
                } else {
                    saw_gedc_line = Some(l.no);
                }
            }
            if let Some((xref, kind)) = cur.clone() {
                // Event instance counter for E008 (BIRT is {0:M}: each block
                // gets its own number so cross-block DATEs are not dups).
                if EVENT_TAGS.contains(&l.tag.as_str()) {
                    let key = (xref.clone(), l.tag.clone());
                    let n = event_inst.get(&key).copied().unwrap_or(0) + 1;
                    event_inst.insert(key, n);
                    cur_event = Some((l.tag.clone(), n, lvl));
                    if l.tag == "DIV" {
                        div_lines.entry(xref.clone()).or_default().push(l.no);
                    } else if l.tag == "MARR" {
                        marr_lines.entry(xref.clone()).or_default().push(l.no);
                    }
                }
                // E009 EVEN/FACT instance counter (TYPE required in 7.0).
                if l.tag == "EVEN" || l.tag == "FACT" {
                    let key = (xref.clone(), l.tag.clone());
                    let n = ef_inst.get(&key).copied().unwrap_or(0) + 1;
                    ef_inst.insert(key.clone(), n);
                    ef_line.insert((key.0, key.1, n), l.no);
                }
                match (kind.as_str(), l.tag.as_str()) {
                    ("INDI", "FAMS") | ("INDI", "FAMC") => {
                        if is_pointer(&l.value) {
                            indi_famc.entry(xref.clone()).or_default();
                            pending.push((l.no, xref.clone(), l.tag.clone(), inner_ptr(&l.value).to_string()));
                            if l.tag == "FAMC" {
                                indi_famc.entry(xref.clone()).or_default().push(inner_ptr(&l.value).to_string());
                            }
                        } else if !l.value.is_empty() {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "E201",
                                    Category::Correctness,
                                    Severity::Error,
                                    l.no,
                                    format!("{}: {} amb valor no punter: {}", xref, l.tag, truncate(&l.value, 40)),
                                )],
                            );
                        }
                    }
                    ("FAM", "HUSB") | ("FAM", "WIFE") | ("FAM", "CHIL") => {
                        if is_pointer(&l.value) {
                            let t = inner_ptr(&l.value).to_string();
                            pending.push((l.no, xref.clone(), l.tag.clone(), t.clone()));
                            match l.tag.as_str() {
                                "CHIL" => {
                                    fam_chil.entry(xref.clone()).or_default().push(t);
                                }
                                // E008: FAM.HUSB / FAM.WIFE are {0:1}.
                                "HUSB" | "WIFE" => {
                                    let slot = if l.tag == "HUSB" { &mut fam_husb } else { &mut fam_wife };
                                    if let Some((_, first)) = slot.get(&xref) {
                                        push_capped(
                                            &mut diags,
                                            vec![Diag::new(
                                                "E008",
                                                Category::Correctness,
                                                Severity::Error,
                                                l.no,
                                                format!("duplicate {} in {} (first at line {})", l.tag, xref, first),
                                            )],
                                        );
                                    } else {
                                        slot.insert(xref.clone(), (t, l.no));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    ("INDI", "NAME") => {
                        // Value may continue via CONC/CONT: W402 runs on the
                        // whole accumulated value at flush time.
                        name_buf = Some((xref.clone(), l.no, l.value.clone()));
                    }
                    ("INDI", "SEX") => {
                        // E008: INDI.SEX is {0:1}.
                        if let Some((_, first)) = indi_sex.get(&xref) {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "E008",
                                    Category::Correctness,
                                    Severity::Error,
                                    l.no,
                                    format!("duplicate SEX in {} (first at line {})", xref, first),
                                )],
                            );
                        } else {
                            indi_sex.insert(xref.clone(), (l.value.clone(), l.no));
                        }
                    }
                    ("FAM", "MARR") | ("INDI", "BIRT") | ("INDI", "DEAT") => {
                        if let Some(y) = year_of(&l.value) {
                            match l.tag.as_str() {
                                "BIRT" => {
                                    indi_birth.insert(xref.clone(), Some(y));
                                }
                                "DEAT" => {
                                    indi_death.insert(xref.clone(), Some(y));
                                }
                                _ => {
                                    if kind == "FAM" {
                                        fam_marr.insert(xref.clone(), Some(y));
                                    }
                                }
                            }
                        }
                        if l.tag == "DEAT" && l.value.trim() == "Y" {
                            // Dead with unknown date: not a year, stays out of W301.
                            indi_died_unknown.insert(xref.clone());
                        }
                    }
                    _ => {
                        // Generic pointers (SOUR, OBJE, NOTE, SUBM...): record for E201.
                        // Note: ADOP takes no pointer in 5.5.1 (event with a
                        // subordinate FAMC), so it is not tracked here.
                        if is_pointer(&l.value) && matches!(l.tag.as_str(), "SOUR" | "OBJE" | "NOTE" | "SUBM" | "REPO") {
                            pending.push((l.no, xref.clone(), l.tag.clone(), inner_ptr(&l.value).to_string()));
                        }
                    }
                }
                // W401: URL inside PLAC (MyHeritage quirk).
                if l.tag == "PLAC" && l.value.contains("http") {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W401",
                            Category::Style,
                            Severity::Warning,
                            l.no,
                            format!("PLAC with URL (MyHeritage quirk): move it to NOTE: {}", truncate(&l.value, 60)),
                        )],
                    );
                }
                // HTML notes inside NOTE.
                if l.tag == "NOTE" && (l.value.contains("<br") || l.value.contains("<notexml") || l.value.contains("&nbsp")) {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W403",
                            Category::Style,
                            Severity::Warning,
                            l.no,
                            format!("NOTE with HTML (exporter quirk): {}", truncate(&l.value, 60)),
                        )],
                    );
                }
                // Upgrade path 5.5.1 -> 7: tags eliminats.
                if version == Version::V551 && l.tag == "RELA" {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "U501",
                            Category::Upgrade,
                            Severity::Info,
                            l.no,
                            "RELA removed in 7.0: use enumerated ROLE (see gedcom.io/migrate)".into(),
                        )],
                    );
                }
                if version == Version::V551 && l.tag.starts_with("_") && matches!(l.tag.as_str(), "_MARNM" | "_UPD" | "_APID" | "_OID") {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "U502",
                            Category::Upgrade,
                            Severity::Info,
                            l.no,
                            format!("vendor tag {}: kept as an undocumented extension in 7.0 (add a SCHMA TAG definition)", l.tag),
                        )],
                    );
                }
                // W306 enum values at level 1 (rare) + OTHER/PHRASE tracking.
                check_enum(
                    &mut diags,
                    &l.tag,
                    &l.value,
                    parent_tag,
                    &xref,
                    parent.clone(),
                    &mut pending_other,
                    version,
                    l.no,
                );
            }
            continue;
        }

        // Level >= 2.
        // NAME value continuation: append CONC verbatim, CONT after a space.
        // The run is only open while CONC/CONT directly follow the NAME at
        // level 2: any other line (a SOUR between, or a CONC deeper down
        // that belongs to a substructure like SOUR.PAGE) closes it first so
        // foreign values are never absorbed into the NAME.
        if name_buf.is_some() {
            if lvl == 2 && (l.tag == "CONC" || l.tag == "CONT") {
                if let Some((_, _, val)) = &mut name_buf {
                    if l.tag == "CONC" {
                        val.push_str(&l.value);
                    } else {
                        val.push(' ');
                        val.push_str(&l.value);
                    }
                }
            } else {
                flush_name(&mut diags, &mut name_buf, &mut indi_name);
            }
        }
        // E008: VERS is {0:1} per HEAD substructure, scoped by its parent
        // block. HEAD.GEDC.VERS and HEAD.SOUR.VERS both appear in every
        // MyHeritage 5.5.1 export and are not duplicates of each other.
        if in_head_main && lvl == 2 && l.tag == "VERS" {
            if let Some((ptag, pline)) = parent.clone() {
                if let Some(first) = head_vers_seen.get(&pline) {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "E008",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            format!("duplicate {}.VERS (first at line {})", ptag, first),
                        )],
                    );
                } else {
                    head_vers_seen.insert(pline, l.no);
                    // E009 requires GEDC.VERS specifically.
                    if ptag == "GEDC" {
                        saw_vers_line = Some(l.no);
                    }
                }
            }
        }
        // Pointers below level 1 (event SOUR/OBJE/NOTE...) resolve for E201 too.
        if is_pointer(&l.value) && matches!(l.tag.as_str(), "SOUR" | "OBJE" | "NOTE" | "REPO" | "SUBM") {
            let from = cur.clone().map(|c| c.0).unwrap_or_else(|| format!("line {}", l.no));
            pending.push((l.no, from, l.tag.clone(), inner_ptr(&l.value).to_string()));
        }
        // W306 enum values + OTHER/PHRASE tracking.
        {
            let rec = cur.clone().map(|c| c.0).unwrap_or_default();
            if l.tag == "PHRASE" {
                if let Some((ptag, pline)) = &parent {
                    phrased.insert((rec.clone(), ptag.clone(), *pline));
                    // A PHRASE may also hang under the OTHER-valued structure
                    // itself (maximal70: "2 TYPE OTHER" + "3 PHRASE"): mark
                    // the grandparent too, not only the sibling slot. The
                    // stack already holds PHRASE itself at the top.
                    if stack.len() >= 3 {
                        let (gtag, gline) = &stack[stack.len() - 3];
                        phrased.insert((rec.clone(), gtag.clone(), *gline));
                    }
                }
            } else {
                check_enum(
                    &mut diags,
                    &l.tag,
                    &l.value,
                    parent_tag,
                    &rec,
                    parent.clone(),
                    &mut pending_other,
                    version,
                    l.no,
                );
            }
            // E009 EVEN/FACT TYPE mark (instance = latest opened block).
            if (cur_sub == "EVEN" || cur_sub == "FACT") && l.tag == "TYPE" && !rec.is_empty() {
                if let Some(n) = ef_inst.get(&(rec.clone(), cur_sub.clone())).copied() {
                    ef_typed.insert((rec.clone(), cur_sub.clone(), n));
                }
            }
            // E009 LDS STAT register; a DATE directly under STAT satisfies it.
            if l.tag == "STAT" && LDS_EVENTS.contains(&cur_sub.as_str()) && !rec.is_empty() {
                lds_stat.push((rec.clone(), cur_sub.clone(), l.no));
            }
            if l.tag == "DATE" && parent_tag == "STAT" && !rec.is_empty() {
                if let Some((_, pline)) = &parent {
                    let pl = *pline;
                    lds_stat.retain(|(r, _, sl)| !(r == &rec && sl == &pl));
                }
            }
            // E008: one {0:1} detail substructure per event instance, at the
            // level directly under the event. DATEs are also collected
            // for the W307 conflicting-duplicate check below.
            if !rec.is_empty() && EVENT_SINGLETONS.contains(&l.tag.as_str()) {
                if let Some((ev, inst, ev_lvl)) = cur_event.clone() {
                    if lvl == ev_lvl + 1 {
                        if l.tag == "DATE" && !l.value.trim().is_empty() {
                            event_dates.insert(
                                (rec.clone(), ev.clone(), inst),
                                (l.value.trim().to_string(), l.no),
                            );
                        }
                        let key = (rec.clone(), ev.clone(), inst, l.tag.clone());
                        if let Some(first) = event_seen.get(&key) {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "E008",
                                    Category::Correctness,
                                    Severity::Error,
                                    l.no,
                                    format!(
                                        "duplicate {} in {} {} (block {}, first at line {})",
                                        l.tag, rec, ev, inst, first
                                    ),
                                )],
                            );
                        } else {
                            event_seen.insert(key, l.no);
                        }
                    }
                }
            }
        }
        // Upgrade path 5.5.1 -> 7 also at sublevels (e.g. ASSO.RELA).
        if version == Version::V551 && l.tag == "RELA" {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "U501",
                    Category::Upgrade,
                    Severity::Info,
                    l.no,
                    "RELA eliminat a 7.0: usar ROLE enumerat (vegeu gedcom.io/migrate)".into(),
                )],
            );
        }
        if cur_sub == "BIRT" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "INDI" {
                    if let Some(y) = year_of(&l.value) {
                        indi_birth.insert(xref, Some(y));
                    }
                } else if kind == "FAM" && cur_sub == "MARR" {
                    // No-op: MARR is handled below.
                }
            }
        }
        if cur_sub == "DEAT" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "INDI" {
                    if let Some(y) = year_of(&l.value) {
                        indi_death.insert(xref, Some(y));
                    }
                }
            }
        }
        if cur_sub == "MARR" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "FAM" {
                    fam_marr.insert(xref, year_of(&l.value));
                }
            }
        }
        if l.tag == "DATE" && cur_sub == "BIRT" {
            // Already handled.
        }
        if l.tag == "PEDI" && version == Version::V551 {
            let v = l.value.trim();
            if v != v.to_uppercase() {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "U501",
                        Category::Upgrade,
                        Severity::Info,
                        l.no,
                        format!("lowercase PEDI ({}): 7.0 requires uppercase", v),
                    )],
                );
            }
        }
        // Nested PLAC with URL (MyHeritage quirk).
        if l.tag == "PLAC" && l.value.contains("http") {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W401",
                    Category::Style,
                    Severity::Warning,
                    l.no,
                    format!("PLAC with URL (MyHeritage quirk): {}", truncate(&l.value, 60)),
                )],
            );
        }
        // DATE with suspicious format (non-ENG months, lowercase "about"...).
        if l.tag == "DATE" && !l.value.is_empty() {
            check_date_style(&mut diags, l.no, &l.value, version);
        }
    }

    // A NAME run ending at EOF (NAME directly before TRLR) still needs W402.
    flush_name(&mut diags, &mut name_buf, &mut indi_name);

    // E002: HEAD/TRLR obligatoris.
    if !saw_head {
        push_capped(
            &mut diags,
            vec![Diag::new("E002", Category::Correctness, Severity::Error, 0, "missing HEAD record".into())],
        );
    }
    if !saw_trlr {
        push_capped(
            &mut diags,
            vec![Diag::new("E002", Category::Correctness, Severity::Error, 0, "missing TRLR record".into())],
        );
    }

    // E009: HEAD.GEDC and GEDC.VERS are {1:1} in both 5.5.1 and 7.0.
    if saw_head && saw_gedc_line.is_none() {
        push_capped(
            &mut diags,
            vec![Diag::new("E009", Category::Correctness, Severity::Error, 0, "HEAD without required GEDC".into())],
        );
    }
    if saw_gedc_line.is_some() && saw_vers_line.is_none() {
        push_capped(
            &mut diags,
            vec![Diag::new("E009", Category::Correctness, Severity::Error, 0, "GEDC without required VERS".into())],
        );
    }

    // W306: OTHER enum values want a sibling PHRASE with the free text.
    for (rec, ptag, pline, tag, line) in &pending_other {
        if !phrased.contains(&(rec.clone(), ptag.clone(), *pline)) {
            let who = if rec.is_empty() { format!("line {}", line) } else { rec.clone() };
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W306",
                    Category::Suspicious,
                    Severity::Info,
                    *line,
                    format!("{}: {} OTHER without a sibling PHRASE (add the free-text phrase)", who, tag),
                )],
            );
        }
    }

    // E009: EVEN/FACT without TYPE and LDS STAT without DATE (7.0 only:
    // 5.5.1 leaves both optional).
    if version == Version::V70 {
        for ((rec, tag, inst), line) in &ef_line {
            if !ef_typed.contains(&(rec.clone(), tag.clone(), *inst)) {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "E009",
                        Category::Correctness,
                        Severity::Error,
                        *line,
                        format!("{}: {} without required TYPE (7.0)", rec, tag),
                    )],
                );
            }
        }
        for (rec, ev, sline) in &lds_stat {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E009",
                    Category::Correctness,
                    Severity::Error,
                    *sline,
                    format!("{}: {} STAT without required DATE (7.0)", rec, ev),
                )],
            );
        }
    }

    // W307: semantically single events twice with conflicting DATEs (classic
    // merge leftover). MARR and DIV pairs separated by the counterpart event
    // are a serial marriage/divorce (spec-legal in one FAM), not conflicts.
    {
        let mut by_event: HashMap<(String, String), Vec<EvHit>> = HashMap::new();
        for ((rec, ev, inst), (val, line)) in &event_dates {
            if CONFLICT_EVENTS.contains(&ev.as_str()) {
                by_event.entry((rec.clone(), ev.clone())).or_default().push((*inst, val.clone(), *line));
            }
        }
        let empty: Vec<usize> = Vec::new();
        for ((rec, ev), mut v) in by_event {
            v.sort();
            let excusers = match ev.as_str() {
                "MARR" => div_lines.get(&rec).unwrap_or(&empty),
                "DIV" => marr_lines.get(&rec).unwrap_or(&empty),
                _ => &empty,
            };
            let mut prev: Option<EvHit> = None;
            for hit in v {
                if let Some((_, pval, pline)) = &prev {
                    if pval != &hit.1 {
                        let excused = excusers.iter().any(|d| *d > *pline && *d < hit.2);
                        if !excused {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "W307",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    hit.2,
                                    format!(
                                        "{}: duplicate {} with conflicting dates ({} vs {})",
                                        rec, ev, pval, hit.1
                                    ),
                                )],
                            );
                            prev = None;
                            continue;
                        }
                    }
                }
                prev = Some(hit);
            }
        }
    }

    // E201: broken references. @VOID@ is the 7.0 null pointer: always valid.
    for (line, from, tag, target) in &pending {
        if target == "@VOID@" {
            continue;
        }
        if !records.contains_key(target) {
            let kind = match tag.as_str() {
                "FAMS" | "FAMC" => "FAM",
                "HUSB" | "WIFE" | "CHIL" => "INDI",
                _ => "record",
            };
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E201",
                    Category::Correctness,
                    Severity::Error,
                    *line,
                    format!("{}: {} {} points to a nonexistent {}", from, tag, target, kind),
                )],
            );
        }
    }

    // W202: FAMC not listed as CHIL (and vice versa).

    // W202: FAMC no llistat com a CHIL (i viceversa).
    for (xref, fams) in &indi_famc {
        for f in fams {
            let listed = fam_chil.get(f).map(|c| c.contains(xref)).unwrap_or(false);
            let fam_exists = records.get(f).map(|r| r.0 == "FAM").unwrap_or(false);
            if fam_exists && !listed {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "W202",
                        Category::Suspicious,
                        Severity::Warning,
                        0,
                        format!("{}: declares FAMC {} but the FAM does not list them as CHIL", xref, f),
                    )],
                );
            }
        }
    }
    for (fam, chils) in &fam_chil {
        for c in chils {
            let declares = indi_famc.get(c).map(|v| v.contains(fam)).unwrap_or(false);
            if !declares && records.contains_key(c) {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "W202",
                        Category::Suspicious,
                        Severity::Warning,
                        0,
                        format!("{}: lists CHIL {} but the INDI declares no FAMC", fam, c),
                    )],
                );
            }
        }
    }

    // W301: death before birth + longevity, per individual.
    for (xref, b) in &indi_birth {
        let d = indi_death.get(xref).copied().flatten();
        if let (Some(bb), Some(dd)) = (*b, d) {
            if dd < 10000 {
                flush_person(&mut diags, xref, Some(bb), Some(dd), records.get(xref).map(|r| r.1).unwrap_or(0));
            }
        }
    }

    // W303: parent age at the child's birth.
    for (fam, chils) in &fam_chil {
        for c in chils {
            let cb = indi_birth.get(c).copied().flatten();
            let Some(cb) = cb else { continue };
            if cb >= 10000 {
                continue;
            }
            for (parent, rol) in [(&fam_husb.get(fam), "father"), (&fam_wife.get(fam), "mother")] {
                if let Some((px, _)) = parent {
                    if let Some(Some(pb)) = indi_birth.get(px) {
                        let age = cb - pb;
                        let max = if rol == "mother" { 50 } else { 70 };
                        if age < 13 || age > max {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "W303",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    0,
                                    format!(
                                        "{}: {} {} (b. {}) was {} at {}'s birth (b. {})",
                                        fam, rol, px, pb, age, c, cb
                                    ),
                                )],
                            );
                        }
                    }
                }
            }
            // Child born before the marriage (when a MARR date exists).
            if let Some(Some(m)) = fam_marr.get(fam) {
                if cb < *m {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W304",
                            Category::Suspicious,
                            Severity::Warning,
                            0,
                            format!("{}: {} born ({}) before marriage ({})", fam, c, cb, m),
                        )],
                    );
                }
            }
        }
    }

    // SEX.
    for (xref, (v, line)) in &indi_sex {
        let ok = match version {
            Version::V70 => matches!(v.trim(), "M" | "F" | "X" | "U"),
            _ => matches!(v.trim(), "M" | "F" | "U"),
        };
        if !ok {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W305",
                    Category::Suspicious,
                    Severity::Warning,
                    *line,
                    format!("{}: invalid SEX ({}), expected M/F/U{}", xref, v, if version == Version::V70 { "/X" } else { "" }),
                )],
            );
        }
    }

    // W302: duplicates (same normalized name + birth within ±2 years).
    let mut by_name: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (xref, b) in &indi_birth {
        if let (Some(nm), Some(bb)) = (indi_name.get(xref), *b) {
            if bb < 10000 {
                by_name.entry(norm_name(nm)).or_default().push((xref.clone(), bb));
            }
        }
    }
    for v in by_name.values() {
        for a in 0..v.len() {
            for b in a + 1..v.len() {
                if (v[a].1 - v[b].1).abs() <= 2 {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W302",
                            Category::Suspicious,
                            Severity::Warning,
                            0,
                            format!("possible duplicate: {} (b. {}) vs {} (b. {})", v[a].0, v[a].1, v[b].0, v[b].1),
                        )],
                    );
                }
            }
        }
    }

    let individuals = indi_birth.len();
    let _count_dbg = fam_chil.len() + fam_husb.len() + fam_wife.len();
    let mut families_set: HashSet<&String> = HashSet::new();
    for k in fam_chil.keys().chain(fam_husb.keys()).chain(fam_wife.keys()) {
        families_set.insert(k);
    }

    diags.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    Report { version, diags, lines: lines.len(), individuals, families: families_set.len() }
}

fn check_date_style(diags: &mut Vec<Diag>, line: usize, value: &str, version: Version) {
    let v = value.trim();
    // Months in other languages or lowercase: GEDCOM requires JAN FEB MAR...
    let lower_months = ["enero", "febrero", "gener", "febrer", "marzo", "març", "abril", "mayo", "maig", "junio", "juny"];
    let vl = v.to_lowercase();
    if lower_months.iter().any(|m| vl.contains(m)) {
        push_capped(
            diags,
            vec![Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE with non-standard month (use JAN/FEB/...): {}", truncate(v, 50)),
            )],
        );
        return;
    }
    if v.starts_with("about") || v.starts_with("circa") || v.starts_with("aprox") {
        push_capped(
            diags,
            vec![Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE with lowercase approximation (use ABT/CAL/EST): {}", truncate(v, 50)),
            )],
        );
    }
    if v.contains("BET") && !v.contains("AND") {
        // DATE_RANGE needs BET x AND y in both 5.5.1 and 7.0.
        if version == Version::V70 {
            push_capped(
                diags,
                vec![Diag::new(
                    "U501",
                    Category::Upgrade,
                    Severity::Info,
                    line,
                    format!("BET without AND (7.0 needs a full range): {}", truncate(v, 50)),
                )],
            );
        } else {
            push_capped(
                diags,
                vec![Diag::new(
                    "W402",
                    Category::Style,
                    Severity::Warning,
                    line,
                    format!("BET without AND (DATE_RANGE needs BET x AND y): {}", truncate(v, 50)),
                )],
            );
        }
    }
    if v.contains("BET") && v.contains("AND") {
        // 7.0 ranges must be chronological (migrate guide: swap if needed).
        let years: Vec<i64> = v
            .split(|c: char| !c.is_ascii_digit())
            .filter(|t| t.len() == 4)
            .filter_map(|t| t.parse().ok())
            .collect();
        if years.len() >= 2 && years[0] > years[1] {
            push_capped(
                diags,
                vec![Diag::new(
                    "U501",
                    Category::Upgrade,
                    Severity::Info,
                    line,
                    format!("BET range out of order (swap to chronological): {}", truncate(v, 50)),
                )],
            );
        }
    }
    // FROM/TO pairing is NOT checked: DATE_PERIOD allows each half alone in
    // both 5.5.1 (p.43) and 7.0 (TGC551LF uses standalone FROM and TO).
    // Balanced parentheses (DATE_PHRASE).
    if v.matches('(').count() != v.matches(')').count() {
        push_capped(
            diags,
            vec![Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE with unbalanced parentheses: {}", truncate(v, 50)),
            )],
        );
    }
    // Calendar escape @#...@: must close and name a known calendar.
    if let Some(start) = v.find("@#") {
        const CALENDARS: &[&str] = &["GREGORIAN", "JULIAN", "HEBREW", "FRENCH_R", "ROMAN", "UNKNOWN"];
        let rest = &v[start + 2..];
        match rest.find('@') {
            Some(end) if CALENDARS.contains(&rest[..end].to_ascii_uppercase().as_str()) => {}
            _ => push_capped(
                diags,
                vec![Diag::new(
                    "W402",
                    Category::Style,
                    Severity::Warning,
                    line,
                    format!("DATE with bad calendar escape (use @#GREGORIAN@ etc.): {}", truncate(v, 50)),
                )],
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// --fix: only safe repairs, always with a .bak copy.
// ---------------------------------------------------------------------------

/// Safe repairs applied by --fix:
/// 0. Lines without a level (MyHeritage NOTE/TEXT continuations without CONT):
///    prefix "{previous_level+1} CONT ".
/// 1. Rejoin UTF-8 characters split across CONC lines (E101).
/// 2. Trim trailing whitespace.
pub fn fix_bytes(data: &[u8]) -> (Vec<u8>, Vec<String>) {
    let mut applied = Vec::new();
    // Note CR normalization only when it actually changes bytes: lone CRs
    // exist (a pure-CRLF file reassembles byte-identical, main.rs then
    // prints nothing thanks to its fixed != data check).
    let cr_fixable = {
        let mut found = false;
        let mut it = data.iter().peekable();
        while let Some(&b) = it.next() {
            if b == b'\r' && it.peek() != Some(&&b'\n') {
                found = true;
                break;
            }
        }
        found
    };
    let norm = normalize_newlines(data);
    let mut lines: Vec<Vec<u8>> = norm.split(|&b| b == b'\n').map(|l| l.to_vec()).collect();

    // 0. Orphans without a leading level.
    let mut fixed_orphans = 0;
    let mut prev_level: Option<usize> = None;
    for l in lines.iter_mut() {
        let body = strip_cr_slice(l);
        // Blank/whitespace-only lines are not orphans (step 2 trims them);
        // same definition lint_lines uses, and they must not break the run.
        if body.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        match leading_level(body) {
            // The anchor only moves on a line that really has a level: a run
            // of orphans is a run of siblings under it, not a staircase.
            Some(n) => prev_level = Some(n),
            None => {
                if let Some(p) = prev_level {
                    if p < MAX_LEVEL as usize {
                        let mut nl = format!("{} CONT ", p + 1).into_bytes();
                        nl.extend_from_slice(body);
                        if l.last() == Some(&b'\r') {
                            nl.push(b'\r');
                        }
                        *l = nl;
                        fixed_orphans += 1;
                    }
                }
            }
        }
    }
    if fixed_orphans > 0 {
        applied.push(format!("E001: {} orphan lines prefixed with CONT", fixed_orphans));
    }

    // 1. CONC split: if the next line's payload starts with a
    // continuation byte, append it to the previous line (dropping "N CONC ").
    let mut fixed_conc = 0;
    let mut i = 0;
    while i < lines.len() {
        let payload = {
            let l = strip_cr(&lines[i]);
            conc_payload_owned(l)
        };
        if let Some(p) = payload {
            if p.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) && i > 0 {
                // Find the payload start inside lines[i] and append it.
                let full = lines[i].clone();
                let stripped = strip_cr_slice(&full);
                if let Some(pos) = find_conc_pos(stripped) {
                    let tail = &stripped[pos..];
                    // Drop the previous line's \r if present.
                    let prev = &mut lines[i - 1];
                    if prev.last() == Some(&b'\r') {
                        prev.pop();
                    }
                    prev.extend_from_slice(tail);
                    lines.remove(i);
                    fixed_conc += 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    if fixed_conc > 0 {
        applied.push(format!("E101: rejoined {} CONC lines with split UTF-8", fixed_conc));
    }

    // 2. Trailing whitespace.
    let mut fixed_ws = 0;
    for l in lines.iter_mut() {
        let has_cr = l.last() == Some(&b'\r');
        let body = if has_cr { &l[..l.len() - 1] } else { &l[..] };
        let trimmed = rtrim_ws(body);
        if trimmed.len() != body.len() {
            fixed_ws += 1;
            let mut nl = trimmed.to_vec();
            if has_cr {
                nl.push(b'\r');
            }
            *l = nl;
        }
    }
    if fixed_ws > 0 {
        applied.push(format!("style: trimmed trailing whitespace on {} lines", fixed_ws));
    }
    if cr_fixable {
        applied.push("style: normalized classic Mac CR line endings to LF".into());
    }

    let mut out = Vec::with_capacity(data.len());
    for (k, l) in lines.iter().enumerate() {
        out.extend_from_slice(l);
        if k + 1 < lines.len() {
            out.push(b'\n');
        }
    }
    // Preserve the original trailing newline.
    if data.ends_with(b"\n") && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    (out, applied)
}

fn strip_cr(line: &[u8]) -> &[u8] {
    strip_cr_slice(line)
}

/// Leading level of a line ("12 TAG..." -> 12). None when there are no
/// leading digits followed by a space or end of line.
fn leading_level(line: &[u8]) -> Option<usize> {
    let mut n: usize = 0;
    let mut digits = 0;
    for &b in line {
        if b.is_ascii_digit() {
            n = n.saturating_mul(10).saturating_add((b - b'0') as usize);
            digits += 1;
        } else {
            break;
        }
    }
    if digits > 0 && n <= MAX_LEVEL as usize && (line.get(digits) == Some(&b' ') || line.len() == digits) {
        Some(n)
    } else {
        None
    }
}

fn strip_cr_slice(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line }
}

fn conc_payload_owned(line: &[u8]) -> Option<Vec<u8>> {
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        Some(line[p + pat.len()..].to_vec())
    } else if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let mut rest = &line[p + 4..];
        if rest.first() == Some(&b' ') {
            rest = &rest[1..];
        }
        Some(rest.to_vec())
    } else {
        None
    }
}

fn find_conc_pos(line: &[u8]) -> Option<usize> {
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        return Some(p + pat.len());
    }
    if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let mut q = p + 4;
        if line.get(q) == Some(&b' ') {
            q += 1;
        }
        return Some(q);
    }
    None
}

fn rtrim_ws(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b' ' || b[end - 1] == b'\t') {
        end -= 1;
    }
    &b[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_order() {
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn json_escapes() {
        let r = Report { version: Version::V551, lines: 1, individuals: 0, families: 0, diags: vec![Diag::new("E1", Category::Correctness, Severity::Error, 1, "a\"b\\c".into())] };
        let j = r.to_json();
        assert!(j.contains("a\\\"b\\\\c"));
    }
}
