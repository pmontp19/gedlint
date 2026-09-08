//! End-to-end CLI tests: run the built binary on temp fixtures.
//! Covers src/main.rs (arg parsing, formats, exit codes, --fix/.bak).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gedlint"))
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gedlint-cli-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).unwrap();
    p
}

const CLEAN: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n0 @I1@ INDI\n1 NAME A /B/\n0 TRLR\n";
const BROKEN_REF: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n0 TRLR\n";
const WARN_ONLY: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n0 TRLR\n";

#[test]
fn clean_exit_0() {
    let d = tmpdir("clean");
    let f = write(&d, "c.ged", CLEAN.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stdout).contains("0 diagnostics"));
}

#[test]
fn errors_exit_2_and_text() {
    let d = tmpdir("err");
    let f = write(&d, "e.ged", BROKEN_REF.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stdout).contains("E201"));
}

#[test]
fn warnings_exit_1() {
    let d = tmpdir("warn");
    let f = write(&d, "w.ged", WARN_ONLY.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").output().unwrap();
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn json_format() {
    let d = tmpdir("json");
    let f = write(&d, "e.ged", BROKEN_REF.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--format").arg("json").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(s.contains("\"version\":\"5.5.1\""));
    assert!(s.contains("\"code\":\"E201\""));
    assert!(s.contains("\"summary\""));
}

#[test]
fn severity_filters_display_but_not_exit() {
    let d = tmpdir("sev");
    let f = write(&d, "w.ged", WARN_ONLY.as_bytes());
    let o = Command::new(bin())
        .arg(&f)
        .arg("--no-color")
        .arg("--severity")
        .arg("error")
        .output()
        .unwrap();
    // Exit still reflects the full report (warnings present).
    assert_eq!(o.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&o.stdout).contains("W305"));
}

#[test]
fn max_caps_text() {
    let d = tmpdir("max");
    let mut g = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n");
    for i in 1..=10 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n", i, i));
    }
    g.push_str("0 TRLR\n");
    let f = write(&d, "m.ged", g.as_bytes());
    // --verbose lists occurrences, so --max caps those (10 U502 -> 3 shown).
    let o = Command::new(bin())
        .arg(&f)
        .arg("--no-color")
        .arg("--verbose")
        .arg("--max")
        .arg("3")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(s.contains("(showing 3)"), "{}", &s[s.len().saturating_sub(300)..]);
}

#[test]
fn max_caps_groups() {
    // Grouped mode caps the rule lines, not the occurrences behind them.
    let d = tmpdir("maxg");
    let mut g = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n");
    for i in 1..=10 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n1 SEX Q\n1 PLAC Reus https://example.com/{}\n", i, i, i));
    }
    g.push_str("0 TRLR\n");
    let f = write(&d, "mg.ged", g.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").arg("--max").arg("2").output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    // Three codes (W305, W401, U502), capped at two group lines. The footer
    // stays a complete census: only the group lines are capped.
    assert!(s.contains("(showing 2 of 3 groups)"), "{}", &s[s.len().saturating_sub(400)..]);
    assert_eq!(s.matches("occurrences (--verbose to list all)").count(), 2, "{}", s);
    assert!(!s.contains("[U502:upgrade]"), "the third group line is not shown: {}", s);
    assert!(s.contains("Rules: W305 10, W401 10, U502 10"), "footer stays complete: {}", s);
}

