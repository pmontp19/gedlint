//! Person and family semantics: SEX (E008/W305), lifespans (W301), parent
//! ages and children born before the marriage (W303/W304) and the duplicate
//! heuristic (W302).

use std::collections::HashMap;
use std::collections::HashSet;

use crate::config::Thresholds;
use crate::diag::{Category, Diag, Severity};
use crate::parse::{is_aft, is_bef, norm_name, year_of, Line, Version};
use crate::rules::graph::Graph;

/// Per-individual and per-family facts collected during the pass.
#[derive(Default)]
pub(crate) struct People {
    pub(crate) indi_birth: HashMap<String, Option<i64>>,
    pub(crate) indi_death: HashMap<String, Option<i64>>,
    pub(crate) indi_name: HashMap<String, String>,
    pub(crate) indi_sex: HashMap<String, (String, usize)>,
    pub(crate) fam_marr: HashMap<String, Option<i64>>,
    /// Raw DATE text per individual birth (for the W704 day ordinal).
    pub(crate) indi_birth_raw: HashMap<String, String>,
    /// `1 MARR` line per family (for W304 and as fallback for W311).
    pub(crate) fam_marr_line: HashMap<String, usize>,
    /// Every MARR instance per family: (year, `1 MARR` line, raw DATE text).
    /// W311 checks each union; `fam_marr` keeps the last for W304.
    pub(crate) fam_marrs: HashMap<String, Vec<(i64, usize, String)>>,
    /// Birth DATE was `BEF`-qualified: the real birth can be earlier.
    pub(crate) birth_is_bef: HashMap<String, bool>,
    /// Birth DATE was `AFT`-qualified: the real birth can be later.
    pub(crate) birth_is_aft: HashMap<String, bool>,
    /// Death DATE was `AFT`-qualified: the real death can be later.
    pub(crate) death_is_aft: HashMap<String, bool>,
    /// Individuals with a `1 DEAT` event, whatever its DATE says. `DEAT Y`
    /// (dead, date unknown) records no year, but the person is still known
    /// dead, which is what `W705` needs to tell apart from the living.
    pub(crate) indi_died: HashSet<String>,
}

/// INDI.SEX: E008 on the {0:1} slot; the value itself is checked at the end.
pub(crate) fn record_sex(diags: &mut Vec<Diag>, st: &mut People, l: &Line, xref: &str) {
    // E008: INDI.SEX is {0:1}.
    if let Some((_, first)) = st.indi_sex.get(xref) {
        diags.push(Diag::new(
            "E008",
            Category::Correctness,
            Severity::Error,
            l.no,
            format!("duplicate SEX in {} (first at line {})", xref, first),
        ));
    } else {
        st.indi_sex
            .insert(xref.to_string(), (l.value.clone(), l.no));
    }
}

/// FAM.MARR / INDI.BIRT / INDI.DEAT at level 1: the year carried on the event
/// line itself, plus "DEAT Y" (dead, date unknown).
pub(crate) fn record_event_year(st: &mut People, l: &Line, xref: &str, kind: &str) {
    if kind == "INDI" && l.tag == "DEAT" {
        st.indi_died.insert(xref.to_string());
    }
    if let Some(y) = year_of(&l.value) {
        match l.tag.as_str() {
            "BIRT" => {
                st.indi_birth.insert(xref.to_string(), Some(y));
                st.indi_birth_raw.insert(xref.to_string(), l.value.clone());
                st.birth_is_bef.insert(xref.to_string(), is_bef(&l.value));
                st.birth_is_aft.insert(xref.to_string(), is_aft(&l.value));
            }
            "DEAT" => {
                st.indi_death.insert(xref.to_string(), Some(y));
                st.death_is_aft.insert(xref.to_string(), is_aft(&l.value));
            }
            _ => {
                if kind == "FAM" {
                    st.fam_marr.insert(xref.to_string(), Some(y));
                    st.fam_marr_line.insert(xref.to_string(), l.no);
                    st.fam_marrs.entry(xref.to_string()).or_default().push((
                        y,
                        l.no,
                        l.value.clone(),
                    ));
                }
            }
        }
    } else if l.tag == "MARR" && kind == "FAM" {
        // A MARR line with no parsable year still anchors W311 reporting
        // when a subordinate DATE later supplies the year. Each MARR block
        // re-anchors: remarriage files hold several unions per family.
        st.fam_marr_line.insert(xref.to_string(), l.no);
    }
    // "DEAT Y" (dead, date unknown) records no year here, which is what
    // keeps it out of the W301 checks in `finish_lifespans`: they need a
    // birth year and a death year. A DEAT Y with a subordinate DATE still
    // records the year via `record_sub_date`, and is checked like any other.
}

