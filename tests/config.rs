//! Config through the public engine API: parse_config is pure (strings in,
//! no filesystem), and lint_str_with / lint_bytes_with apply it.
//!
//! Exit-code behaviour (a rule "off" or re-levelled must move the exit
//! code) is additionally covered end to end in tests/cli.rs.

use std::io::Cursor;

use gedlint::{
    lint_bytes_with, lint_reader, lint_reader_with, lint_str, lint_str_with, parse_config, Config,
    RuleLevel, Severity,
};

/// Triggers E201 (error), W305 (warning) and U502 (info) at once.
const MULTI: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME A /B/\n1 SEX Q\n1 _UPD X\n1 FAMC @F9@\n0 TRLR\n";

fn parse_ok(text: &str) -> Config {
    parse_config(text).unwrap_or_else(|e| panic!("config should parse: {}", e))
}

fn codes(text: &str, cfg: &Config) -> Vec<String> {
    lint_str_with(text, cfg)
        .diags
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

#[test]
fn default_config_matches_plain_lint_str() {
    let plain = lint_str(MULTI);
    let with = lint_str_with(MULTI, &Config::default());
    assert_eq!(plain.diags.len(), with.diags.len());
    for (a, b) in plain.diags.iter().zip(with.diags.iter()) {
        assert_eq!((a.code, a.severity), (b.code, b.severity));
    }
    assert_eq!(plain.exit_code(), with.exit_code());
}

#[test]
fn default_config_preserves_per_finding_severities() {
    // W306 fires at info for ROLE OTHER without a PHRASE but at warn
    // elsewhere: config with no opinion on the rule must not flatten that.
    let g = "0 HEAD\n1 GEDC\n2 VERS 7.0\n0 @I1@ INDI\n1 NAME A /B/\n1 ASSO @I2@\n2 ROLE OTHER\n0 @I2@ INDI\n1 NAME C /D/\n0 TRLR\n";
    let key = |d: &gedlint::Diag| (d.code, d.severity);
    let plain: Vec<_> = lint_str(g).diags.iter().map(key).collect();
    let with: Vec<_> = lint_str_with(g, &Config::default())
        .diags
        .iter()
        .map(key)
        .collect();
    assert!(
        plain
            .iter()
            .any(|(c, s)| *c == "W306" && *s == Severity::Info),
        "{:?}",
        plain
    );
    assert_eq!(plain, with);
}

#[test]
fn default_config_matches_plain_lint_bytes() {
    let plain = lint_str(MULTI);
    let with = lint_bytes_with(MULTI.as_bytes(), &Config::default());
    assert_eq!(plain.diags.len(), with.diags.len());
}

#[test]
fn an_explicit_recommended_preset_behaves_like_the_default() {
    let cfg = parse_ok("[lints]\npresets = [\"recommended\"]\n");
    assert_eq!(codes(MULTI, &cfg), codes(MULTI, &Config::default()));
}

#[test]
fn an_empty_or_comment_only_file_is_the_default() {
    for text in ["", "\n\n", "# nothing but comments\n", "   \n"] {
        let cfg = parse_ok(text);
        assert_eq!(codes(MULTI, &cfg), codes(MULTI, &Config::default()));
    }
}

#[test]
fn the_fixture_covers_all_three_severities() {
    let found: Vec<(String, Severity)> = lint_str(MULTI)
        .diags
        .iter()
        .map(|d| (d.code.to_string(), d.severity))
        .collect();
    assert!(
        found.contains(&("E201".to_string(), Severity::Error)),
        "{:?}",
        found
    );
    assert!(
        found.contains(&("W305".to_string(), Severity::Warning)),
        "{:?}",
        found
    );
    assert!(
        found.contains(&("U502".to_string(), Severity::Info)),
        "{:?}",
        found
    );
}

#[test]
fn off_rule_emits_nothing_and_does_not_affect_the_exit_code() {
    // Baseline: error + warning -> exit 2.
    assert_eq!(lint_str(MULTI).exit_code(), 2);
    // The error off -> the remaining warning alone decides: exit 1.
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"off\"\n");
    let r = lint_str_with(MULTI, &cfg);
    assert!(!r.diags.iter().any(|d| d.code == "E201"), "{:?}", r.diags);
    assert_eq!(r.exit_code(), 1);
    // Every error and warning off -> exit 0, even though U502 still fires.
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"off\"\n\"W305\" = \"off\"\n");
    let r = lint_str_with(MULTI, &cfg);
    assert_eq!(
        r.diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec!["U502"]
    );
    assert_eq!(r.exit_code(), 0);
}