#[test]
fn default_output_groups_by_rule() {
    // Issue 20: runs of one code collapse into one line, worst and most
    // frequent first, with a summary footer by category and by rule.
    let d = tmpdir("grouped");
    let mut g = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n");
    for i in 1..=5 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n", i, i));
    }
    g.push_str("0 @I9@ INDI\n1 NAME A /B/\n1 SEX Q\n1 FAMC @F9@\n0 TRLR\n");
    let f = write(&d, "g.ged", g.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    // E201 (error) first, then W305, then the 5-count U502 collapsed.
    let e201 = s.lines().find(|l| l.contains("E201")).unwrap();
    assert!(e201.starts_with("ERROR [E201:correctness] line 22:"), "{}", e201);
    let u502 = s.lines().find(|l| l.contains("U502")).unwrap();
    assert!(
        u502.starts_with("INFO  [U502:upgrade] vendor tag _UPD") && u502.contains(", 5 occurrences (--verbose to list all)"),
        "{}",
        u502
    );
    assert!(u502.find("U502").unwrap() < s.find("W305").unwrap(), "severity descending: {}", s);
    // Footer by category and by rule code.
    let cats = s.lines().find(|l| l.starts_with("Categories:")).unwrap();
    assert_eq!(cats, "Categories: correctness 1, suspicious 1, upgrade 5", "{}", cats);
    let rules = s.lines().find(|l| l.starts_with("Rules:")).unwrap();
    assert_eq!(rules, "Rules: E201 1, W305 1, U502 5", "{}", rules);
}

#[test]
fn verbose_reproduces_the_per_occurrence_output() {
    // --verbose must list every occurrence again, one line each, in the
    // classic format (issue 20 acceptance).
    let d = tmpdir("verbose");
    let mut g = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n");
    for i in 1..=3 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n", i, i));
    }
    g.push_str("0 TRLR\n");
    let f = write(&d, "v.ged", g.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--no-color").arg("--verbose").output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    let n = s.lines().filter(|l| l.contains("[U502:upgrade]")).count();
    assert_eq!(n, 3, "every occurrence listed: {}", s);
    for i in 1..=3 {
        let expect = format!("INFO  [U502:upgrade] line {}: vendor tag _UPD", 3 * i + 3);
        assert!(s.lines().any(|l| l.starts_with(&expect)), "missing '{}': {}", expect, s);
    }
    assert!(!s.contains("--verbose to list all"), "{}", s);
}

#[test]
fn quiet_summary() {
    let d = tmpdir("quiet");
    let f = write(&d, "c.ged", CLEAN.as_bytes());
    let o = Command::new(bin()).arg(&f).arg("--quiet").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(s.contains("lines") && s.contains("GEDCOM 5.5.1"));
}

#[test]
fn fix_writes_bak_and_repairs() {
    let d = tmpdir("fix");
    let orig = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT hi\norphan line\n0 TRLR\n".to_vec();
    let f = write(&d, "f.ged", &orig);
    let o = Command::new(bin()).arg(&f).arg("--fix").arg("--no-color").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("fix:"));
    let bak = PathBuf::from(format!("{}.bak", f.display()));
    assert!(bak.exists());
    assert_eq!(fs::read(&bak).unwrap(), orig);
    let fixed = fs::read(&f).unwrap();
    assert!(String::from_utf8_lossy(&fixed).contains("3 CONT orphan line"));
}

#[test]
fn unknown_option_exit_2() {
    let o = Command::new(bin()).arg("--bogus").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn help_exit_0() {
    let o = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stdout).contains("USAGE"));
}

#[test]
fn explain_lists_every_rule() {
    let o = Command::new(bin()).arg("--explain").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(s.starts_with("core ("), "the listing groups by ruleset: {}", &s[..s.len().min(60)]);
    for r in gedlint::RULES {
        assert!(s.contains(r.code) && s.contains(r.name), "{} missing from --explain", r.code);
    }
}

#[test]
fn explain_one_rule_by_code_or_by_name() {
    // The code is matched case-insensitively; the name needs its ruleset.
    for arg in ["w202", "core/asymmetric-famc-chil"] {
        let o = Command::new(bin()).arg("--explain").arg(arg).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "--explain {}", arg);
        let s = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(s.contains("W202") && s.contains("asymmetric-famc-chil"));
        assert!(s.contains("suspicious") && s.contains("warn") && s.contains("no automatic fix"));
        assert!(s.contains("WHY") && s.contains("REMEDY"));
    }
    // A fixable rule says so.
    let o = Command::new(bin()).arg("--explain").arg("E101").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("fixable by --fix"));
}

