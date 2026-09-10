//! `--fix`: structured, selectable repairs. Only safe, invertible ones are
//! applied by default (`AGENTS.md`); the caller (`src/main.rs`) is the one
//! that writes files and keeps the `.bak` copy, this module is pure.
//!
//! A repair is data: [`compute_edits_with`] proposes every candidate over
//! an inclusive 1-based **line range** gated by the configuration (a
//! disabled rule proposes nothing, #44), [`apply_edits`] applies a chosen
//! subset. Line ranges (not intra-line spans) are the right unit here
//! because the repairs are not all intra-line: rejoining a split `CONC`
//! replaces two lines with one, prefixing an orphan rewrites one line in
//! place, and an empty replacement deletes the range.
//!
//! Line-ending normalization is whole-file preprocessing
//! ([`normalize_endings`]), never a per-rule edit.

use std::borrow::Cow;

use crate::config::Config;
use crate::parse::{normalize_newlines, MAX_LEVEL};

/// How much a repair can be trusted, in Biome's sense. This is what lets an
/// opinionated repair exist without ever firing automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    /// Provably meaning-preserving. Applied by a bare `--fix`.
    Safe,
    /// Probably right, needs human eyes. Never applied without opt-in.
    MaybeIncorrect,
}

impl Applicability {
    /// Stable string for JSON consumers (`fixable` in the registry).
    pub fn as_str(&self) -> &'static str {
        match self {
            Applicability::Safe => "safe",
            Applicability::MaybeIncorrect => "maybe-incorrect",
        }
    }
}

/// One candidate repair: replace an inclusive, 1-based range of lines with
/// `replacement`.
///
/// `replacement` holds **bytes**, not `String`s, for the same reason `Diag`
/// spans are byte offsets: the E101 repair joins a UTF-8 sequence that the
/// exporter split across two lines, so neither the input lines nor (in the
/// pathological cases) the joined result are guaranteed to be valid UTF-8,
/// and a lossy conversion would corrupt exactly the files the repair exists
/// for. Text rules build one with `line.into_bytes()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub code: &'static str,
    /// Inclusive 1-based line range this edit replaces.
    pub lines: (usize, usize),
    /// Replacement lines, without terminators. Empty = delete the range.
    pub replacement: Vec<Vec<u8>>,
    pub applicability: Applicability,
    /// One line for the UI: "rejoin a CONC line split inside a UTF-8 sequence".
    pub note: String,
}

/// Which repairs a caller wants applied. `Default` is the `--fix` contract:
/// every `Safe` edit, nothing else.
#[derive(Debug, Clone, Default)]
pub struct FixSelection {
    /// Restrict to these rule codes (ASCII case-insensitive). Empty = all.
    pub only: Vec<String>,
    /// Also apply `MaybeIncorrect` edits (the CLI's `--unsafe`).
    pub allow_unsafe: bool,
}

impl FixSelection {
    /// Whether this selection wants `e` applied.
    pub fn allows(&self, e: &Edit) -> bool {
        if !self.allow_unsafe && e.applicability != Applicability::Safe {
            return false;
        }
        self.only.is_empty() || self.only.iter().any(|c| c.eq_ignore_ascii_case(e.code))
    }
}

/// Whole-file preprocessing: bare CR (classic Mac, legal in 5.5.1) to LF,
/// CRLF untouched. The bool says whether any lone CR was rewritten.
///
/// This is not a per-rule edit: it is the step [`compute_edits`] and
/// [`apply_edits`] assume, since both split lines on `\n`. Once it has run,
/// edit line numbers agree with `Diag::line`.
pub fn normalize_endings(data: &[u8]) -> (Cow<'_, [u8]>, bool) {
    // Only report it when it actually changes bytes: a pure-CRLF file
    // reassembles byte-identical, and main.rs then prints nothing thanks to
    // its `fixed != data` check.
    let mut lone_cr = false;
    let mut it = data.iter().peekable();
    while let Some(&b) = it.next() {
        if b == b'\r' && it.peek() != Some(&&b'\n') {
            lone_cr = true;
            break;
        }
    }
    (normalize_newlines(data), lone_cr)
}

