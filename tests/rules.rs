//! One rule per test with minimal fixtures (acceptance criterion 5).
//! Each test builds the smallest GEDCOM that triggers a single rule.

use gedlint::{Severity, Version, fix_bytes, lint_bytes, lint_str};

fn codes(input: &str) -> Vec<String> {
    lint_str(input).diags.iter().map(|d| d.code.to_string()).collect()
}

fn has(input: &str, code: &str) -> bool {
    codes(input).iter().any(|c| c == code)
}

const HEAD551: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n";
const HEAD70: &str = "0 HEAD\n1 GEDC\n2 VERS 7.0\n";

fn wrap551(body: &str) -> String {
    format!("{}0 TRLR\n", format_args!("{HEAD551}{body}"))
}

#[test]
fn e002_missing_head_trlr() {
    let r = lint_str("0 @I1@ INDI\n1 NAME Joan /Osó/\n");
    assert!(r.diags.iter().any(|d| d.code == "E002"));
    assert_eq!(r.exit_code(), 2);
}

#[test]
fn e001_level_jump() {
    let g = wrap551("0 @I1@ INDI\n3 NAME Joan /Osó/\n");
    assert!(has(&g, "E001"));
}

#[test]
fn e003_dup_xref() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n0 @I1@ INDI\n1 NAME C /D/\n");
    assert!(has(&g, "E003"));
}

#[test]
fn e201_broken_fam_ref() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn e201_broken_chil_ref() {
    let g = wrap551("0 @F1@ FAM\n1 CHIL @I9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn w202_famc_not_listed() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n0 @F1@ FAM\n1 HUSB @I2@\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(has(&g, "W202"));
}

#[test]
fn w202_chil_not_declaring_famc() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(has(&g, "W202"));
}

#[test]
fn w301_death_before_birth() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 12 SEP 1909\n1 DEAT\n2 DATE 3 JAN 1900\n");
    assert!(has(&g, "W301"));
}

#[test]
fn w301_longevity_112() {
    // The 112-year entry from the real corpus: must trigger W301.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1 JAN 1800\n1 DEAT\n2 DATE 1 JAN 1912\n");
    assert!(has(&g, "W301"));
}

#[test]
fn w302_duplicates() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Osó/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    // "Osó" vs "Oso" does not normalize accents: use the exact same name.
    let g2 = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    assert!(!has(&g, "W302") || has(&g2, "W302"));
    assert!(has(&g2, "W302"));
}

#[test]
fn w303_parent_age() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1900\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 1905\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(has(&g, "W303"));
}

#[test]
fn w304_child_before_marriage() {
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME C /D/\n1 BIRT\n2 DATE 1902\n\
         0 @I3@ INDI\n1 NAME E /F/\n1 BIRT\n2 DATE 1910\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n1 MARR\n2 DATE 1920\n",
    );
    assert!(has(&g, "W304"));
}

#[test]
fn w305_sex_invalid() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    assert!(has(&g, "W305"));
}

#[test]
fn w401_plac_url() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC Reus https://example.com/x\n");
    assert!(has(&g, "W401"));
}

#[test]
fn w401_plac_url_multibyte_no_panic() {
    // truncate() used to slice at byte 60, panicking on multibyte chars.
    let pad = "x".repeat(59) + "é";
    let g = wrap551(&format!(
        "0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 PLAC {pad} https://example.com/{}\n",
        "y".repeat(100)
    ));
    let r = lint_str(&g);
    assert!(r.diags.iter().any(|d| d.code == "W401"));
    assert_eq!(r.exit_code(), 1);
}

#[test]
fn w402_name_slashes() {
    let g = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n");
    assert!(has(&g, "W402"));
}

#[test]
fn e101_conc_split_fix() {
    // Build the MyHeritage bug at byte level: "é" (U+00E9 = C3 A9)
    // split across two CONC lines: first ends with C3, next starts with A9.
    let mut data = Vec::new();
    data.extend_from_slice("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Jos".as_bytes());
    data.push(0xC3);
    data.extend_from_slice("\n2 CONC ".as_bytes());
    data.push(0xA9);
    data.extend_from_slice(" /Oso/\n0 TRLR\n".as_bytes());
    let r = lint_bytes(&data);
    assert!(r.diags.iter().any(|d| d.code == "E101"), "cal E101, trobat: {:?}", r.diags);
    let (fixed, applied) = fix_bytes(&data);
    assert!(!applied.is_empty());
    let r2 = lint_bytes(&fixed);
    assert!(
        !r2.diags.iter().any(|d| d.code == "E101"),
        "E101 must be gone after --fix: {:?}",
        r2.diags
    );
    assert!(String::from_utf8(fixed).is_ok());
}

