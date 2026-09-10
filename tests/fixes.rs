//! Structured fixes (issue 19): `Edit` + `Applicability` + `apply_edits`,
//! and the byte-for-byte behaviour `fix_bytes` must keep.

use std::collections::BTreeSet;

use gedlint::{
    apply_edits, compute_edits, compute_edits_with, fix_bytes, fix_bytes_with, lint_bytes,
    lint_bytes_with, normalize_endings, parse_config, Applicability, Config, Edit, FixSelection,
};

const HEAD: &[u8] = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n";

fn edit(
    code: &'static str,
    lines: (usize, usize),
    replacement: &[&str],
    applicability: Applicability,
) -> Edit {
    Edit {
        code,
        lines,
        replacement: replacement.iter().map(|l| l.as_bytes().to_vec()).collect(),
        applicability,
        note: format!("test edit {}", code),
    }
}

fn golden(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golden/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    ))
    .unwrap()
}

// ---------------------------------------------------------------------------
// apply_edits
// ---------------------------------------------------------------------------

#[test]
fn empty_selection_is_a_no_op() {
    // Nothing selected must give the bytes back untouched, including the
    // shapes that are easy to get wrong: no trailing newline, CRLF, empty.
    for data in [
        &b"0 HEAD\n1 GEDC\n0 TRLR\n"[..],
        &b"0 HEAD\n1 GEDC\n0 TRLR"[..],
        &b"0 HEAD\r\n1 GEDC\r\n0 TRLR\r\n"[..],
        &b"   \n\n"[..],
        &b"\n"[..],
        &b""[..],
        &golden("TGC551.ged")[..],
    ] {
        let (out, dropped) = apply_edits(data, &[]);
        assert_eq!(out, data, "empty selection rewrote {} bytes", data.len());
        assert!(dropped.is_empty());
    }
}

#[test]
fn overlapping_edits_first_wins_and_second_is_returned() {
    let data = b"a\nb\nc\n";
    let first = edit("T1", (1, 2), &["joined"], Applicability::Safe);
    let second = edit("T2", (2, 3), &["other"], Applicability::Safe);
    let (out, dropped) = apply_edits(data, &[first, second.clone()]);
    assert_eq!(out, b"joined\nc\n");
    assert_eq!(
        dropped,
        vec![second],
        "the loser comes back so the caller can re-run"
    );
}

#[test]
fn re_running_applies_the_dropped_edit() {
    let data = b"a\nb\nc\n";
    let second = edit("T2", (2, 3), &["other"], Applicability::Safe);
    let (once, dropped) = apply_edits(
        data,
        &[edit("T1", (1, 2), &["joined"], Applicability::Safe), second],
    );
    // Line numbers moved with the first edit, so the caller recomputes.
    let (twice, left) = apply_edits(
        &once,
        &[edit("T2", (2, 2), &["other"], Applicability::Safe)],
    );
    assert_eq!(dropped.len(), 1);
    assert_eq!(twice, b"joined\nother\n");
    assert!(left.is_empty());
}

#[test]
fn an_edit_can_delete_its_range() {
    // Supported by the type, and not something any current rule does:
    // `--fix` never deletes genealogical data (AGENTS.md).
    let data = b"a\nb\nc\n";
    let (out, dropped) = apply_edits(data, &[edit("T1", (2, 2), &[], Applicability::Safe)]);
    assert_eq!(out, b"a\nc\n");
    assert!(dropped.is_empty());
    assert!(compute_edits(&golden("maximal70.ged"))
        .iter()
        .all(|e| !e.replacement.is_empty()));
}

#[test]
fn one_edit_can_replace_a_range_with_several_lines() {
    let data = b"a\nb\nc\n";
    let (out, _) = apply_edits(
        data,
        &[edit("T1", (1, 2), &["x", "y", "z"], Applicability::Safe)],
    );
    assert_eq!(out, b"x\ny\nz\nc\n");
}

#[test]
fn out_of_range_and_inverted_ranges_are_dropped() {
    let data = b"a\nb\n"; // 3 lines: "a", "b", ""
    for bad in [(0, 1), (1, 9), (9, 9), (3, 2)] {
        let e = edit("T1", bad, &["x"], Applicability::Safe);
        let (out, dropped) = apply_edits(data, std::slice::from_ref(&e));
        assert_eq!(out, data, "range {:?} must not rewrite anything", bad);
        assert_eq!(dropped, vec![e], "range {:?} must come back", bad);
    }
}

