//! The referential index and the rules built on it: record identity (E003),
//! pointer targets (E201) and FAMC/CHIL symmetry (W202).

use std::collections::HashMap;

use crate::diag::{Category, Diag, Severity};
use crate::parse::{inner_ptr, is_pointer, truncate, Line};
use crate::rules::individuals::People;

/// Records seen so far plus every pointer waiting to be resolved.
#[derive(Default)]
pub(crate) struct Graph {
    // xref -> (kind, line)
    pub(crate) records: HashMap<String, (String, usize)>,
    // xref -> [(fam xref, line of the 1 FAMC)]
    pub(crate) indi_famc: HashMap<String, Vec<(String, usize)>>,
    // fam xref -> [(child xref, line of the 1 CHIL)]
    pub(crate) fam_chil: HashMap<String, Vec<(String, usize)>>,
    pub(crate) fam_husb: HashMap<String, (String, usize)>,
    pub(crate) fam_wife: HashMap<String, (String, usize)>,
    // (line, from, tag, target)
    pub(crate) pending: Vec<(usize, String, String, String)>,
}

/// Registers a level-0 record (E003 on a repeated xref) and returns the new
/// current record, or `None` when the line opens no addressable record.
pub(crate) fn open_record(
    diags: &mut Vec<Diag>,
    st: &mut Graph,
    people: &mut People,
    l: &Line,
) -> Option<(String, String)> {
    let mut cur = None;
    if !l.xref.is_empty() {
        if let Some((_, first_line)) = st.records.get(&l.xref) {
            diags.push(Diag::new(
                "E003",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("duplicate xref {} (first at line {})", l.xref, first_line),
            ));
        } else {
            st.records.insert(l.xref.clone(), (l.tag.clone(), l.no));
            match l.tag.as_str() {
                "INDI" => {
                    people.indi_birth.insert(l.xref.clone(), None);
                    people.indi_death.insert(l.xref.clone(), None);
                    cur = Some((l.xref.clone(), "INDI".into()));
                }
                "FAM" => {
                    cur = Some((l.xref.clone(), "FAM".into()));
                }
                _ => {
                    cur = Some((l.xref.clone(), l.tag.clone()));
                }
            }
        }
    }
    cur
}

/// INDI.FAMS / INDI.FAMC: pointer bookkeeping, or E201 on a non-pointer value.
pub(crate) fn indi_fam_link(diags: &mut Vec<Diag>, st: &mut Graph, l: &Line, xref: &str) {
    if is_pointer(&l.value) {
        st.indi_famc.entry(xref.to_string()).or_default();
        st.pending.push((
            l.no,
            xref.to_string(),
            l.tag.clone(),
            inner_ptr(&l.value).to_string(),
        ));
        if l.tag == "FAMC" {
            // The line is kept with the link so W202 can point at the FAMC
            // itself (issue 30) instead of at line 0.
            st.indi_famc
                .entry(xref.to_string())
                .or_default()
                .push((inner_ptr(&l.value).to_string(), l.no));
        }
    } else if !l.value.is_empty() {
        diags.push(Diag::new(
            "E201",
            Category::Correctness,
            Severity::Error,
            l.no,
            format!(
                "{}: {} with a non-pointer value: {}",
                xref,
                l.tag,
                truncate(&l.value, 40)
            ),
        ));
    }
}

/// FAM.HUSB / FAM.WIFE / FAM.CHIL: pointer bookkeeping plus E008 on the
/// {0:1} spouse slots.
pub(crate) fn fam_member_link(diags: &mut Vec<Diag>, st: &mut Graph, l: &Line, xref: &str) {
    if is_pointer(&l.value) {
        let t = inner_ptr(&l.value).to_string();
        st.pending
            .push((l.no, xref.to_string(), l.tag.clone(), t.clone()));
        match l.tag.as_str() {
            "CHIL" => {
                // The line is kept with the link so W202 can point at the
                // CHIL itself (issue 30) instead of at line 0.
                st.fam_chil
                    .entry(xref.to_string())
                    .or_default()
                    .push((t, l.no));
            }
            // E008: FAM.HUSB / FAM.WIFE are {0:1}.
            "HUSB" | "WIFE" => {
                let slot = if l.tag == "HUSB" {
                    &mut st.fam_husb
                } else {
                    &mut st.fam_wife
                };
                if let Some((_, first)) = slot.get(xref) {
                    diags.push(Diag::new(
                        "E008",
                        Category::Correctness,
                        Severity::Error,
                        l.no,
                        format!("duplicate {} in {} (first at line {})", l.tag, xref, first),
                    ));
                } else {
                    slot.insert(xref.to_string(), (t, l.no));
                }
            }
            _ => {}
        }
    }
}