#[test]
fn severity_can_be_lowered_and_the_exit_code_follows() {
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"warn\"\n");
    let r = lint_str_with(MULTI, &cfg);
    let e201 = r
        .diags
        .iter()
        .find(|d| d.code == "E201")
        .expect("E201 still present");
    assert_eq!(e201.severity, Severity::Warning);
    // E201 (now warn) and W305 (warn) remain: exit 1, not 2.
    assert_eq!(r.exit_code(), 1);

    let cfg = parse_ok("[lints.rules]\n\"W305\" = \"info\"\n\"E201\" = \"off\"\n");
    assert_eq!(lint_str_with(MULTI, &cfg).exit_code(), 0);
}

#[test]
fn severity_can_be_raised_and_the_exit_code_follows() {
    let cfg = parse_ok("[lints.rules]\n\"U502\" = \"error\"\n");
    let r = lint_str_with(MULTI, &cfg);
    let u502 = r
        .diags
        .iter()
        .find(|d| d.code == "U502")
        .expect("U502 still present");
    assert_eq!(u502.severity, Severity::Error);
    assert_eq!(r.exit_code(), 2);
}

#[test]
fn a_rule_is_addressable_by_ruleset_and_name() {
    let by_code = parse_ok("[lints.rules]\n\"W305\" = \"off\"\n");
    let by_name = parse_ok("[lints.rules]\n\"core/invalid-sex-value\" = \"off\"\n");
    assert_eq!(codes(MULTI, &by_code), codes(MULTI, &by_name));
    assert!(!codes(MULTI, &by_name).contains(&"W305".to_string()));
}

#[test]
fn explicit_rules_beat_the_preset() {
    // recommended enables W305 as a warning; the explicit entry wins.
    let cfg =
        parse_ok("[lints]\npresets = [\"recommended\"]\n\n[lints.rules]\n\"W305\" = \"error\"\n");
    let r = lint_str_with(MULTI, &cfg);
    assert_eq!(
        r.diags.iter().find(|d| d.code == "W305").unwrap().severity,
        Severity::Error
    );
    assert_eq!(r.exit_code(), 2);
}

#[test]
fn an_empty_preset_list_starts_from_silence() {
    // Only what [lints.rules] turns on exists: the escape hatch for
    // "lint nothing but this one rule".
    let cfg = parse_ok("[lints]\npresets = []\n\n[lints.rules]\n\"W305\" = \"warn\"\n");
    assert_eq!(codes(MULTI, &cfg), vec!["W305".to_string()]);
}

#[test]
fn comments_and_blank_lines_are_allowed() {
    let cfg = parse_ok(
        "# gedlint.toml\n\n[lints] # the presets table\npresets = [\"recommended\"] # the default\n\n[lints.rules]\n# silence the noisy upgrade nudge\n\"U502\" = \"off\" # 516 of these on the real tree\n",
    );
    assert!(!codes(MULTI, &cfg).contains(&"U502".to_string()));
    // A '#' inside a string is data, not a comment start.
    let e = parse_config("[lints.rules]\n\"W#305\" = \"off\"\n").expect_err("no such rule");
    assert!(e.msg.contains("W#305"), "{}", e.msg);
}

#[test]
fn bare_keys_are_accepted_for_codes() {
    let cfg = parse_ok("[lints.rules]\nU502 = \"off\"\n");
    assert!(!codes(MULTI, &cfg).contains(&"U502".to_string()));
}

#[test]
fn a_bom_is_tolerated() {
    let cfg = parse_ok("\u{FEFF}[lints.rules]\n\"U502\" = \"off\"\n");
    assert!(!codes(MULTI, &cfg).contains(&"U502".to_string()));
}

#[test]
fn lint_bytes_with_matches_lint_str_with() {
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"off\"\n");
    let a = lint_str_with(MULTI, &cfg);
    let b = lint_bytes_with(MULTI.as_bytes(), &cfg);
    assert_eq!(a.diags.len(), b.diags.len());
    assert_eq!(a.exit_code(), b.exit_code());
}

