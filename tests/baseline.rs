//! Baseline ratchet (RFC 014 section 5, issue 22): end-to-end CLI tests and
//! engine-level matching tests. `--write-baseline` records a run;
//! `--baseline` fails only on NEW findings, never on line-number shifts,
//! and never lets a baselined finding move the exit code.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use gedlint::{
    apply_baseline, baseline_from_report, baseline_to_json, fingerprint, parse_baseline, Baseline,
    BaselineEntry, Category, Diag, Report, Severity, Version,
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gedlint"))
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gedlint-baseline-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).unwrap();
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(bin()).args(args).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// One broken FAMC ref (E201, error), one invalid SEX (W305, warning) and
/// one vendor tag (U502, info): the day-one CI failure the ratchet exists
/// for.
const MESSY: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Anna /B/
1 SEX Q
1 FAMC @F9@
1 _UPD 2020
0 @I2@ INDI
1 NAME Anna /B/
0 TRLR
";

const CLEAN: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Anna /B/
0 @I2@ INDI
1 NAME Anna /B/
0 TRLR
";

/// A tree whose findings quote it back: every rule here puts a value from
/// the file into its message (W702 the surname, W403 the note body, W306
/// the enum value, W401 the place). Deliberately invented strings, chosen
/// so a leak is unmistakable and so none of them is hex.
const TALKATIVE: &str = "\
0 HEAD
1 GEDC
2 VERS 5.5.1
1 CHAR UTF-8
0 @I1@ INDI
1 NAME Quimeta /VILAGRASSA/
1 RESN reservatissim
1 NOTE <br>Quimeta remembered by Sinforosa
1 BIRT
2 PLAC Torderola http://example.invalid/tree
0 @I2@ INDI
1 NAME Nicasi /PUIGMARTORELL/
0 TRLR
";

/// W702 lives in the opt-in "hygiene" ruleset, so TALKATIVE needs it on.
const TALKATIVE_CFG: &str = "[lints]\npresets = [\"recommended\", \"hygiene\"]\n";

/// Every string of TALKATIVE a diagnostic message quotes.
const SECRETS: &[&str] = &[
    "quimeta",
    "vilagrassa",
    "reservatissim",
    "sinforosa",
    "torderola",
    "example.invalid",
    "puigmartorell",
];

/// The shape `fingerprint` writes: "h1:" and 16 lowercase hex digits.
fn is_digest(fp: &str) -> bool {
    fp.len() == 19
        && fp.starts_with("h1:")
        && fp[3..].chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

fn messy_plus(extra: &str) -> String {
    let mut g = MESSY.to_string();
    let cut = g.find("0 TRLR").unwrap();
    g.insert_str(cut, extra);
    g
}

#[test]
fn write_baseline_exits_0_and_records_every_finding() {
    let d = tmpdir("write");
    let f = write(&d, "t.ged", MESSY);
    let base = d.join("g.baseline.json");
    let (code, out, _) = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(
        code, 0,
        "recording a baseline must not fail CI bootstrap: {}",
        out
    );
    assert!(out.contains("baseline:"), "{}", out);
    let b = gedlint::parse_baseline(&fs::read_to_string(&base).unwrap()).unwrap();
    assert_eq!(b.entries.len(), 3, "{:?}", b.entries);
    for code in ["E201", "W305", "U502"] {
        assert!(
            b.entries.iter().any(|e| e.code == code),
            "{} missing: {:?}",
            code,
            b.entries
        );
    }
    // Entries carry counts and digests: no line numbers, no message text.
    assert!(
        b.entries
            .iter()
            .all(|e| e.count == 1 && is_digest(&e.fingerprint)),
        "{:?}",
        b.entries
    );
}

#[test]
fn baseline_makes_the_recorded_tree_pass() {
    let d = tmpdir("pass");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    let (code, out, _) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 0, "baselined findings must not fail: {}", out);
    assert!(out.contains("0 new diagnostics"), "{}", out);
    assert!(out.contains("3 baselined"), "{}", out);
    // Suppressed findings are not listed in text mode (the full report is
    // still available via --format json).
}

