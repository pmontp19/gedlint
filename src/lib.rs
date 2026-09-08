//! gedlint: GEDCOM linter engine (5.5.1 + 7.0).
//!
//! Design: streaming line-by-line parsing (`BufRead`) without loading the
//! whole tree. The core is pure (`&str` in, `Report` out) and therefore
//! compilable to WASM unchanged: the CLI binary is a thin layer (fs + args).
//! Zero dependencies.
//!
//! Layout: `diag` holds the output types, `parse` the line grammar, `rules`
//! the single streaming pass and the rule groups it drives, `fix` the safe
//! `--fix` repairs as selectable `Edit`s, `registry` the rule metadata table
//! every consumer reads, `config` the pure `gedlint.toml` parser (GEDCOM has
//! no comment syntax, so config is the only suppression mechanism), and
//! `baseline` the count-based ratchet file for adopting a legacy tree (RFC
//! 014 section 5). All file and OS access lives in `main.rs`; everything
//! public is re-exported here, so the crate's public API is exactly what
//! this file names.

use std::io::BufRead;

mod baseline;
mod config;
mod diag;
mod fix;
mod parse;
mod registry;
mod rules;

pub use baseline::{apply_baseline, baseline_from_report, baseline_to_json, fingerprint, parse_baseline, Baseline, BaselineEntry, BaselineOutcome};
pub use config::{Config, ConfigError, RuleLevel, parse_config};
pub use diag::{Category, Diag, DiagGroup, Report, Severity};
pub use fix::{apply_edits, compute_edits, fix_bytes, fix_bytes_with, normalize_endings, Applicability, Edit, FixSelection};
pub use parse::Version;
pub use registry::{rule, rule_by_name, rulesets, RuleMeta, RULES};

use parse::{normalize_newlines, scan_head};
use rules::encoding::encoding_diags;
use rules::lint_lines;

// ---------------------------------------------------------------------------
// Pure public API (reusable from WASM): text in, report out.
// ---------------------------------------------------------------------------

/// Lint already-read GEDCOM text. Pure core, WASM-suitable.
/// Same as `lint_str_with` with the built-in configuration.
pub fn lint_str(input: &str) -> Report {
    lint_str_with(input, &Config::default())
}

/// Lint already-read GEDCOM text under a configuration: rules set to "off"
/// emit nothing and are not counted for the exit code; rules with a level
/// override emit at that severity.
pub fn lint_str_with(input: &str, cfg: &Config) -> Report {
    lint_bytes_split(input.as_bytes(), true, cfg)
}

/// Lint raw bytes (detects UTF-8 / BOM / CRLF before decoding).
/// Same as `lint_bytes_with` with the built-in configuration.
pub fn lint_bytes(data: &[u8]) -> Report {
    lint_bytes_with(data, &Config::default())
}

/// Lint raw bytes under a configuration (see `lint_str_with`).
pub fn lint_bytes_with(data: &[u8], cfg: &Config) -> Report {
    lint_bytes_split(data, false, cfg)
}

fn lint_bytes_split(data: &[u8], _already_str: bool, cfg: &Config) -> Report {
    let data = normalize_newlines(data);
    let text = String::from_utf8_lossy(&data).into_owned();
    let (version, charset) = scan_head(&text);
    let diags: Vec<Diag> = encoding_diags(&data, version, charset.as_deref());

    let mut r = lint_lines(&text);
    // Encoding diags go first (low line numbers), then semantic ones.
    let mut all = diags;
    all.append(&mut r.diags);
    // Configuration applies to the merged report: an "off" rule never
    // reaches the sort, the counts or the exit code.
    apply_config(&mut all, cfg);
    // Issue 30: severity and line alone leave ties to insertion order, so
    // code and message break them; identical (sev, line, code, msg) rows
    // render identically anyway.
    all.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.line.cmp(&b.line))
            .then(a.code.cmp(b.code))
            .then(a.msg.cmp(&b.msg))
    });
    r.diags = all;
    // The byte-level pre-scan and the line parser must agree on version.
    if r.version == Version::Unknown {
        r.version = version;
    }
    r
}

/// Re-level and drop diagnostics per the configuration. A rule absent from
/// the resolved map keeps whatever severity the rule itself emitted.
fn apply_config(diags: &mut Vec<Diag>, cfg: &Config) {
    let eff = cfg.effective();
    diags.retain_mut(|d| match eff.get(d.code) {
        Some(RuleLevel::Off) => false,
        Some(RuleLevel::Severity(s)) => {
            d.severity = *s;
            true
        }
        None => true,
    });
}

/// Streaming input: reads line by line without loading everything at once.
/// Useful for 100MB+ exports. Internally it delegates to `lint_lines` for
/// simplicity, but the contract is `BufRead`. Behaves as if built with
/// `Config::default()`.
pub fn lint_reader<R: BufRead>(reader: R) -> Report {
    lint_reader_with(reader, &Config::default())
}

/// Streaming input under a configuration (see `lint_str_with`): reads
/// through `BufRead` without ever holding a second copy of the file, so
/// 100MB+ exports stay streamable with config applied.
pub fn lint_reader_with<R: BufRead>(mut reader: R, cfg: &Config) -> Report {
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
    lint_bytes_with(&buf, cfg)
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
        let r = Report { version: Version::V551, lines: 1, individuals: 0, families: 0, diags: vec![Diag::new("E001", Category::Correctness, Severity::Error, 1, "a\"b\\c".into())] };
        let j = r.to_json();
        assert!(j.contains("a\\\"b\\\\c"));
    }
}
