//! GEDCOM line grammar: version, the parsed `Line`, newline normalization,
//! the HEAD pre-scan and the value helpers the rule modules share.

use std::borrow::Cow;

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

/// Bare CR is a legal line terminator in 5.5.1 (classic Mac; the original
/// TGC551.ged uses it): normalize lone CR to LF, keep CRLF untouched.
/// Borrows the input when there is nothing to do (the common case).
pub(crate) fn normalize_newlines(data: &[u8]) -> Cow<'_, [u8]> {
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
pub(crate) fn scan_head(text: &str) -> (Version, Option<String>) {
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

// Line parser
// ---------------------------------------------------------------------------

pub(crate) struct Line {
    pub(crate) no: usize,
    pub(crate) level: Option<u32>,
    pub(crate) xref: String,
    pub(crate) tag: String,
    pub(crate) value: String,
    pub(crate) raw: String,
    /// Byte offset of `value` inside `raw`. Relative to `raw`, so feed it to
    /// `span_at`, which is what converts to on-disk coordinates.
    pub(crate) value_col: usize,
    /// Bytes this line has in the file before `raw` starts. Non-zero only for
    /// line 1 of a BOM'd file: the parser never sees the BOM, but the file a
    /// consumer slices does, so every span has to add it back.
    pub(crate) span_base: u32,
}

impl Line {
    /// Byte span of `needle` inside this line, in the on-disk coordinates a
    /// consumer slices with. `(0, 0)`, i.e. "no span", when the needle is
    /// absent. Never char or UTF-16 offsets.
    pub(crate) fn span_of(&self, needle: &str) -> (u32, u32) {
        match self.raw.find(needle) {
            Some(at) => self.span_at(at, needle.len()),
            None => (0, 0),
        }
    }

    /// Same, for an offset already resolved against `raw`. The `u32` casts
    /// would truncate past 4 GiB, which no single GEDCOM line reaches (5.5.1
    /// caps a line at 255 characters and real exporters stay far below).
    pub(crate) fn span_at(&self, at: usize, len: usize) -> (u32, u32) {
        (self.span_base + at as u32, len as u32)
    }

    /// Record how many bytes of the on-disk line the parser skipped.
    pub(crate) fn with_span_base(mut self, base: u32) -> Line {
        self.span_base = base;
        self
    }
}

/// A UTF-8 BOM, which GEDCOM 7 recommends, is stripped before parsing but is
/// still in the file: it is the first `BOM_LEN` bytes of line 1.
pub(crate) const BOM_LEN: u32 = 3;

/// Deepest legal level (5.5.1 ch. 1). A bigger leading number is text, not a
/// level: a biography continuation line may start with a year ("1936 va ...").
pub(crate) const MAX_LEVEL: u32 = 99;

pub(crate) fn parse_line(no: usize, raw: &str) -> Line {
    // Grammar: LEVEL [XREF] TAG [VALUE]. XREF only at level 0.
    let mut it = raw.splitn(3, char::is_whitespace);
    let lvl: Option<u32> = it
        .next()
        .and_then(|x| x.parse().ok())
        .filter(|n| *n <= MAX_LEVEL);
    let second = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    let (xref, tag, value) =
        if second.starts_with('@') && second.ends_with('@') && second.len() >= 3 {
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
    // The value is always trimmed out of a slice that runs to the end of the
    // line, so it ends where the line's trailing whitespace begins: that
    // pins its offset exactly, without re-walking the split.
    let value_col = if value.is_empty() {
        0
    } else {
        raw.trim_end().len() - value.len()
    };
    Line {
        no,
        level: lvl,
        xref,
        tag,
        value,
        raw: raw.to_string(),
        value_col,
        span_base: 0,
    }
}

pub(crate) fn is_pointer(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 3
        && t.starts_with('@')
        && t.ends_with('@')
        && !t[1..t.len() - 1].contains(char::is_whitespace)
}

pub(crate) fn inner_ptr(s: &str) -> &str {
    s.trim()
}

pub(crate) fn year_of(s: &str) -> Option<i64> {
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

pub(crate) fn norm_name(s: &str) -> String {
    s.to_lowercase()
        .replace('/', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn truncate(s: &str, n: usize) -> String {
    // Cut on char boundaries: &s[..n] panics when n splits a multibyte char
    // (e.g. a long Catalan PLAC with a URL near byte 60).
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

/// Version state (HEAD.GEDC.VERS) read off the already parsed lines. Same
/// walk as `scan_head`, but over `Line` rather than raw text.
pub(crate) fn detect_version(lines: &[Line]) -> Version {
    let mut version = Version::Unknown;
    let mut in_head = false;
    let mut in_gedc = false;
    for l in lines {
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
    version
}