#[test]
fn new_finding_of_a_new_shape_still_fails() {
    let d = tmpdir("new");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    // A new broken pointer with a different tag: a fingerprint the baseline
    // has no entry for, even though E201 itself is baselined.
    let f2 = write(
        &d,
        "t2.ged",
        &messy_plus("0 @I9@ INDI\n1 NAME Nou /P/\n1 FAMS @F7@\n"),
    );
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 2, "a new error must fail: {}", out);
    assert!(out.contains("1 new diagnostics"), "{}", out);
    assert!(out.contains("3 baselined"), "{}", out);
    assert!(
        out.contains("FAMS @F7@") || out.contains("E201"),
        "the new finding is listed: {}",
        out
    );
}

#[test]
fn count_exhaustion_reports_surplus() {
    let d = tmpdir("count");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    // A second identical broken FAMC @F9@: same fingerprint, count already
    // exhausted, so the surplus is new. This is count-based matching, not
    // code-based muting.
    let f2 = write(
        &d,
        "t2.ged",
        &messy_plus("0 @I8@ INDI\n1 NAME Nou /P/\n1 FAMC @F9@\n"),
    );
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 2, "surplus findings must fail: {}", out);
    assert!(out.contains("1 new diagnostics"), "{}", out);
    assert!(out.contains("3 baselined"), "{}", out);
}

#[test]
fn inserting_10_lines_does_not_invalidate_the_baseline() {
    let d = tmpdir("shift");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);

    // HEAD must stay the first non-blank line (E002), so the 10 inserted
    // lines go directly after the HEAD block: every finding's line number
    // shifts by 10 and the keying must not care.
    let mut shifted = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n");
    for i in 1..=10 {
        shifted.push_str(&format!("0 NOTE filler {i}\n"));
    }
    shifted.push_str(&MESSY["0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n".len()..]);
    let f2 = write(&d, "t2.ged", &shifted);

    // Sanity: without the baseline the shifted tree still fails.
    let (code, _, _) = run(&[f2.to_str().unwrap(), "--no-color"]);
    assert_eq!(code, 2);
    // With the baseline it passes.
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(
        code, 0,
        "a line insertion must not invalidate the baseline: {}",
        out
    );
    assert!(out.contains("0 new diagnostics"), "{}", out);
    assert!(out.contains("3 baselined"), "{}", out);
}

// ---------------------------------------------------------------------------
// Issue 61: a baseline is a file users are told to commit, so it must carry
// no text from the tree it was generated on.
// ---------------------------------------------------------------------------