#[test]
fn a_dropped_edit_still_reserves_its_lines() {
    // Otherwise a lower-priority edit slips inside the range of a repair
    // that is only postponed, and the postponed one then reads a line it
    // did not expect.
    let data = b"a\nb\nc\nd\n";
    let (out, dropped) = apply_edits(
        data,
        &[
            edit("T1", (2, 2), &["B"], Applicability::Safe),
            edit("T2", (1, 2), &["merged"], Applicability::Safe), // loses to T1
            edit("T3", (1, 1), &["A"], Applicability::Safe),      // inside T2's range
        ],
    );
    assert_eq!(out, b"a\nB\nc\nd\n");
    assert_eq!(dropped.len(), 2, "T2 and T3 both wait: {:?}", dropped);
}

#[test]
fn edits_may_arrive_in_any_order() {
    let data = b"a\nb\nc\n";
    let (out, dropped) = apply_edits(
        data,
        &[
            edit("T1", (3, 3), &["C"], Applicability::Safe),
            edit("T2", (1, 1), &["A"], Applicability::Safe),
        ],
    );
    assert_eq!(out, b"A\nb\nC\n");
    assert!(dropped.is_empty());
}

// ---------------------------------------------------------------------------
// Applicability and selection
// ---------------------------------------------------------------------------

#[test]
fn maybe_incorrect_needs_an_explicit_opt_in() {
    let opinionated = edit("W702", (1, 1), &["0 Head"], Applicability::MaybeIncorrect);
    let provable = edit("E001", (1, 1), &["1 CONT x"], Applicability::Safe);

    let default = FixSelection::default();
    assert!(
        !default.allows(&opinionated),
        "a bare --fix must never apply MaybeIncorrect"
    );
    assert!(default.allows(&provable));

    let opt_in = FixSelection {
        only: Vec::new(),
        allow_unsafe: true,
    };
    assert!(opt_in.allows(&opinionated));
    assert!(opt_in.allows(&provable));
}

#[test]
fn only_restricts_by_code_ignoring_case() {
    let sel = FixSelection {
        only: vec!["e001".into()],
        allow_unsafe: false,
    };
    assert!(sel.allows(&edit("E001", (1, 1), &["x"], Applicability::Safe)));
    assert!(!sel.allows(&edit("E101", (1, 1), &["x"], Applicability::Safe)));
    // `only` never widens applicability.
    assert!(!sel.allows(&edit("E001", (1, 1), &["x"], Applicability::MaybeIncorrect)));
}

#[test]
fn only_applies_one_repair_and_reports_one_line() {
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT part   \norphan line\n0 TRLR\n",
    ]
    .concat();
    let (all, applied_all) = fix_bytes(&data);
    assert_eq!(applied_all.len(), 2, "{:?}", applied_all);

    let sel = FixSelection {
        only: vec!["E001".into()],
        allow_unsafe: false,
    };
    let (only_e001, applied) = fix_bytes_with(&data, &sel, &Config::default());
    assert_eq!(applied, vec!["E001: 1 orphan lines prefixed with CONT"]);
    let text = String::from_utf8(only_e001).unwrap();
    assert!(text.contains("3 CONT orphan line"), "{}", text);
    assert!(
        text.contains("2 TEXT part   \n"),
        "the whitespace repair was not selected: {:?}",
        text
    );
    assert_ne!(String::from_utf8(all).unwrap(), text);
}

#[test]
fn an_unknown_only_code_repairs_nothing() {
    let data = [HEAD, b"0 TRLR   \n"].concat();
    let sel = FixSelection {
        only: vec!["W999".into()],
        allow_unsafe: false,
    };
    let (out, applied) = fix_bytes_with(&data, &sel, &Config::default());
    assert_eq!(out, data);
    assert!(applied.is_empty());
}

// ---------------------------------------------------------------------------
// compute_edits
// ---------------------------------------------------------------------------

#[test]
fn compute_edits_describes_the_orphan_repair() {
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT part\norphan line\n0 TRLR\n",
    ]
    .concat();
    let edits = compute_edits(&data);
    assert_eq!(edits.len(), 1, "{:?}", edits);
    let e = &edits[0];
    assert_eq!(e.code, "E001");
    assert_eq!(e.lines, (7, 7));
    assert_eq!(e.replacement, vec![b"3 CONT orphan line".to_vec()]);
    assert_eq!(e.applicability, Applicability::Safe);
    assert_eq!(e.note, "prefix the orphan line with \"3 CONT \"");
}

#[test]
fn compute_edits_rejoins_a_conc_run_as_one_edit() {
    // Three CONC lines each cut inside a character belong to the line above
    // the run, not to each other.
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n2 CONC ");
    data.extend_from_slice(&[0xA9, b' ', b'M', b'a', 0xC3]);
    data.extend_from_slice(b"\n2 CONC ");
    data.extend_from_slice(&[0xB1, b'a', b'n', b'a']);
    data.extend_from_slice(b"\n0 TRLR\n");

    let edits = compute_edits(&data);
    assert_eq!(edits.len(), 1, "{:?}", edits);
    assert_eq!(edits[0].code, "E101");
    assert_eq!(edits[0].lines, (5, 7), "the anchor plus both CONC lines");
    assert_eq!(
        edits[0].note,
        "rejoin 2 CONC lines split inside a UTF-8 sequence"
    );
    assert_eq!(
        edits[0].replacement,
        vec!["1 NAME José Mañana".as_bytes().to_vec()]
    );

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec!["E101: rejoined 2 CONC lines with split UTF-8"]
    );
    assert!(String::from_utf8(fixed)
        .unwrap()
        .contains("1 NAME José Mañana"));
}