/// Every candidate repair, without applying any, under the built-in
/// configuration (`recommended`, no opt-in ruleset): a pattern only an
/// opt-in ruleset repairs is left alone here, exactly as `lint_str` leaves
/// it unreported. Same as [`compute_edits_with`] with [`Config::default`].
///
/// - `E001`: a line without a level (MyHeritage NOTE/TEXT continuations
///   without `CONT`) gets a `{previous_level + 1} CONT ` prefix.
/// - `E005`: a `CONT`/`CONC` nested under another `CONT`/`CONC` is
///   re-leveled to sit beside the run, one under the value line.
/// - `style`: trailing whitespace is trimmed. The linter has no code for
///   this, so the pseudo-code matches the `--fix` report line.
/// - `E101`: a run of `CONC` lines whose payload starts mid-UTF-8-sequence
///   is rejoined into the line above it.
///
/// Returned in **repair-priority order**, not line order: `apply_edits`
/// keeps the first of two overlapping edits, and the pass that runs first
/// decides what a later pass on the same lines reads (`E001` before
/// `E005` because the `CONT` prefix it writes can itself become the
/// parent of a deeper continuation; `E005` before `E101` because the
/// rejoin absorbs the `CONC` line a nested continuation hangs from;
/// `style` before `E101` because a pad the rejoin absorbs must not land
/// between the two halves of the cut character). A caller that re-runs
/// picks up the dropped ones.
pub fn compute_edits(data: &[u8]) -> Vec<Edit> {
    compute_edits_with(data, &Config::default())
}

/// [`compute_edits`] under an explicit configuration, gated at production
/// (#44): an edit belonging to a rule the configuration disables is never
/// produced, so no consumer can rewrite a file according to a rule the same
/// configuration reports nothing for. The gate lives here rather than at
/// the call sites so every consumer (the CLI's `--fix`, the web viewer)
/// inherits it instead of having to remember it. The `style` pseudo-code
/// has no rule to configure and is always proposed; line-ending
/// normalization is preprocessing and always runs.
pub fn compute_edits_with(data: &[u8], cfg: &Config) -> Vec<Edit> {
    let lines = split_lines(data);
    let mut out = Vec::new();
    if cfg.enables("E001") {
        orphan_edits(&lines, &mut out);
    }
    if cfg.enables("E005") {
        nested_cont_edits(&lines, &mut out);
    }
    whitespace_edits(&lines, &mut out);
    if cfg.enables("E101") {
        conc_edits(&lines, &mut out);
    }
    if cfg.enables("W601") {
        w601_edits(&lines, &mut out);
    }
    if cfg.enables("W702") {
        w702_edits(&lines, &mut out);
    }
    if cfg.enables("W703") {
        w703_edits(&lines, &mut out);
    }
    out
}

/// Apply a chosen subset. Overlapping ranges are resolved by keeping the
/// first and dropping the rest; an out-of-range or inverted range is dropped
/// too. The dropped edits are returned so the caller can re-run.
///
/// A dropped edit still reserves its lines, so a lower-priority edit cannot
/// slip inside the range of a repair that is merely postponed.
pub fn apply_edits(data: &[u8], edits: &[Edit]) -> (Vec<u8>, Vec<Edit>) {
    let lines = split_lines(data);
    let mut kept: Vec<&Edit> = Vec::new();
    let mut dropped: Vec<Edit> = Vec::new();
    // One flag per line instead of comparing every pair of ranges: a file
    // with trailing whitespace on most of its lines yields one edit per
    // line, and O(n^2) there is measurable.
    let mut claimed = vec![false; lines.len()];
    for e in edits {
        let (a, b) = e.lines;
        if a < 1 || a > b || b > lines.len() {
            dropped.push(e.clone());
            continue;
        }
        if claimed[a - 1..b].iter().any(|&c| c) {
            dropped.push(e.clone());
        } else {
            kept.push(e);
        }
        claimed[a - 1..b].fill(true);
    }
    kept.sort_by_key(|e| e.lines.0);

    let mut out = Vec::with_capacity(data.len());
    let mut first = true;
    let mut i = 0; // 0-based line index
    let mut next = 0; // index into `kept`
    while i < lines.len() {
        match kept.get(next) {
            Some(e) if e.lines.0 == i + 1 => {
                for r in &e.replacement {
                    push_line(&mut out, &mut first, r);
                }
                i = e.lines.1; // 1-based end == 0-based index just past it
                next += 1;
            }
            _ => {
                push_line(&mut out, &mut first, lines[i]);
                i += 1;
            }
        }
    }
    // Preserve the original trailing newline.
    if data.ends_with(b"\n") && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    (out, dropped)
}

