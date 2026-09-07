//! `--fix`: only safe, invertible repairs. The caller (`src/main.rs`) is the
//! one that writes files and keeps the `.bak` copy; this module is pure.

use crate::parse::{normalize_newlines, MAX_LEVEL};

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