/// Acceptance: no substring of a name, note body or place survives into the
/// file, and the run that produced it really did quote them.
#[test]
fn baseline_carries_no_text_from_the_tree() {
    let d = tmpdir("privacy");
    let f = write(&d, "t.ged", TALKATIVE);
    let cfg = write(&d, "gedlint.toml", TALKATIVE_CFG);
    let cfg = cfg.to_str().unwrap().to_string();
    let base = d.join("g.baseline.json");

    // The messages quote the file: without that this test proves nothing.
    let (_, report, _) = run(&[
        f.to_str().unwrap(),
        "--config",
        &cfg,
        "--verbose",
        "--no-color",
    ]);
    let report = report.to_lowercase();
    for secret in SECRETS {
        assert!(
            report.contains(secret),
            "the fixture must make a rule quote {}: {}",
            secret,
            report
        );
    }

    let (code, _, _) = run(&[
        f.to_str().unwrap(),
        "--config",
        &cfg,
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    let text = fs::read_to_string(&base).unwrap();
    let lower = text.to_lowercase();
    for secret in SECRETS {
        assert!(
            !lower.contains(secret),
            "the baseline leaked {}:\n{}",
            secret,
            text
        );
    }
    // And not just those: every fingerprint is a digest, so there is no
    // room in the file for text of any kind.
    let b = gedlint::parse_baseline(&text).unwrap();
    assert!(!b.entries.is_empty());
    assert!(
        b.entries.iter().all(|e| is_digest(&e.fingerprint)),
        "{:?}",
        b.entries
    );
}

/// Acceptance: digests do not break the ratchet. Round trip on an unchanged
/// file reports zero new findings.
#[test]
fn digest_baseline_round_trips() {
    let d = tmpdir("privacy-match");
    let f = write(&d, "t.ged", TALKATIVE);
    let cfg = write(&d, "gedlint.toml", TALKATIVE_CFG);
    let cfg = cfg.to_str().unwrap().to_string();
    let base = d.join("g.baseline.json");
    let _ = run(&[
        f.to_str().unwrap(),
        "--config",
        &cfg,
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    let (code, out, _) = run(&[
        f.to_str().unwrap(),
        "--config",
        &cfg,
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("0 new diagnostics"), "{}", out);
    assert!(out.contains("0 resolved"), "{}", out);
}

/// Acceptance: two findings of one rule that differ only in the text they
/// quote stay two entries, so counts cannot collapse into one.
#[test]
fn distinct_findings_of_one_rule_keep_distinct_entries() {
    let d = tmpdir("privacy-distinct");
    let f = write(&d, "t.ged", TALKATIVE);
    let cfg = write(&d, "gedlint.toml", TALKATIVE_CFG);
    let cfg = cfg.to_str().unwrap().to_string();
    let base = d.join("g.baseline.json");
    let _ = run(&[
        f.to_str().unwrap(),
        "--config",
        &cfg,
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    let b = gedlint::parse_baseline(&fs::read_to_string(&base).unwrap()).unwrap();
    let caps: Vec<&gedlint::BaselineEntry> =
        b.entries.iter().filter(|e| e.code == "W702").collect();
    assert_eq!(
        caps.len(),
        2,
        "two all-caps surnames are two shapes: {:?}",
        b.entries
    );
    assert!(caps.iter().all(|e| e.count == 1), "{:?}", caps);
    assert_ne!(caps[0].fingerprint, caps[1].fingerprint);

    // Fixing one of them leaves the other baselined, which is the whole
    // point of keeping them apart.
    let f2 = write(
        &d,
        "t2.ged",
        &TALKATIVE.replace("/PUIGMARTORELL/", "/Puigmartorell/"),
    );
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--config",
        &cfg,
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("0 new diagnostics"), "{}", out);
    assert!(out.contains("1 resolved"), "{}", out);
}

/// Acceptance: a baseline in the old readable format fails loudly, naming
/// the way out, rather than quietly matching nothing.
#[test]
fn a_version_1_baseline_fails_loudly() {
    let d = tmpdir("privacy-old");
    let f = write(&d, "t.ged", MESSY);
    let base = write(
        &d,
        "old.baseline.json",
        "{\"gedlint-baseline\": 1,\n  \"entries\": [\n    {\"code\": \"W305\", \
\"fingerprint\": \"@i#@: invalid sex value q\", \"count\": 1}\n  ]\n}\n",
    );
    let (code, _, err) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 2, "an unreadable baseline must not pass: {}", err);
    assert!(err.contains("unsupported baseline version 1"), "{}", err);
    assert!(
        err.contains("regenerate it with --write-baseline"),
        "the error must say what to do: {}",
        err
    );
}

/// The resolved appendix names the rule, since the digest alone says
/// nothing to a reader.
#[test]
fn resolved_appendix_names_the_rule() {
    let d = tmpdir("privacy-resolved");
    let f = write(&d, "t.ged", MESSY);
    let base = d.join("g.baseline.json");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    let f2 = write(&d, "t2.ged", CLEAN);
    let (_, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    let name = gedlint::rule("W305").unwrap().name;
    assert!(out.contains(name), "{} missing from: {}", name, out);
    assert!(
        out.contains("h1:"),
        "the digest identifies the entry: {}",
        out
    );
}

#[test]
fn resolved_findings_are_reported_and_pruned() {
    let d = tmpdir("resolved");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);

    // The user fixes all three findings: the baseline entries are resolved,
    // the run passes, and a rewrite prunes them (the ratchet).
    let f2 = write(&d, "t2.ged", CLEAN);
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("resolved"), "{}", out);
    for code in ["E201", "W305", "U502"] {
        assert!(
            out.contains(code),
            "{} should be listed as resolved: {}",
            code,
            out
        );
    }
    assert!(out.contains("3 resolved"), "{}", out);

    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{}", out);
    let b = gedlint::parse_baseline(&fs::read_to_string(&base).unwrap()).unwrap();
    assert!(
        b.entries.is_empty(),
        "the rewrite must prune resolved entries: {:?}",
        b.entries
    );
    assert!(out.contains("0 entries"), "{}", out);

    // And the pruned baseline still passes the clean tree.
    let (code, out, _) = run(&[
        f2.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("0 baselined, 0 resolved"), "{}", out);
}

#[test]
fn missing_baseline_file_is_a_clear_error() {
    let d = tmpdir("missing");
    let f = write(&d, "t.ged", MESSY);
    let gone = d.join("nope.json");
    let (code, out, err) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        gone.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 2, "out: {} err: {}", out, err);
    assert!(
        err.contains("cannot read baseline") && err.contains(gone.to_str().unwrap()),
        "{}",
        err
    );
}

#[test]
fn corrupt_baseline_file_is_a_clear_error() {
    let d = tmpdir("corrupt");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "this is not json");
    let (code, out, err) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--no-color",
    ]);
    assert_eq!(code, 2, "out: {} err: {}", out, err);
    assert!(
        err.contains("baseline file:") && err.contains("expected"),
        "{}",
        err
    );
}

#[test]
fn baseline_flags_are_mutually_exclusive() {
    let d = tmpdir("exclusive");
    let f = write(&d, "t.ged", MESSY);
    let (code, _, err) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        d.join("a.json").to_str().unwrap(),
        "--write-baseline",
        d.join("b.json").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(err.contains("mutually exclusive"), "{}", err);
}

#[test]
fn json_output_stays_full_and_exit_follows_the_ratchet() {
    let d = tmpdir("json");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    // JSON stays the complete report (gh-report.js contract), but the exit
    // code counts only findings the baseline could not absorb.
    let (code, out, _) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(code, 0, "{}", out);
    assert!(out.contains("\"code\":\"E201\""), "{}", out);
    assert!(out.contains("\"summary\""), "{}", out);
}

#[test]
fn quiet_reports_the_ratchet_counts() {
    let d = tmpdir("quiet");
    let f = write(&d, "t.ged", MESSY);
    let base = write(&d, "g.baseline.json", "");
    let _ = run(&[
        f.to_str().unwrap(),
        "--write-baseline",
        base.to_str().unwrap(),
    ]);
    let (code, out, _) = run(&[
        f.to_str().unwrap(),
        "--baseline",
        base.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("3 baselined, 0 resolved"), "{}", out);
}

// ---------------------------------------------------------------------------
// Engine-level matching tests. The synthetic diagnostics are built here, in
// an integration test, on purpose: tests/registry.rs scans src/ for
// diagnostic constructor call sites so that every engine emission site
// carries a literal rule code. src/baseline.rs is not an emission site (it
// clones diagnostics out of a Report and models resolved entries as
// BaselineEntry, never as Diag), so its fixtures live outside that scan.
// ---------------------------------------------------------------------------

fn diag(code: &'static str, line: usize, msg: &str) -> Diag {
    Diag {
        code,
        category: Category::Correctness,
        ruleset: "core",
        severity: Severity::Error,
        line,
        col: 0,
        len: 0,
        msg: msg.to_string(),
    }
}

fn report(diags: Vec<Diag>) -> Report {
    Report {
        version: Version::V551,
        diags,
        lines: 0,
        individuals: 0,
        families: 0,
    }
}

#[test]
fn counts_absorb_and_surplus_is_new() {
    let r = report(vec![
        diag("E201", 3, "FAMC @F9@ points nowhere"),
        diag("E201", 7, "FAMC @F9@ points nowhere"),
        diag("E201", 9, "FAMC @F8@ points nowhere"),
    ]);
    let b = Baseline {
        entries: vec![BaselineEntry {
            code: "E201".to_string(),
            fingerprint: fingerprint("FAMC @F9@ points nowhere"),
            count: 2,
        }],
    };
    let o = apply_baseline(&r, &b);
    assert_eq!(o.baselined, 2);
    assert_eq!(o.new_diags.len(), 1);
    assert_eq!(o.new_diags[0].line, 9);
    assert!(o.resolved.is_empty());
    // The per-diagnostic classification mirrors the counts: the first two
    // absorbed, the surplus new.
    assert_eq!(o.known_flags, vec![true, true, false]);
}

#[test]
fn shifted_lines_match_and_resolved_are_reported() {
    let r = report(vec![diag("W305", 13, "SEX value Q is not a valid enum")]);
    let b = baseline_from_report(&report(vec![
        diag("W305", 3, "SEX value Q is not a valid enum"),
        diag("E201", 4, "FAMC @F9@ points nowhere"),
    ]));
    let o = apply_baseline(&r, &b);
    assert_eq!(o.baselined, 1);
    assert!(o.new_diags.is_empty());
    assert_eq!(o.resolved.len(), 1);
    assert_eq!(o.resolved[0].code, "E201");
}

#[test]
fn unknown_code_is_new() {
    let r = report(vec![diag("W999", 1, "brand new rule")]);
    let b = baseline_from_report(&report(vec![diag("E201", 2, "FAMC @F9@ points nowhere")]));
    let o = apply_baseline(&r, &b);
    assert_eq!(o.new_diags.len(), 1);
    assert_eq!(o.baselined, 0);
    assert_eq!(o.resolved.len(), 1);
}

#[test]
fn from_report_sorts_and_absorbs_shifted_line_references() {
    // A report shaped like the motivating case: many findings of one code,
    // and a message that embeds line references. Entries come out sorted
    // by (code, fingerprint), and a line shift does not break the match.
    let mut diags = Vec::new();
    for i in 0..516 {
        diags.push(diag(
            "U502",
            i * 3,
            "_UPD is not part of GEDCOM 5.5.1 or 7.0",
        ));
    }
    for i in 0..171 {
        diags.push(diag(
            "E001",
            i * 7 + 1,
            "malformed line (non-numeric level): orphan",
        ));
    }
    diags.push(diag("E003", 42, "duplicate xref @F1@ (first at line 30)"));
    let r = report(diags);
    let b = baseline_from_report(&r);
    assert_eq!(b.entries.len(), 3);
    let codes: Vec<&str> = b.entries.iter().map(|e| e.code.as_str()).collect();
    assert_eq!(
        codes,
        ["E001", "E003", "U502"],
        "entries must be sorted by code"
    );

    let parsed = parse_baseline(&baseline_to_json(&b)).unwrap();
    assert_eq!(parsed, b, "JSON round trip must preserve the entries");
    let o = apply_baseline(&r, &parsed);
    assert_eq!(o.baselined, r.diags.len());
    assert!(o.new_diags.is_empty());
    assert!(o.resolved.is_empty());

    // Inserting a line shifts the E003 message's embedded line numbers,
    // but the digit-collapsed fingerprint still matches.
    let shifted = report(vec![diag(
        "E003",
        52,
        "duplicate xref @F1@ (first at line 40)",
    )]);
    let o = apply_baseline(&shifted, &b);
    assert_eq!(o.baselined, 1);
    assert!(o.new_diags.is_empty());
    assert_eq!(o.known_flags, vec![true]);
}