/// Safe repairs applied by `--fix`: normalize line endings, then apply every
/// `Safe` edit, both under the built-in configuration. Returns the repaired
/// bytes and one report line per code.
pub fn fix_bytes(data: &[u8]) -> (Vec<u8>, Vec<String>) {
    fix_bytes_with(data, &FixSelection::default(), &Config::default())
}

/// `fix_bytes` with an explicit selection (`--fix --only` / `--fix
/// --unsafe`) and configuration (`gedlint.toml`): the selection narrows the
/// repairs, the configuration gates them (#44), and neither can widen the
/// other's reach (`--only W601` without the ruleset enabled repairs
/// nothing). Line-ending normalization is preprocessing and always runs:
/// the line model depends on it.
pub fn fix_bytes_with(data: &[u8], sel: &FixSelection, cfg: &Config) -> (Vec<u8>, Vec<String>) {
    let (norm, lone_cr) = normalize_endings(data);
    let mut cur = norm.into_owned();
    let mut applied: Vec<String> = Vec::new();
    // Recomputed only by a pass that actually rewrote something, so a file
    // whose only repair is trailing whitespace costs two scans, not four.
    let mut edits = compute_edits_with(&cur, cfg);

    for code in REPAIR_ORDER {
        run_stage(&mut cur, &mut edits, &mut applied, sel, cfg, code);
    }
    // A repair `REPAIR_ORDER` does not name would never be applied. Adding
    // one is a deliberate decision about where in the pipeline it belongs,
    // so fail loudly in the test build rather than pick an order silently.
    debug_assert!(
        edits.iter().all(|e| REPAIR_ORDER.contains(&e.code)),
        "a repair code is missing from REPAIR_ORDER, so --fix would skip it: {:?}",
        edits
            .iter()
            .map(|e| e.code)
            .filter(|c| !REPAIR_ORDER.contains(c))
            .collect::<Vec<_>>()
    );

    if lone_cr {
        applied.push("style: normalized classic Mac CR line endings to LF".into());
    }
    (cur, applied)
}

/// The order `--fix` applies repairs, which is also the order
/// `compute_edits` returns them in. Exactly **one pass per code**, in this
/// order, and that is the whole control flow.
///
/// `E005` sits between `E001` and `style`/`E101`, and both neighbours are
/// load-bearing:
///
/// * After `E001`: the `CONT` prefix E001 writes can itself become the
///   parent of a deeper continuation that was legal before (its parent
///   used to be the levelless line, which never enters the stack), so the
///   nesting E001 exposes must be repaired by a later pass. Each pass
///   recomputes over what the previous left, so one `--fix` run settles
///   the compounded case.
/// * Before `E101` (#36 is what happens when this kind of ordering is
///   guessed wrong): the rejoin absorbs the `CONC` line a nested
///   continuation hangs from. Run first, it would leave that continuation
///   at a level no line supports any more, and `--fix` would finish
///   having minted a fresh E001 level jump no repair can settle. `E005`
///   flattens the continuation first, then the rejoin runs over siblings.
/// * `style` only trims the tail and `E005` only rewrites the leading
///   digits, so the two commute on a line needing both; it groups with
///   `E001`, the other column-one repair.
///
/// One pass each because two edits on one line (an orphan that also has
/// trailing whitespace) overlap by construction, so a single pass cannot
/// apply both.
///
/// Exactly one, and never a fixpoint, because re-running a code would
/// repair things `--fix` never reported: rejoining a `CONC` can turn a
/// blank line into a levelless one, and a second `E001` pass would prefix
/// it with `CONT`, silently inventing a `CONT` record. The next `--fix`
/// picks up whatever this one exposed.
const REPAIR_ORDER: [&str; 7] = ["E001", "E005", "style", "E101", "W601", "W702", "W703"];

/// One pass for one code, over whatever the previous pass left behind.
fn run_stage(
    cur: &mut Vec<u8>,
    edits: &mut Vec<Edit>,
    applied: &mut Vec<String>,
    sel: &FixSelection,
    cfg: &Config,
    code: &str,
) {
    let wanted = |e: &Edit| e.code == code && sel.allows(e);
    if !edits.iter().any(wanted) {
        return;
    }
    let chosen: Vec<Edit> = std::mem::take(edits).into_iter().filter(wanted).collect();
    let (next, dropped) = apply_edits(cur, &chosen);
    *cur = next;
    *edits = compute_edits_with(cur, cfg);
    // Same-code edits never overlap today, so `dropped` is empty; a future
    // rule that breaks that gets the rest on the next run.
    let n = weight(&chosen).saturating_sub(weight(&dropped));
    if n > 0 {
        applied.push(report(code, n));
    }
}