/// BIRT/DEAT/MARR DATE directly below the event: the authoritative year for
/// W301-W304. A DATE nested deeper (e.g. BIRT -> SOUR -> DATA -> DATE) is
/// citation metadata, not the event date.
pub(crate) fn record_sub_date(
    st: &mut People,
    l: &Line,
    parent_tag: &str,
    cur: &Option<(String, String)>,
) {
    if parent_tag == "BIRT" && l.tag == "DATE" {
        if let Some((xref, kind)) = cur.clone() {
            if kind == "INDI" {
                if let Some(y) = year_of(&l.value) {
                    st.indi_birth.insert(xref.clone(), Some(y));
                    st.indi_birth_raw.insert(xref.clone(), l.value.clone());
                    st.birth_is_bef.insert(xref.clone(), is_bef(&l.value));
                    st.birth_is_aft.insert(xref, is_aft(&l.value));
                }
            }
        }
    }
    if parent_tag == "DEAT" && l.tag == "DATE" {
        if let Some((xref, kind)) = cur.clone() {
            if kind == "INDI" {
                if let Some(y) = year_of(&l.value) {
                    st.indi_death.insert(xref.clone(), Some(y));
                    st.death_is_aft.insert(xref, is_aft(&l.value));
                }
            }
        }
    }
    if parent_tag == "MARR" && l.tag == "DATE" {
        if let Some((xref, kind)) = cur.clone() {
            if kind == "FAM" {
                if let Some(y) = year_of(&l.value) {
                    st.fam_marr.insert(xref.clone(), Some(y));
                    // The level-1 MARR precedes its DATE, so its line is
                    // already anchored; fall back to the DATE line itself.
                    let line = st.fam_marr_line.get(&xref).copied().unwrap_or(l.no);
                    let v = st.fam_marrs.entry(xref).or_default();
                    // A year on the MARR line itself plus a subordinate DATE
                    // is one union, not two: the DATE refines the event line.
                    if v.last().map(|e| e.1) == Some(line) {
                        v.pop();
                    }
                    v.push((y, line, l.value.clone()));
                } else {
                    st.fam_marr.insert(xref, None);
                }
            }
        }
    }
}

/// W301 for one individual: died before birth, or an implausible lifespan.
pub(crate) fn flush_person(
    diags: &mut Vec<Diag>,
    xref: &str,
    b: Option<i64>,
    d: Option<i64>,
    line: usize,
    max_lifespan: i64,
) {
    if let (Some(bb), Some(dd)) = (b, d) {
        if dd < 10000 && bb < 10000 && dd < bb {
            diags.push(Diag::new(
                "W301",
                Category::Suspicious,
                Severity::Warning,
                line,
                format!("{}: died ({}) before being born ({})", xref, dd, bb),
            ));
        }
        if dd < 10000 && dd - bb > max_lifespan {
            diags.push(Diag::new(
                "W301",
                Category::Suspicious,
                Severity::Warning,
                line,
                format!(
                    "{}: {} - {} = {} years, please verify",
                    xref,
                    bb,
                    dd,
                    dd - bb
                ),
            ));
        }
    }
}

/// End of run: W301 death before birth + longevity, per individual.
pub(crate) fn finish_lifespans(
    diags: &mut Vec<Diag>,
    st: &People,
    graph: &Graph,
    thr: &Thresholds,
) {
    // W301: death before birth + longevity, per individual.
    for (xref, b) in &st.indi_birth {
        let d = st.indi_death.get(xref).copied().flatten();
        if let (Some(bb), Some(dd)) = (*b, d) {
            if dd < 10000 {
                flush_person(
                    diags,
                    xref,
                    Some(bb),
                    Some(dd),
                    graph.records.get(xref).map(|r| r.1).unwrap_or(0),
                    thr.max_lifespan,
                );
            }
        }
    }
}

