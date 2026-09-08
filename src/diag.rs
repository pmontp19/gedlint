//! Diagnostic types and their dependency-free JSON serialization.
//!
//! `Severity`, `Category`, `Diag`, `Report` and `DiagGroup` are part of the
//! public API (re-exported from the crate root) and are consumed verbatim by
//! the CLI, by `scripts/gh-report.js` and by the web viewer.

use std::collections::HashMap;

use crate::parse::Version;

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

/// Ruleset every rule shipped so far belongs to: normative, on by default.
/// Opt-in rulesets (RFC 014 section 0.5) carry their own name instead.
pub(crate) const CORE: &str = "core";

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub category: Category,
    /// Domain the rule belongs to ("core", and later opt-in rulesets).
    /// Orthogonal to `category`, which is the rule's intent.
    pub ruleset: &'static str,
    pub severity: Severity,
    pub line: usize,
    /// 0-based **byte** offset of the span, never a char or UTF-16 offset:
    /// slice the line's bytes at `col..col + len` and decode the three pieces.
    /// Counted from the start of the line **as it exists in the file**, with
    /// nothing stripped, so a UTF-8 BOM is the first 3 bytes of line 1 (RFC
    /// 014 section 1). Only meaningful when `len > 0`.
    pub col: u32,
    /// Span length in **bytes**. `0` means "no span": highlight the whole line.
    pub len: u32,
    pub msg: String,
}

impl Diag {
    /// Spanless diagnostic: `ruleset` defaults to `core`, `col`/`len` to 0.
    /// The signature is deliberately frozen so rules adopt spans one at a time.
    pub(crate) fn new(code: &'static str, category: Category, severity: Severity, line: usize, msg: String) -> Diag {
        Diag { code, category, ruleset: CORE, severity, line, col: 0, len: 0, msg }
    }

    /// Same as `new` plus the byte span of the offending substring.
    pub(crate) fn with_span(code: &'static str, category: Category, severity: Severity, line: usize, col: u32, len: u32, msg: String) -> Diag {
        Diag { code, category, ruleset: CORE, severity, line, col, len, msg }
    }

    /// Move a diagnostic out of `core` into an opt-in ruleset.
    pub fn in_ruleset(mut self, ruleset: &'static str) -> Diag {
        self.ruleset = ruleset;
        self
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

/// One collapsed rule-code group over a report's diagnostics (issue 20 /
/// RFC 014 section 7). The CLI's default output, the Action's job summary
/// and the web viewer all render from this one shape so they cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagGroup {
    pub code: &'static str,
    pub category: Category,
    /// Worst severity among the group's occurrences (a rule may emit two:
    /// W306 warns on bad values and informs on OTHER without PHRASE).
    pub severity: Severity,
    pub count: usize,
    /// Line of the first occurrence: diagnostics arrive severity/line
    /// sorted, so this is the topmost one.
    pub line: usize,
    /// Message of that first occurrence: the example a consumer shows.
    pub example: String,
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

    /// The whole report grouped by rule code (issue 20). Sorted by severity
    /// descending, then count descending, then code.
    pub fn grouped(&self) -> Vec<DiagGroup> {
        let refs: Vec<&Diag> = self.diags.iter().collect();
        Self::group_diags(&refs)
    }

    /// Group an arbitrary slice of diagnostics by rule code. This is the
    /// single grouping algorithm: the CLI, `scripts/gh-report.js` (through
    /// the JSON `groups` key) and the web viewer all consume its output.
    pub fn group_diags(diags: &[&Diag]) -> Vec<DiagGroup> {
        let mut groups: HashMap<&'static str, DiagGroup> = HashMap::new();
        for d in diags {
            match groups.get_mut(d.code) {
                Some(g) => {
                    g.count += 1;
                    if d.severity > g.severity {
                        g.severity = d.severity;
                    }
                }
                None => {
                    groups.insert(
                        d.code,
                        DiagGroup {
                            code: d.code,
                            category: d.category,
                            severity: d.severity,
                            count: 1,
                            line: d.line,
                            example: d.msg.clone(),
                        },
                    );
                }
            }
        }
        let mut out: Vec<DiagGroup> = groups.into_values().collect();
        out.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(b.count.cmp(&a.count))
                .then(a.code.cmp(b.code))
        });
        out
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
            // Additive keys (ruleset/col/len): no existing key changes name,
            // type or meaning, so scripts/gh-report.js keeps working unmodified.
            out.push_str("\",\"ruleset\":\"");
            out.push_str(d.ruleset);
            out.push_str("\",\"severity\":\"");
            out.push_str(d.severity.tag().trim());
            out.push_str("\",\"line\":");
            out.push_str(&d.line.to_string());
            out.push_str(",\"col\":");
            out.push_str(&d.col.to_string());
            out.push_str(",\"len\":");
            out.push_str(&d.len.to_string());
            out.push_str(",\"message\":\"");
            out.push_str(&escape_json(&d.msg));
            out.push_str("\"}");
        }
        out.push_str("],\"groups\":[");
        // The engine's own grouping (issue 20): the Action's summary and the
        // web viewer consume this instead of recomputing it. Additive key,
        // like the span keys above.
        for (i, g) in self.grouped().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"code\":\"");
            out.push_str(g.code);
            out.push_str("\",\"category\":\"");
            out.push_str(g.category.as_str());
            out.push_str("\",\"severity\":\"");
            out.push_str(g.severity.tag().trim());
            out.push_str("\",\"count\":");
            out.push_str(&g.count.to_string());
            out.push_str(",\"line\":");
            out.push_str(&g.line.to_string());
            out.push_str(",\"example\":\"");
            out.push_str(&escape_json(&g.example));
            out.push_str("\"}");
        }
        out.push_str("]}");
        out
    }
}

/// Shared by `Report::to_json` and the baseline serializer.
pub(crate) fn escape_json(s: &str) -> String {
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