#[test]
fn lint_reader_still_works_and_uses_the_default_config() {
    let r = lint_reader(Cursor::new(MULTI));
    assert_eq!(r.exit_code(), 2);
    assert!(r.diags.iter().any(|d| d.code == "E201"));
}

#[test]
fn lint_reader_with_applies_the_config_while_streaming() {
    // The streaming entry point must honour the config exactly like the
    // byte one: off drops, a raised severity decides the exit code. Guards
    // the CLI's streaming path against regressing to a non-config entry.
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"off\"\n\"W305\" = \"error\"\n");
    let r = lint_reader_with(Cursor::new(MULTI), &cfg);
    let got: Vec<String> = r.diags.iter().map(|d| d.code.to_string()).collect();
    assert!(!got.contains(&"E201".to_string()), "{:?}", got);
    assert_eq!(
        r.diags.iter().find(|d| d.code == "W305").unwrap().severity,
        Severity::Error
    );
    assert_eq!(r.exit_code(), 2, "the raised W305 decides the exit code");
    // And silence through the streaming path:
    let silence = parse_ok("[lints]\npresets = []\n");
    assert!(lint_reader_with(Cursor::new(MULTI), &silence)
        .diags
        .is_empty());
}

#[test]
fn an_equals_sign_inside_a_quoted_key_still_reports_unknown_rule() {
    let e = parse_config("[lints.rules]\n\"my=rule\" = \"off\"\n").expect_err("no such rule");
    assert!(e.msg.contains("unknown rule"), "{}", e.msg);
    assert!(e.msg.contains("my=rule"), "{}", e.msg);
}

#[test]
fn unknown_rule_key_is_a_hard_error() {
    for key in [
        "NOPE",
        "nope/no-such-rule",
        "hygiene/bogus-name",
        "core/no-such-rule",
        "w305",
        "W305X",
    ] {
        let e = parse_config(&format!("[lints.rules]\n\"{}\" = \"off\"\n", key)).expect_err(key);
        assert!(e.msg.contains("unknown rule"), "{}: {}", key, e.msg);
        assert_eq!(e.line, 2, "{}", key);
    }
}

#[test]
fn unknown_preset_is_a_hard_error() {
    let e = parse_config("[lints]\npresets = [\"recommended\", \"nope\"]\n")
        .expect_err("unknown preset");
    assert!(e.msg.contains("unknown preset"), "{}", e.msg);
    assert!(
        e.msg.contains("\"recommended\""),
        "the message names the valid presets: {}",
        e.msg
    );
    assert_eq!(e.line, 2);
}

#[test]
fn malformed_toml_is_a_hard_error() {
    for (text, fragment) in [
        ("[lints\npresets = []\n", "close with ']'"),
        ("[lintings]\n", "unknown section"),
        ("[]\n", "unknown section"),
        ("[lints] trailing\n", "after the section header"),
        ("presets = [\"recommended\"]\n", "before any [section]"),
        ("[lints]\nbogus = 1\n", "unknown key"),
        ("[lints]\npre sets = []\n", "key must be"),
        ("[lints]\n\"presets\" x = []\n", "after the key"),
        ("[lints]\npresets = \"recommended\"\n", "must be an array"),
        ("[lints]\npresets = 5\n", "must be an array"),
        ("[lints]\npresets = [\"recommended\"\n", "same line"),
        ("[lints]\npresets = [recommended]\n", "double-quoted"),
        ("[lints]\npresets = [\"a\" \"b\"]\n", "',' between"),
        ("[lints]\npresets = [\"a\"] extra\n", "same line"),
        (
            "[lints]\npresets = [\"recommended\"]\npresets = [\"recommended\"]\n",
            "twice",
        ),
        (
            "[lints]\npresets = [\"recommended\", \"recommended\"]\n",
            "listed twice",
        ),
        ("[lints]\n[lints]\n", "appears twice"),
        ("[lints.rules]\n[lints.rules]\n", "appears twice"),
        ("[lints] presets = []\n", "after the section header"),
        (
            "[lints.rules]\nW305 = off\n",
            "double-quoted string, e.g. \"off\"",
        ),
        (
            "[lints.rules]\n\"W305\" = off\n",
            "double-quoted string, e.g. \"off\"",
        ),
        ("[lints.rules]\n\"W305\" = \"warning\"\n", "invalid level"),
        ("[lints.rules]\n\"W305\" = \"\"\n", "invalid level"),
        (
            "[lints.rules]\n\"W305\" = \"off\n",
            "value string must close",
        ),
        (
            "[lints.rules]\n\"W305\" = \"off\" extra\n",
            "after the value",
        ),
        ("[lints.rules]\n\"W305 = \"off\"\n", "`key = value`"),
        ("[lints.rules]\n\"W305\" \"off\"\n", "`key = value`"),
        (
            "[lints.rules]\n\"W305\" = \"off\"\n\"core/invalid-sex-value\" = \"off\"\n",
            "configured twice",
        ),
    ] {
        let e = parse_config(text).expect_err(text);
        assert!(
            e.msg.contains(fragment),
            "{:?}: expected \"{}\" in {}",
            text,
            fragment,
            e.msg
        );
    }
}

