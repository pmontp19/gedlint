//! The rules, grouped by domain, and the single streaming pass that runs them.
//!
//! `lint_lines` walks the parsed lines once, in the original order, handing
//! each line to the rule groups that care about it. Every group owns its own
//! piece of mutable state (`Structure`, `Graph`, `People`, `Events`, `Names`,
//! `EnumState`); the pass itself only owns the traversal cursor.

pub(crate) mod dates;
pub(crate) mod encoding;
pub(crate) mod enums;
pub(crate) mod events;
pub(crate) mod graph;
pub(crate) mod individuals;
pub(crate) mod names;
pub(crate) mod structure;
pub(crate) mod style;
pub(crate) mod upgrade;

use std::collections::HashSet;

use crate::diag::{Category, Diag, Report, Severity};
use crate::parse::{detect_version, parse_line, truncate, Line, BOM_LEN};

use enums::EnumState;
use events::Events;
use graph::Graph;
use individuals::People;
use names::Names;
use structure::Structure;

pub(crate) fn lint_lines(text: &str) -> Report {
    // The BOM is not part of the grammar: it is reported in encoding_diags
    // and stripped so "0 HEAD" on the first line is recognized. Spans, on the
    // other hand, are on-disk offsets, so line 1 carries the stripped bytes as
    // its span base and every span lands where the file really has it.
    let bom = if text.starts_with('\u{FEFF}') { BOM_LEN } else { 0 };
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut diags: Vec<Diag> = Vec::new();
    let raw_lines: Vec<&str> = text.lines().collect();
    let lines: Vec<Line> = raw_lines
        .iter()
        .enumerate()
        .map(|(i, l)| parse_line(i + 1, l).with_span_base(if i == 0 { bom } else { 0 }))
        .collect();

    // Version state (HEAD.GEDC.VERS).
    let version = detect_version(&lines);

    // Per-domain rule state.
    let mut structure = Structure::default();
    let mut graph = Graph::default();
    let mut people = People::default();
    let mut events = Events::default();
    let mut names = Names::default();
    let mut enum_state = EnumState::default();

    // Traversal cursor owned by the pass itself.
    let mut cur: Option<(String, String)> = None; // (xref, kind)
    let mut cur_sub = String::new();
    // Parent stack for context-sensitive rules (enums, OTHER/PHRASE):
    // stack[i] is the tag/line of the nearest preceding line at level i.
    let mut stack: Vec<(String, usize)> = Vec::new();

    for l in &lines {
        let Some(lvl) = l.level else {
            // A blank line has no level and no content: not a malformed line
            // (E002 already treats blanks as non-content). prev_level survives
            // so a level jump across a blank line is still caught.
            if l.raw.trim().is_empty() {
                continue;
            }
            diags.push(Diag::new(
                "E001",
                Category::Correctness,
                Severity::Error,
                l.no,
                format!("malformed line (non-numeric level): {}", truncate(&l.raw, 60)),
            ));
            structure.prev_level = None;
            continue;
        };
        // A NAME run ends here: any level <= 1 line closes the CONC/CONT run.
        if lvl <= 1 {
            names::flush(&mut diags, &mut names.name_buf, &mut people.indi_name);
        }
        structure::check_position(&mut diags, &mut structure, l, lvl);
        structure::check_level_jump(&mut diags, &mut structure, l, lvl);
        // Parent stack: truncate to the current level, read the parent
        // (nearest preceding line one level up), then push this line.
        while stack.len() > lvl as usize {
            stack.pop();
        }
        let parent: Option<(String, usize)> = stack.last().cloned();
        stack.push((l.tag.clone(), l.no));
        let parent_tag: &str = parent.as_ref().map(|p| p.0.as_str()).unwrap_or("");

        structure::check_xref_syntax(&mut diags, l);
        structure::check_continuation(&mut diags, l, version, &parent);
        style::check_control_chars(&mut diags, l);

        if lvl == 0 {
            if let Some((xref, _)) = cur.take() {
                let _ = xref;
            }
            // Birth/death resolve at record change via the already stored maps.
            cur_sub.clear();
            events.cur_event = None;
            structure::enter_record(&mut structure, l);
            cur = graph::open_record(&mut diags, &mut graph, &mut people, l);
            continue;
        }

        if lvl == 1 {
            cur_sub = l.tag.clone();
            events.cur_event = None;
            structure::check_head_char(&mut diags, &structure, l, version);
            structure::check_head_gedc(&mut diags, &mut structure, l);
            if let Some((xref, kind)) = cur.clone() {
                events::open_instances(&mut events, l, &xref, lvl);
                match (kind.as_str(), l.tag.as_str()) {
                    ("INDI", "FAMS") | ("INDI", "FAMC") => {
                        graph::indi_fam_link(&mut diags, &mut graph, l, &xref);
                    }
                    ("FAM", "HUSB") | ("FAM", "WIFE") | ("FAM", "CHIL") => {
                        graph::fam_member_link(&mut diags, &mut graph, l, &xref);
                    }
                    ("INDI", "NAME") => {
                        names::open(&mut names, l, &xref);
                    }
                    ("INDI", "SEX") => {
                        individuals::record_sex(&mut diags, &mut people, l, &xref);
                    }
                    ("FAM", "MARR") | ("INDI", "BIRT") | ("INDI", "DEAT") => {
                        individuals::record_event_year(&mut people, l, &xref, &kind);
                    }
                    _ => {
                        graph::generic_pointer(&mut graph, l, &xref);
                    }
                }
                style::check_plac_url_record(&mut diags, l);
                style::check_note_html(&mut diags, l);
                upgrade::check_rela_record(&mut diags, l, version);
                upgrade::check_vendor_tag(&mut diags, l, version);
                // W306 enum values at level 1 (rare) + OTHER/PHRASE tracking.
                enums::check_enum(
                    &mut diags,
                    &l.tag,
                    &l.value,
                    parent_tag,
                    &xref,
                    parent.clone(),
                    &mut enum_state.pending_other,
                    version,
                    l.no,
                );
            }
            continue;
        }

        // Level >= 2.
        names::continue_value(&mut diags, &mut names, &mut people.indi_name, l, lvl);
        structure::check_head_vers(&mut diags, &mut structure, l, lvl, &parent);
        graph::sub_pointer(&mut graph, l, &cur);
        // W306 enum values + OTHER/PHRASE tracking.
        {
            let rec = cur.clone().map(|c| c.0).unwrap_or_default();
            if l.tag == "PHRASE" {
                if let Some((ptag, pline)) = &parent {
                    enum_state.phrased.insert((rec.clone(), ptag.clone(), *pline));
                    // A PHRASE may also hang under the OTHER-valued structure
                    // itself (maximal70: "2 TYPE OTHER" + "3 PHRASE"): mark
                    // the grandparent too, not only the sibling slot. The
                    // stack already holds PHRASE itself at the top.
                    if stack.len() >= 3 {
                        let (gtag, gline) = &stack[stack.len() - 3];
                        enum_state.phrased.insert((rec.clone(), gtag.clone(), *gline));
                    }
                }
            } else {
                enums::check_enum(
                    &mut diags,
                    &l.tag,
                    &l.value,
                    parent_tag,
                    &rec,
                    parent.clone(),
                    &mut enum_state.pending_other,
                    version,
                    l.no,
                );
            }
            events::record_required(&mut events, l, &rec, &cur_sub, &parent);
            events::check_detail_singletons(&mut diags, &mut events, l, lvl, &rec);
        }
        upgrade::check_rela_sub(&mut diags, l, version);
        individuals::record_sub_date(&mut people, l, &cur_sub, &cur);
        upgrade::check_pedi_case(&mut diags, l, version);
        style::check_plac_url(&mut diags, l);
        // DATE with suspicious format (non-ENG months, lowercase "about"...).
        if l.tag == "DATE" && !l.value.is_empty() {
            dates::check_date_style(&mut diags, l.no, &l.value, version);
        }
    }

    // A NAME run ending at EOF (NAME directly before TRLR) still needs W402.
    names::flush(&mut diags, &mut names.name_buf, &mut people.indi_name);

    structure::finish(&mut diags, &structure);
    enums::finish(&mut diags, &enum_state);
    events::finish(&mut diags, &events, version);
    graph::finish_refs(&mut diags, &graph);
    graph::finish_symmetry(&mut diags, &graph);
    individuals::finish_lifespans(&mut diags, &people, &graph);
    individuals::finish_parent_ages(&mut diags, &people, &graph);
    individuals::finish_sex(&mut diags, &people, version);
    individuals::finish_duplicates(&mut diags, &people, &graph);

    let individuals = people.indi_birth.len();
    let mut families_set: HashSet<&String> = HashSet::new();
    for k in graph.fam_chil.keys().chain(graph.fam_husb.keys()).chain(graph.fam_wife.keys()) {
        families_set.insert(k);
    }

    // Issue 30: severity and line alone leave ties (whole-file findings)
    // to insertion order, which a HashMap iteration re-seeds per process.
    // The code and message backstop makes the order a total one, so the
    // same input can never print in a different order.
    diags.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.line.cmp(&b.line))
            .then(a.code.cmp(b.code))
            .then(a.msg.cmp(&b.msg))
    });
    Report { version, diags, lines: lines.len(), individuals, families: families_set.len() }
}
