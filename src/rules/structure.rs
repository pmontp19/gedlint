//! Document skeleton rules: HEAD/TRLR placement, level continuity, xref
//! syntax, CONT/CONC anchoring and the HEAD singletons (E001-E009).

use std::collections::HashMap;

use crate::diag::{Category, Diag, Severity};
use crate::parse::{is_pointer, truncate, Line, Version};

/// Skeleton state carried across the single pass.
#[derive(Default)]
pub(crate) struct Structure {
    pub(crate) saw_head: bool,
    pub(crate) saw_trlr: bool,
    pub(crate) first_done: bool,
    pub(crate) after_trlr: bool,
    pub(crate) prev_level: Option<u32>,
    // HEAD scope for E008 (GEDC/VERS singletons) and E009.
    pub(crate) in_head_main: bool,
    pub(crate) saw_gedc_line: Option<usize>,
    pub(crate) saw_vers_line: Option<usize>,
    // E008 VERS singletons inside HEAD, keyed by the parent's line: GEDC.VERS
    // (GEDCOM version), SOUR.VERS (product version) and CHAR.VERS are three
    // different {0:1} slots, not one.
    pub(crate) head_vers_seen: HashMap<usize, usize>,
}

/// E002: HEAD must be the first line; nothing may follow TRLR.
pub(crate) fn check_position(diags: &mut Vec<Diag>, st: &mut Structure, l: &Line, lvl: u32) {
    if !l.raw.trim().is_empty() {
        if !st.first_done {
            st.first_done = true;
            if !(lvl == 0 && l.tag == "HEAD") {
                diags.push(Diag::new(
                    "E002",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    "HEAD must be the first line".into(),
                ));
            }
        } else if st.after_trlr {
            diags.push(Diag::new(
                "E002",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("content after TRLR: {}", truncate(&l.raw, 50)),
            ));
        }
    }
}

/// E001: level jump > +1. Also advances the level cursor.
pub(crate) fn check_level_jump(diags: &mut Vec<Diag>, st: &mut Structure, l: &Line, lvl: u32) {
    // E001: level jump > +1.
    if let Some(p) = st.prev_level {
        if lvl > p + 1 {
            diags.push(Diag::new(
                "E001",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("level jump {} -> {} (max +1)", p, lvl),
            ));
        }
    }
    st.prev_level = Some(lvl);
}

/// E004: xref syntax (@id@, no spaces, closed).
pub(crate) fn check_xref_syntax(diags: &mut Vec<Diag>, l: &Line) {
    if !l.xref.is_empty() && !is_pointer(&l.xref) {
        // Span: the xref token. It is the second whitespace-separated field
        // and everything before it is the numeric level, so the first
        // occurrence of the token in the raw line is the token itself.
        let (col, len) = l.span_of(&l.xref);
        diags.push(Diag::with_span(
            "E004",
            Category::Correctness,
            Severity::Error,
            l.no,
            col,
            len,
            format!("malformed xref: {}", l.xref),
        ));
    }
}

/// E005/E007 on CONT/CONC, and the anchor the next line is judged against.
pub(crate) fn check_continuation(
    diags: &mut Vec<Diag>,
    l: &Line,
    version: Version,
    parent: &Option<(String, usize)>,
) {
    let parent_tag: &str = parent.as_ref().map(|p| p.0.as_str()).unwrap_or("");
    // CONT/CONC must hang off a parent line one level up (structural
    // check, not value-based: a parent with an empty value, e.g.
    // `2 TEXT` followed by `3 CONT foo`, is still a valid parent since
    // the value is optional in both 5.5.1 and 7.0). CONT/CONC are
    // pseudo-substructures of the value-bearing line and never nest,
    // so a CONT/CONC hanging off another CONT/CONC is also malformed.
    // Note: on input that already trips E001 (a level jump skips a
    // level), the truncate above is a no-op and `parent` resolves to
    // the nearest actual ancestor rather than the true one-level-up
    // line, so this check can miss a genuine orphan in that case.
    if l.tag == "CONT" || l.tag == "CONC" {
        let nested_under_cont_conc = parent_tag == "CONT" || parent_tag == "CONC";
        if parent.is_none() || nested_under_cont_conc {
            let msg = if parent.is_none() {
                format!("{} has no parent line to continue", l.tag)
            } else {
                format!(
                    "{} cannot continue a {} line (CONT/CONC do not nest)",
                    l.tag, parent_tag
                )
            };
            diags.push(Diag::new(
                "E005",
                Category::Correctness,
                Severity::Error,
                l.no,
                msg,
            ));
        }
    }
    // E007: CONC was removed in 7.0 (spec 1.3, reserved tag): reflow to CONT.
    if version == Version::V70 && l.tag == "CONC" {
        diags.push(Diag::new(
            "E007",
            Category::Correctness,
            Severity::Error,
            l.no,
            "CONC is reserved in 7.0 (spec 1.3): split the value into CONT lines".into(),
        ));
    }
}