#[test]
fn fix_trims_trailing_ws() {
    let data = b"0 HEAD   \n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    assert!(!applied.is_empty());
    assert!(String::from_utf8_lossy(&fixed).starts_with("0 HEAD\n"));
}

#[test]
fn e001_orphan_cont_fix() {
    // MyHeritage continuation without CONT prefix (real cleaned-tree case).
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT <p>origen\ncontinuacio sense prefix\n0 TRLR\n"
        .to_vec();
    let r = lint_bytes(&data);
    assert!(r.diags.iter().any(|d| d.code == "E001"));
    let (fixed, applied) = fix_bytes(&data);
    assert!(applied.iter().any(|a| a.contains("CONT")));
    let r2 = lint_bytes(&fixed);
    assert!(!r2.diags.iter().any(|d| d.code == "E001" && d.line == 7), "orphan line 7 repaired: {:?}", r2.diags);
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT continuacio sense prefix"));
}

#[test]
fn no_global_cap_hides_errors() {
    // 516 real _UPD infos used to hide errors behind the 200 global cap: now everything is collected.
    let mut g = String::from(HEAD551);
    for i in 1..=300 {
        g.push_str(&format!("0 @I{}@ INDI\n1 NAME A{} /B/\n1 _UPD X\n", i, i));
    }
    g.push_str("0 @F9@ FAM\n1 CHIL @I999@\n0 TRLR\n");
    let r = lint_str(&g);
    assert!(r.diags.iter().any(|d| d.code == "E201"), "E201 must not stay hidden: {:?}", r.diags.len());
    assert!(r.infos() >= 300);
}

#[test]
fn bom_does_not_hide_head_or_version() {
    // The real MyHeritage export starts with a BOM: HEAD and VERS must still be detected.
    let g = "\u{FEFF}0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    let r = lint_str(g);
    assert_eq!(r.version, Version::V551);
    assert!(!r.diags.iter().any(|d| d.code == "E002"));
    let rb = lint_bytes(g.as_bytes());
    assert!(rb.diags.iter().any(|d| d.code == "W102"));
}

#[test]
fn deat_y_without_date_is_not_longevity() {
    // DEAT Y without DATE = death with unknown date, not 8097 years.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1902\n1 DEAT Y\n");
    assert!(!has(&g, "W301"));
}

#[test]
fn w302_no_false_positive_distant_births() {
    // Same name but 10 years apart: must NOT be a duplicate (kills ±200 mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1871\n",
    );
    assert!(!has(&g, "W302"));
}

#[test]
fn w302_case_insensitive_duplicates() {
    // Same name in different case: still a duplicate (kills no-lowercase mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME JOAN /OSO/\n1 BIRT\n2 DATE 1861\n0 @I2@ INDI\n1 NAME Joan /Oso/\n1 BIRT\n2 DATE 1862\n",
    );
    assert!(has(&g, "W302"));
}

#[test]
fn w303_old_mother_60() {
    // Mother aged 60 (>50) fires; father aged 60 (<=70) does not matter here.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1870\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1870\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 1930\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(has(&g, "W303"));
}

#[test]
fn date_year_3000_ignored() {
    // Year 3000 is outside 100..=2100: no birth year, no W303 (kills 9999 mutant).
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Pare /X/\n1 BIRT\n2 DATE 1900\n\
         0 @I2@ INDI\n1 NAME Mare /Y/\n1 BIRT\n2 DATE 1900\n\
         0 @I3@ INDI\n1 NAME Fill /Z/\n1 BIRT\n2 DATE 3000\n1 FAMC @F1@\n\
         0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n",
    );
    assert!(!has(&g, "W303"));
}

#[test]
fn e004_malformed_xref() {
    // Unclosed xref is malformed.
    let g = wrap551("0 @I1 INDI\n1 NAME A /B/\n");
    assert!(has(&g, "E004"));
}

#[test]
fn e005_orphan_cont() {
    // CONT as the first line has no parent.
    assert!(has("2 CONT orphan\n", "E005"));
}

