use std::fs;
use std::io::{BufReader, Write};
use std::process::ExitCode;

use gedlint::{Diag, FixSelection, Report, Severity, fix_bytes_with, lint_bytes, lint_reader};

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
        --format text|json    output (default: text)\n  \
        --severity N          minimum level: error, warning, info (default: info)\n  \
        --max N               cap on text diagnostics shown (0 = all; JSON always complete)\n  \
        --no-color            no ANSI colors\n  \
        --quiet               summary + exit code only\n  \
        -h, --help            this help\n  \
        -V, --version         version\n\
        \n\
        EXIT: 0 clean, 1 warnings, 2 errors\n\
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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut fix = false;
    let mut fix_only: Vec<String> = Vec::new();
    let mut fix_unsafe = false;
    let mut format = "text".to_string();
    let mut min_sev = Severity::Info;
    let mut max_show: usize = 0;
    let mut no_color = false;
    let mut quiet = false;
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
            "--quiet" | "-q" => quiet = true,
            "-h" | "--help" => {
                print!("{}", help());
                return ExitCode::from(0);
            }
            "-V" | "--version" => {
                println!("gedlint {}", VERSION);
                return ExitCode::from(0);
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

    // Streaming via BufReader (no whole-file fs::read in the engine).
    let report: Report = match fs::File::open(&path) {
        Ok(f) => lint_reader(BufReader::new(f)),
        Err(e) => {
            eprintln!("cannot read {}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    // Note: lint_reader already covers encoding; lint_bytes is equivalent.

    if format == "json" {
        println!("{}", report.to_json());
    } else if quiet {
        println!(
            "{}: {} lines, {} INDI, {} FAM, {} errors, {} warnings, {} infos (GEDCOM {})",
            path,
            report.lines,
            report.individuals,
            report.families,
            report.errors(),
            report.warnings(),
            report.infos(),
            report.version.as_str()
        );
    } else {
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        let shown_all = report.filtered(min_sev);
        let total = shown_all.len();
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
            "\n{}: {} diagnostics{} ({} errors, {} warnings, {} infos), {} lines, {} INDI, {} FAM [GEDCOM {}]",
            path,
            total,
            if total > shown.len() { format!(" (showing {})", shown.len()) } else { String::new() },
            report.errors(),
            report.warnings(),
            report.infos(),
            report.lines,
            report.individuals,
            report.families,
            report.version.as_str()
        );
    }

    // Silence unused-import warning if the engine changes.
    let _ = lint_bytes;
    ExitCode::from(report.exit_code() as u8)
}