/// How many repairs a set of applied edits stands for in the report line:
/// a rejoin removes one line per `CONC` it swallowed, everything else is
/// one line rewritten in place.
fn weight(edits: &[Edit]) -> usize {
    edits
        .iter()
        .map(|e| {
            if e.code == "E101" {
                e.lines.1 - e.lines.0
            } else {
                1
            }
        })
        .sum()
}

/// The `--fix` report line for one code. The wording is CLI output. A code
/// with no line of its own gets a generic one instead of no report at all.
fn report(code: &str, n: usize) -> String {
    match code {
        "E001" => format!("E001: {} orphan lines prefixed with CONT", n),
        "E005" => format!("E005: re-leveled {} nested CONT/CONC lines", n),
        "E101" => format!("E101: rejoined {} CONC lines with split UTF-8", n),
        "W601" => format!("W601: removed comma from {} surnames", n),
        "W702" => format!("W702: converted {} surnames to title case", n),
        "W703" => format!("W703: removed doubled commas from {} place names", n),
        "style" => format!("style: trimmed trailing whitespace on {} lines", n),
        c => format!("{}: {} repairs applied", c, n),
    }
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// E001: a line with no level is a continuation the exporter forgot to tag.
fn orphan_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    let mut prev_level: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let body = strip_cr(l);
        // Blank/whitespace-only lines are not orphans (the whitespace rule
        // trims them); same definition lint_lines uses, and they must not
        // break the run.
        if body.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        match leading_level(body) {
            // The anchor only moves on a line that really has a level: a run
            // of orphans is a run of siblings under it, not a staircase.
            Some(lvl) => prev_level = Some(lvl),
            None => {
                let Some(p) = prev_level else { continue };
                if p >= MAX_LEVEL as usize {
                    continue;
                }
                let mut nl = format!("{} CONT ", p + 1).into_bytes();
                nl.extend_from_slice(body);
                if l.last() == Some(&b'\r') {
                    nl.push(b'\r');
                }
                out.push(Edit {
                    code: "E001",
                    lines: (i + 1, i + 1),
                    replacement: vec![nl],
                    applicability: Applicability::Safe,
                    note: format!("prefix the orphan line with \"{} CONT \"", p + 1),
                });
            }
        }
    }
}

/// E005, nesting branch only: a `CONT`/`CONC` sitting one or more levels
/// below another `CONT`/`CONC` instead of beside it. `CONT`/`CONC` are
/// pseudo-substructures of the value-bearing line and never nest, so a
/// `CONT` under a `CONC` has exactly one reading: it continues the value
/// the `CONC` continues, and only its level number is wrong. The repair
/// rewrites column one to `{level of the nearest enclosing non-CONT/CONC
/// line} + 1`, which is what makes it safe and invertible: no byte of the
/// content moves.
///
/// The other branch of E005 (a `CONT`/`CONC` with no parent at all) is
/// deliberately not repaired: no line says what it was meant to continue,
/// and inventing a parent would guess at the user's data.
///
/// A staircase of increasingly nested continuations collapses in one pass
/// because every line is re-leveled independently, against the original
/// context.
fn nested_cont_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    for (i, l) in lines.iter().enumerate() {
        let body = strip_cr(l);
        let Some((lvl, digits, tag)) = level_tag_and_digits(body) else {
            continue;
        };
        if tag != b"CONT" && tag != b"CONC" {
            continue;
        }
        let Some((pj, plvl, ptag)) = nearest_ancestor(lines, i, lvl) else {
            continue; // no parent at all: reported, never repaired
        };
        if ptag != b"CONT" && ptag != b"CONC" {
            continue; // an ordinary parent: not this branch of E005
        }
        // Walk the parent chain to the nearest enclosing line that is not
        // itself a CONT/CONC: that is the value line the whole run
        // continues, and the repair puts the continuation directly under
        // it. Each chain step is at least one level shallower than the
        // line it parents, so the target is always a legal level. A chain
        // that tops out inside the unrepaired orphan branch has no anchor;
        // the line stays reported.
        let mut j = pj;
        let mut cur = plvl;
        let target = loop {
            match nearest_ancestor(lines, j, cur) {
                Some((j2, l2, t2)) if t2 == b"CONT" || t2 == b"CONC" => {
                    j = j2;
                    cur = l2;
                }
                Some((_, l2, _)) => break Some(l2 + 1),
                None => break None,
            }
        };
        let Some(target) = target else { continue };
        // Only the digit run is replaced; the separator byte and everything
        // after it are copied verbatim, so a pathological leading-zero
        // level ("02 CONT ...") cannot grow an extra space.
        let mut nl = target.to_string().into_bytes();
        nl.extend_from_slice(&body[digits..]);
        if l.last() == Some(&b'\r') {
            nl.push(b'\r');
        }
        out.push(Edit {
            code: "E005",
            lines: (i + 1, i + 1),
            replacement: vec![nl],
            applicability: Applicability::Safe,
            note: format!("rewrite the level to {} (CONT/CONC do not nest)", target),
        });
    }
}