#[test]
fn a_rejoin_keeps_bytes_that_are_not_valid_utf8() {
    // The whole point of E101 is a file that is not valid UTF-8, and a
    // pathological split does not become valid by being rejoined. The
    // replacement must be the raw bytes, never a lossy string.
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n2 CONC ");
    data.extend_from_slice(&[0xA9, 0xA9, b' ', b'x']);
    data.extend_from_slice(b"\n0 TRLR\n");

    let edits = compute_edits(&data);
    assert_eq!(
        edits[0].replacement,
        vec![b"1 NAME Jos\xC3\xA9\xA9 x".to_vec()]
    );
    let (fixed, _) = fix_bytes(&data);
    assert!(
        fixed.windows(3).any(|w| w == [0xC3, 0xA9, 0xA9]),
        "the stray byte must survive verbatim"
    );
    assert!(String::from_utf8(fixed).is_err());
}

#[test]
fn compute_edits_returns_repairs_in_priority_order() {
    // The pass that runs first decides what a later pass on the same lines
    // reads: E001 before E101 (the CONT prefix is what the rejoin reads),
    // and style before E101 (a pad the rejoin absorbs must not land
    // between the halves of the cut character, #36).
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT part  \norphan line  \n0 TRLR\n",
    ]
    .concat();
    let codes: Vec<&str> = compute_edits(&data).iter().map(|e| e.code).collect();
    assert_eq!(codes, vec!["E001", "style", "style"]);
}

#[test]
fn compute_edits_finds_nothing_in_a_clean_file() {
    for name in [
        "minimal70.ged",
        "maximal70.ged",
        "remarriage1.ged",
        "same-sex-marriage.ged",
    ] {
        assert!(compute_edits(&golden(name)).is_empty(), "{}", name);
    }
}

// ---------------------------------------------------------------------------
// E005: nested CONT/CONC collapse to siblings (#51)
// ---------------------------------------------------------------------------

#[test]
fn a_single_nested_cont_repairs_and_re_lints_clean() {
    // The trailing `2 CONT` is a legal sibling (its parent is the NOTE,
    // not the CONC beside it): only the nested line may get an edit.
    let data = [
        HEAD,
        b"0 @I1@ INDI\n1 NOTE some text\n2 CONC continued\n3 CONT next paragraph\n2 CONT last line\n0 TRLR\n",
    ]
    .concat();
    assert!(lint_bytes(&data).diags.iter().any(|d| d.code == "E005"));

    let edits = compute_edits(&data);
    assert_eq!(edits.len(), 1, "{:?}", edits);
    let e = &edits[0];
    assert_eq!(e.code, "E005");
    assert_eq!(e.lines, (7, 7));
    assert_eq!(e.replacement, vec![b"2 CONT next paragraph".to_vec()]);
    assert_eq!(e.applicability, Applicability::Safe);
    assert_eq!(e.note, "rewrite the level to 2 (CONT/CONC do not nest)");

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(applied, vec!["E005: re-leveled 1 nested CONT/CONC lines"]);
    // Byte-identical but for the level digit: that is the whole repair.
    let expected = [
        HEAD,
        b"0 @I1@ INDI\n1 NOTE some text\n2 CONC continued\n2 CONT next paragraph\n2 CONT last line\n0 TRLR\n",
    ]
    .concat();
    assert_eq!(fixed, expected);
    assert!(!lint_bytes(&fixed).diags.iter().any(|d| d.code == "E005"));

    // Repairing twice changes nothing.
    let (again, applied_again) = fix_bytes(&fixed);
    assert_eq!(again, fixed);
    assert!(applied_again.is_empty(), "{:?}", applied_again);
}