#[test]
fn u502_vendor_tag() {
    // MyHeritage _UPD is a vendor tag: upgrade info, never an error.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 _UPD 20240101\n");
    let r = lint_str(&g);
    assert!(r.diags.iter().any(|d| d.code == "U502" && d.severity == Severity::Info));
}

#[test]
fn u501_lowercase_pedi() {
    // 5.5.1 lowercase PEDI values must be uppercase in 7.0.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI birth\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(has(&g, "U501"));
}

#[test]
fn w402_date_approximations_and_months() {
    // Lowercase "about" and non-English months violate DATE style.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE about 1900\n");
    assert!(has(&g, "W402"));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 18 gener 1861\n");
    assert!(has(&g2, "W402"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE ABT 1900\n");
    assert!(!has(&ok, "W402"));
}

#[test]
fn w102_control_char_and_mixed_endings() {
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A\x01 /B/\n0 TRLR\n";
    assert!(has(g, "W102"));
    let g2 = "0 HEAD\r\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(g2, "W102"));
}

#[test]
fn e201_level2_pointer() {
    // Broken pointer below level 1 (e.g. event SOUR) is still E201.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 SOUR @S9@\n");
    assert!(has(&g, "E201"));
}

#[test]
fn e008_duplicate_singletons() {
    // INDI.SEX, FAM.HUSB, HEAD.GEDC, GEDC.VERS are all {0:1}/{1:1}.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX M\n1 SEX F\n");
    assert!(has(&g, "E008"));
    let g2 = wrap551("0 @F1@ FAM\n1 HUSB @I1@\n1 HUSB @I2@\n0 @I1@ INDI\n1 NAME A /B/\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(has(&g2, "E008"));
    let g3 = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(g3, "E008"));
    // Singletons present exactly once: clean.
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX M\n");
    assert!(!has(&ok, "E008"));
}

#[test]
fn e008_vers_scoped_by_parent() {
    // Issue #5: HEAD.GEDC.VERS (GEDCOM version) and HEAD.SOUR.VERS (product
    // version) are different singletons; every MyHeritage 5.5.1 export has both.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 SOUR MYHERITAGE\n2 NAME MyHeritage Family Tree Builder\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(!has(g, "E008"), "{:?}", codes(g));
    // HEAD.CHAR.VERS is a third slot (5.5.1 only).
    let chr = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n2 VERS 1.0\n0 TRLR\n";
    assert!(!has(chr, "E008"), "{:?}", codes(chr));
    // Two VERS inside the SAME block are still duplicates.
    let dup_gedc = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 VERS 5.5.1\n0 TRLR\n";
    assert!(has(dup_gedc, "E008"));
    let dup_sour = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 SOUR X\n2 VERS 1\n2 VERS 2\n0 TRLR\n";
    assert!(has(dup_sour, "E008"));
}

#[test]
fn e009_gedc_vers_not_satisfied_by_sour_vers() {
    // A SOUR.VERS must not stand in for the required GEDC.VERS.
    let g = "0 HEAD\n1 GEDC\n1 SOUR X\n2 VERS 9\n0 TRLR\n";
    assert!(has(g, "E009"), "{:?}", codes(g));
}

#[test]
fn e001_blank_lines_are_not_malformed() {
    // Issue #5: trailing blank lines after TRLR are not malformed lines.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n\n\n";
    assert!(!has(g, "E001"), "{:?}", codes(g));
    assert!(!has(g, "E002"), "{:?}", codes(g));
    assert_eq!(lint_str(g).exit_code(), 0);
    // A blank line must not mask the level jump that follows it.
    let jump = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n\n3 NAME A /B/\n0 TRLR\n";
    assert!(has(jump, "E001"), "{:?}", codes(jump));
}

#[test]
fn e001_year_leading_orphan_is_repaired() {
    // Issue #5: a biography continuation starting with a year read as level
    // 1936 ("level jump"), so --fix left it behind. Levels stop at 99.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT Primera part\n1936 va ser un any dur\n0 TRLR\n".to_vec();
    let r = lint_bytes(&data);
    assert!(r.diags.iter().any(|d| d.code == "E001" && d.line == 7), "{:?}", r.diags);
    let (fixed, applied) = fix_bytes(&data);
    assert!(applied.iter().any(|a| a.contains("CONT")));
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT 1936 va ser un any dur"), "{}", text);
    assert!(lint_str(&text).diags.is_empty(), "{:?}", lint_str(&text).diags);
}

#[test]
fn e001_orphan_run_stays_flat() {
    // A run of orphan lines is a run of siblings under the anchor, not a
    // staircase: nested CONTs would hang the 2nd/3rd paragraph off the 1st
    // CONT instead of off TEXT, silently losing them for a strict reader.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT Primera part\n1936 un any dur\n1937 un altre\n1938 un altre mes\n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    assert!(applied.iter().any(|a| a.contains("3 orphan lines")), "{:?}", applied);
    let text = String::from_utf8(fixed).unwrap();
    assert!(text.contains("3 CONT 1936 un any dur"), "{}", text);
    assert!(text.contains("3 CONT 1937 un altre"), "{}", text);
    assert!(text.contains("3 CONT 1938 un altre mes"), "{}", text);
    assert!(lint_str(&text).diags.is_empty(), "{:?}", lint_str(&text).diags);
    // The anchor resets on the next line that really has a level.
    let mixed = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT a\norfe u\norfe dos\n1 NOTE x\ndespres\n0 TRLR\n".to_vec();
    let t2 = String::from_utf8(fix_bytes(&mixed).0).unwrap();
    assert!(t2.contains("3 CONT orfe u") && t2.contains("3 CONT orfe dos"), "{}", t2);
    assert!(t2.contains("2 CONT despres"), "{}", t2);
}

#[test]
fn fix_leaves_whitespace_only_lines_alone() {
    // lint ignores a whitespace-only line, so --fix must not turn it into an
    // empty CONT; the trailing-whitespace pass trims it instead.
    let data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @S1@ SOUR\n1 DATA\n2 TEXT part\n   \n0 TRLR\n".to_vec();
    let (fixed, applied) = fix_bytes(&data);
    let text = String::from_utf8(fixed).unwrap();
    assert!(!applied.iter().any(|a| a.contains("orphan")), "{:?}", applied);
    assert!(!text.contains("CONT"), "{}", text);
    assert!(text.contains("2 TEXT part\n\n0 TRLR"), "{}", text);
}

#[test]
fn e009_missing_required() {
    // HEAD.GEDC and GEDC.VERS are {1:1}.
    assert!(has("0 HEAD\n1 CHAR UTF-8\n0 TRLR\n", "E009"));
    assert!(has("0 HEAD\n1 GEDC\n1 CHAR UTF-8\n0 TRLR\n", "E009"));
    assert!(!has("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n", "E009"));
}

#[test]
fn w306_enum_values() {
    // Invalid ROLE / QUAY / RESN values fire W306.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE ALIEN\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(has(&g, "W306"));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE FRIEND\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"));
    let q = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 SOUR @S1@\n3 QUAY 9\n0 @S1@ SOUR\n1 TITL T\n");
    assert!(has(&q, "W306"));
    let r = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN locked\n0 TRLR\n", HEAD70);
    assert!(has(&r, "W306"));
    let r2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN LOCKED\n0 TRLR\n", HEAD70);
    assert!(!has(&r2, "W306"));
}

#[test]
fn w306_other_wants_phrase() {
    // ROLE OTHER without a sibling PHRASE: info; with PHRASE: silent.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    let r = lint_str(&g);
    assert!(r.diags.iter().any(|d| d.code == "W306" && d.severity == Severity::Info));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n2 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(!lint_str(&ok).diags.iter().any(|d| d.code == "W306"));
}

#[test]
fn w302_3digit_years() {
    // 3-digit years are valid: same name + 1 year apart still duplicates.
    let g = wrap551(
        "0 @I1@ INDI\n1 NAME Joan /Puig/\n1 BIRT\n2 DATE 1 JAN 950\n0 @I2@ INDI\n1 NAME Joan /Puig/\n1 BIRT\n2 DATE 1 JAN 951\n",
    );
    assert!(has(&g, "W302"));
}

#[test]
fn e101_ansel_exempt_conc_split() {
    // Declared ANSEL: a high byte starting a CONC payload is legal, no E101.
    let mut data = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR ANSEL\n0 @I1@ INDI\n1 NOTE ab\n2 CONC ".to_vec();
    data.push(0x83);
    data.extend_from_slice(b"\n0 TRLR\n");
    let r = lint_bytes(&data);
    assert!(!r.diags.iter().any(|d| d.code == "E101"), "ANSEL: {:?}", r.diags);
}

#[test]
fn w306_pedi_per_version() {
    // PEDI case follows the version: uppercase in 7.0, lowercase in 5.5.1.
    let fam70 = "0 @F1@ FAM\n1 CHIL @I1@\n";
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI ADOPTED\n{}0 TRLR\n", HEAD70, fam70);
    assert!(!has(&g, "W306"));
    let g2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI adopted\n{}0 TRLR\n", HEAD70, fam70);
    assert!(has(&g2, "W306"));
    let g3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI adopted\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(!has(&g3, "W306"));
    // Issue 9 (finding 4): 5.5.1 enum checks are case-insensitive; commercial
    // exporters capitalize ("ADOPTED"). 7.0 stays strict (registry case).
    let g4 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI ADOPTED\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(!has(&g4, "W306"), "{:?}", codes(&g4));
}

#[test]
fn w306_551_enum_case_insensitive() {
    // Issue 9 (The Kennedy Family.ged): `2 TYPE Birth` must not warn in 5.5.1.
    let t = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 TYPE Birth\n");
    assert!(!has(&t, "W306"), "{:?}", codes(&t));
    let t2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 TYPE MARRIED\n");
    assert!(!has(&t2, "W306"));
    // 7.0 registry spellings are uppercase: "Birth" is still flagged there.
    let t3 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n2 TYPE Birth\n0 TRLR\n", HEAD70);
    assert!(has(&t3, "W306"));
    // MEDI under 5.5.1 accepts capitalized spellings.
    let m = wrap551("0 @O1@ OBJE\n1 FILE\n2 FORM jpeg\n3 MEDI Photo\n");
    assert!(!has(&m, "W306"));
    // 5.5.1 has no ASSO.ROLE; source-citation ROLE has its own small set.
    let r5 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE HUSB\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(!has(&r5, "W306"));
    let r5b = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE CLERGY\n0 @I2@ INDI\n1 NAME C /D/\n");
    assert!(has(&r5b, "W306"));
}

#[test]
fn w306_resn_list_70() {
    // Issue 9 (maximal70.ged): 7.0 RESN is type-List#Enum (comma-separated).
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, LOCKED\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"), "{:?}", codes(&ok));
    let ok2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, LOCKED, PRIVACY\n0 TRLR\n", HEAD70);
    assert!(!has(&ok2, "W306"));
    let bad = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 RESN CONFIDENTIAL, BOGUS\n0 TRLR\n", HEAD70);
    assert!(has(&bad, "W306"));
    // 5.5.1 RESN is a single enum (no lists); case-insensitive.
    let ok3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 RESN Locked\n");
    assert!(!has(&ok3, "W306"));
    let bad3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 RESN confidential, locked\n");
    assert!(has(&bad3, "W306"));
}

#[test]
fn w306_data_even_list_70() {
    // Issue 9 (maximal70.ged line 676): DATA.EVEN is a List#Enum of
    // event/attribute tags: "BIRT, DEAT" and "MARR" are valid.
    let ok = format!("{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRT, DEAT\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"), "{:?}", codes(&ok));
    let ok2 = format!("{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN MARR\n0 TRLR\n", HEAD70);
    assert!(!has(&ok2, "W306"));
    let bad = format!("{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRT, BOGUS\n0 TRLR\n", HEAD70);
    assert!(has(&bad, "W306"));
}

#[test]
fn e201_message_grammar() {
    // Issue 9 (Queen.ged): "points to nonexistent a FAM" double article.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F9@\n");
    let r = lint_str(&g);
    let d = r.diags.iter().find(|d| d.code == "E201").unwrap();
    assert!(d.msg.contains("points to a nonexistent FAM"), "{}", d.msg);
    assert!(!d.msg.contains("nonexistent a"), "{}", d.msg);
}

#[test]
fn w307_remarriage_after_div_is_not_a_conflict() {
    // Issue 9 (remarriage1.ged): MARR 1911, DIV 1912, MARR 1914 in one FAM.
    let g = wrap551(
        "0 @F1@ FAM\n1 HUSB @I1@\n1 MARR\n2 DATE 1911\n1 DIV\n2 DATE 1912\n1 MARR\n2 DATE 1914\n0 @I1@ INDI\n1 NAME A /B/\n",
    );
    assert!(!has(&g, "W307"), "{:?}", codes(&g));
    // The DIV need not carry a date: the event alone separates the marriages.
    let g3 = wrap551("0 @F1@ FAM\n1 MARR\n2 DATE 1911\n1 DIV\n1 MARR\n2 DATE 1914\n");
    assert!(!has(&g3, "W307"));
    // Without an intervening DIV it is still a conflict.
    let g2 = wrap551("0 @F1@ FAM\n1 MARR\n2 DATE 1911\n1 MARR\n2 DATE 1914\n");
    assert!(has(&g2, "W307"));
}

#[test]
fn w307_repeatable_events_exempt() {
    // Issue 9 (TGC551LF): two OCCU/RESI/CENS with different dates is normal.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 OCCU Baker\n2 DATE 31 DEC 1997\n1 OCCU Miller\n2 DATE 31 DEC 1998\n");
    assert!(!has(&g, "W307"), "{:?}", codes(&g));
    let g2 = wrap551(
        "0 @I1@ INDI\n1 NAME A /B/\n1 RESI\n2 DATE 1900\n1 RESI\n2 DATE 1910\n1 CENS\n2 DATE 1901\n1 CENS\n2 DATE 1911\n",
    );
    assert!(!has(&g2, "W307"));
    // Semantically single events remain checked.
    let g3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE 1870\n");
    assert!(has(&g3, "W307"));
}

#[test]
fn cr_line_endings_parse() {
    // Issue 9 (TGC551.ged): bare CR is a legal line terminator (5.5.1 s.1).
    let g = "0 HEAD\r1 GEDC\r2 VERS 5.5.1\r1 CHAR ANSEL\r0 @I1@ INDI\r1 NAME A /B/\r0 TRLR\r";
    let r = lint_bytes(g.as_bytes());
    assert_eq!(r.version, Version::V551);
    assert_eq!(r.lines, 7);
    for code in ["E002", "E101", "E009", "W102", "E001"] {
        assert!(!r.diags.iter().any(|d| d.code == code), "{}: {:?}", code, r.diags);
    }
    // CRLF still counts as one terminator.
    let crlf = "0 HEAD\r\n1 GEDC\r\n2 VERS 7.0\r\n0 TRLR\r\n";
    let r2 = lint_bytes(crlf.as_bytes());
    assert_eq!(r2.lines, 4);
    assert_eq!(r2.version, Version::V70);
}

#[test]
fn w306_other_phrase_child_ok() {
    // Issue 9 (maximal70.ged): PHRASE may hang under the OTHER-valued
    // structure itself (3 ROLE OTHER / 4 PHRASE), not only beside it.
    let g = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n3 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n",
        HEAD70
    );
    assert!(!lint_str(&g).diags.iter().any(|d| d.code == "W306"), "{:?}", codes(&g));
    // Sibling PHRASE keeps working.
    let ok = format!(
        "{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n2 PHRASE Teacher\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n",
        HEAD70
    );
    assert!(!lint_str(&ok).diags.iter().any(|d| d.code == "W306"));
    // And OTHER without any PHRASE still reports.
    let g2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD70);
    assert!(lint_str(&g2).diags.iter().any(|d| d.code == "W306"));
}

