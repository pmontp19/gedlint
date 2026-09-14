//! MyHeritage-style consistency checks: the gap analysis items 1-9.
//!
//! Item 4 (any dated fact before birth or after death) extends the core
//! `W310`; the rest are heuristics with real false-positive rates (a
//! namesake after an infant death, a late father, a forest of detached
//! branches), so they all live in the opt-in `hygiene` ruleset as
//! `W705`-`W712` and every numeric threshold comes from
//! `[lints.thresholds]`, never from a literal.
//!
//! The pass in `rules/mod.rs` feeds this module every `DATE` (with the
//! level-1 tag it hangs under) and every `PLAC` value; the finish functions
//! below join those against the `People` and `Graph` indexes. Everything is
//! pure string and integer work, WASM-safe.

use crate::config::Thresholds;
use crate::diag::{Category, Diag, Severity};
use crate::parse::{is_aft, is_inexact, truncate, year_of};
use crate::rules::graph::Graph;
use crate::rules::individuals::People;
use crate::rules::names::surname_slot;

/// Every `DATE` and `PLAC` payload seen in the pass, with the context the
/// finish functions need. `fact_dates` carries the level-1 tag the DATE
/// hangs under (`OCCU`, `RESI`, `BIRT`, ...), so the generic chronology
/// check can skip the vital events `W310` already owns.
#[derive(Default)]
pub(crate) struct Consistency {
    /// (record xref, level-1 tag, DATE value, DATE line).
    pub(crate) fact_dates: Vec<(String, String, String, usize)>,
    /// (record xref, PLAC value, PLAC line).
    pub(crate) plac_values: Vec<(String, String, usize)>,
}

impl Consistency {
    pub(crate) fn record_fact(&mut self, rec: &str, tag: &str, val: &str, line: usize) {
        if !val.is_empty() {
            self.fact_dates
                .push((rec.to_string(), tag.to_string(), val.to_string(), line));
        }
    }

    pub(crate) fn record_plac(&mut self, rec: &str, val: &str, line: usize) {
        if !val.is_empty() {
            self.plac_values
                .push((rec.to_string(), val.to_string(), line));
        }
    }
}

/// Run every check in this module. Emission is unconditional: `lib.rs`
/// drops the diagnostics of rules the configuration leaves off, exactly as
/// it does for the other opt-in rulesets.
pub(crate) fn finish_all(
    diags: &mut Vec<Diag>,
    st: &Consistency,
    people: &People,
    graph: &Graph,
    thr: &Thresholds,
) {
    finish_fact_chronology(diags, st, people, graph);
    finish_alive_too_old(diags, people, graph, thr);
    finish_spouse_gap(diags, people, graph, thr);
    finish_marriage_age(diags, people, graph, thr);
    finish_same_first_name(diags, people, graph);
    finish_disconnected(diags, graph);
    finish_name_hygiene(diags, st, people, graph);
    finish_children_surnames(diags, people, graph);
    finish_place_content(diags, st);
}

// ---------------------------------------------------------------------------
// Item 4 (core): any dated fact before birth or after death (W310)
// ---------------------------------------------------------------------------

/// The vital events `W310` in `events.rs` already sequences. Everything
/// else with a date is compared here against the same birth and death.
const VITAL_EVENTS: &[&str] = &["BIRT", "CHR", "BAPM", "DEAT", "BURI", "MARR", "DIV"];

