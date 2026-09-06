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
    let o = Command::new(bin()).arg(&f).arg("--no-color").arg("--max").arg("3").output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(s.contains("(showing 3)"), "{}", &s[s.len().saturating_sub(300)..]);
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
