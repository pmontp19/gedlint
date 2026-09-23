use crate::config::Thresholds;
use crate::diag::{Category, Diag, Severity};
use crate::parse::{truncate, Line, Version};
use crate::rules::dates::date_ordinal;
use crate::rules::graph::Graph;
use crate::rules::individuals::People;
use crate::rules::names::surname_slot;

/// W701: polluted-name, on any raw name value: the whole `1 NAME` value or
/// the value of a `2 SURN` subtag. Returns whether it fired.
pub(crate) fn check_polluted_name(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    let mut polluted = false;
    if val.contains('*')
        || val.chars().any(|c| c.is_ascii_digit())
        || val.contains('º')
        || val.contains('ª')
    {
        polluted = true;
    } else if val.contains('(') {
        let lower = val.to_lowercase();
        // Conservative: a parenthetical can be a legitimate Catalan house name
        if !lower.contains("(cal ")
            && !lower.contains("(can ")
            && !lower.contains("(mas ")
            && !lower.contains("(casa ")
            && !lower.contains("(de ")
        {
            polluted = true;
        }
    }

    if polluted {
        diags.push(
            Diag::new(
                "W701",
                Category::Style,
                Severity::Info,
                line,
                format!("polluted name field: {}", truncate(val, 60)),
            )
            .in_ruleset("hygiene"),
        );
        return true;
    }
    false
}

/// W702: all-caps-name, on the whole `1 NAME` value (the surname is read
/// out of the slash slot). A `2 SURN` subtag goes through
/// [`check_all_caps_value`]. Returns whether it fired.
pub(crate) fn check_all_caps_name(diags: &mut Vec<Diag>, line: usize, val: &str) -> bool {
    match surname_slot(val) {
        Some(surname) => check_all_caps_value(diags, line, surname),
        None => false,
    }
}

/// The defect W702 targets, as a pure predicate shared with the `--fix`
/// repair: letters present, no lowercase among them.
pub(crate) fn is_all_caps(surname: &str) -> bool {
    let has_letters = surname.chars().any(|c| c.is_alphabetic());
    let has_lower = surname.chars().any(|c| c.is_lowercase());
    has_letters && !has_lower
}

/// W702 on a bare surname string: the value of a `2 SURN` subtag, or the
/// slot cut out of a `1 NAME`. Returns whether it fired.
pub(crate) fn check_all_caps_value(diags: &mut Vec<Diag>, line: usize, surname: &str) -> bool {
    if !is_all_caps(surname) {
        return false;
    }
    diags.push(
        Diag::new(
            "W702",
            Category::Style,
            Severity::Info,
            line,
            format!("all-caps surname: {}", truncate(surname, 60)),
        )
        .in_ruleset("hygiene"),
    );
    true
}

/// W703: malformed-place
pub(crate) fn check_malformed_place(diags: &mut Vec<Diag>, l: &Line) {
    if l.tag == "PLAC" && (l.value.contains(",,") || l.value.contains(", ,")) {
        diags.push(
            Diag::new(
                "W703",
                Category::Style,
                Severity::Info,
                l.no,
                format!("doubled commas in place: {}", truncate(&l.value, 60)),
            )
            .in_ruleset("hygiene"),
        );
    }
}

/// W704: impossible sibling spacing. Children of the same mother born 1 to
/// `sibling_max_gap` days apart (same day = twins, skipped). Only exact
/// `DD MMM YYYY` dates participate; year-only or qualified dates cannot be
/// measured and are skipped. Reports at the later-born child's record line.
pub(crate) fn finish_sibling_spacing(
    diags: &mut Vec<Diag>,
    st: &People,
    graph: &Graph,
    thr: &Thresholds,
) {
    use std::collections::HashMap;
    // mother xref -> [(ordinal, child xref)].
    let mut by_mother: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    for (fam, chils) in &graph.fam_chil {
        let Some((mother, _)) = graph.fam_wife.get(fam) else {
            continue;
        };
        for (child, _) in chils {
            let raw = st.indi_birth_raw.get(child);
            let Some(raw) = raw else { continue };
            let Some(ord) = date_ordinal(raw) else {
                continue;
            };
            by_mother
                .entry(mother.clone())
                .or_default()
                .push((ord, child.clone()));
        }
    }
    for kids in by_mother.values_mut() {
        kids.sort();
        // Consecutive pairs suffice: the list is sorted, so any wider gap
        // is larger than the step inside it.
        for w in 1..kids.len() {
            let (prev_ord, prev_child) = kids[w - 1].clone();
            let (ord, child) = kids[w].clone();
            let gap = ord - prev_ord;
            if gap >= 1 && gap <= thr.sibling_max_gap {
                diags.push(
                    Diag::new(
                        "W704",
                        Category::Suspicious,
                        Severity::Warning,
                        graph.records.get(&child).map(|r| r.1).unwrap_or(0),
                        format!(
                            "{} and {} born {} days apart: possible merged family or duplicate",
                            prev_child, child, gap
                        ),
                    )
                    .in_ruleset("hygiene"),
                );
            }
        }
    }
}

/// Maximum physical line length in GEDCOM 5.5.1 (spec chapter 1 grammar: 255 chars).
pub(crate) const MAX_LINE_LEN: usize = 255;

/// W713: line-too-long. Flags any line exceeding 255 characters or 255 UTF-8 bytes
/// in GEDCOM 5.5.1. GEDCOM 7.0 explicitly eliminated this restriction (and removed CONC),
/// so the check only applies to non-7.0 files.
pub(crate) fn check_line_length(diags: &mut Vec<Diag>, l: &Line, version: Version) {
    if version == Version::V70 {
        return;
    }
    let char_len = l.raw.chars().count();
    let byte_len = l.raw.len();
    if char_len <= MAX_LINE_LEN && byte_len <= MAX_LINE_LEN {
        return;
    }
    let tag_str = if l.tag.is_empty() {
        "line".to_string()
    } else {
        format!("{} line", l.tag)
    };
    let count_str = if char_len == byte_len {
        format!("{} chars", char_len)
    } else {
        format!("{} chars, {} bytes", char_len, byte_len)
    };
    let unit = if char_len > MAX_LINE_LEN {
        "characters"
    } else {
        "bytes"
    };
    diags.push(
        Diag::new(
            "W713",
            Category::Style,
            Severity::Info,
            l.no,
            format!(
                "{} exceeds 255 {} ({}): split with CONC",
                tag_str, unit, count_str
            ),
        )
        .in_ruleset("hygiene"),
    );
}