/// Generic level-1 pointers (SOUR, OBJE, NOTE, SUBM...): recorded for E201.
pub(crate) fn generic_pointer(st: &mut Graph, l: &Line, xref: &str) {
    // Note: ADOP takes no pointer in 5.5.1 (event with a
    // subordinate FAMC), so it is not tracked here.
    if is_pointer(&l.value) && matches!(l.tag.as_str(), "SOUR" | "OBJE" | "NOTE" | "SUBM" | "REPO")
    {
        st.pending.push((
            l.no,
            xref.to_string(),
            l.tag.clone(),
            inner_ptr(&l.value).to_string(),
        ));
    }
}

/// Pointers below level 1 (event SOUR/OBJE/NOTE...) resolve for E201 too.
pub(crate) fn sub_pointer(st: &mut Graph, l: &Line, cur: &Option<(String, String)>) {
    if is_pointer(&l.value) && matches!(l.tag.as_str(), "SOUR" | "OBJE" | "NOTE" | "REPO" | "SUBM")
    {
        let from = cur
            .clone()
            .map(|c| c.0)
            .unwrap_or_else(|| format!("line {}", l.no));
        st.pending
            .push((l.no, from, l.tag.clone(), inner_ptr(&l.value).to_string()));
    }
}

/// End of run: E201 broken references.
pub(crate) fn finish_refs(diags: &mut Vec<Diag>, st: &Graph) {
    // E201: broken references. @VOID@ is the 7.0 null pointer: always valid.
    for (line, from, tag, target) in &st.pending {
        if target == "@VOID@" {
            continue;
        }
        if !st.records.contains_key(target) {
            let kind = match tag.as_str() {
                "FAMS" | "FAMC" => "FAM",
                "HUSB" | "WIFE" | "CHIL" => "INDI",
                _ => "record",
            };
            diags.push(Diag::new(
                "E201",
                Category::Correctness,
                Severity::Error,
                *line,
                format!(
                    "{}: {} {} points to a nonexistent {}",
                    from, tag, target, kind
                ),
            ));
        }
    }
}