/// E101: a `CONC` payload starting with a UTF-8 continuation byte is the
/// second half of a character the exporter cut in two. A run of them all
/// belongs to the line above the run, not to each other.
fn conc_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    let mut i = 1; // the first line has nothing to rejoin into
    while i < lines.len() {
        if split_conc_pos(lines[i]).is_none() {
            i += 1;
            continue;
        }
        let mut joined = strip_cr(lines[i - 1]).to_vec();
        // A CRLF file must stay CRLF (#36): the joined line takes the
        // anchor's terminator, which the payloads never carry.
        let anchor_cr = lines[i - 1].last() == Some(&b'\r');
        let mut end = i;
        while let Some(pos) = lines.get(end).and_then(|l| split_conc_pos(l)) {
            joined.extend_from_slice(&strip_cr(lines[end])[pos..]);
            end += 1;
        }
        if anchor_cr {
            joined.push(b'\r');
        }
        let n = end - i;
        out.push(Edit {
            code: "E101",
            lines: (i, end),
            replacement: vec![joined],
            applicability: Applicability::Safe,
            note: if n == 1 {
                "rejoin a CONC line split inside a UTF-8 sequence".into()
            } else {
                format!("rejoin {} CONC lines split inside a UTF-8 sequence", n)
            },
        });
        i = end;
    }
}

/// Trailing whitespace. No rule code reports it, so the edit carries the
/// same `style` pseudo-code the `--fix` report line has always used.
fn whitespace_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    for (i, l) in lines.iter().enumerate() {
        let has_cr = l.last() == Some(&b'\r');
        let body = strip_cr(l);
        let trimmed = rtrim_ws(body);
        if trimmed.len() == body.len() {
            continue;
        }
        let mut nl = trimmed.to_vec();
        if has_cr {
            nl.push(b'\r');
        }
        out.push(Edit {
            code: "style",
            lines: (i + 1, i + 1),
            replacement: vec![nl],
            applicability: Applicability::Safe,
            note: "trim trailing whitespace".into(),
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    data.split(|&b| b == b'\n').collect()
}

fn push_line(out: &mut Vec<u8>, first: &mut bool, line: &[u8]) {
    if *first {
        *first = false;
    } else {
        out.push(b'\n');
    }
    out.extend_from_slice(line);
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
    if digits > 0
        && n <= MAX_LEVEL as usize
        && (line.get(digits) == Some(&b' ') || line.len() == digits)
    {
        Some(n)
    } else {
        None
    }
}

/// `(level, digit count, tag)` of a raw line, byte-based so the walk also
/// works on lines that are not valid UTF-8. The level grammar is
/// [`leading_level`]'s: digits, then a space or end of line. A level-0
/// record line reports its xref token as the tag, which is harmless here:
/// the only comparison is against CONT/CONC, and an xref token never
/// spells either.
fn level_tag_and_digits(line: &[u8]) -> Option<(usize, usize, &[u8])> {
    let lvl = leading_level(line)?;
    let digits = line.iter().take_while(|b| b.is_ascii_digit()).count();
    // A bare number is a level with an empty tag, the same way
    // `parse_line` reads it, so it anchors the walk like any other line.
    let rest = line.get(digits + 1..).unwrap_or(&[]);
    let end = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
    Some((lvl, digits, &rest[..end]))
}

/// The parent the linter resolves for `lines[i]`: the nearest preceding
/// line with a level strictly below `below`. That is exactly
/// `stack.last()` after the truncation in `lint_lines`, including on
/// input that trips E001, where the stack degrades to the nearest actual
/// ancestor. Blank and levelless lines never enter the linter's stack, so
/// they are skipped here too.
fn nearest_ancestor<'a>(
    lines: &[&'a [u8]],
    i: usize,
    below: usize,
) -> Option<(usize, usize, &'a [u8])> {
    (0..i).rev().find_map(|j| {
        let (lvl, _, tag) = level_tag_and_digits(strip_cr(lines[j]))?;
        (lvl < below).then_some((j, lvl, tag))
    })
}

fn strip_cr(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') {
        &line[..line.len() - 1]
    } else {
        line
    }
}

/// Payload start of a `CONC` line whose payload begins with a UTF-8
/// continuation byte (0x80..=0xBF), i.e. one half of a split character.
fn split_conc_pos(line: &[u8]) -> Option<usize> {
    let s = strip_cr(line);
    let pos = conc_pos(s)?;
    if s.get(pos)
        .map(|b| (0x80..=0xBF).contains(b))
        .unwrap_or(false)
    {
        Some(pos)
    } else {
        None
    }
}

/// Byte offset just after "CONC " anywhere in the line, or after a "CONC"
/// with no value separator at all, which is what a broken exporter emits
/// when the payload starts with the second half of a character.
fn conc_pos(line: &[u8]) -> Option<usize> {
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        return Some(p + pat.len());
    }
    // No space after this "CONC": if there were one, the search above would
    // have matched it first.
    Some(line.windows(4).position(|w| w == b"CONC")? + 4)
}

