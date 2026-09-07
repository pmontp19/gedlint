//! Diagnostic types and their dependency-free JSON serialization.
//!
//! `Severity`, `Category`, `Diag` and `Report` are part of the public API
//! (re-exported from the crate root) and are consumed verbatim by the CLI,
//! by `scripts/gh-report.js` and by the web viewer.

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

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub category: Category,
    pub severity: Severity,
    pub line: usize,
    pub msg: String,
}

impl Diag {
    pub(crate) fn new(code: &'static str, category: Category, severity: Severity, line: usize, msg: String) -> Diag {
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

/// No global cap during collection: a real file (516 `_UPD`)
/// must not hide errors behind infos. The limit applies at output time
/// via `--max N` (0 = unlimited). The name is kept to avoid touching
/// every call site.
pub(crate) fn push_capped(dst: &mut Vec<Diag>, mut v: Vec<Diag>) {
    dst.append(&mut v);
}