/// End of run: E202 ancestral cycle (an individual is their own ancestor).
/// O(V+E) DFS over child -> parent edges built from FAMC/CHIL plus the
/// HUSB/WIFE of each family. Reports once per back edge, at the FAMC line
/// when the link exists, else at the CHIL line, so output is deterministic.
pub(crate) fn finish_cycles(diags: &mut Vec<Diag>, st: &Graph) {
    use std::collections::{HashMap, HashSet};
    // child -> [(parent, line)].
    let mut parents: HashMap<String, Vec<(String, usize)>> = HashMap::new();
    for (child, fams) in &st.indi_famc {
        parents.entry(child.clone()).or_default();
        for (fam, fam_line) in fams {
            for slot in [&st.fam_husb, &st.fam_wife] {
                if let Some((px, _)) = slot.get(fam) {
                    if st.records.contains_key(px) {
                        parents
                            .entry(child.clone())
                            .or_default()
                            .push((px.clone(), *fam_line));
                    }
                }
            }
        }
    }
    for (fam, chils) in &st.fam_chil {
        let ps: Vec<String> = [&st.fam_husb, &st.fam_wife]
            .iter()
            .filter_map(|slot| slot.get(fam).map(|(px, _)| px.clone()))
            .filter(|px| st.records.contains_key(px))
            .collect();
        for (child, chil_line) in chils {
            parents.entry(child.clone()).or_default();
            for px in &ps {
                let v = parents.entry(child.clone()).or_default();
                if !v.iter().any(|(p, _)| p == px) {
                    v.push((px.clone(), *chil_line));
                }
            }
        }
    }
    for v in parents.values_mut() {
        v.sort();
        v.dedup();
    }
    let mut nodes: Vec<String> = parents.keys().cloned().collect();
    nodes.sort();
    // 0 = unvisited, 1 = on stack, 2 = done.
    let mut color: HashMap<String, u8> = nodes.iter().map(|n| (n.clone(), 0)).collect();
    let mut stack: Vec<String> = Vec::new();
    let mut in_stack: HashSet<String> = HashSet::new();
    // Iterative DFS from each root in sorted order. The explicit stack
    // holds (node, next child index).
    for root in &nodes {
        if color[root] != 0 {
            continue;
        }
        let mut work: Vec<(String, usize)> = vec![(root.clone(), 0)];
        color.insert(root.clone(), 1);
        stack.push(root.clone());
        in_stack.insert(root.clone());
        while let Some((node, idx)) = work.pop() {
            let kids: Vec<(String, usize)> = parents.get(&node).cloned().unwrap_or_default();
            if idx < kids.len() {
                work.push((node.clone(), idx + 1));
                let (par, line) = kids[idx].clone();
                let pc = color.get(&par).copied().unwrap_or(0);
                if pc == 0 {
                    color.insert(par.clone(), 1);
                    stack.push(par.clone());
                    in_stack.insert(par.clone());
                    work.push((par, 0));
                } else if in_stack.contains(&par) {
                    // Back edge closes a directed cycle through the stack.
                    // Global output order comes from the total sort in
                    // `lint_lines`; insertion here follows sorted roots.
                    let mut cyc: Vec<String> = Vec::new();
                    for s in stack.iter().rev() {
                        cyc.push(s.clone());
                        if s == &par {
                            break;
                        }
                    }
                    cyc.reverse();
                    diags.push(Diag::new(
                        "E202",
                        Category::Correctness,
                        Severity::Error,
                        line,
                        format!("{}: ancestral cycle ({})", node, cyc.join(" -> ")),
                    ));
                }
            } else {
                color.insert(node.clone(), 2);
                in_stack.remove(&node);
                stack.pop();
            }
        }
    }
}

/// End of run: W202 FAMC/CHIL asymmetry.
pub(crate) fn finish_symmetry(diags: &mut Vec<Diag>, st: &Graph) {
    // W202: FAMC not listed as CHIL (and vice versa). Each half reports at
    // the line of the link that exists (issue 30): the FAMC for a child the
    // family does not list, the CHIL for a child that declares no FAMC.
    // A real line also makes the output deterministic, which the old line-0
    // sort ties never were.
    for (xref, fams) in &st.indi_famc {
        for (f, fam_line) in fams {
            let listed = st
                .fam_chil
                .get(f)
                .map(|c| c.iter().any(|(c_x, _)| c_x == xref))
                .unwrap_or(false);
            let fam_exists = st.records.get(f).map(|r| r.0 == "FAM").unwrap_or(false);
            if fam_exists && !listed {
                diags.push(Diag::new(
                    "W202",
                    Category::Suspicious,
                    Severity::Warning,
                    *fam_line,
                    format!(
                        "{}: declares FAMC {} but the FAM does not list them as CHIL",
                        xref, f
                    ),
                ));
            }
        }
    }
    for (fam, chils) in &st.fam_chil {
        for (c, chil_line) in chils {
            let declares = st
                .indi_famc
                .get(c)
                .map(|v| v.iter().any(|(f, _)| f == fam))
                .unwrap_or(false);
            if !declares && st.records.contains_key(c) {
                diags.push(Diag::new(
                    "W202",
                    Category::Suspicious,
                    Severity::Warning,
                    *chil_line,
                    format!("{}: lists CHIL {} but the INDI declares no FAMC", fam, c),
                ));
            }
        }
    }
}
