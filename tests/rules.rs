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
    let g4 = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 FAMC @F1@\n2 PEDI ADOPTED\n0 @F1@ FAM\n1 CHIL @I1@\n");
    assert!(has(&g4, "W306"));
}

#[test]
fn w306_medi_ignored_in_70() {
    // MEDI is a 5.5.1 tag: never validated under 7.0.
    let g = format!("{}0 @O1@ OBJE\n1 FILE\n2 FORM image/jpeg\n3 MEDI PHOTO\n0 TRLR\n", HEAD70);
    assert!(!has(&g, "W306"));
}

#[test]
fn w402_date_period_phrase_calendar() {
    // FROM without TO, unbalanced parens, bad calendar escape: all W402.
    let g = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE FROM 1900\n");
    assert!(has(&g, "W402"));
    let ok = wrap551("0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE FROM 1900 TO 1910\n");
    assert!(!has(&ok, "W402"));
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
