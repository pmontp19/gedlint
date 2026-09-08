use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gedlint::{apply_baseline, baseline_from_report, baseline_to_json, parse_baseline, Applicability, Category, Config, Diag, DiagGroup, FixSelection, Report, RuleMeta, Severity, fix_bytes_with, lint_reader_with, parse_config};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The file discovery looks for, walking up from the linted file.
const CONFIG_FILE: &str = "gedlint.toml";

fn help() -> String {
    format!(
        "gedlint {VERSION} (GEDCOM 5.5.1 + 7.0 linter, Rust)\n\
        \n\
        USAGE: gedlint [options] <file.ged>\n\
        \n\
        OPTIONS:\n  \
        --fix                 repair (E001 orphan lines, E101 split CONC, trailing whitespace) with .bak copy\n  \
        --only CODE           with --fix: restrict to this repair code (repeatable)\n  \
        --unsafe              with --fix: also apply MaybeIncorrect repairs (never the default)\n  \
        --config PATH         config file to use instead of the discovered gedlint.toml\n  \
        --no-config           ignore any gedlint.toml (built-in rules only)\n  \
        --format text|json    output (default: text)\n  \
         --severity N          minimum level: error, warning, info (default: info)\n  \
         --max N               cap rule groups by default, single diagnostics with --verbose (0 = all; JSON always complete)\n  \
         --verbose             list every occurrence instead of one line per rule\n  \
         --baseline FILE       fail only on findings not already recorded in FILE (the ratchet)\n  \
         --write-baseline      record every current finding in FILE and exit 0\n  \
         --no-color            no ANSI colors\n  \
        --quiet               summary + exit code only\n  \
        --explain [CODE]      explain a rule (no CODE: every rule by ruleset)\n  \
        -h, --help            this help\n  \
        -V, --version         version\n\
        \n\
        CONFIG: gedlint.toml is looked up in the file's directory and every\n  \
        parent. It sets presets and per-rule severities; an unknown rule,\n  \
        preset or spelling is an error, never a silent no-op.\n\
        \n\
        EXIT: 0 clean, 1 warnings, 2 errors (also: bad usage, unreadable\n  \
        file, invalid config; with --baseline: only NEW findings count)\n\
        \n\
        RULES: E001 level, E002 HEAD/TRLR, E003 duplicate xref, E004 xref,\n  \
        E005 CONT/CONC, E007 CONC in 7.0, E008 duplicate singleton,\n  \
        E009 missing required, E101 UTF-8/split CONC, E201 broken refs,\n  \
        W202 FAMC/CHIL mismatch, W301 death/longevity, W302 duplicates,\n  \
        W303 parent age, W304 child before marriage, W305 SEX, W306 enums,\n  \
        W307 conflicting duplicate events, W401 PLAC+URL,\n  \
        W402 style (NAME/DATE), W403 NOTE+HTML,\n  \
        W102 encoding/style, U501/U502 upgrade path 5.5.1 -> 7.0"
    )
}

fn color_for(sev: &Severity, no_color: bool) -> (&'static str, &'static str) {
    if no_color {
        return ("", "");
    }
    match sev {
        Severity::Error => ("\x1b[31m", "\x1b[0m"),
        Severity::Warning => ("\x1b[33m", "\x1b[0m"),
        Severity::Info => ("\x1b[36m", "\x1b[0m"),
    }
}

