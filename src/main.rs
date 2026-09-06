use std::fs;
use std::io::{BufReader, Write};
use std::process::ExitCode;

use gedlint::{Report, Severity, fix_bytes, lint_bytes, lint_reader};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn help() -> String {
    format!(
        "gedlint {VERSION} (linter GEDCOM 5.5.1 + 7.0, Rust)\n\
        \n\
        ÚS: gedlint [opcions] <fitxer.ged>\n\
        \n\
        OPCIONS:\n  \
        --fix                 repara (E101 CONC partit, espais finals) amb còpia .bak\n  \
        --format text|json    sortida (defecte: text)\n  \
        --severity N          nivell mínim: error, warning, info (defecte: info)\n  \
        --no-color            sense colors ANSI\n  \
        --quiet               només resum + exit code\n  \
        -h, --help            aquesta ajuda\n  \
        -V, --version         versió\n\
        \n\
        EXIT: 0 net, 1 avisos, 2 errors\n\
        \n\
        REGLES: E001 nivell, E002 HEAD/TRLR, E003 xref duplicat, E004 xref,\n  \
        E005 CONT/CONC, E101 UTF-8/CONC partit, E201 refs trencades,\n  \
        W202 FAMC/CHIL creuat, W301 mort/longevitat, W302 duplicats,\n  \
        W303 edat pares, W304 fill abans matrimoni, W305 SEX, W401 PLAC+URL,\n  \
        W402 estil (NAME/DATE), W403 NOTE+HTML, W102 encoding/estil,\n  \
        U501/U502 upgrade path 5.5.1 -> 7.0"
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
    let mut format = "text".to_string();
    let mut min_sev = Severity::Info;
    let mut no_color = false;
    let mut quiet = false;
    let mut path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--fix" => fix = true,
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
                    eprintln!("--format necessita text|json");
                    return ExitCode::from(2);
                }
                format = args[i].clone();
                if format != "text" && format != "json" {
                    eprintln!("--format ha de ser text|json");
                    return ExitCode::from(2);
                }
            }
            "--severity" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("--severity necessita error|warning|info");
                    return ExitCode::from(2);
                }
                match Severity::parse(&args[i]) {
                    Some(s) => min_sev = s,
                    None => {
                        eprintln!("--severity ha de ser error|warning|info");
                        return ExitCode::from(2);
                    }
                }
            }
            a if a.starts_with('-') => {
                eprintln!("opció desconeguda: {} (prova --help)", a);
                return ExitCode::from(2);
            }
            a => {
                if path.is_some() {
                    eprintln!("només un fitxer per invocació");
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

    // --fix abans de lintar: llegeix bytes, repara, escriu .bak.
    if fix {
        match fs::read(&path) {
            Ok(data) => {
                let (fixed, applied) = fix_bytes(&data);
                if fixed != data {
                    let bak = format!("{}.bak", path);
                    if let Err(e) = fs::write(&bak, &data) {
                        eprintln!("no s'ha pogut escriure {}: {}", bak, e);
                        return ExitCode::from(2);
                    }
                    if let Err(e) = fs::write(&path, &fixed) {
                        eprintln!("no s'ha pogut escriure {}: {}", path, e);
                        return ExitCode::from(2);
                    }
                    let stdout = std::io::stdout();
                    let mut h = stdout.lock();
                    let _ = writeln!(h, "fix: {} (còpia {})", applied.join("; "), bak);
                }
            }
            Err(e) => {
                eprintln!("no es pot llegir {}: {}", path, e);
                return ExitCode::from(2);
            }
        }
    }

    // Streaming via BufReader (no fs::read sencer al motor).
    let report: Report = match fs::File::open(&path) {
        Ok(f) => lint_reader(BufReader::new(f)),
        Err(e) => {
            // Fallback: si no es pot obrir com a fitxer, missatge clar.
            eprintln!("no es pot llegir {}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    // Nota: lint_reader ja cobreix encoding; lint_bytes seria equivalent.

    if format == "json" {
        println!("{}", report.to_json());
    } else if quiet {
        println!(
            "{}: {} línies, {} INDI, {} FAM, {} errors, {} avisos, {} infos (GEDCOM {})",
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
        for d in report.filtered(min_sev) {
            let (c1, c2) = color_for(&d.severity, no_color);
            let loc = if d.line > 0 { format!("línia {}", d.line) } else { "-".to_string() };
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
            "\n{}: {} diagnòstics ({} errors, {} avisos, {} infos), {} línies, {} INDI, {} FAM [GEDCOM {}]",
            path,
            report.filtered(min_sev).len(),
            report.errors(),
            report.warnings(),
            report.infos(),
            report.lines,
            report.individuals,
            report.families,
            report.version.as_str()
        );
    }

    // Silencia warning d'import si canvia el motor.
    let _ = lint_bytes;
    ExitCode::from(report.exit_code() as u8)
}