#[test]
fn a_staircase_of_continuations_collapses_in_one_pass() {
    // The real-world shape (#51): each continuation one deeper than the
    // last. Every line re-levels against the same value line, so one
    // --fix flattens the run rather than one run per step.
    let data = [
        HEAD,
        b"0 @I1@ INDI\n1 NOTE some text\n2 CONC continued\n3 CONT one\n4 CONT two\n5 CONT three\n0 TRLR\n",
    ]
    .concat();
    let edits = compute_edits(&data);
    let e005: Vec<(usize, usize)> = edits
        .iter()
        .filter(|e| e.code == "E005")
        .map(|e| e.lines)
        .collect();
    assert_eq!(e005, vec![(7, 7), (8, 8), (9, 9)], "{:?}", edits);
    for e in edits.iter().filter(|e| e.code == "E005") {
        assert!(
            e.replacement[0].starts_with(b"2 CONT"),
            "{:?}",
            e.replacement
        );
    }

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(applied, vec!["E005: re-leveled 3 nested CONT/CONC lines"]);
    assert!(
        String::from_utf8_lossy(&fixed)
            .contains("2 CONC continued\n2 CONT one\n2 CONT two\n2 CONT three\n"),
        "{:?}",
        String::from_utf8_lossy(&fixed)
    );
    assert!(!lint_bytes(&fixed).diags.iter().any(|d| d.code == "E005"));

    // Repairing twice changes nothing.
    let (again, applied_again) = fix_bytes(&fixed);
    assert_eq!(again, fixed);
    assert!(applied_again.is_empty(), "{:?}", applied_again);
}

#[test]
fn a_level_zero_cont_is_reported_but_never_repaired() {
    // The other branch of E005: no parent at all. Nothing says which line
    // it was meant to continue, so --fix leaves it for the user (#51).
    let data = [HEAD, b"0 CONT orphan\n0 TRLR\n"].concat();
    assert!(lint_bytes(&data).diags.iter().any(|d| d.code == "E005"));
    assert!(
        compute_edits(&data).is_empty(),
        "{:?}",
        compute_edits(&data)
    );
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(fixed, data);
    assert!(applied.is_empty());
}

#[test]
fn a_continuation_chain_without_an_anchor_stays_put() {
    // The `0 CONT` has no parent (the unrepaired branch), so the `1 CONT`
    // under it has no non-CONT/CONC ancestor either: there is no value
    // line to re-anchor against, and inventing one would guess at the
    // data. Both stay reported, neither is repaired.
    let data = [HEAD, b"0 CONT top\n1 CONT under\n0 TRLR\n"].concat();
    let n = lint_bytes(&data)
        .diags
        .iter()
        .filter(|d| d.code == "E005")
        .count();
    assert_eq!(n, 2);
    assert!(
        compute_edits(&data).is_empty(),
        "{:?}",
        compute_edits(&data)
    );
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(fixed, data);
    assert!(applied.is_empty());
}

#[test]
fn an_orphan_below_a_staircase_repairs_fully_in_one_run() {
    // E001 runs before E005: the "5 CONT " prefix it writes lands under
    // the still-nested "4 CONT", and the E005 pass then re-levels both in
    // the same --fix. One run ends with every line a sibling.
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT a\n3 CONT b\n4 CONT c\norphan line\n0 TRLR\n",
    ]
    .concat();
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec![
            "E001: 1 orphan lines prefixed with CONT",
            "E005: re-leveled 2 nested CONT/CONC lines",
        ]
    );
    let text = String::from_utf8_lossy(&fixed);
    assert!(
        text.contains("2 TEXT a\n3 CONT b\n3 CONT c\n3 CONT orphan line\n"),
        "{}",
        text
    );
    let diags = lint_bytes(&fixed).diags;
    assert!(
        !diags.iter().any(|d| d.code == "E005" || d.code == "E001"),
        "{:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

#[test]
fn a_nested_cont_hanging_off_a_split_conc_flattens_before_the_rejoin() {
    // E101's rejoin absorbs the CONC line the CONT hangs from. Run first
    // it would strand the CONT at a level nothing supports any more, so
    // E005 runs before E101 (the ordering decision REPAIR_ORDER records).
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @S1@ SOUR\n1 DATA\n2 TEXT Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n3 CONC ");
    data.push(0xA9);
    data.extend_from_slice(b" x\n4 CONT more\n0 TRLR\n");

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec![
            "E005: re-leveled 1 nested CONT/CONC lines",
            "E101: rejoined 1 CONC lines with split UTF-8"
        ]
    );
    let text = String::from_utf8(fixed.clone()).unwrap();
    assert!(
        text.contains("2 TEXT Jos\u{e9} x\n3 CONT more\n"),
        "{}",
        text
    );
    let diags = lint_bytes(&fixed).diags;
    assert!(
        !diags.iter().any(|d| d.code == "E005" || d.code == "E101"),
        "{:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
}

#[test]
fn nested_edits_come_between_the_orphan_and_style_edits() {
    // Priority order, not line order: E001, then E005, then style.
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT a\n3 CONT b\n4 CONT c  \norphan line  \n0 TRLR\n",
    ]
    .concat();
    let codes: Vec<&str> = compute_edits(&data).iter().map(|e| e.code).collect();
    assert_eq!(codes, vec!["E001", "E005", "style", "style"]);
}