/// W310 on non-vital dated facts: a residence, occupation, census entry or
/// custom fact dated before the birth or after the death is the largest
/// MyHeritage warning group (record hints attached to the wrong person, or
/// to the wrong marriage when several spouses exist). Inexact dates prove
/// nothing near a boundary and are skipped, exactly as in `events.rs`.
pub(crate) fn finish_fact_chronology(
    diags: &mut Vec<Diag>,
    st: &Consistency,
    people: &People,
    graph: &Graph,
) {
    let mut facts = st.fact_dates.clone();
    facts.sort_by_key(|f| f.3);
    for (rec, tag, val, line) in &facts {
        if VITAL_EVENTS.contains(&tag.as_str()) {
            continue;
        }
        if graph
            .records
            .get(rec)
            .map(|(k, _)| k != "INDI")
            .unwrap_or(true)
        {
            continue;
        }
        if is_inexact(val) {
            continue;
        }
        let Some(y) = year_of(val) else { continue };
        if y >= 10000 {
            continue;
        }
        if let Some(Some(b)) = people.indi_birth.get(rec) {
            if *b < 10000 && y < *b {
                diags.push(Diag::new(
                    "W310",
                    Category::Suspicious,
                    Severity::Error,
                    *line,
                    format!("{}: {} ({}) before birth ({})", rec, tag, y, b),
                ));
            }
        }
        if let Some(Some(d)) = people.indi_death.get(rec) {
            if *d < 10000 && y > *d {
                diags.push(Diag::new(
                    "W310",
                    Category::Suspicious,
                    Severity::Error,
                    *line,
                    format!("{}: {} ({}) after death ({})", rec, tag, y, d),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Item 1 (W705): alive but too old
// ---------------------------------------------------------------------------

/// W705: no death recorded and the birth is more than `max_alive_years`
/// before the latest year found anywhere in the file. The file's own
/// latest year stands in for today, so the engine needs no clock: a file
/// whose newest date is 1850 judges age against 1850, not against the wall
/// calendar. `DEAT Y` (dead, date unknown) and `AFT`-qualified births
/// suppress, since neither proves a living person.
fn finish_alive_too_old(diags: &mut Vec<Diag>, people: &People, graph: &Graph, thr: &Thresholds) {
    let mut latest: Option<i64> = None;
    for b in people.indi_birth.values().flatten() {
        if *b < 10000 {
            latest = Some(latest.map_or(*b, |m: i64| m.max(*b)));
        }
    }
    for d in people.indi_death.values().flatten() {
        if *d < 10000 {
            latest = Some(latest.map_or(*d, |m: i64| m.max(*d)));
        }
    }
    for years in people.fam_marrs.values() {
        for (y, _, _) in years {
            if *y < 10000 {
                latest = Some(latest.map_or(*y, |m: i64| m.max(*y)));
            }
        }
    }
    let Some(now) = latest else { return };
    let mut xrefs: Vec<&String> = people
        .indi_birth
        .keys()
        .filter(|x| {
            graph
                .records
                .get(*x)
                .map(|(k, _)| k == "INDI")
                .unwrap_or(false)
        })
        .collect();
    xrefs.sort();
    for xref in xrefs {
        let Some(Some(b)) = people.indi_birth.get(xref) else {
            continue;
        };
        if *b >= 10000 || *b + thr.max_alive_years > now {
            continue;
        }
        if people.indi_died.contains(xref) {
            continue;
        }
        if people.indi_death.get(xref).copied().flatten().is_some() {
            continue;
        }
        if people.birth_is_aft.get(xref).copied().unwrap_or(false) {
            continue;
        }
        diags.push(
            Diag::new(
                "W705",
                Category::Suspicious,
                Severity::Warning,
                graph.records.get(xref).map(|r| r.1).unwrap_or(0),
                format!(
                    "{}: born ({}) with no death recorded, would be {} in {}",
                    xref,
                    b,
                    now - b,
                    now
                ),
            )
            .in_ruleset("hygiene"),
        );
    }
}

// ---------------------------------------------------------------------------
// Item 2 (W706): large spouse age difference
// ---------------------------------------------------------------------------

/// W706: spouses' birth years further apart than `max_spouse_gap`. A gap
/// of a century is certainly a wrong year; a gap just over the threshold
/// may be real, so the message asks for verification. Qualified births on
/// either side suppress.
fn finish_spouse_gap(diags: &mut Vec<Diag>, people: &People, graph: &Graph, thr: &Thresholds) {
    let mut fams: Vec<&String> = graph.fam_husb.keys().collect();
    fams.sort();
    for fam in fams {
        let Some((h, _)) = graph.fam_husb.get(fam) else {
            continue;
        };
        let Some((w, _)) = graph.fam_wife.get(fam) else {
            continue;
        };
        let (Some(Some(hb)), Some(Some(wb))) = (people.indi_birth.get(h), people.indi_birth.get(w))
        else {
            continue;
        };
        if *hb >= 10000 || *wb >= 10000 {
            continue;
        }
        if people.birth_is_bef.get(h).copied().unwrap_or(false)
            || people.birth_is_aft.get(h).copied().unwrap_or(false)
            || people.birth_is_bef.get(w).copied().unwrap_or(false)
            || people.birth_is_aft.get(w).copied().unwrap_or(false)
        {
            continue;
        }
        let gap = (hb - wb).abs();
        if gap > thr.max_spouse_gap {
            diags.push(
                Diag::new(
                    "W706",
                    Category::Suspicious,
                    Severity::Warning,
                    graph.records.get(fam).map(|r| r.1).unwrap_or(0),
                    format!(
                        "{}: spouses {} (b. {}) and {} (b. {}) born {} years apart",
                        fam, h, hb, w, wb, gap
                    ),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Item 3 (W707): married too young / died too young to be a spouse
// ---------------------------------------------------------------------------

/// W707, first arm: a spouse younger than `min_marriage_age` at a wedding.
/// Suppression mirrors `W311`: an `AFT` marriage can be later than written
/// and a `BEF` birth can be earlier, so either qualifier excuses the gap.
fn finish_marriage_age(diags: &mut Vec<Diag>, people: &People, graph: &Graph, thr: &Thresholds) {
    let mut fams: Vec<&String> = people.fam_marrs.keys().collect();
    fams.sort();
    // Spouse -> first marriage line, for the died-too-young arm below.
    let mut married: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for fam in &fams {
        let Some(instances) = people.fam_marrs.get(*fam) else {
            continue;
        };
        for (mm, marr_line, raw) in instances {
            if *mm >= 10000 {
                continue;
            }
            let marr_aft = is_aft(raw);
            for slot in [&graph.fam_husb.get(*fam), &graph.fam_wife.get(*fam)] {
                let Some((px, _)) = slot else { continue };
                married.entry(px.clone()).or_insert(*marr_line);
                let Some(Some(pb)) = people.indi_birth.get(px) else {
                    continue;
                };
                if *pb >= 10000 {
                    continue;
                }
                if people.birth_is_bef.get(px).copied().unwrap_or(false) {
                    continue;
                }
                let age = mm - pb;
                if age < thr.min_marriage_age && !marr_aft {
                    diags.push(
                        Diag::new(
                            "W707",
                            Category::Suspicious,
                            Severity::Warning,
                            *marr_line,
                            format!(
                                "{}: {} married at age {} (b. {}, marr {})",
                                fam, px, age, pb, mm
                            ),
                        )
                        .in_ruleset("hygiene"),
                    );
                }
            }
        }
    }
    // Second arm: a married person who died before reaching marriageable
    // age. A `BEF` birth (really older) or an `AFT` death (really later)
    // excuses the arithmetic.
    for (px, marr_line) in &married {
        let (Some(Some(pb)), Some(Some(pd))) =
            (people.indi_birth.get(px), people.indi_death.get(px))
        else {
            continue;
        };
        if *pb >= 10000 || *pd >= 10000 {
            continue;
        }
        if people.birth_is_bef.get(px).copied().unwrap_or(false)
            || people.birth_is_aft.get(px).copied().unwrap_or(false)
            || people.death_is_aft.get(px).copied().unwrap_or(false)
        {
            continue;
        }
        if pd - pb < thr.min_marriage_age {
            diags.push(
                Diag::new(
                    "W707",
                    Category::Suspicious,
                    Severity::Warning,
                    *marr_line,
                    format!(
                        "{}: died at age {} (b. {}, d. {}), too young to marry",
                        px,
                        pd - pb,
                        pb,
                        pd
                    ),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Item 5 (W708): siblings with the same first name
// ---------------------------------------------------------------------------

/// The given part of a NAME value: before the first slash, trimmed.
fn given_part(name: &str) -> &str {
    let end = name.find('/').unwrap_or(name.len());
    name[..end].trim()
}

fn norm_given(name: &str) -> String {
    given_part(name)
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// W708: two children of one family sharing a first name. Often a
/// namesake after an infant death, sometimes the same person entered
/// twice: either way worth one look. Compares the given part of `1 NAME`,
/// case-folded; children without a usable name are skipped.
fn finish_same_first_name(diags: &mut Vec<Diag>, people: &People, graph: &Graph) {
    let mut fams: Vec<&String> = graph.fam_chil.keys().collect();
    fams.sort();
    for fam in fams {
        let Some(chils) = graph.fam_chil.get(fam) else {
            continue;
        };
        let mut kids: Vec<(&String, String)> = Vec::new();
        for (c, _) in chils {
            let Some(nm) = people.indi_name.get(c) else {
                continue;
            };
            let g = norm_given(nm);
            if g.is_empty() {
                continue;
            }
            kids.push((c, g));
        }
        kids.sort();
        for a in 0..kids.len() {
            for b in a + 1..kids.len() {
                if kids[a].1 == kids[b].1 {
                    diags.push(
                        Diag::new(
                            "W708",
                            Category::Suspicious,
                            Severity::Info,
                            graph.records.get(kids[b].0).map(|r| r.1).unwrap_or(0),
                            format!(
                                "{}: {} and {} share first name '{}'",
                                fam, kids[a].0, kids[b].0, kids[a].1
                            ),
                        )
                        .in_ruleset("hygiene"),
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Item 6 (W709): disconnected individuals
// ---------------------------------------------------------------------------

/// W709: an individual linked to no family at all: no FAMC/FAMS, and no
/// family names them as spouse or child. Detached branches are legitimate
/// (a second tree in one file), so this is an `Info`: it catches the
/// leftovers of a deleted or moved branch.
fn finish_disconnected(diags: &mut Vec<Diag>, graph: &Graph) {
    use std::collections::HashSet;
    let mut linked: HashSet<&String> = HashSet::new();
    for (child, fams) in &graph.indi_famc {
        if !fams.is_empty() {
            linked.insert(child);
        }
    }
    for (_, from, tag, _) in &graph.pending {
        if tag == "FAMC" || tag == "FAMS" {
            linked.insert(from);
        }
    }
    for (px, _) in graph.fam_husb.values().chain(graph.fam_wife.values()) {
        linked.insert(px);
    }
    for chils in graph.fam_chil.values() {
        for (c, _) in chils {
            linked.insert(c);
        }
    }
    let mut loners: Vec<(&String, usize)> = Vec::new();
    for (xref, (kind, line)) in &graph.records {
        if kind == "INDI" && !linked.contains(xref) {
            loners.push((xref, *line));
        }
    }
    loners.sort();
    for (xref, line) in loners {
        diags.push(
            Diag::new(
                "W709",
                Category::Suspicious,
                Severity::Info,
                line,
                format!("{}: individual is not connected to any family", xref),
            )
            .in_ruleset("hygiene"),
        );
    }
}

// ---------------------------------------------------------------------------
// Item 7 (W710): name spacing, misplaced affixes, short years, missing sex
// ---------------------------------------------------------------------------

/// Tokens that belong in a prefix field, not in the given name. Compared
/// uppercased with a trailing period stripped.
const NAME_PREFIXES: &[&str] = &[
    "DR", "MR", "MRS", "MS", "MISS", "REV", "PROF", "PASTOR", "SIR", "DAME", "LORD", "LADY",
    "CAPT", "COL", "GEN", "LT", "HON", "FR",
];

/// Tokens that belong in a suffix field, not in the name value. `SR` is
/// deliberately included: as a Catalan surname particle it never stands
/// alone as a whitespace-separated token the way a suffix does.
const NAME_SUFFIXES: &[&str] = &[
    "JR", "SR", "II", "III", "IV", "V", "VI", "ESQ", "ESQUIRE", "PHD", "MD", "DDS",
];

fn clean_token(t: &str) -> String {
    t.trim_end_matches('.').to_ascii_uppercase()
}

/// True when a DATE value carries a 1-2 digit year: a month plus a short
/// trailing year (`12 JAN 22`), or a bare short year (`22`). A month with
/// no year at all (`12 JAN`) is incomplete, not short, and stays silent.
fn has_short_year(val: &str) -> bool {
    if year_of(val).is_some() {
        return false;
    }
    const MONTHS: &[&str] = &[
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let toks: Vec<&str> = val.split_whitespace().collect();
    if toks.is_empty() {
        return false;
    }
    let last = toks[toks.len() - 1];
    if last.len() > 2 || !last.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if toks.len() == 1 {
        return true;
    }
    toks[..toks.len() - 1]
        .iter()
        .any(|t| MONTHS.contains(&t.to_ascii_uppercase().as_str()))
}

/// W710: four cheap name and date hygiene checks in one rule. Spacing and
/// affixes read the accumulated `1 NAME` value at the record line;
/// short years read every DATE payload at its own line; missing sex reads
/// the record index. All `Info`: each has legitimate counterexamples.
fn finish_name_hygiene(diags: &mut Vec<Diag>, st: &Consistency, people: &People, graph: &Graph) {
    let mut xrefs: Vec<&String> = graph
        .records
        .iter()
        .filter(|(_, (k, _))| *k == "INDI")
        .map(|(x, _)| x)
        .collect();
    xrefs.sort();
    for xref in xrefs {
        let line = graph.records.get(xref).map(|r| r.1).unwrap_or(0);
        if let Some(nm) = people.indi_name.get(xref) {
            if nm != &nm.trim().to_string() || nm.contains("  ") {
                diags.push(
                    Diag::new(
                        "W710",
                        Category::Style,
                        Severity::Info,
                        line,
                        format!(
                            "{}: name has leading, trailing or double spaces: {}",
                            xref,
                            truncate(nm, 50)
                        ),
                    )
                    .in_ruleset("hygiene"),
                );
            }
            let given_toks: Vec<String> =
                given_part(nm).split_whitespace().map(clean_token).collect();
            let surname_toks: Vec<String> = surname_slot(nm)
                .unwrap_or("")
                .split_whitespace()
                .map(clean_token)
                .collect();
            let mut hits: Vec<String> = Vec::new();
            for t in &given_toks {
                if NAME_PREFIXES.contains(&t.as_str()) {
                    hits.push(format!("prefix '{}' in given name", t));
                }
            }
            for t in given_toks.iter().chain(surname_toks.iter()) {
                if NAME_SUFFIXES.contains(&t.as_str()) {
                    hits.push(format!("suffix '{}' in name", t));
                }
            }
            if !hits.is_empty() {
                diags.push(
                    Diag::new(
                        "W710",
                        Category::Style,
                        Severity::Info,
                        line,
                        format!(
                            "{}: {} (move it to a prefix or suffix field)",
                            xref,
                            hits.join("; ")
                        ),
                    )
                    .in_ruleset("hygiene"),
                );
            }
        }
        if !people.indi_sex.contains_key(xref) {
            diags.push(
                Diag::new(
                    "W710",
                    Category::Style,
                    Severity::Info,
                    line,
                    format!("{}: no SEX recorded", xref),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
    let mut dates = st.fact_dates.clone();
    dates.sort_by_key(|d| d.3);
    for (rec, _, val, line) in &dates {
        if has_short_year(val) {
            diags.push(
                Diag::new(
                    "W710",
                    Category::Style,
                    Severity::Info,
                    *line,
                    format!(
                        "{}: DATE with a 2-digit year (write all four digits): {}",
                        rec,
                        truncate(val, 40)
                    ),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Item 8 (W711): children with different surnames
// ---------------------------------------------------------------------------

/// W711: children of one family carrying different surnames. Remarriage,
/// adoption and double surnames make this common enough to stay `Info`,
/// but an unexpected split often means a child attached to the wrong
/// household. Compares the slash-delimited surname slot, case-folded.
fn finish_children_surnames(diags: &mut Vec<Diag>, people: &People, graph: &Graph) {
    let mut fams: Vec<&String> = graph.fam_chil.keys().collect();
    fams.sort();
    for fam in fams {
        let Some(chils) = graph.fam_chil.get(fam) else {
            continue;
        };
        let mut surnames: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut count = 0;
        for (c, _) in chils {
            let Some(nm) = people.indi_name.get(c) else {
                continue;
            };
            let s = surname_slot(nm)
                .unwrap_or("")
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if s.is_empty() {
                continue;
            }
            count += 1;
            surnames.insert(s);
        }
        if count >= 2 && surnames.len() > 1 {
            let list: Vec<&String> = surnames.iter().collect();
            diags.push(
                Diag::new(
                    "W711",
                    Category::Suspicious,
                    Severity::Info,
                    graph.records.get(fam).map(|r| r.1).unwrap_or(0),
                    format!(
                        "{}: children carry different surnames: {}",
                        fam,
                        list.iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Item 9 (W712): place resembles a cause of death, or a date
// ---------------------------------------------------------------------------

/// Cause-of-death words that turn up inside PLAC values. Lowercase match
/// against the lowercased value; deliberately short to stay precise.
const CAUSE_WORDS: &[&str] = &[
    "holocaust",
    "died of",
    "cause of death",
    "cancer",
    "pneumonia",
    "tuberculosis",
    "typhoid",
    "cholera",
    "suicide",
    "murdered",
    "killed",
    "drowned",
    "executed",
];

fn month_token_present(val: &str) -> bool {
    const MONTHS: &[&str] = &[
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    val.split(|c: char| !c.is_ascii_alphabetic())
        .any(|t| MONTHS.contains(&t.to_ascii_uppercase().as_str()))
}

/// W712: a PLAC that names a cause of death (`Holocaust` in the death
/// place belongs in CAUS) or that looks like a date (a birth year typed
/// into the place field). Never repaired: only a human knows which field
/// the text belongs in.
fn finish_place_content(diags: &mut Vec<Diag>, st: &Consistency) {
    let mut placs = st.plac_values.clone();
    placs.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
    for (rec, val, line) in &placs {
        let lower = val.to_lowercase();
        if let Some(hit) = CAUSE_WORDS.iter().find(|w| lower.contains(*w)) {
            diags.push(
                Diag::new(
                    "W712",
                    Category::Style,
                    Severity::Info,
                    *line,
                    format!(
                        "{}: place resembles a cause of death ('{}'): {} (move it to CAUS)",
                        rec,
                        hit,
                        truncate(val, 50)
                    ),
                )
                .in_ruleset("hygiene"),
            );
            continue;
        }
        if month_token_present(val) || year_of(val).is_some() {
            diags.push(
                Diag::new(
                    "W712",
                    Category::Style,
                    Severity::Info,
                    *line,
                    format!(
                        "{}: place looks like a date: {} (it belongs in DATE)",
                        rec,
                        truncate(val, 50)
                    ),
                )
                .in_ruleset("hygiene"),
            );
        }
    }
}