fn rtrim_ws(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b' ' || b[end - 1] == b'\t') {
        end -= 1;
    }
    &b[..end]
}

// `"<level> <TAG> <value>"` -> `(level, TAG, value)`; None when the prefix
// before the tag is not a bare level number, so a NOTE value that happens
// to contain the tag text can never be mistaken for the line itself.
fn split_tag(line: &str) -> Option<(u32, &str, &str)> {
    let tag_at = line.find(|c: char| !c.is_ascii_digit())?;
    if tag_at == 0 {
        return None;
    }
    let level: u32 = line[..tag_at].parse().ok()?;
    let rest = line[tag_at..].strip_prefix(' ')?;
    let (tag, value) = match rest.find(' ') {
        Some(sp) => (&rest[..sp], &rest[sp + 1..]),
        None => (rest, ""),
    };
    Some((level, tag, value))
}

// Whether the SURN line at `i` really hangs under a NAME line: the
// diagnostic only fires there (the linter walks the same parent relation),
// so a repair must not reach a stray SURN under some other level-1 tag.
fn parent_is_name(lines: &[&[u8]], i: usize) -> bool {
    for j in (0..i).rev() {
        if let Some((level, tag, _)) = split_tag(&String::from_utf8_lossy(lines[j])) {
            if level < 2 {
                return tag == "NAME";
            }
        }
    }
    false
}

// W601
fn w601_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    for (i, l) in lines.iter().enumerate() {
        let s = String::from_utf8_lossy(l);
        let Some((_, tag, value)) = split_tag(&s) else {
            continue;
        };
        match tag {
            // The surname slot of "1 NAME": the same shape check the
            // diagnostic uses, rewritten in place between the slashes.
            "NAME" => {
                if let Some(start) = s.find('/') {
                    if let Some(end) = s[start + 1..].find('/') {
                        let surname = &s[start + 1..start + 1 + end];
                        if let Some((a, b)) = crate::rules::hispanic_naming::comma_split(surname) {
                            let mut nl = s.to_string();
                            let new_surname = format!("{} {}", a, b);
                            nl.replace_range(start + 1..start + 1 + end, &new_surname);
                            out.push(Edit {
                                code: "W601",
                                lines: (i + 1, i + 1),
                                replacement: vec![nl.into_bytes()],
                                applicability: Applicability::Safe,
                                note: "remove comma from surname".to_string(),
                            });
                        }
                    }
                }
            }
            // "2 SURN": the whole value is the surname, same conservative
            // shape, same note in the registry. Only under a NAME, where
            // the diagnostic lives.
            "SURN" if parent_is_name(lines, i) => {
                // Cut any CR terminator off first (CRLF file): the
                // rewrite must never touch line endings.
                let body = value.strip_suffix('\r').unwrap_or(value);
                if let Some((a, b)) = crate::rules::hispanic_naming::comma_split(body) {
                    let val_off = s.len() - value.len();
                    let body_end = val_off + body.len();
                    let mut nl = s.to_string();
                    nl.replace_range(val_off..body_end, &format!("{} {}", a, b));
                    out.push(Edit {
                        code: "W601",
                        lines: (i + 1, i + 1),
                        replacement: vec![nl.into_bytes()],
                        applicability: Applicability::Safe,
                        note: "remove comma from surname".to_string(),
                    });
                }
            }
            _ => {}
        }
    }
}