#[test]
fn w306_medi_ignored_in_70() {
    // MEDI is a 5.5.1 tag: never validated under 7.0.
    let g = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM image/jpeg\n3 MEDI PHOTO\n0 TRLR\n", HEAD70);
    assert!(!has(&g, "W306"));
}

#[test]
fn w402_date_period_phrase_calendar() {
    // Unbalanced parens, bad calendar escape: all W402.
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE (seen on stone\n");
    assert!(has(&g2, "W402"));
    let g3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#MARS@ 1900\n");
    assert!(has(&g3, "W402"));
    let ok3 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#DJULIAN@ 1900\n");
    // DJULIAN is not a registry calendar: flagged. GREGORIAN passes.
    assert!(has(&ok3, "W402"));
    let ok4 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE @#GREGORIAN@ 1900\n");
    assert!(!has(&ok4, "W402"));
}

#[test]
fn w402_one_sided_date_period_is_valid() {
    // Issue 9 (TGC551LF BASM/ADOP): DATE_PERIOD allows each half alone
    // (5.5.1 p.43 and 7.0 DATE_PERIOD both accept one-sided FROM/TO).
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BASM\n2 DATE FROM 31 DEC 1997\n");
    assert!(!has(&g, "W402"), "{:?}", codes(&g));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 ADOP Y\n2 DATE TO 31 DEC 1997\n");
    assert!(!has(&g2, "W402"), "{:?}", codes(&g2));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE FROM 1900 TO 1910\n");
    assert!(!has(&ok, "W402"));
}

#[test]
fn w402_name_slashes_across_conc() {
    // Issue 9 (Long26CC.ged): a surname split across CONC makes each line
    // unbalanced but the whole NAME balanced. Only the whole value counts.
    let g = wrap551("0 @I1@ INDI\n1 NAME /VeryLongSurnameThatKeeps\n2 CONC Going/\n");
    assert!(!has(&g, "W402"), "{:?}", codes(&g));
    let g2 = wrap551("0 @I1@ INDI\n1 NAME Very /Long/\n2 CONC Name\n");
    assert!(!has(&g2, "W402"));
    // CONT also continues the value.
    let g3 = wrap551("0 @I1@ INDI\n1 NAME /Surname\n2 CONT more/\n");
    assert!(!has(&g3, "W402"));
    // Genuinely odd totals across CONC still fire.
    let g4 = wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n2 CONC broken\n");
    assert!(has(&g4, "W402"));
    // Without continuation, unbalanced still fires (existing behavior).
    assert!(has(&wrap551("0 @I1@ INDI\n1 NAME Joan /Oso\n"), "W402"));
}

#[test]
fn e002_head_first_trlr_last() {
    // TRLR must end the file; HEAD must open it.
    assert!(has("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n0 @I1@ INDI\n1 NAME A /B/\n", "E002"));
    assert!(has("0 @I1@ INDI\n1 NAME A /B/\n0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n", "E002"));
    assert!(!has("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 TRLR\n", "E002"));
}

#[test]
fn char_rules() {
    // 5.5.1 CHAR has 4 legal values; 7.0 drops CHAR entirely.
    let g = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR ANSI\n0 TRLR\n";
    assert!(has(g, "W306"));
    let ok = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n0 TRLR\n";
    assert!(!has(ok, "W306"));
    let g70 = format!("{}1 CHAR UTF-8\n0 TRLR\n", HEAD70);
    assert!(has(&g70, "U501"));
}

#[test]
fn e008_two_birt_blocks_ok() {
    // BIRT is {0:M} (real @I131@ case): one DATE per block is legal.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE ABT 1870\n");
    assert!(!has(&g, "E008"));
}

#[test]
fn e008_event_detail_singleton() {
    // Two DATEs under one BIRT: E008.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n2 DATE 1901\n");
    assert!(has(&g, "E008"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1900\n2 PLAC Reus\n");
    assert!(!has(&ok, "E008"));
}

#[test]
fn w306_lds_stat() {
    // LDS ordinance STAT is enumerated (accepts PRE and PRE_1970).
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT BOGUS\n0 TRLR\n", HEAD70);
    assert!(has(&g, "W306"));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"));
    let ok2 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT PRE_1970\n0 TRLR\n", HEAD70);
    assert!(!has(&ok2, "W306"));
}

#[test]
fn w307_conflicting_duplicate_events() {
    // Real @I131@ pattern: two BIRT blocks, different DATEs.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE ABT 1870\n");
    assert!(has(&g, "W307"));
    let same = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n1 BIRT\n2 DATE 1867\n");
    assert!(!has(&same, "W307"));
    let single = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 1867\n");
    assert!(!has(&single, "W307"));
}

#[test]
fn e009_even_fact_type_70_only() {
    // EVEN/FACT need TYPE in 7.0; 5.5.1 leaves it optional.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 DATE 1900\n0 TRLR\n", HEAD70);
    assert!(has(&g, "E009"));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 TYPE Military service\n2 DATE 1900\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "E009"));
    let g551 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 DATE 1900\n");
    assert!(!has(&g551, "E009"));
}

#[test]
fn e009_lds_stat_date_70_only() {
    // LDS STAT needs a DATE under 7.0.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n0 TRLR\n", HEAD70);
    assert!(has(&g, "E009"));
    let ok = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 BAPL\n2 STAT COMPLETED\n3 DATE 1900\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "E009"));
}