#[test]
fn explain_unknown_rule_exits_2() {
    for arg in ["NOPE", "core/nope", "nope/asymmetric-famc-chil"] {
        let o = Command::new(bin()).arg("--explain").arg(arg).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "--explain {}", arg);
        assert!(String::from_utf8_lossy(&o.stderr).contains("unknown rule"));
    }
}

#[test]
fn explain_rejects_an_option_in_place_of_a_code() {
    // Silently printing the whole listing would hide the typo.
    let o = Command::new(bin()).arg("--explain").arg("--fix").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("takes a rule code"));
    assert!(o.stdout.is_empty());
}

// ---------------------------------------------------------------------------
// Determinism (issue 30) and grouped output (issue 20).
// ---------------------------------------------------------------------------

/// Triggers W202 (both directions), W302 (two pairs), W303 and W304, with
/// several occurrences per rule so HashMap iteration order used to show.
fn graph_rules_fixture() -> String {
    let mut g = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n");
    // Two W302 pairs.
    for (i, (name, year)) in [(1, ("Joan /Oso/", 1861)), (2, ("Joan /Oso/", 1862)), (3, ("Anna /Puig/", 1900)), (4, ("Anna /Puig/", 1901))] {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME {}\n1 BIRT\n2 DATE {}\n", i, name, year));
    }
    // W303 (father 5, mother 65 at the birth) and W304 (born before MARR).
    g.push_str("0 @I5@ INDI\n1 NAME Father /Vell/\n1 BIRT\n2 DATE 1990\n");
    g.push_str("0 @I6@ INDI\n1 NAME Mother /Vell/\n1 BIRT\n2 DATE 1930\n");
    g.push_str("0 @I7@ INDI\n1 NAME Child /Vell/\n1 BIRT\n2 DATE 1995\n1 FAMC @F3@\n");
    g.push_str("0 @F3@ FAM\n1 HUSB @I5@\n1 WIFE @I6@\n1 CHIL @I7@\n1 MARR\n2 DATE 1999\n");
    // W202, child declares FAMC the FAM does not answer.
    g.push_str("0 @I8@ INDI\n1 NAME One /A/\n1 FAMC @F1@\n");
    g.push_str("0 @I9@ INDI\n1 NAME Two /A/\n1 FAMC @F2@\n");
    g.push_str("0 @F1@ FAM\n1 HUSB @I1@\n");
    g.push_str("0 @F2@ FAM\n1 HUSB @I2@\n");
    // W202, FAM lists CHIL that declares no FAMC.
    g.push_str("0 @F4@ FAM\n1 CHIL @I10@\n");
    g.push_str("0 @F5@ FAM\n1 CHIL @I11@\n");
    g.push_str("0 @I10@ INDI\n1 NAME Ten /B/\n");
    g.push_str("0 @I11@ INDI\n1 NAME Eleven /B/\n0 TRLR\n");
    g
}

#[test]
fn output_is_byte_identical_across_runs() {
    // Issue 30: W202/W302/W303/W304 were emitted in HashMap iteration order
    // (re-seeded per process), so the same binary could print them in a
    // different order run to run. Every run of both output modes must be
    // byte-identical.
    let d = tmpdir("det");
    let f = write(&d, "g.ged", graph_rules_fixture().as_bytes());
    for extra in [Vec::<&str>::new(), vec!["--verbose"]] {
        let mut runs: Vec<Vec<u8>> = Vec::new();
        for _ in 0..3 {
            let mut cmd = Command::new(bin());
            cmd.arg(&f).arg("--no-color");
            for a in &extra {
                cmd.arg(*a);
            }
            let o = cmd.output().unwrap();
            runs.push(o.stdout);
        }
        let head = String::from_utf8_lossy(&runs[0]).into_owned();
        assert!(
            ["W202", "W302", "W303", "W304"].iter().all(|c| head.contains(c)),
            "fixture must trigger all four rules ({:?}):\n{}",
            extra,
            head
        );
        assert_eq!(runs[0], runs[1], "run 2 differs ({:?})", extra);
        assert_eq!(runs[1], runs[2], "run 3 differs ({:?})", extra);
    }
}