// W702
fn w702_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    for (i, l) in lines.iter().enumerate() {
        let s = String::from_utf8_lossy(l);
        let Some((_, tag, value)) = split_tag(&s) else {
            continue;
        };
        match tag {
            // The surname slot of "1 NAME".
            "NAME" => {
                if let Some(start) = s.find('/') {
                    if let Some(end) = s[start + 1..].find('/') {
                        let surname = &s[start + 1..start + 1 + end];
                        if crate::rules::hygiene::is_all_caps(surname) {
                            let mut nl = s.to_string();
                            nl.replace_range(start + 1..start + 1 + end, &title_case(surname));
                            out.push(Edit {
                                code: "W702",
                                lines: (i + 1, i + 1),
                                replacement: vec![nl.into_bytes()],
                                applicability: Applicability::MaybeIncorrect,
                                note: "convert all-caps surname to title case".to_string(),
                            });
                        }
                    }
                }
            }
            // "2 SURN": the whole value is the surname. Only under a NAME.
            "SURN" if parent_is_name(lines, i) => {
                let body = value.strip_suffix('\r').unwrap_or(value);
                if crate::rules::hygiene::is_all_caps(body) {
                    let val_off = s.len() - value.len();
                    let body_end = val_off + body.len();
                    let mut nl = s.to_string();
                    nl.replace_range(val_off..body_end, &title_case(body));
                    out.push(Edit {
                        code: "W702",
                        lines: (i + 1, i + 1),
                        replacement: vec![nl.into_bytes()],
                        applicability: Applicability::MaybeIncorrect,
                        note: "convert all-caps surname to title case".to_string(),
                    });
                }
            }
            _ => {}
        }
    }
}

// Title case for the W702 repair: each whitespace-separated token keeps its
// first letter and lowercases the rest. Lossy on purpose (MCDONALD, DE LA
// O), which is exactly why the repair is MaybeIncorrect.
fn title_case(surname: &str) -> String {
    let mut out = String::new();
    for token in surname.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            for u in first.to_uppercase() {
                out.push(u);
            }
            for c in chars {
                for l in c.to_lowercase() {
                    out.push(l);
                }
            }
        }
    }
    out
}

// W703
fn w703_edits(lines: &[&[u8]], out: &mut Vec<Edit>) {
    for (i, l) in lines.iter().enumerate() {
        let s = String::from_utf8_lossy(l);
        if s.contains(" PLAC ") && (s.contains(",,") || s.contains(", ,")) {
            let mut nl = s.to_string();
            while nl.contains(",,") || nl.contains(", ,") {
                nl = nl.replace(",,", ",");
                nl = nl.replace(", , ", ", ");
                nl = nl.replace(", ,", ",");
            }
            out.push(Edit {
                code: "W703",
                lines: (i + 1, i + 1),
                replacement: vec![nl.into_bytes()],
                applicability: Applicability::Safe,
                note: "remove doubled commas from place".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_repair_code_has_a_pass_and_a_report_line() {
        for code in REPAIR_ORDER {
            assert!(
                !report(code, 1).ends_with("repairs applied"),
                "{} needs its own report line",
                code
            );
            assert!(report(code, 7).starts_with(&format!("{}: ", code)));
        }
        // A code nobody wrote a line for still gets reported.
        assert_eq!(report("Z999", 2), "Z999: 2 repairs applied");
    }

    #[test]
    fn weight_counts_removed_lines_for_a_rejoin() {
        let rejoin = Edit {
            code: "E101",
            lines: (4, 7),
            replacement: vec![b"joined".to_vec()],
            applicability: Applicability::Safe,
            note: String::new(),
        };
        // Three CONC lines swallowed, so three repairs, not one edit.
        assert_eq!(weight(std::slice::from_ref(&rejoin)), 3);
        let in_place = Edit {
            code: "E001",
            lines: (4, 4),
            ..rejoin
        };
        assert_eq!(weight(std::slice::from_ref(&in_place)), 1);
    }
}