#[test]
fn crlf_nested_continuations_keep_their_cr() {
    // The rewrite must never touch line endings: a CRLF file stays CRLF.
    let data = b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @I1@ INDI\r\n1 NOTE a\r\n2 CONT b\r\n3 CONT c\r\n0 TRLR\r\n";
    let edits = compute_edits(data);
    assert_eq!(edits.len(), 1, "{:?}", edits);
    assert_eq!(edits[0].replacement, vec![b"2 CONT c\r".to_vec()]);
    let (fixed, _) = fix_bytes(data);
    assert_eq!(
        fixed,
        b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @I1@ INDI\r\n1 NOTE a\r\n2 CONT b\r\n2 CONT c\r\n0 TRLR\r\n"
    );
}

// ---------------------------------------------------------------------------
// Line-ending normalization: preprocessing, not an edit
// ---------------------------------------------------------------------------

#[test]
fn normalize_endings_is_not_a_rule() {
    let (out, changed) = normalize_endings(b"0 HEAD\r1 GEDC\r");
    assert_eq!(&*out, b"0 HEAD\n1 GEDC\n");
    assert!(changed);
    // CRLF is left alone and reported as unchanged.
    let (out, changed) = normalize_endings(b"0 HEAD\r\n1 GEDC\r\n");
    assert_eq!(&*out, b"0 HEAD\r\n1 GEDC\r\n");
    assert!(!changed);
    let (out, changed) = normalize_endings(b"0 HEAD\n");
    assert_eq!(&*out, b"0 HEAD\n");
    assert!(!changed);
    // No rule ever proposes it, so it cannot be selected away.
    let cr = b"0 HEAD\r1 GEDC\r2 VERS 5.5.1\r0 TRLR\r".to_vec();
    assert!(compute_edits(&cr)
        .iter()
        .all(|e| e.code != "style" || !e.replacement.is_empty()));
    let sel = FixSelection {
        only: vec!["E001".into()],
        allow_unsafe: false,
    };
    let (fixed, applied) = fix_bytes_with(&cr, &sel, &Config::default());
    assert!(
        !fixed.contains(&b'\r'),
        "normalization runs even under --only"
    );
    assert_eq!(
        applied,
        vec!["style: normalized classic Mac CR line endings to LF"]
    );
}

// ---------------------------------------------------------------------------
// fix_bytes: the report lines and the bytes are the CLI contract
// ---------------------------------------------------------------------------

#[test]
fn fix_bytes_report_lines_for_the_golden_set() {
    let expected: [(&str, &[&str]); 8] = [
        ("minimal70.ged", &[]),
        ("maximal70.ged", &[]),
        ("remarriage1.ged", &[]),
        ("remarriage2.ged", &[]),
        ("same-sex-marriage.ged", &[]),
        (
            "escapes.ged",
            &["style: trimmed trailing whitespace on 1 lines"],
        ),
        (
            "TGC551LF.ged",
            &["style: trimmed trailing whitespace on 352 lines"],
        ),
        (
            "TGC551.ged",
            &[
                "style: trimmed trailing whitespace on 352 lines",
                "style: normalized classic Mac CR line endings to LF",
            ],
        ),
    ];
    for (name, want) in &expected {
        let data = golden(name);
        let (fixed, applied) = fix_bytes(&data);
        assert_eq!(&applied, want, "{}", name);
        if want.is_empty() {
            assert_eq!(fixed, data, "{} must come back untouched", name);
        }
        // Repairing twice changes nothing more for these files.
        let (again, applied_again) = fix_bytes(&fixed);
        assert_eq!(again, fixed, "{} is not stable", name);
        assert!(applied_again.is_empty(), "{}: {:?}", name, applied_again);
    }
}

#[test]
fn fix_bytes_only_repairs_what_it_reports() {
    // Rejoining a CONC turns this blank line into a levelless one. Reporting
    // one orphan and prefixing two would be a repair the linter never asked
    // for, so a code gets exactly one pass and the next --fix picks up the
    // rest.
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @S1@ SOUR\n1 DATA\n2 TEXT a\n   \n3 CONC ");
    data.extend_from_slice(&[0xB1, b'x']);
    data.extend_from_slice(b"\n0 TRLR\n");

    let (fixed, applied) = fix_bytes(&data);
    // style runs first (#36), so the whitespace-only line is trimmed and
    // reported before the rejoin absorbs it.
    assert_eq!(
        applied,
        vec![
            "style: trimmed trailing whitespace on 1 lines",
            "E101: rejoined 1 CONC lines with split UTF-8"
        ]
    );
    let text = String::from_utf8_lossy(&fixed).into_owned();
    assert!(
        !text.contains("CONT"),
        "no CONT was reported, so none may appear: {}",
        text
    );
    // The exposed line is a genuine E001, and a second run repairs it.
    assert!(lint_bytes(&fixed).diags.iter().any(|d| d.code == "E001"));
    let (twice, applied_twice) = fix_bytes(&fixed);
    assert_eq!(
        applied_twice,
        vec!["E001: 1 orphan lines prefixed with CONT"]
    );
    assert!(String::from_utf8_lossy(&twice).contains("CONT"));
}

