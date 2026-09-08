//! Person and family semantics: SEX (E008/W305), lifespans (W301), parent
//! ages and children born before the marriage (W303/W304) and the duplicate
//! heuristic (W302).

use std::collections::{HashMap, HashSet};

use crate::diag::{push_capped, Category, Diag, Severity};
use crate::parse::{norm_name, year_of, Line, Version};
use crate::rules::graph::Graph;

/// Per-individual and per-family facts collected during the pass.
#[derive(Default)]
pub(crate) struct People {
    pub(crate) indi_birth: HashMap<String, Option<i64>>,
    pub(crate) indi_death: HashMap<String, Option<i64>>,
    // Dead with unknown date (DEAT Y without DATE): counts as dead
    // but stays out of W301 (no year, no longevity to check).
    pub(crate) indi_died_unknown: HashSet<String>,
    pub(crate) indi_name: HashMap<String, String>,
    pub(crate) indi_sex: HashMap<String, (String, usize)>,
    pub(crate) fam_marr: HashMap<String, Option<i64>>,
}

/// INDI.SEX: E008 on the {0:1} slot; the value itself is checked at the end.
pub(crate) fn record_sex(diags: &mut Vec<Diag>, st: &mut People, l: &Line, xref: &str) {
    // E008: INDI.SEX is {0:1}.
    if let Some((_, first)) = st.indi_sex.get(xref) {
        push_capped(
            diags,
            vec![Diag::new(
                "E008",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("duplicate SEX in {} (first at line {})", xref, first),
            )],
        );
    } else {
        st.indi_sex.insert(xref.to_string(), (l.value.clone(), l.no));
    }
}

/// FAM.MARR / INDI.BIRT / INDI.DEAT at level 1: the year carried on the event
/// line itself, plus "DEAT Y" (dead, date unknown).
pub(crate) fn record_event_year(st: &mut People, l: &Line, xref: &str, kind: &str) {
    if let Some(y) = year_of(&l.value) {
        match l.tag.as_str() {
            "BIRT" => {
                st.indi_birth.insert(xref.to_string(), Some(y));
            }
            "DEAT" => {
                st.indi_death.insert(xref.to_string(), Some(y));
            }
            _ => {
                if kind == "FAM" {
                    st.fam_marr.insert(xref.to_string(), Some(y));
                }
            }
        }
    }
    if l.tag == "DEAT" && l.value.trim() == "Y" {
        // Dead with unknown date: not a year, stays out of W301.
        st.indi_died_unknown.insert(xref.to_string());
    }
}

/// BIRT/DEAT/MARR DATE below level 1: the authoritative year for W301-W304.
pub(crate) fn record_sub_date(st: &mut People, l: &Line, cur_sub: &str, cur: &Option<(String, String)>) {
    if cur_sub == "BIRT" && l.tag == "DATE" {
        if let Some((xref, kind)) = cur.clone() {
            if kind == "INDI" {
                if let Some(y) = year_of(&l.value) {
                    st.indi_birth.insert(xref, Some(y));
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
                    st.indi_death.insert(xref, Some(y));
                }
            }
        }
    }
    if cur_sub == "MARR" && l.tag == "DATE" {
        if let Some((xref, kind)) = cur.clone() {
            if kind == "FAM" {
                st.fam_marr.insert(xref, year_of(&l.value));
            }
        }
    }
}

/// W301 for one individual: died before birth, or an implausible lifespan.
pub(crate) fn flush_person(diags: &mut Vec<Diag>, xref: &str, b: Option<i64>, d: Option<i64>, line: usize) {
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
}

/// End of run: W301 death before birth + longevity, per individual.
pub(crate) fn finish_lifespans(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    // W301: death before birth + longevity, per individual.
    for (xref, b) in &st.indi_birth {
        let d = st.indi_death.get(xref).copied().flatten();
        if let (Some(bb), Some(dd)) = (*b, d) {
            if dd < 10000 {
                flush_person(diags, xref, Some(bb), Some(dd), graph.records.get(xref).map(|r| r.1).unwrap_or(0));
            }
        }
    }
}

/// End of run: W303 parent age at the child's birth and W304 child born
/// before the marriage. Both report at the child's record line (issue 30):
/// the child's birth is what triggers them, and a real line makes the
/// whole-file graph findings deterministic.
pub(crate) fn finish_parent_ages(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    // W303: parent age at the child's birth.
    for (fam, chils) in &graph.fam_chil {
        for (c, _) in chils {
            let cb = st.indi_birth.get(c).copied().flatten();
            let Some(cb) = cb else { continue };
            if cb >= 10000 {
                continue;
            }
            // The child's record line: where the reader lands to fix the link.
            let child_line = graph.records.get(c).map(|r| r.1).unwrap_or(0);
            for (parent, rol) in [(&graph.fam_husb.get(fam), "father"), (&graph.fam_wife.get(fam), "mother")] {
                if let Some((px, _)) = parent {
                    if let Some(Some(pb)) = st.indi_birth.get(px) {
                        let age = cb - pb;
                        let max = if rol == "mother" { 50 } else { 70 };
                        if age < 13 || age > max {
                            push_capped(
                                diags,
                                vec![Diag::new(
                                    "W303",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    child_line,
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
            if let Some(Some(m)) = st.fam_marr.get(fam) {
                if cb < *m {
                    push_capped(
                        diags,
                        vec![Diag::new(
                            "W304",
                            Category::Suspicious,
                            Severity::Warning,
                            child_line,
                            format!("{}: {} born ({}) before marriage ({})", fam, c, cb, m),
                        )],
                    );
                }
            }
        }
    }
}

/// End of run: W305 SEX payload.
pub(crate) fn finish_sex(diags: &mut Vec<Diag>, st: &People, version: Version) {
    // SEX.
    for (xref, (v, line)) in &st.indi_sex {
        let ok = match version {
            Version::V70 => matches!(v.trim(), "M" | "F" | "X" | "U"),
            _ => matches!(v.trim(), "M" | "F" | "U"),
        };
        if !ok {
            push_capped(
                diags,
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
}

/// End of run: W302 duplicates (same normalized name + birth within +-2 years).
pub(crate) fn finish_duplicates(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    // W302: duplicates (same normalized name + birth within ±2 years).
    let mut by_name: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (xref, b) in &st.indi_birth {
        if let (Some(nm), Some(bb)) = (st.indi_name.get(xref), *b) {
            if bb < 10000 {
                by_name.entry(norm_name(nm)).or_default().push((xref.clone(), bb));
            }
        }
    }
    for v in by_name.values() {
        // The group arrives in HashMap iteration order, so the pairs (and the
        // "A vs B" wording) would differ between runs: sort by xref first, and
        // report at the second record's line (issue 30), which lands the
        // reader on the suspect duplicate.
        let mut v = v.clone();
        v.sort();
        for a in 0..v.len() {
            for b in a + 1..v.len() {
                if (v[a].1 - v[b].1).abs() <= 2 {
                    push_capped(
                        diags,
                        vec![Diag::new(
                            "W302",
                            Category::Suspicious,
                            Severity::Warning,
                            graph.records.get(&v[b].0).map(|r| r.1).unwrap_or(0),
                            format!("possible duplicate: {} (b. {}) vs {} (b. {})", v[a].0, v[a].1, v[b].0, v[b].1),
                        )],
                    );
                }
            }
        }
    }
}