/// Level-0 record change: HEAD/TRLR bookkeeping.
pub(crate) fn enter_record(st: &mut Structure, l: &Line) {
    // HEAD scope for E008 (GEDC/VERS singletons) and E009.
    st.in_head_main = l.tag == "HEAD";
    if l.tag == "HEAD" {
        st.saw_head = true;
    }
    if l.tag == "TRLR" {
        st.saw_trlr = true;
        st.after_trlr = true;
    }
}

/// HEAD.CHAR: removed in 7.0 (UTF-8 assumed); 5.5.1 has 4 legal values.
pub(crate) fn check_head_char(diags: &mut Vec<Diag>, st: &Structure, l: &Line, version: Version) {
    if st.in_head_main && l.tag == "CHAR" {
        if version == Version::V70 {
            diags.push(Diag::new(
                "U501",
                Category::Upgrade,
                Severity::Info,
                l.no,
                "CHAR removed in 7.0 (UTF-8 is assumed)".into(),
            ));
        } else {
            const CHAR551: &[&str] = &["ANSEL", "ASCII", "UNICODE", "UTF-8"];
            if !CHAR551.contains(&l.value.trim().to_ascii_uppercase().as_str()) {
                diags.push(Diag::new(
                    "W306",
                    Category::Suspicious,
                    Severity::Warning,
                    l.no,
                    format!(
                        "invalid HEAD.CHAR {:?} (expected ANSEL/ASCII/UNICODE/UTF-8)",
                        l.value.trim()
                    ),
                ));
            }
        }
    }
}

/// E008: HEAD.GEDC is a {1:1} singleton.
pub(crate) fn check_head_gedc(diags: &mut Vec<Diag>, st: &mut Structure, l: &Line) {
    if st.in_head_main && l.tag == "GEDC" {
        if let Some(first) = st.saw_gedc_line {
            diags.push(Diag::new(
                "E008",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("duplicate HEAD.GEDC (first at line {})", first),
            ));
        } else {
            st.saw_gedc_line = Some(l.no);
        }
    }
}

/// E008: VERS is {0:1} per HEAD substructure, scoped by its parent block.
pub(crate) fn check_head_vers(
    diags: &mut Vec<Diag>,
    st: &mut Structure,
    l: &Line,
    lvl: u32,
    parent: &Option<(String, usize)>,
) {
    if st.in_head_main && lvl == 2 && l.tag == "VERS" {
        if let Some((ptag, pline)) = parent.clone() {
            if let Some(first) = st.head_vers_seen.get(&pline) {
                diags.push(Diag::new(
                    "E008",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("duplicate {}.VERS (first at line {})", ptag, first),
                ));
            } else {
                st.head_vers_seen.insert(pline, l.no);
                // E009 requires GEDC.VERS specifically.
                if ptag == "GEDC" {
                    st.saw_vers_line = Some(l.no);
                }
            }
        }
    }
}

/// End of run: the records and substructures that must exist (E002, E009).
pub(crate) fn finish(diags: &mut Vec<Diag>, st: &Structure) {
    // E002: HEAD/TRLR are required.
    if !st.saw_head {
        diags.push(Diag::new(
            "E002",
            Category::Correctness,
            Severity::Error,
            0,
            "missing HEAD record".into(),
        ));
    }
    if !st.saw_trlr {
        diags.push(Diag::new(
            "E002",
            Category::Correctness,
            Severity::Error,
            0,
            "missing TRLR record".into(),
        ));
    }

    // E009: HEAD.GEDC and GEDC.VERS are {1:1} in both 5.5.1 and 7.0.
    if st.saw_head && st.saw_gedc_line.is_none() {
        diags.push(Diag::new(
            "E009",
            Category::Correctness,
            Severity::Error,
            0,
            "HEAD without required GEDC".into(),
        ));
    }
    if st.saw_gedc_line.is_some() && st.saw_vers_line.is_none() {
        diags.push(Diag::new(
            "E009",
            Category::Correctness,
            Severity::Error,
            0,
            "GEDC without required VERS".into(),
        ));
    }
}