#[test]
fn fix_bytes_matches_a_hand_applied_selection() {
    // fix_bytes is a thin wrapper: driving compute_edits/apply_edits by hand
    // in the same order must give the same bytes.
    let data = [
        HEAD,
        b"0 @S1@ SOUR\n1 DATA\n2 TEXT part  \norphan line  \n0 TRLR\n",
    ]
    .concat();
    let mut cur = data.clone();
    for code in ["E001", "E005", "style", "E101"] {
        let chosen: Vec<Edit> = compute_edits(&cur)
            .into_iter()
            .filter(|e| e.code == code)
            .collect();
        if chosen.is_empty() {
            continue;
        }
        let (next, dropped) = apply_edits(&cur, &chosen);
        assert!(
            dropped.is_empty(),
            "same-code edits must not overlap: {:?}",
            dropped
        );
        cur = next;
    }
    assert_eq!(cur, fix_bytes(&data).0);
}

// ---------------------------------------------------------------------------
// Shapes the line model has to get right
// ---------------------------------------------------------------------------

#[test]
fn crlf_lines_keep_their_cr() {
    // The repairs work on bytes, and a CRLF file must not silently become a
    // mixed-ending one.
    let data = b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @S1@ SOUR\r\n1 DATA\r\n2 TEXT a  \r\norphan line  \r\n0 TRLR\r\n";
    let edits = compute_edits(data);
    assert_eq!(edits[0].code, "E001");
    assert_eq!(
        edits[0].replacement,
        vec![b"3 CONT orphan line  \r".to_vec()]
    );
    let (fixed, applied) = fix_bytes(data);
    assert_eq!(
        applied,
        vec![
            "E001: 1 orphan lines prefixed with CONT",
            "style: trimmed trailing whitespace on 2 lines"
        ]
    );
    assert_eq!(fixed, &b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @S1@ SOUR\r\n1 DATA\r\n2 TEXT a\r\n3 CONT orphan line\r\n0 TRLR\r\n"[..]);
}

#[test]
fn an_orphan_under_the_deepest_level_is_left_alone() {
    // Level 99 is the deepest legal one (5.5.1 ch. 1), so there is no level
    // to give the continuation: report it and do not invent a level 100.
    let data = [HEAD, b"0 @S1@ SOUR\n99 DATA\norphan under 99\n0 TRLR\n"].concat();
    assert!(
        compute_edits(&data).is_empty(),
        "{:?}",
        compute_edits(&data)
    );
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(fixed, data);
    assert!(applied.is_empty());
    assert!(
        lint_bytes(&data).diags.iter().any(|d| d.code == "E001"),
        "still reported, just not repaired"
    );
}

#[test]
fn a_conc_tag_without_a_space_is_still_rejoined() {
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\n2 CONC");
    data.extend_from_slice(&[0xA9, b' ', b'/', b'O', b's', b'o', b'/']);
    data.extend_from_slice(b"\n0 TRLR\n");
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec!["E101: rejoined 1 CONC lines with split UTF-8"]
    );
    assert!(String::from_utf8(fixed)
        .unwrap()
        .contains("1 NAME José /Oso/"));
}

#[test]
fn a_padded_anchor_is_trimmed_before_the_rejoin() {
    // MyHeritage pads lines with trailing spaces. Trimmed after the rejoin,
    // the pad lands between the two halves of the cut character and the
    // file is still not valid UTF-8 (#36): --fix reports success and E101
    // fires again. Trimming runs first, so the pad is gone before the
    // rejoin absorbs the line.
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @I1@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"  \n2 CONC ");
    data.push(0xA9);
    data.extend_from_slice(b" /Oso/\n0 TRLR\n");

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec![
            "style: trimmed trailing whitespace on 1 lines",
            "E101: rejoined 1 CONC lines with split UTF-8"
        ]
    );
    let text = String::from_utf8(fixed.clone()).unwrap();
    assert!(text.contains("1 NAME José /Oso/"), "{}", text);
    assert!(
        !lint_bytes(&fixed).diags.iter().any(|d| d.code == "E101"),
        "{}",
        text
    );

    // Repairing twice changes nothing.
    let (again, applied_again) = fix_bytes(&fixed);
    assert_eq!(again, fixed);
    assert!(applied_again.is_empty(), "{:?}", applied_again);
}