/// End of run: W303 parent age at the child's birth and W304 child born
/// before the marriage. Both report at the child's record line (issue 30):
/// the child's birth is what triggers them, and a real line makes the
/// whole-file graph findings deterministic.
pub(crate) fn finish_parent_ages(
    diags: &mut Vec<Diag>,
    st: &People,
    graph: &Graph,
    thr: &Thresholds,
) {
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
            for (parent, rol) in [
                (&graph.fam_husb.get(fam), "father"),
                (&graph.fam_wife.get(fam), "mother"),
            ] {
                if let Some((px, _)) = parent {
                    if let Some(Some(pb)) = st.indi_birth.get(px) {
                        let age = cb - pb;
                        let max = if rol == "mother" {
                            thr.max_mother_age
                        } else {
                            thr.max_father_age
                        };
                        if age < thr.min_parent_age || age > max {
                            diags.push(Diag::new(
                                "W303",
                                Category::Suspicious,
                                Severity::Warning,
                                child_line,
                                format!(
                                    "{}: {} {} (b. {}) was {} at {}'s birth (b. {})",
                                    fam, rol, px, pb, age, c, cb
                                ),
                            ));
                        }
                    }
                }
            }
            // Child born before the marriage (when a MARR date exists).
            if let Some(Some(m)) = st.fam_marr.get(fam) {
                if cb < *m {
                    diags.push(Diag::new(
                        "W304",
                        Category::Suspicious,
                        Severity::Warning,
                        child_line,
                        format!("{}: {} born ({}) before marriage ({})", fam, c, cb, m),
                    ));
                }
            }
        }
    }
}