#[test]
fn w306_data_even_and_form() {
    // DATA.EVEN payload and FILE.FORM media type under 7.0.
    let g = format!("{}0 @S1@ SOUR\n1 TITL T\n1 DATA\n2 EVEN BIRTHS\n0 TRLR\n", HEAD70);
    assert!(has(&g, "W306"));
    let f = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM textplain\n0 TRLR\n", HEAD70);
    assert!(has(&f, "W306"));
    let ok = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM image/jpeg\n0 TRLR\n", HEAD70);
    assert!(!has(&ok, "W306"));
}

#[test]
fn void_pointer_is_valid() {
    // @VOID@ is the 7.0 null pointer (voidptr.ged): never E201.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 SOUR @VOID@\n0 TRLR\n", HEAD70);
    assert!(!has(&g, "E201"));
}

#[test]
fn conc_illegal_in_70() {
    // CONC is a reserved tag in 7.0 (spec 1.3): E007.
    let g = format!("{}0 @I1@ INDI\n1 NAME A /B/\n2 CONC more\n0 TRLR\n", HEAD70);
    assert!(has(&g, "E007"));
    let g551 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n2 CONC more\n");
    assert!(!has(&g551, "E007"));
}

#[test]
fn bet_without_and_flagged_in_551() {
    // 5.5.1 DATE_RANGE mandates BET x AND y: W402.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1900\n");
    assert!(has(&g, "W402"));
}

