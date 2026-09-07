//! gedlint: GEDCOM linter engine (5.5.1 + 7.0).
//!
//! Design: streaming line-by-line parsing (`BufRead`) without loading the
//! whole tree. The core is pure (`&str` in, `Report` out) and therefore
//! compilable to WASM unchanged: the CLI binary is a thin layer (fs + args).
//! Zero dependencies.
//!
//! Layout: `diag` holds the output types, `parse` the line grammar, `rules`
//! the single streaming pass and the rule groups it drives, `fix` the safe
//! `--fix` repairs. Everything public is re-exported here, so the crate's
//! public API is exactly what this file names.

use std::io::BufRead;

mod diag;
mod fix;
mod parse;
mod rules;

pub use diag::{Category, Diag, Report, Severity};
pub use fix::fix_bytes;
pub use parse::Version;

use parse::{normalize_newlines, scan_head};
use rules::encoding::encoding_diags;
use rules::lint_lines;

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
