//! Event bookkeeping: the {0:1} detail singletons per event instance (E008),
//! EVEN/FACT without TYPE and LDS STAT without DATE (E009, 7.0 only), and
//! conflicting duplicate events (W307).

use std::collections::{HashMap, HashSet};

use crate::diag::{push_capped, Category, Diag, Severity};
use crate::parse::{Line, Version};

// Common individual/family events whose detail singletons are {0:1}.
const EVENT_TAGS: &[&str] = &[
    "BIRT", "CHR", "DEAT", "BURI", "MARR", "DIV", "OCCU", "RESI", "EVEN", "FACT",
    "CENS", "EMIG", "IMMI", "GRAD", "RETI", "BAPM", "CONF",
];
const EVENT_SINGLETONS: &[&str] = &[
    "DATE", "PLAC", "ADDR", "AGNC", "CAUS", "RELI", "RESN", "TYPE", "AGE", "SDATE", "PAGE",
];
// LDS ordinances whose STAT is enumerated (spec TSV has PRE_1970/DNS_CAN;
// the human page shows PRE: accept all three spellings).
pub(crate) const LDS_EVENTS: &[&str] = &["BAPL", "CONL", "ENDL", "SLGC", "SLGS", "INIL"];

// W307 only watches events that are semantically single: one birth, one
// death. Repeatable attributes (OCCU/RESI/CENS/EVEN/FACT) accumulate new
// instances naturally, so differing dates there are not a merge leftover.
const CONFLICT_EVENTS: &[&str] = &["BIRT", "CHR", "DEAT", "BURI", "MARR", "DIV", "BAPM", "CONF"];

// (record, event, instance) key shared by the E008/E009/W307 trackers.
type EvKey = (String, String, usize);
// (instance, DATE value, line) hit inside one record+event group.
type EvHit = (usize, String, usize);
/// Everything the event rules accumulate during the single pass.
#[derive(Default)]
pub(crate) struct Events {
    // E008 event-detail singletons: (record, event, instance, sub) -> first line.
    // The instance matters: INDI.BIRT is {0:M}, so two BIRT blocks may each
    // carry one DATE; only two DATEs under the SAME block are duplicates.
    pub(crate) event_seen: HashMap<(String, String, usize, String), usize>,
    pub(crate) event_inst: HashMap<(String, String), usize>,
    // Current event instance + the level it opened at: deeper levels
    // (SOUR.DATA.DATE) belong to other structures, not to the event.
    pub(crate) cur_event: Option<(String, usize, u32)>,
    // W307 conflicting duplicate events: EvKey -> (DATE, line).
    pub(crate) event_dates: HashMap<EvKey, (String, usize)>,
    // DIV and MARR event lines per record: a MARR pair separated by a DIV
    // is a remarriage and a DIV pair separated by a MARR is a serial
    // divorce; the 7.0 spec allows a single FAM to hold multiple MARR/DIV.
    pub(crate) div_lines: HashMap<String, Vec<usize>>,
    pub(crate) marr_lines: HashMap<String, Vec<usize>>,
    // E009 EVEN/FACT without TYPE (7.0 only): EvKey line + typed set.
    pub(crate) ef_inst: HashMap<(String, String), usize>,
    pub(crate) ef_line: HashMap<EvKey, usize>,
    pub(crate) ef_typed: HashSet<EvKey>,
    // E009 LDS STAT without DATE (7.0 only): (record, event, stat line).
    pub(crate) lds_stat: Vec<(String, String, usize)>,
}

/// Level-1 event openers: bump the per-record instance counters E008 and E009
/// key on, and remember the MARR/DIV line order W307 needs.
pub(crate) fn open_instances(st: &mut Events, l: &Line, xref: &str, lvl: u32) {
    // Event instance counter for E008 (BIRT is {0:M}: each block
    // gets its own number so cross-block DATEs are not dups).
    if EVENT_TAGS.contains(&l.tag.as_str()) {
        let key = (xref.to_string(), l.tag.clone());
        let n = st.event_inst.get(&key).copied().unwrap_or(0) + 1;
        st.event_inst.insert(key, n);
        st.cur_event = Some((l.tag.clone(), n, lvl));
        if l.tag == "DIV" {
            st.div_lines.entry(xref.to_string()).or_default().push(l.no);
        } else if l.tag == "MARR" {
            st.marr_lines.entry(xref.to_string()).or_default().push(l.no);
        }
    }
    // E009 EVEN/FACT instance counter (TYPE required in 7.0).
    if l.tag == "EVEN" || l.tag == "FACT" {
        let key = (xref.to_string(), l.tag.clone());
        let n = st.ef_inst.get(&key).copied().unwrap_or(0) + 1;
        st.ef_inst.insert(key.clone(), n);
        st.ef_line.insert((key.0, key.1, n), l.no);
    }
}