/// `--explain [CODE]`: the rule registry rendered for a human. Without an
/// argument it lists every rule grouped by ruleset; with one it prints the
/// full entry for that rule, addressed by code or by `<ruleset>/<name>`.
fn explain(what: Option<&str>) -> ExitCode {
    let stdout = std::io::stdout();
    let mut h = stdout.lock();
    let Some(what) = what else {
        for rs in gedlint::rulesets() {
            let rules: Vec<&RuleMeta> = gedlint::RULES.iter().filter(|r| r.ruleset == rs).collect();
            let on = if rules.iter().all(|r| r.default_enabled) { "on by default" } else { "off by default" };
            let _ = writeln!(h, "{} ({} rules, {})", rs, rules.len(), on);
            for r in rules {
                let _ = writeln!(h, "  {:<5} {:<30} {}", r.code, r.name, r.title);
            }
            let _ = writeln!(h);
        }
        let _ = writeln!(h, "gedlint --explain <CODE> prints why one rule exists and how to satisfy it.");
        return ExitCode::from(0);
    };
    let found = match what.split_once('/') {
        Some((ruleset, name)) => gedlint::rule_by_name(ruleset, name),
        None => gedlint::rule(&what.to_ascii_uppercase()),
    };
    let Some(r) = found else {
        eprintln!("unknown rule: {} (run --explain with no argument to list every rule)", what);
        return ExitCode::from(2);
    };
    let _ = writeln!(h, "{}  {}  [{}/{}]", r.code, r.title, r.ruleset, r.name);
    let _ = writeln!(
        h,
        "\ncategory {}   severity {}   {}   {}\n",
        r.category.as_str(),
        r.default_severity.tag().trim().to_ascii_lowercase(),
        if r.default_enabled { "on by default" } else { "off by default" },
        match r.fixable {
            Some(Applicability::Safe) => "fixable by --fix",
            Some(Applicability::MaybeIncorrect) => "fixable, but only under --fix --unsafe",
            None => "no automatic fix",
        }
    );
    let _ = writeln!(h, "WHY");
    wrapped(&mut h, r.why);
    let _ = writeln!(h, "\nREMEDY");
    wrapped(&mut h, r.remedy);
    ExitCode::from(0)
}

/// Prose at a readable terminal width, indented two spaces.
fn wrapped(h: &mut impl Write, text: &str) {
    let mut line = String::new();
    for w in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + w.chars().count() > 76 {
            let _ = writeln!(h, "  {}", line);
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(w);
    }
    if !line.is_empty() {
        let _ = writeln!(h, "  {}", line);
    }
}

