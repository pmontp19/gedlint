//! Byte-level encoding rules, run before the line parser: E101 and W102.

use crate::diag::{Category, Diag, Severity};
use crate::parse::Version;

/// E101/W102: UTF-8, BOM, continuation byte at line start (MyHeritage bug
/// splitting multibyte sequences across CONC lines), mixed CRLF, controls.
/// `charset` is the declared HEAD.CHAR: ANSEL/ASCII files are not UTF-8,
/// so E101 does not apply to them. A BOM is recommended by GEDCOM 7
/// (spec 1.1) and only warned about otherwise.
pub(crate) fn encoding_diags(data: &[u8], version: Version, charset: Option<&str>) -> Vec<Diag> {
    let mut out = Vec::new();
    let non_utf8 = matches!(
        charset,
        Some("ANSEL") | Some("ASCII") | Some("IBMPC") | Some("MACINTOSH")
    );
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
    // Span of the orphan bytes on the first offending line, so a viewer can
    // highlight exactly the tail of the character that was cut in half.
    let mut first_span = (0u32, 0u32);
    if !non_utf8 {
        for (i, line) in data.split(|&b| b == b'\n').enumerate() {
            let l = if line.last() == Some(&b'\r') {
                &line[..line.len() - 1]
            } else {
                line
            };
            // Skip the "N CONC ..." header: the useful content starts after it.
            let at = conc_payload_start(l);
            let payload = &l[at..];
            if payload
                .first()
                .map(|b| (0x80..=0xBF).contains(b))
                .unwrap_or(false)
            {
                bad += 1;
                if first == 0 {
                    first = i + 1;
                    let run = payload
                        .iter()
                        .take_while(|&&b| (0x80..=0xBF).contains(&b))
                        .count();
                    first_span = (at as u32, run as u32);
                }
            }
        }
    }
    if bad > 0 {
        out.push(Diag::with_span(
            "E101",
            Category::Correctness,
            Severity::Error,
            first,
            first_span.0,
            first_span.1,
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

/// Byte offset where the payload starts: after "N CONC " when the line has a
/// CONC header, else 0 (the whole line is the payload). An offset rather than
/// a subslice, because E101 reports it as the `col` of its span.
fn conc_payload_start(line: &[u8]) -> usize {
    // Find " CONC " at byte level.
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        p + pat.len()
    } else if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let after = p + 4;
        if line.get(after) == Some(&b' ') {
            after + 1
        } else {
            after
        }
    } else {
        0
    }
}