#[test]
fn a_crlf_rejoin_stays_crlf() {
    // The joined line must take the anchor's CR: a bare LF inside an
    // otherwise CRLF file is exactly the W102 the linter then reports (#36).
    let mut data = Vec::new();
    data.extend_from_slice(b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @I1@ INDI\r\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"\r\n2 CONC ");
    data.push(0xA9);
    data.extend_from_slice(b" /Oso/\r\n0 TRLR\r\n");

    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(
        applied,
        vec!["E101: rejoined 1 CONC lines with split UTF-8"]
    );
    let expected = {
        let mut e = b"0 HEAD\r\n1 GEDC\r\n2 VERS 5.5.1\r\n0 @I1@ INDI\r\n1 NAME Jos".to_vec();
        e.extend_from_slice(&[0xC3, 0xA9]);
        e.extend_from_slice(b" /Oso/\r\n0 TRLR\r\n");
        e
    };
    assert_eq!(fixed, expected);
    assert!(
        fixed.windows(2).all(|w| w[1] != b'\n' || w[0] == b'\r'),
        "every LF is preceded by a CR"
    );
    let diags = lint_bytes(&fixed).diags;
    assert!(
        !diags.iter().any(|d| d.code == "W102"),
        "{:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>()
    );
    assert!(!diags.iter().any(|d| d.code == "E101"));

    // Repairing twice changes nothing.
    let (again, applied_again) = fix_bytes(&fixed);
    assert_eq!(again, fixed);
    assert!(applied_again.is_empty(), "{:?}", applied_again);
}

#[test]
fn deleting_the_last_line_keeps_the_trailing_newline() {
    // "a\nb\n" is three lines, the third being the empty one the trailing
    // newline leaves behind; deleting it must not eat the newline.
    let (out, dropped) = apply_edits(b"a\nb\n", &[edit("T1", (3, 3), &[], Applicability::Safe)]);
    assert_eq!(out, b"a\nb\n");
    assert!(dropped.is_empty());
    let (out, _) = apply_edits(b"a\nb\n", &[edit("T1", (2, 3), &[], Applicability::Safe)]);
    assert_eq!(out, b"a\n");
}

#[test]
fn a_file_without_a_trailing_newline_does_not_gain_one() {
    let data = [HEAD, b"0 @S1@ SOUR\n1 DATA\n2 TEXT a\norphan no newline"].concat();
    let (fixed, applied) = fix_bytes(&data);
    assert_eq!(applied, vec!["E001: 1 orphan lines prefixed with CONT"]);
    assert!(
        fixed.ends_with(b"3 CONT orphan no newline"),
        "{:?}",
        String::from_utf8_lossy(&fixed)
    );
}

// ---------------------------------------------------------------------------
// The config gate (#44): diagnostics and edits must agree
// ---------------------------------------------------------------------------

/// One file carrying every repairable pattern, core and opt-in alike:
/// E001 (an orphan line), E005 (a CONT nested under a CONT), style
/// (trailing whitespace, twice), E101 (a UTF-8 character split across
/// CONC, in the #36 shape with a padded anchor), W601 (a comma surname in
/// both the NAME slot and a `2 SURN`), W702 (an all-caps `2 SURN`) and
/// W703 (doubled commas in a PLAC).
fn repairable_patterns() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(HEAD);
    data.extend_from_slice(b"0 @I1@ INDI\n1 NAME Maria /Montpeo, Osso/\n2 SURN Montpeo, Osso\n");
    data.extend_from_slice(
        b"0 @I2@ INDI\n1 NAME Anna /Puig/\n2 SURN PUIG SOLE\n1 BIRT\n2 PLAC Alcover, , Tarragona\n",
    );
    data.extend_from_slice(b"0 @S1@ SOUR\n1 DATA\n2 TEXT part  \norphan line\n");
    data.extend_from_slice(b"0 @I3@ INDI\n1 NAME Jos");
    data.push(0xC3);
    data.extend_from_slice(b"  \n2 CONC ");
    data.push(0xA9);
    data.extend_from_slice(b" /Oso/\n");
    data.extend_from_slice(b"0 @I4@ INDI\n1 NOTE some text\n2 CONT first\n3 CONT second\n0 TRLR\n");
    data
}

