//! End-to-end CLI tests for the structured-fix flags (issue 19).
//! `tests/cli.rs` covers the unchanged `--fix` contract and stays untouched.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_gedlint"))
}

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gedlint-clifix-{}-{}", std::process::id(), name));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// One orphan line plus trailing whitespace: two repairs, two codes.
const TWO_REPAIRS: &[u8] =
    b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT part   \norphan line\n0 TRLR\n";

fn write(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).unwrap();
    p
}

#[test]
fn only_restricts_the_repair_and_the_report() {
    let d = tmpdir("only");
    let f = write(&d, "o.ged", TWO_REPAIRS);
    let o = Command::new(bin()).arg(&f).arg("--fix").arg("--only").arg("E001").arg("--no-color").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("fix: E001: 1 orphan lines prefixed with CONT (backup"), "{}", out);
    assert!(!out.contains("trailing whitespace"), "{}", out);
    let fixed = String::from_utf8(fs::read(&f).unwrap()).unwrap();
    assert!(fixed.contains("3 CONT orphan line"), "{}", fixed);
    assert!(fixed.contains("2 TEXT part   \n"), "the unselected repair must not run: {:?}", fixed);
    assert!(fs::read(d.join("o.ged.bak")).unwrap() == TWO_REPAIRS, ".bak is still written");
}

#[test]
fn only_is_repeatable() {
    let d = tmpdir("only2");
    let f = write(&d, "o.ged", TWO_REPAIRS);
    let o = Command::new(bin())
        .arg(&f)
        .arg("--fix")
        .arg("--only")
        .arg("E001")
        .arg("--only")
        .arg("style")
        .arg("--no-color")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("E001: 1 orphan lines") && out.contains("trailing whitespace on 1 lines"), "{}", out);
    // Both codes selected is the same as a bare --fix here.
    let d2 = tmpdir("only2b");
    let f2 = write(&d2, "o.ged", TWO_REPAIRS);
    Command::new(bin()).arg(&f2).arg("--fix").arg("--no-color").output().unwrap();
    assert_eq!(fs::read(&f).unwrap(), fs::read(&f2).unwrap());
}

#[test]
fn only_without_a_code_exits_2() {
    let o = Command::new(bin()).arg("--fix").arg("--only").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--only needs a rule code"));
}

#[test]
fn only_and_unsafe_require_fix() {
    let d = tmpdir("nofix");
    let f = write(&d, "o.ged", TWO_REPAIRS);
    for flag in [vec!["--only", "E001"], vec!["--unsafe"]] {
        let o = Command::new(bin()).arg(&f).args(&flag).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{:?}", flag);
        assert!(String::from_utf8_lossy(&o.stderr).contains("only apply with --fix"), "{:?}", flag);
    }
    assert!(!d.join("o.ged.bak").exists(), "nothing may be written when the flags are rejected");
}

#[test]
fn unsafe_changes_nothing_while_every_repair_is_safe() {
    // No rule emits MaybeIncorrect yet (the opt-in rulesets do), so --unsafe
    // must be a no-op rather than a behaviour change.
    let d = tmpdir("unsafe");
    let a = write(&d, "a.ged", TWO_REPAIRS);
    let b = write(&d, "b.ged", TWO_REPAIRS);
    let plain = Command::new(bin()).arg(&a).arg("--fix").arg("--no-color").output().unwrap();
    let opted = Command::new(bin()).arg(&b).arg("--fix").arg("--unsafe").arg("--no-color").output().unwrap();
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let strip = |o: &std::process::Output, n: &str| String::from_utf8_lossy(&o.stdout).replace(n, "F");
    assert_eq!(strip(&plain, "a.ged"), strip(&opted, "b.ged"));
}

#[test]
fn help_lists_the_new_flags() {
    let o = Command::new(bin()).arg("--help").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("--only CODE"), "{}", out);
    assert!(out.contains("--unsafe"), "{}", out);
    // Each option is one line and every description starts in column 24: a
    // `\`-continuation in the format string silently eats the indentation of
    // the next line, which used to leave "JSON is always complete)" hanging
    // in the option column.
    let options: Vec<&str> = out.lines().filter(|l| l.starts_with("  -")).collect();
    assert!(options.len() >= 10, "{:?}", options);
    for l in options {
        assert!(l.len() > 24, "option line is too short to be aligned: {:?}", l);
        assert_eq!(&l[22..24], "  ", "description column is not 24: {:?}", l);
        assert!(!l[24..].starts_with(' '), "description does not start in column 24: {:?}", l);
    }
}

// ---------------------------------------------------------------------------
// The config gate, end to end (#44): --fix must pass the resolved config
// ---------------------------------------------------------------------------

/// W601 and W703 patterns with no core defect: under the built-in rules
/// this file is reported clean, so --fix has nothing to say about it.
const OPT_IN_ONLY: &[u8] =
    b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /Cognom1, Cognom2/\n1 BIRT\n2 PLAC Reus,, Spain\n0 TRLR\n";

#[test]
fn fix_without_a_preset_leaves_opt_in_patterns_alone() {
    // The bug itself: 0 diagnostics, then a rewrite. Now: no repair, no
    // report line, no .bak.
    let d = tmpdir("gate-default");
    let f = write(&d, "o.ged", OPT_IN_ONLY);
    let o = Command::new(bin()).arg(&f).arg("--fix").arg("--no-config").arg("--no-color").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(!out.contains("fix:"), "nothing was repaired: {}", out);
    assert_eq!(fs::read(&f).unwrap(), OPT_IN_ONLY, "the file must come back untouched");
    assert!(!d.join("o.ged.bak").exists(), "no rewrite means no backup either");
}

#[test]
fn fix_applies_opt_in_repairs_only_under_their_preset() {
    let d = tmpdir("gate-preset");
    let f = write(&d, "o.ged", OPT_IN_ONLY);
    write(&d, "gedlint.toml", b"[lints]\npresets = [\"recommended\", \"hispanic-naming\", \"hygiene\"]\n");
    let o = Command::new(bin()).arg(&f).arg("--fix").arg("--no-color").output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(out.contains("W601") && out.contains("W703"), "{}", out);
    let fixed = String::from_utf8(fs::read(&f).unwrap()).unwrap();
    assert!(fixed.contains("/Cognom1 Cognom2/"), "{}", fixed);
    assert!(fixed.contains("PLAC Reus, Spain"), "{}", fixed);
    assert_eq!(fs::read(d.join("o.ged.bak")).unwrap(), OPT_IN_ONLY, ".bak keeps the original");
}

#[test]
fn only_cannot_override_the_config_gate() {
    // --only narrows what the config allowed; it must not widen it.
    let d = tmpdir("gate-only");
    let f = write(&d, "o.ged", OPT_IN_ONLY);
    let o = Command::new(bin())
        .arg(&f)
        .arg("--fix")
        .arg("--only")
        .arg("W601")
        .arg("--no-config")
        .arg("--no-color")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(!out.contains("fix:"), "{}", out);
    assert_eq!(fs::read(&f).unwrap(), OPT_IN_ONLY);
    assert!(!d.join("o.ged.bak").exists());
}