#[test]
fn error_line_numbers_point_at_the_offending_line() {
    let e =
        parse_config("[lints]\n\n[lints.rules]\n\"W305\" = \"moderate\"\n").expect_err("bad level");
    assert_eq!(e.line, 4);
    assert!(e.to_string().contains("line 4"), "{}", e);
}

#[test]
fn rule_level_maps_all_four_values() {
    let cfg = parse_ok("[lints.rules]\n\"E201\" = \"off\"\n\"W305\" = \"error\"\n\"U502\" = \"warn\"\n\"W402\" = \"info\"\n");
    let eff = cfg.effective();
    assert_eq!(eff["E201"], RuleLevel::Off);
    assert_eq!(eff["W305"], RuleLevel::Severity(Severity::Error));
    assert_eq!(eff["U502"], RuleLevel::Severity(Severity::Warning));
    assert_eq!(eff["W402"], RuleLevel::Severity(Severity::Info));
    // A rule with no explicit entry is absent: on, emitting as produced.
    assert!(!eff.contains_key("W202"));
}

#[test]
fn an_enabled_preset_turns_its_rules_on() {
    // Silence everything, then let only the recommended core back in minus
    // one rule: presets decide presence, explicit entries refine it.
    let silence = parse_ok("[lints]\npresets = []\n");
    assert!(lint_str_with(MULTI, &silence).diags.is_empty());
    let back =
        parse_ok("[lints]\npresets = [\"recommended\"]\n\n[lints.rules]\n\"U502\" = \"off\"\n");
    assert_eq!(
        lint_str_with(MULTI, &back)
            .diags
            .iter()
            .map(|d| d.code)
            .collect::<Vec<_>>(),
        vec!["E201", "W305"]
    );
}

#[test]
fn rule_level_is_public_and_comparable() {
    assert_ne!(RuleLevel::Off, RuleLevel::Severity(Severity::Info));
    assert_ne!(
        RuleLevel::Severity(Severity::Info),
        RuleLevel::Severity(Severity::Error)
    );
}

#[test]
fn config_error_is_a_std_error() {
    let e = parse_config("[lints]\n\"x\" = 1\n").expect_err("bad key");
    let _: &dyn std::error::Error = &e;
}

#[test]
fn enables_reports_the_state_the_fix_gate_reads() {
    // #44: the repair side asks the config which rules emit anything, so
    // the answer must mirror `effective` exactly.
    let d = Config::default();
    assert!(
        d.enables("E001") && d.enables("E101"),
        "core rules are on by default"
    );
    assert!(
        !d.enables("W601") && !d.enables("W702") && !d.enables("W703"),
        "opt-in rulesets are off"
    );

    // A preset turns its ruleset on, nothing else.
    let p = parse_ok("[lints]\npresets = [\"hispanic-naming\"]\n");
    assert!(p.enables("W601"));
    assert!(!p.enables("W703") && !p.enables("E001"));

    // An explicit "off" beats a preset that enabled the rule.
    let off = parse_ok("[lints]\npresets = [\"hygiene\"]\n\n[lints.rules]\n\"W703\" = \"off\"\n");
    assert!(!off.enables("W703"));

    // A severity entry enables a rule the presets left off.
    let on = parse_ok("[lints]\npresets = []\n\n[lints.rules]\n\"W601\" = \"warn\"\n");
    assert!(on.enables("W601"));

    // A code no rule carries (the "style" pseudo-code) is always enabled:
    // no configuration can name it.
    assert!(d.enables("style"));
}