/// The test #44 is made of. The bug was a drift between two surfaces:
/// `apply_config` filtered the diagnostics and nothing filtered the edits,
/// so a file reported clean was rewritten anyway. What prevents a repeat is
/// not any single behaviour check but the missing link itself: for every
/// configuration, no edit may exist whose rule code produces no diagnostic
/// under that same configuration.
#[test]
fn diagnostics_and_edits_agree_under_every_config() {
    let data = repairable_patterns();
    let (norm, _) = normalize_endings(&data);

    let configs: Vec<(&str, Config)> = vec![
        ("the default config", Config::default()),
        (
            "every ruleset on",
            parse_config("[lints]\npresets = [\"recommended\", \"hispanic-naming\", \"hygiene\"]\n").unwrap(),
        ),
        (
            "a preset plus an explicit off",
            parse_config(
                "[lints]\npresets = [\"recommended\", \"hispanic-naming\", \"hygiene\"]\n\n[lints.rules]\n\"W601\" = \"off\"\n",
            )
            .unwrap(),
        ),
        ("total silence", parse_config("[lints]\npresets = []\n").unwrap()),
    ];

    for (name, cfg) in &configs {
        let report = lint_bytes_with(&norm, cfg);
        let edit_codes: BTreeSet<&str> = compute_edits_with(&norm, cfg)
            .iter()
            .map(|e| e.code)
            .collect();
        for code in &edit_codes {
            // "style" is the one repair with no diagnostic by design: the
            // linter has no code for trailing whitespace, and CR
            // normalization is preprocessing rather than a rule. Everything
            // else must be a rule the same configuration reports, or the
            // two surfaces have drifted apart again.
            if *code == "style" {
                continue;
            }
            assert!(
                report.diags.iter().any(|d| d.code == *code),
                "{name}: a {code} edit exists while the same config reports no {code} diagnostic"
            );
        }
    }

    // Teeth, so the loop above cannot pass vacuously on a fixture that
    // stopped triggering anything: every preset state produces the exact
    // edit set it should.
    let codes = |cfg: &Config| -> BTreeSet<&'static str> {
        compute_edits_with(&norm, cfg)
            .iter()
            .map(|e| e.code)
            .collect()
    };
    assert_eq!(
        codes(&configs[1].1),
        ["E001", "E005", "E101", "W601", "W702", "W703", "style"]
            .into_iter()
            .collect(),
        "every ruleset on must exercise every repair"
    );
    // An explicit "off" silences the edit even with the preset enabled.
    assert!(
        !codes(&configs[2].1).contains("W601"),
        "the override must beat the preset"
    );
    assert!(codes(&configs[2].1).contains("W702") && codes(&configs[2].1).contains("W703"));
    // The built-in config repairs exactly the core.
    assert_eq!(
        codes(&configs[0].1),
        ["E001", "E005", "E101", "style"].into_iter().collect()
    );
    // And silence leaves only the unruled style repair.
    assert_eq!(codes(&configs[3].1), ["style"].into_iter().collect());
}

#[test]
fn the_default_config_leaves_opt_in_patterns_untouched() {
    // #44 acceptance: a bare --fix repairs the core defects and reports
    // nothing about the W6xx/W7xx patterns, so it must not touch them.
    let data = repairable_patterns();
    let (fixed, applied) = fix_bytes(&data);
    let text = String::from_utf8_lossy(&fixed);
    assert!(
        text.contains("/Montpeo, Osso/"),
        "W601 not enabled: no comma removed"
    );
    assert!(
        text.contains("2 SURN PUIG SOLE"),
        "W702 not enabled: no case change"
    );
    assert!(
        text.contains("PLAC Alcover, , Tarragona"),
        "W703 not enabled: no comma removed"
    );
    assert!(
        applied.iter().any(|a| a.starts_with("E001")),
        "the core repairs still run: {:?}",
        applied
    );
    assert!(
        !applied
            .iter()
            .any(|a| a.starts_with("W6") || a.starts_with("W7")),
        "{:?}",
        applied
    );
}

#[test]
fn an_enabled_preset_repairs_its_ruleset_and_only_its_ruleset() {
    // presets = ["hispanic-naming"] without "recommended": the W601 repair
    // applies, and the core rules being off gates their repairs off too,
    // exactly as their diagnostics are silenced.
    let data = repairable_patterns();
    let cfg = parse_config("[lints]\npresets = [\"hispanic-naming\"]\n").unwrap();
    let (fixed, applied) = fix_bytes_with(&data, &FixSelection::default(), &cfg);
    assert_eq!(
        applied,
        vec![
            "style: trimmed trailing whitespace on 2 lines",
            "W601: removed comma from 2 surnames"
        ]
    );
    let text = String::from_utf8_lossy(&fixed);
    assert!(
        text.contains("/Montpeo Osso/") && text.contains("2 SURN Montpeo Osso"),
        "{}",
        text
    );
    assert!(
        text.contains("orphan line\n"),
        "E001 is off under this config, so no CONT prefix"
    );
    assert!(
        fixed.windows(4).any(|w| w == b"CONC"),
        "E101 is off under this config, so no rejoin"
    );
    assert!(
        text.contains("PLAC Alcover, , Tarragona"),
        "hygiene is off under this config"
    );
}

#[test]
fn only_narrows_the_gate_but_never_overrides_it() {
    // --only selects among the edits the configuration produced; it cannot
    // grant an edit the gate refused (#44 acceptance).
    let data = repairable_patterns();
    let sel = FixSelection {
        only: vec!["W601".into()],
        allow_unsafe: false,
    };
    let (out, applied) = fix_bytes_with(&data, &sel, &Config::default());
    assert!(applied.is_empty(), "{:?}", applied);
    assert_eq!(out, data, "the file must come back byte-identical");
}