/// Marks the substructures that satisfy a 7.0 requirement: EVEN/FACT TYPE and
/// a DATE directly under an LDS STAT.
pub(crate) fn record_required(
    st: &mut Events,
    l: &Line,
    rec: &str,
    cur_sub: &str,
    parent: &Option<(String, usize)>,
) {
    let parent_tag: &str = parent.as_ref().map(|p| p.0.as_str()).unwrap_or("");
    // E009 EVEN/FACT TYPE mark (instance = latest opened block).
    if (cur_sub == "EVEN" || cur_sub == "FACT") && l.tag == "TYPE" && !rec.is_empty() {
        if let Some(n) = st.ef_inst.get(&(rec.to_string(), cur_sub.to_string())).copied() {
            st.ef_typed.insert((rec.to_string(), cur_sub.to_string(), n));
        }
    }
    // E009 LDS STAT register; a DATE directly under STAT satisfies it.
    if l.tag == "STAT" && LDS_EVENTS.contains(&cur_sub) && !rec.is_empty() {
        st.lds_stat.push((rec.to_string(), cur_sub.to_string(), l.no));
    }
    if l.tag == "DATE" && parent_tag == "STAT" && !rec.is_empty() {
        if let Some((_, pline)) = parent {
            let pl = *pline;
            st.lds_stat.retain(|(r, _, sl)| !(r == rec && sl == &pl));
        }
    }
}

/// E008 on the {0:1} detail substructures of the current event instance, plus
/// the DATE values W307 compares at the end of the run.
pub(crate) fn check_detail_singletons(
    diags: &mut Vec<Diag>,
    st: &mut Events,
    l: &Line,
    lvl: u32,
    rec: &str,
) {
    // E008: one {0:1} detail substructure per event instance, at the
    // level directly under the event. DATEs are also collected
    // for the W307 conflicting-duplicate check below.
    if !rec.is_empty() && EVENT_SINGLETONS.contains(&l.tag.as_str()) {
        if let Some((ev, inst, ev_lvl)) = st.cur_event.clone() {
            if lvl == ev_lvl + 1 {
                if l.tag == "DATE" && !l.value.trim().is_empty() {
                    st.event_dates.insert(
                        (rec.to_string(), ev.clone(), inst),
                        (l.value.trim().to_string(), l.no),
                    );
                }
                let key = (rec.to_string(), ev.clone(), inst, l.tag.clone());
                if let Some(first) = st.event_seen.get(&key) {
                    push_capped(
                        diags,
                        vec![Diag::new(
                            "E008",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            format!(
                                "duplicate {} in {} {} (block {}, first at line {})",
                                l.tag, rec, ev, inst, first
                            ),
                        )],
                    );
                } else {
                    st.event_seen.insert(key, l.no);
                }
            }
        }
    }
}

/// End of run: 7.0 required substructures (E009) and conflicting duplicate
/// events (W307).
pub(crate) fn finish(diags: &mut Vec<Diag>, st: &Events, version: Version) {
    // E009: EVEN/FACT without TYPE and LDS STAT without DATE (7.0 only:
    // 5.5.1 leaves both optional).
    if version == Version::V70 {
        for ((rec, tag, inst), line) in &st.ef_line {
            if !st.ef_typed.contains(&(rec.clone(), tag.clone(), *inst)) {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "E009",
                        Category::Correctness,
                        Severity::Error,
                        *line,
                        format!("{}: {} without required TYPE (7.0)", rec, tag),
                    )],
                );
            }
        }
        for (rec, ev, sline) in &st.lds_stat {
            push_capped(
                diags,
                vec![Diag::new(
                    "E009",
                    Category::Correctness,
                    Severity::Error,
                    *sline,
                    format!("{}: {} STAT without required DATE (7.0)", rec, ev),
                )],
            );
        }
    }

    // W307: semantically single events twice with conflicting DATEs (classic
    // merge leftover). MARR and DIV pairs separated by the counterpart event
    // are a serial marriage/divorce (spec-legal in one FAM), not conflicts.
    {
        let mut by_event: HashMap<(String, String), Vec<EvHit>> = HashMap::new();
        for ((rec, ev, inst), (val, line)) in &st.event_dates {
            if CONFLICT_EVENTS.contains(&ev.as_str()) {
                by_event.entry((rec.clone(), ev.clone())).or_default().push((*inst, val.clone(), *line));
            }
        }
        let empty: Vec<usize> = Vec::new();
        for ((rec, ev), mut v) in by_event {
            v.sort();
            let excusers = match ev.as_str() {
                "MARR" => st.div_lines.get(&rec).unwrap_or(&empty),
                "DIV" => st.marr_lines.get(&rec).unwrap_or(&empty),
                _ => &empty,
            };
            let mut prev: Option<EvHit> = None;
            for hit in v {
                if let Some((_, pval, pline)) = &prev {
                    if pval != &hit.1 {
                        let excused = excusers.iter().any(|d| *d > *pline && *d < hit.2);
                        if !excused {
                            push_capped(
                                diags,
                                vec![Diag::new(
                                    "W307",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    hit.2,
                                    format!(
                                        "{}: duplicate {} with conflicting dates ({} vs {})",
                                        rec, ev, pval, hit.1
                                    ),
                                )],
                            );
                            prev = None;
                            continue;
                        }
                    }
                }
                prev = Some(hit);
            }
        }
    }
}