/// Walk up from the linted file's directory looking for a `gedlint.toml`.
/// A bare filename resolves against the current directory, so discovery
/// still starts there. Stops at the filesystem root.
fn discover_config(from: &Path) -> Option<PathBuf> {
    let file = fs::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let mut dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    if dir.as_os_str().is_empty() {
        dir = PathBuf::from(".");
    }
    loop {
        let candidate = dir.join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Which configuration the run uses: `--no-config` keeps the built-ins,
/// `--config PATH` names the file, otherwise discovery decides. A file that
/// exists but does not parse is a hard error, never a silent fallback.
fn load_config(lint_path: &str, explicit: Option<&str>, no_config: bool) -> Result<Config, String> {
    if no_config {
        return Ok(Config::default());
    }
    let file = match explicit {
        Some(p) => PathBuf::from(p),
        None => match discover_config(Path::new(lint_path)) {
            Some(f) => f,
            None => return Ok(Config::default()),
        },
    };
    let text = fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {}", file.display(), e))?;
    parse_config(&text).map_err(|e| format!("{}: {}", file.display(), e))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut fix = false;
    let mut fix_only: Vec<String> = Vec::new();
    let mut fix_unsafe = false;
    let mut config_path: Option<String> = None;
    let mut no_config = false;
    let mut format = "text".to_string();
    let mut min_sev = Severity::Info;
    let mut max_show: usize = 0;
    let mut verbose = false;
    let mut no_color = false;
    let mut quiet = false;
    let mut baseline_path: Option<String> = None;
    let mut write_baseline_path: Option<String> = None;
    let mut path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--fix" => fix = true,
            "--unsafe" => fix_unsafe = true,
            "--only" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--only needs a rule code");
                    return ExitCode::from(2);
                }
                fix_only.push(args[i].clone());
            }
            "--no-color" => no_color = true,
            "--verbose" | "-v" => verbose = true,
            "--quiet" | "-q" => quiet = true,
            "--config" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--config needs a path to a gedlint.toml file");
                    return ExitCode::from(2);
                }
                config_path = Some(args[i].clone());
            }
            "--no-config" => no_config = true,
            "-h" | "--help" => {
                print!("{}", help());
                return ExitCode::from(0);
            }
            "-V" | "--version" => {
                println!("gedlint {}", VERSION);
                return ExitCode::from(0);
            }
            "--explain" => {
                // The code is optional, but another option in its place is a
                // mistake, not a request for the whole listing.
                match args.get(i + 1) {
                    Some(a) if a.starts_with('-') => {
                        eprintln!("--explain takes a rule code, not {} (try --explain with no argument)", a);
                        return ExitCode::from(2);
                    }
                    what => return explain(what.map(String::as_str)),
                }
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--format needs text|json");
                    return ExitCode::from(2);
                }
                format = args[i].clone();
                if format != "text" && format != "json" {
                    eprintln!("--format must be text|json");
                    return ExitCode::from(2);
                }
            }
            "--severity" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--severity needs error|warning|info");
                    return ExitCode::from(2);
                }
                match Severity::parse(&args[i]) {
                    Some(s) => min_sev = s,
                    None => {
                        eprintln!("--severity must be error|warning|info");
                        return ExitCode::from(2);
                    }
                }
            }
            "--max" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--max needs a number");
                    return ExitCode::from(2);
                }
                match args[i].parse::<usize>() {
                    Ok(n) => max_show = n,
                    Err(_) => {
                        eprintln!("--max must be a number >= 0");
                        return ExitCode::from(2);
                    }
                }
            }
            "--baseline" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--baseline needs a file path");
                    return ExitCode::from(2);
                }
                baseline_path = Some(args[i].clone());
            }
            "--write-baseline" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--write-baseline needs a file path");
                    return ExitCode::from(2);
                }
                write_baseline_path = Some(args[i].clone());
            }
            a if a.starts_with('-') => {
                eprintln!("unknown option: {} (try --help)", a);
                return ExitCode::from(2);
            }
            a => {
                if path.is_some() {
                    eprintln!("only one file per invocation");
                    return ExitCode::from(2);
                }
                path = Some(a.to_string());
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        eprint!("{}", help());
        return ExitCode::from(2);
    };

    if !fix && (!fix_only.is_empty() || fix_unsafe) {
        eprintln!("--only and --unsafe only apply with --fix");
        return ExitCode::from(2);
    }
    if baseline_path.is_some() && write_baseline_path.is_some() {
        eprintln!("--baseline and --write-baseline are mutually exclusive");
        return ExitCode::from(2);
    }

    if no_config && config_path.is_some() {
        eprintln!("--no-config and --config cannot be used together");
        return ExitCode::from(2);
    }

    // Load before any --fix write: a broken config must stop the run, not
    // let a repair happen under rules the user meant to silence.
    let cfg = match load_config(&path, config_path.as_deref(), no_config) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{}", msg);
            return ExitCode::from(2);
        }
    };

    // --fix before linting: read bytes, repair, write .bak.
    if fix {
        let sel = FixSelection { only: fix_only, allow_unsafe: fix_unsafe };
        match fs::read(&path) {
            Ok(data) => {
                let (fixed, applied) = fix_bytes_with(&data, &sel);
                if fixed != data {
                    let bak = format!("{}.bak", path);
                    if let Err(e) = fs::write(&bak, &data) {
                        eprintln!("cannot write {}: {}", bak, e);
                        return ExitCode::from(2);
                    }
                    if let Err(e) = fs::write(&path, &fixed) {
                        eprintln!("cannot write {}: {}", path, e);
                        return ExitCode::from(2);
                    }
                    let stdout = std::io::stdout();
                    let mut h = stdout.lock();
                    let _ = writeln!(h, "fix: {} (backup {})", applied.join("; "), bak);
                }
            }
            Err(e) => {
                eprintln!("cannot read {}: {}", path, e);
                return ExitCode::from(2);
            }
        }
    }

    // Streaming via BufReader (no whole-file fs::read in the engine): the
    // config is resolved before the read, so the engine stays fs-free and
    // large exports are never loaded whole by the caller either.
    let report: Report = match fs::File::open(&path) {
        Ok(f) => lint_reader_with(BufReader::new(f), &cfg),
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            return ExitCode::from(2);
        }
    };

    // --write-baseline: snapshot the current findings and stop. Exit 0 so a
    // CI bootstrap (`--write-baseline && --baseline`) succeeds on a messy
    // tree; the recorded state is what the next run ratchets against.
    if let Some(wb) = &write_baseline_path {
        let b = baseline_from_report(&report);
        if let Err(e) = fs::write(wb, baseline_to_json(&b)) {
            eprintln!("cannot write {}: {}", wb, e);
            return ExitCode::from(2);
        }
        println!(
            "baseline: {} entries ({} findings) written to {}",
            b.entries.len(),
            report.diags.len(),
            wb
        );
        return ExitCode::from(0);
    }

    // --baseline: match the run against the recorded counts. Only findings
    // beyond the counts (new) drive the exit code; findings that vanished
    // from the run are reported as resolved (the ratchet).
    let outcome = match &baseline_path {
        Some(bp) => {
            let text = match fs::read_to_string(bp) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("cannot read baseline {}: {}", bp, e);
                    return ExitCode::from(2);
                }
            };
            let b = match parse_baseline(&text) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("{}: {}", bp, e);
                    return ExitCode::from(2);
                }
            };
            Some(apply_baseline(&report, &b))
        }
        None => None,
    };

    if format == "json" {
        // The JSON contract (scripts/gh-report.js, web viewer) is the full
        // report, unchanged; the baseline only moves the exit code.
        println!("{}", report.to_json());
    } else if quiet {
        println!(
            "{}: {} lines, {} INDI, {} FAM, {} errors, {} warnings, {} infos (GEDCOM {}){}",
            path,
            report.lines,
            report.individuals,
            report.families,
            report.errors(),
            report.warnings(),
            report.infos(),
            report.version.as_str(),
            match &outcome {
                Some(o) => format!(", {} baselined, {} resolved", o.baselined, o.resolved.len()),
                None => String::new(),
            }
        );
    } else {
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        let shown_all: Vec<&Diag> = match &outcome {
            Some(o) => o.new_diags.iter().filter(|d| d.severity >= min_sev).collect(),
            None => report.filtered(min_sev),
        };
        let total = shown_all.len();
        // Baseline-aware summary numbers (issue 22): with --baseline the
        // footer counts only NEW findings, and the suppressed/resolved
        // counts ride along; without one it is exactly #20's summary.
        let (new_total, new_e, new_w, new_i, base_extra) = match &outcome {
            Some(o) => (
                o.new_diags.len(),
                o.new_diags.iter().filter(|d| d.severity == Severity::Error).count(),
                o.new_diags.iter().filter(|d| d.severity == Severity::Warning).count(),
                o.new_diags.iter().filter(|d| d.severity == Severity::Info).count(),
                format!(", {} baselined, {} resolved", o.baselined, o.resolved.len()),
            ),
            None => (total, report.errors(), report.warnings(), report.infos(), String::new()),
        };
        let new_word = if outcome.is_some() { "new " } else { "" };
        if verbose {
            // --verbose lists every occurrence, exactly as the output always
            // looked (issue 20).
            let shown: &[&Diag] = if max_show > 0 && total > max_show { &shown_all[..max_show] } else { &shown_all };
            for d in shown {
                let (c1, c2) = color_for(&d.severity, no_color);
                let loc = if d.line > 0 { format!("line {}", d.line) } else { "-".to_string() };
                let _ = writeln!(
                    h,
                    "{}{} [{}:{}]{} {}: {}",
                    c1,
                    d.severity.tag(),
                    d.code,
                    d.category.as_str(),
                    c2,
                    loc,
                    d.msg
                );
            }
            let _ = writeln!(
                h,
                "\n{}: {} {}diagnostics{} ({} errors, {} warnings, {} infos){}, {} lines, {} INDI, {} FAM [GEDCOM {}]",
                path,
                new_total,
                new_word,
                if total > shown.len() { format!(" (showing {})", shown.len()) } else { String::new() },
                new_e,
                new_w,
                new_i,
                base_extra,
                report.lines,
                report.individuals,
                report.families,
                report.version.as_str()
            );
        } else {
            // Default (issue 20): one line per rule code, worst and most
            // frequent first, from the engine's grouping (Report::grouped).
            let groups = Report::group_diags(&shown_all);
            let shown_groups: &[DiagGroup] = if max_show > 0 && groups.len() > max_show { &groups[..max_show] } else { &groups };
            for g in shown_groups {
                let (c1, c2) = color_for(&g.severity, no_color);
                if g.count == 1 {
                    let loc = if g.line > 0 { format!("line {}", g.line) } else { "-".to_string() };
                    let _ = writeln!(h, "{}{} [{}:{}]{} {}: {}", c1, g.severity.tag(), g.code, g.category.as_str(), c2, loc, g.example);
                } else {
                    let _ = writeln!(
                        h,
                        "{}{} [{}:{}]{} {}, {} occurrences (--verbose to list all)",
                        c1,
                        g.severity.tag(),
                        g.code,
                        g.category.as_str(),
                        c2,
                        g.example,
                        g.count
                    );
                }
            }
            let capped = max_show > 0 && groups.len() > shown_groups.len();
            let _ = writeln!(
                h,
                "\n{}: {} {}diagnostics{} ({} errors, {} warnings, {} infos){}, {} lines, {} INDI, {} FAM [GEDCOM {}]",
                path,
                new_total,
                new_word,
                if capped { format!(" (showing {} of {} groups)", shown_groups.len(), groups.len()) } else { String::new() },
                new_e,
                new_w,
                new_i,
                base_extra,
                report.lines,
                report.individuals,
                report.families,
                report.version.as_str()
            );
            // Summary footer by category and by rule code (issue 20),
            // over everything the severity filter let through.
            if total > 0 {
                let cats: Vec<String> = [
                    (Category::Correctness, "correctness"),
                    (Category::Suspicious, "suspicious"),
                    (Category::Style, "style"),
                    (Category::Upgrade, "upgrade"),
                ]
                .iter()
                .map(|(c, name)| (shown_all.iter().filter(|d| d.category == *c).count(), *name))
                .filter(|(n, _)| *n > 0)
                .map(|(n, name)| format!("{} {}", name, n))
                .collect();
                let _ = writeln!(h, "Categories: {}", cats.join(", "));
                let rules: String = groups
                    .iter()
                    .map(|g| format!("{} {}", g.code, g.count))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(h, "Rules: {}", rules);
            }
        }

        // Baseline appendix (issue 22): findings the baseline recorded but
        // this run no longer has are the ratchet's progress report;
        // --write-baseline prunes them from the file.
        if let Some(o) = &outcome {
            if !o.resolved.is_empty() {
                let _ = writeln!(
                    h,
                    "\nresolved (recorded in the baseline but absent from this run; --write-baseline prunes them):"
                );
                for e in &o.resolved {
                    let _ = writeln!(h, "  {}x {} {}", e.count, e.code, e.fingerprint);
                }
            }
        }
    }

    // Baselined findings never affect the exit code: only the surplus. The
    // fallback is the config-filtered report (#21 has already dropped "off"
    // rules inside the engine, so they cannot reappear here).
    let exit = match &outcome {
        Some(o) => Report {
            version: report.version,
            lines: report.lines,
            individuals: report.individuals,
            families: report.families,
            diags: o.new_diags.clone(),
        }
        .exit_code(),
        None => report.exit_code(),
    };
    ExitCode::from(exit as u8)
}