/// End of run: W308 child born after a parent's death, W309 child born
/// before a parent's birth. Both report at the child's record line.
pub(crate) fn finish_parent_death_birth(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    for (fam, chils) in &graph.fam_chil {
        for (c, _) in chils {
            let cb = st.indi_birth.get(c).copied().flatten();
            let Some(cb) = cb else { continue };
            if cb >= 10000 {
                continue;
            }
            let child_line = graph.records.get(c).map(|r| r.1).unwrap_or(0);
            // W308 is suppressed when the uncertainty runs the other way:
            // a BEF birth can be earlier, an AFT death can be later.
            let birth_bef = st.birth_is_bef.get(c).copied().unwrap_or(false);
            if !birth_bef {
                for (parent, rol, is_mother) in [
                    (&graph.fam_husb.get(fam), "father", false),
                    (&graph.fam_wife.get(fam), "mother", true),
                ] {
                    if let Some((px, _)) = parent {
                        if let Some(Some(pd)) = st.indi_death.get(px) {
                            if *pd >= 10000 {
                                continue;
                            }
                            if st.death_is_aft.get(px).copied().unwrap_or(false) {
                                continue;
                            }
                            // Fathers get one gestation year of slack for a
                            // posthumous birth; mothers do not.
                            let late = if is_mother { cb > *pd } else { cb > *pd + 1 };
                            if late {
                                diags.push(Diag::new(
                                    "W308",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    child_line,
                                    format!(
                                        "{}: {} born ({}) after {} {} death ({})",
                                        fam, c, cb, rol, px, pd
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
            // W309: a child older than their own parent. W303 already warns
            // on implausible gaps; a non-positive gap is impossible, an error.
            // Suppressed when the uncertainty runs the other way: an AFT
            // child birth can be later, a BEF parent birth can be earlier.
            let child_aft = st.birth_is_aft.get(c).copied().unwrap_or(false);
            if !child_aft {
                for (parent, rol) in [
                    (&graph.fam_husb.get(fam), "father"),
                    (&graph.fam_wife.get(fam), "mother"),
                ] {
                    if let Some((px, _)) = parent {
                        if st.birth_is_bef.get(px).copied().unwrap_or(false) {
                            continue;
                        }
                        if let Some(Some(pb)) = st.indi_birth.get(px) {
                            if *pb >= 10000 {
                                continue;
                            }
                            if cb <= *pb {
                                diags.push(Diag::new(
                                    "W309",
                                    Category::Suspicious,
                                    Severity::Error,
                                    child_line,
                                    format!(
                                        "{}: {} born ({}) before {} {} birth ({})",
                                        fam, c, cb, rol, px, pb
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// End of run: W311 marriage after a spouse's death or before a spouse's
/// birth. Checks every MARR instance in the family (a remarriage file holds
/// several) and reports at that union's `1 MARR` line. A BEF-qualified
/// marriage can predate the written year, an AFT one can follow it, so each
/// arm suppresses on the qualifier that runs its way.
pub(crate) fn finish_marriage_sequence(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    use crate::parse::{is_aft, is_bef};
    // Deterministic iteration: families sorted, instances in pass order.
    let mut fams: Vec<&String> = st.fam_marrs.keys().collect();
    fams.sort();
    for fam in fams {
        let Some(instances) = st.fam_marrs.get(fam) else {
            continue;
        };
        for (mm, marr_line, raw) in instances {
            if *mm >= 10000 {
                continue;
            }
            let marr_bef = is_bef(raw);
            let marr_aft = is_aft(raw);
            for (parent, rol) in [
                (&graph.fam_husb.get(fam), "husband"),
                (&graph.fam_wife.get(fam), "wife"),
            ] {
                if let Some((px, _)) = parent {
                    if let Some(Some(pb)) = st.indi_birth.get(px) {
                        let birth_bef = st.birth_is_bef.get(px).copied().unwrap_or(false);
                        if *pb < 10000 && *mm < *pb && !marr_aft && !birth_bef {
                            diags.push(Diag::new(
                                "W311",
                                Category::Suspicious,
                                Severity::Error,
                                *marr_line,
                                format!(
                                    "{}: marriage ({}) before {} {} birth ({})",
                                    fam, mm, rol, px, pb
                                ),
                            ));
                        }
                    }
                    if let Some(Some(pd)) = st.indi_death.get(px) {
                        let death_aft = st.death_is_aft.get(px).copied().unwrap_or(false);
                        if *pd < 10000 && *mm > *pd && !marr_bef && !death_aft {
                            diags.push(Diag::new(
                                "W311",
                                Category::Suspicious,
                                Severity::Error,
                                *marr_line,
                                format!(
                                    "{}: marriage ({}) after {} {} death ({})",
                                    fam, mm, rol, px, pd
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// End of run: W312 HUSB/WIFE pointing at an individual recorded with the
/// other sex. Only fires on a jointly inverted pair (HUSB is F *and* WIFE
/// is M): a single mismatched side is indistinguishable from a same-sex
/// marriage, which the official 7.0 `same-sex-marriage.ged` records with
/// HUSB+WIFE. Reports at both link lines.
pub(crate) fn finish_spouse_sex(diags: &mut Vec<Diag>, st: &People, graph: &Graph) {
    for (fam, (husb, husb_line)) in &graph.fam_husb {
        let Some((wife, wife_line)) = graph.fam_wife.get(fam) else {
            continue;
        };
        let husb_f = st
            .indi_sex
            .get(husb)
            .map(|(s, _)| s.trim().eq_ignore_ascii_case("F"))
            .unwrap_or(false);
        let wife_m = st
            .indi_sex
            .get(wife)
            .map(|(s, _)| s.trim().eq_ignore_ascii_case("M"))
            .unwrap_or(false);
        if husb_f && wife_m {
            diags.push(Diag::new(
                "W312",
                Category::Suspicious,
                Severity::Warning,
                *husb_line,
                format!("{}: HUSB {} is recorded as female (SEX F)", fam, husb),
            ));
            diags.push(Diag::new(
                "W312",
                Category::Suspicious,
                Severity::Warning,
                *wife_line,
                format!("{}: WIFE {} is recorded as male (SEX M)", fam, wife),
            ));
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
            diags.push(Diag::new(
                "W305",
                Category::Suspicious,
                Severity::Warning,
                *line,
                format!(
                    "{}: invalid SEX ({}), expected M/F/U{}",
                    xref,
                    v,
                    if version == Version::V70 { "/X" } else { "" }
                ),
            ));
        }
    }
}

/// End of run: W302 duplicates (same normalized name + birth within the
/// configured window).
pub(crate) fn finish_duplicates(
    diags: &mut Vec<Diag>,
    st: &People,
    graph: &Graph,
    thr: &Thresholds,
) {
    // W302: duplicates (same normalized name + birth within ±2 years).
    let mut by_name: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (xref, b) in &st.indi_birth {
        if let (Some(nm), Some(bb)) = (st.indi_name.get(xref), *b) {
            if bb < 10000 {
                by_name
                    .entry(norm_name(nm))
                    .or_default()
                    .push((xref.clone(), bb));
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
                if (v[a].1 - v[b].1).abs() <= thr.duplicate_window {
                    diags.push(Diag::new(
                        "W302",
                        Category::Suspicious,
                        Severity::Warning,
                        graph.records.get(&v[b].0).map(|r| r.1).unwrap_or(0),
                        format!(
                            "possible duplicate: {} (b. {}) vs {} (b. {})",
                            v[a].0, v[a].1, v[b].0, v[b].1
                        ),
                    ));
                }
            }
        }
    }
}