#[test]
fn bet_range_order() {
    // Out-of-order range: U501 info suggesting a swap.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1920 AND 1900\n");
    assert!(has(&g, "U501"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE BET 1900 AND 1920\n");
    assert!(!has(&ok, "U501"));
}

#[test]
fn ansel_skips_utf8_checks() {
    // Declared ANSEL: high bytes are legal, E101 must stay silent.
    let mut data = b"0 HEAD\n1 CHAR ANSEL\n0 @I1@ INDI\n1 NAME Jos\xe9 /Oso/\n0 TRLR\n".to_vec();
    let r = lint_bytes(&data);
    assert!(!r.diags.iter().any(|d| d.code == "E101"), "ANSEL: {:?}", r.diags);
    let _ = &mut data;
}

#[test]
fn bom_silent_in_70() {
    // GEDCOM 7 recommends the BOM (spec 1.1): no W102 for it.
    let g = "\u{FEFF}0 HEAD\n1 GEDC\n2 VERS 7.0\n0 TRLR\n";
    let r = lint_bytes(g.as_bytes());
    assert!(!r.diags.iter().any(|d| d.code == "W102" && d.line == 1));
}

#[test]
fn version_detect_70_and_upgrade_rules() {
    // RELA under 5.5.1 triggers U501; SEX X is valid in 7.0 but not in 5.5.1.
    let g551 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 RELA germà\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n", HEAD551);
    assert!(has(&g551, "U501"));
    let g70 = format!("{}0 @I1@ INDI\n1 NAME A /B/\n1 SEX X\n0 TRLR\n", HEAD70);
    let r70 = lint_str(&g70);
    assert_eq!(r70.version, Version::V70);
    assert!(!r70.diags.iter().any(|d| d.code == "W305"));
    let g551x = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX X\n");
    assert!(has(&g551x, "W305"));
}

#[test]
fn gedcom70_minimal_clean() {
    // Own 7.0 fixture (criterion 5): gedcom.io minimal70 reduced to HEAD+SOUR+TRLR.
    let g = "0 HEAD\n1 GEDC\n2 VERS 7.0\n1 SOUR gedlint\n0 TRLR\n";
    let r = lint_str(g);
    assert_eq!(r.version, Version::V70);
    assert!(!r.diags.iter().any(|d| d.severity == Severity::Error), "errors: {:?}", r.diags);
}

#[test]
fn exit_codes_and_json() {
    let clean = wrap551("0 @I1@ INDI\n1 NAME A /B/\n");
    let r = lint_str(&clean);
    assert_eq!(r.exit_code(), 0);
    let j = r.to_json();
    assert!(j.contains("\"version\":\"5.5.1\""));
    let warn = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    assert_eq!(lint_str(&warn).exit_code(), 1);
    let err = "0 @I1@ INDI\n".to_string();
    assert_eq!(lint_str(&err).exit_code(), 2);
}

#[test]
fn severity_filter() {
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n");
    let r = lint_str(&g);
    assert!(r.filtered(Severity::Error).is_empty());
    assert!(!r.filtered(Severity::Warning).is_empty());
}
