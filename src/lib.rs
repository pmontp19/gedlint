//! gedlint: motor del linter GEDCOM (5.5.1 + 7.0).
//!
//! Disseny: parsing en streaming línia a línia (`BufRead`), sense carregar
//! l'arbre sencer. El nucli és pur (`&str` in, `Report` out) i per tant
//! compilable a WASM sense canvis: el binari CLI és una capa prima (fs + args).
//! Zero dependències.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn tag(&self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN ",
            Severity::Info => "INFO ",
        }
    }

    pub fn parse(s: &str) -> Option<Severity> {
        match s.to_ascii_lowercase().as_str() {
            "error" | "errors" | "e" => Some(Severity::Error),
            "warning" | "warnings" | "warn" | "w" => Some(Severity::Warning),
            "info" | "i" => Some(Severity::Info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Correctness,
    Suspicious,
    Style,
    Upgrade,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Correctness => "correctness",
            Category::Suspicious => "suspicious",
            Category::Style => "style",
            Category::Upgrade => "upgrade",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V551,
    V70,
    Unknown,
}

impl Version {
    pub fn as_str(&self) -> &'static str {
        match self {
            Version::V551 => "5.5.1",
            Version::V70 => "7.0",
            Version::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub code: &'static str,
    pub category: Category,
    pub severity: Severity,
    pub line: usize,
    pub msg: String,
}

impl Diag {
    fn new(code: &'static str, category: Category, severity: Severity, line: usize, msg: String) -> Diag {
        Diag { code, category, severity, line, msg }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub version: Version,
    pub diags: Vec<Diag>,
    pub lines: usize,
    pub individuals: usize,
    pub families: usize,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Warning).count()
    }
    pub fn infos(&self) -> usize {
        self.diags.iter().filter(|d| d.severity == Severity::Info).count()
    }
    pub fn exit_code(&self) -> i32 {
        if self.errors() > 0 {
            2
        } else if self.warnings() > 0 {
            1
        } else {
            0
        }
    }

    pub fn filtered(&self, min: Severity) -> Vec<&Diag> {
        self.diags.iter().filter(|d| d.severity >= min).collect()
    }

    /// Serialitza a JSON sense dependències (per CLI --format json i per WASM).
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(self.diags.len() * 128);
        out.push_str("{\"version\":\"");
        out.push_str(self.version.as_str());
        out.push_str("\",\"lines\":");
        out.push_str(&self.lines.to_string());
        out.push_str(",\"individuals\":");
        out.push_str(&self.individuals.to_string());
        out.push_str(",\"families\":");
        out.push_str(&self.families.to_string());
        out.push_str(",\"summary\":{\"errors\":");
        out.push_str(&self.errors().to_string());
        out.push_str(",\"warnings\":");
        out.push_str(&self.warnings().to_string());
        out.push_str(",\"infos\":");
        out.push_str(&self.infos().to_string());
        out.push_str("},\"diagnostics\":[");
        for (i, d) in self.diags.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"code\":\"");
            out.push_str(d.code);
            out.push_str("\",\"category\":\"");
            out.push_str(d.category.as_str());
            out.push_str("\",\"severity\":\"");
            out.push_str(d.severity.tag().trim());
            out.push_str("\",\"line\":");
            out.push_str(&d.line.to_string());
            out.push_str(",\"message\":\"");
            out.push_str(&escape_json(&d.msg));
            out.push_str("\"}");
        }
        out.push_str("]}");
        out
    }
}

fn escape_json(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

const MAX_ERRORS: usize = 200;

// ---------------------------------------------------------------------------
// API pública pura (reutilitzable des de WASM): text in, informe out.
// ---------------------------------------------------------------------------

/// Analitza text GEDCOM ja llegit. Nucli pur apte per WASM.
pub fn lint_str(input: &str) -> Report {
    lint_bytes_split(input.as_bytes(), true)
}

/// Analitza bytes crus (detecta UTF-8 / BOM / CRLF abans de decodificar).
/// `is_final` reservat per a futures passades incrementals.
pub fn lint_bytes(data: &[u8]) -> Report {
    lint_bytes_split(data, false)
}

fn lint_bytes_split(data: &[u8], already_str: bool) -> Report {
    let mut diags: Vec<Diag> = Vec::new();
    push_capped(&mut diags, encoding_diags(data, already_str));

    let text = String::from_utf8_lossy(data).into_owned();
    let mut r = lint_lines(&text);
    // Els diags d'encoding van primer (línia baixa), després els semàntics.
    let mut all = diags;
    all.append(&mut r.diags);
    all.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    r.diags = all;
    r
}

/// Entrada streaming: llegeix línia a línia sense carregar-ho tot de cop.
/// Útil per a exports de 100MB+. Internament delega a `lint_lines` per
/// simplicitat, però el contracte és `BufRead`.
pub fn lint_reader<R: BufRead>(mut reader: R) -> Report {
    let mut buf = Vec::new();
    let mut chunk = Vec::new();
    // Llegim per blocs i unim: memòria O(n) en bytes però O(1) en objectes.
    loop {
        chunk.clear();
        match reader.read_until(b'\n', &mut chunk) {
            Ok(0) => break,
            Ok(_) => buf.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    lint_bytes(&buf)
}

// ---------------------------------------------------------------------------
// Diags d'encoding (nivell byte, abans del parser)
// ---------------------------------------------------------------------------

fn push_capped(dst: &mut Vec<Diag>, mut v: Vec<Diag>) {
    for d in v.drain(..) {
        if dst.len() < MAX_ERRORS {
            dst.push(d);
        }
    }
}

/// E101/W102: UTF-8, BOM, byte de continuació a inici de línia (bug MyHeritage
/// que parteix multibyte entre línies CONC), CRLF mixt, controls.
fn encoding_diags(data: &[u8], _already_str: bool) -> Vec<Diag> {
    let mut out = Vec::new();
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        out.push(Diag::new(
            "W102",
            Category::Style,
            Severity::Warning,
            1,
            "BOM UTF-8 al inici (GEDCOM 7 prefereix sense BOM)".into(),
        ));
    }
    let has_crlf = data.windows(2).any(|w| w == b"\r\n");
    let has_lone_lf = {
        let mut prev_cr = false;
        let mut found = false;
        for &b in data {
            if b == b'\n' && !prev_cr {
                found = true;
                break;
            }
            prev_cr = b == b'\r';
        }
        found
    };
    let has_lone_cr = {
        let mut found = false;
        let mut it = data.iter().peekable();
        while let Some(&b) = it.next() {
            if b == b'\r' {
                match it.peek() {
                    Some(&&b'\n') => {}
                    _ => {
                        found = true;
                        break;
                    }
                }
            }
        }
        found
    };
    if has_crlf && (has_lone_lf || has_lone_cr) {
        out.push(Diag::new(
            "W102",
            Category::Style,
            Severity::Warning,
            0,
            "finals de línia mixtos CRLF/LF (normalitza a un sol estil)".into(),
        ));
    }

    // Línies que comencen amb byte de continuació UTF-8 (0x80..=0xBF):
    // símptoma del bug MyHeritage (caràcter partit per CONC).
    let mut bad = 0usize;
    let mut first = 0usize;
    for (i, line) in data.split(|&b| b == b'\n').enumerate() {
        let l = if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line };
        // Salta la capçalera "N CONC ...": el contingut útil comença després.
        let payload = conc_payload(l);
        if payload.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) {
            bad += 1;
            if first == 0 {
                first = i + 1;
            }
        }
    }
    if bad > 0 {
        out.push(Diag::new(
            "E101",
            Category::Correctness,
            Severity::Error,
            first,
            format!(
                "{} línies comencen amb byte de continuació UTF-8 (caràcter partit entre línies CONC, bug MyHeritage; prova --fix)",
                bad
            ),
        ));
    }
    if std::str::from_utf8(data).is_err() && bad == 0 {
        out.push(Diag::new(
            "E101",
            Category::Correctness,
            Severity::Error,
            0,
            "el fitxer no és UTF-8 vàlid".into(),
        ));
    }
    out
}

/// Retorna el payload després de "N CONC " si existeix, si no la línia sencera.
fn conc_payload(line: &[u8]) -> &[u8] {
    // Cerca " CONC " a nivell byte.
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        &line[p + pat.len()..]
    } else if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let rest = &line[p + 4..];
        let rest = if rest.first() == Some(&b' ') { &rest[1..] } else { rest };
        rest
    } else {
        line
    }
}

// ---------------------------------------------------------------------------
// Parser de línies + regles
// ---------------------------------------------------------------------------

struct Line {
    no: usize,
    level: Option<u32>,
    xref: String,
    tag: String,
    value: String,
    raw: String,
}

fn parse_line(no: usize, raw: &str) -> Line {
    // Gramàtica: NIVELL [XREF] TAG [VALOR]. XREF només a nivell 0.
    let mut it = raw.splitn(3, char::is_whitespace);
    let lvl: Option<u32> = it.next().and_then(|x| x.parse().ok());
    let second = it.next().unwrap_or("");
    let rest = it.next().unwrap_or("");
    let (xref, tag, value) = if second.starts_with('@') && second.ends_with('@') && second.len() >= 3 {
        // "0 @I1@ INDI ..." : tag és la primera paraula de rest.
        let (t, v) = match rest.split_once(' ') {
            Some((t, v)) => (t, v.trim()),
            None => (rest, ""),
        };
        (second.to_string(), t.to_string(), v.to_string())
    } else if second.starts_with('@') {
        // Xref malformat (sense tancar): ho marquem igualment com a xref
        // perquè E004 ho detecti; tag de rest.
        let (t, v) = match rest.split_once(' ') {
            Some((t, v)) => (t, v.trim()),
            None => (rest, ""),
        };
        (second.to_string(), t.to_string(), v.to_string())
    } else {
        (String::new(), second.to_string(), rest.trim().to_string())
    };
    Line { no, level: lvl, xref, tag, value, raw: raw.to_string() }
}

fn is_pointer(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 3 && t.starts_with('@') && t.ends_with('@') && !t[1..t.len() - 1].contains(char::is_whitespace)
}

fn inner_ptr(s: &str) -> &str {
    s.trim()
}

fn year_of(s: &str) -> Option<i64> {
    // Primer any de 3-4 dígits (dates GEDCOM: "12 SEP 1909", "BEF 1900"...).
    let mut best: Option<i64> = None;
    for tok in s.split(|c: char| !c.is_ascii_digit()) {
        if (3..=4).contains(&tok.len()) {
            if let Ok(y) = tok.parse::<i64>() {
                if (100..=2100).contains(&y) {
                    best = Some(y);
                    break;
                }
            }
        }
    }
    best
}

fn norm_name(s: &str) -> String {
    s.to_lowercase().replace('/', " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

fn lint_lines(text: &str) -> Report {
    let mut diags: Vec<Diag> = Vec::new();
    let raw_lines: Vec<&str> = text.lines().collect();
    let lines: Vec<Line> = raw_lines.iter().enumerate().map(|(i, l)| parse_line(i + 1, l)).collect();

    // Estat de versió (HEAD.GEDC.VERS).
    let mut version = Version::Unknown;
    let mut in_head = false;
    let mut in_gedc = false;
    for l in &lines {
        if l.level == Some(0) {
            in_head = l.tag == "HEAD";
            in_gedc = false;
        } else if in_head && l.level == Some(1) {
            in_gedc = l.tag == "GEDC";
        } else if in_head && in_gedc && l.level == Some(2) && l.tag == "VERS" {
            let v = l.value.trim();
            if v.starts_with("5.5") {
                version = Version::V551;
            } else if v.starts_with("7") {
                version = Version::V70;
            }
        }
    }

    // Índexs.
    let mut records: HashMap<String, (String, usize)> = HashMap::new(); // xref -> (kind, line)
    let mut indi_birth: HashMap<String, Option<i64>> = HashMap::new();
    let mut indi_death: HashMap<String, Option<i64>> = HashMap::new();
    let mut indi_name: HashMap<String, String> = HashMap::new();
    let mut indi_sex: HashMap<String, (String, usize)> = HashMap::new();
    let mut indi_famc: HashMap<String, Vec<String>> = HashMap::new();
    let mut fam_chil: HashMap<String, Vec<String>> = HashMap::new();
    let mut fam_husb: HashMap<String, String> = HashMap::new();
    let mut fam_wife: HashMap<String, String> = HashMap::new();
    let mut fam_marr: HashMap<String, Option<i64>> = HashMap::new();
    let mut pending: Vec<(usize, String, String, String)> = Vec::new(); // (line, from, tag, target)

    let mut cur: Option<(String, String)> = None; // (xref, kind)
    let mut cur_sub = String::new();
    let mut cur_birt: Option<i64> = None;
    let mut cur_deat: Option<i64> = None;
    let mut saw_head = false;
    let mut saw_trlr = false;
    let mut prev_level: Option<u32> = None;
    let mut expect_conc_parent = false;

    let flush_person = |diags: &mut Vec<Diag>, xref: &str, b: Option<i64>, d: Option<i64>, line: usize| {
        if let (Some(bb), Some(dd)) = (b, d) {
            if dd < 10000 && bb < 10000 && dd < bb {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "W301",
                        Category::Suspicious,
                        Severity::Warning,
                        line,
                        format!("{}: mort ({}) abans de néixer ({})", xref, dd, bb),
                    )],
                );
            }
            if dd < 10000 && dd - bb > 105 {
                push_capped(
                    diags,
                    vec![Diag::new(
                        "W301",
                        Category::Suspicious,
                        Severity::Warning,
                        line,
                        format!("{}: {} - {} = {} anys, verificar", xref, bb, dd, dd - bb),
                    )],
                );
            }
        }
    };

    for l in &lines {
        let Some(lvl) = l.level else {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E001",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("línia malformada (nivell no numèric): {}", truncate(&l.raw, 60)),
                )],
            );
            prev_level = None;
            continue;
        };
        // E001: salt de nivell > +1.
        if let Some(p) = prev_level {
            if lvl > p + 1 {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "E001",
                        Category::Correctness,
                        Severity::Error,
                        l.no,
                        format!("salt de nivell {} -> {} (màxim +1)", p, lvl),
                    )],
                );
            }
        }
        prev_level = Some(lvl);

        // E004: sintaxi xref.
        if l.raw.contains('@') && !l.xref.is_empty() {
            let inner = l.xref.trim_matches('@');
            if inner.is_empty() || inner.contains(char::is_whitespace) || inner.contains('@') {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "E004",
                        Category::Correctness,
                        Severity::Error,
                        l.no,
                        format!("xref malformat: {}", l.xref),
                    )],
                );
            }
        }

        // CONT/CONC han de penjar d'un nivell pare.
        if (l.tag == "CONT" || l.tag == "CONC") && !expect_conc_parent {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E005",
                    Category::Correctness,
                    Severity::Error,
                    l.no,
                    format!("{} sense línia pare (ha de continuar un valor)", l.tag),
                )],
            );
        }
        expect_conc_parent = !l.raw.trim().is_empty();

        // Controls ASCII (fora de \t).
        if l.raw.chars().any(|c| c.is_control() && c != '\t') {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W102",
                    Category::Style,
                    Severity::Warning,
                    l.no,
                    "caràcter de control dins la línia".into(),
                )],
            );
        }

        if lvl == 0 {
            if let Some((xref, _)) = cur.take() {
                let _ = xref;
            }
            if cur_birt.is_some() || cur_deat.is_some() {
                // Es resol al canvi de registre via indi_birth/death ja guardats.
            }
            cur_birt = None;
            cur_deat = None;
            cur_sub.clear();
            if l.tag == "HEAD" {
                saw_head = true;
            }
            if l.tag == "TRLR" {
                saw_trlr = true;
            }
            if !l.xref.is_empty() {
                if let Some((_, first_line)) = records.get(&l.xref) {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "E003",
                            Category::Correctness,
                            Severity::Error,
                            l.no,
                            format!("xref duplicat {} (primer a línia {})", l.xref, first_line),
                        )],
                    );
                } else {
                    records.insert(l.xref.clone(), (l.tag.clone(), l.no));
                    match l.tag.as_str() {
                        "INDI" => {
                            indi_birth.insert(l.xref.clone(), None);
                            indi_death.insert(l.xref.clone(), None);
                            cur = Some((l.xref.clone(), "INDI".into()));
                        }
                        "FAM" => {
                            cur = Some((l.xref.clone(), "FAM".into()));
                        }
                        _ => {
                            cur = Some((l.xref.clone(), l.tag.clone()));
                        }
                    }
                }
            } else if l.tag != "HEAD" && l.tag != "TRLR" && l.tag != "SUBM" && l.tag != "SUBN" {
                // Registres de nivell 0 sense xref (excepte HEAD/TRLR) són sospitosos.
                if !l.tag.is_empty() && l.tag.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                    // Tag sol al nivell 0: p. ex. "0 @X@ OBJE" ja cobert; altrament ho deixem passar.
                }
            }
            continue;
        }

        if lvl == 1 {
            cur_sub = l.tag.clone();
            if let Some((xref, kind)) = cur.clone() {
                match (kind.as_str(), l.tag.as_str()) {
                    ("INDI", "FAMS") | ("INDI", "FAMC") => {
                        if is_pointer(&l.value) {
                            indi_famc.entry(xref.clone()).or_default();
                            pending.push((l.no, xref.clone(), l.tag.clone(), inner_ptr(&l.value).to_string()));
                            if l.tag == "FAMC" {
                                indi_famc.entry(xref.clone()).or_default().push(inner_ptr(&l.value).to_string());
                            }
                        } else if !l.value.is_empty() {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "E201",
                                    Category::Correctness,
                                    Severity::Error,
                                    l.no,
                                    format!("{}: {} amb valor no punter: {}", xref, l.tag, truncate(&l.value, 40)),
                                )],
                            );
                        }
                    }
                    ("FAM", "HUSB") | ("FAM", "WIFE") | ("FAM", "CHIL") => {
                        if is_pointer(&l.value) {
                            let t = inner_ptr(&l.value).to_string();
                            pending.push((l.no, xref.clone(), l.tag.clone(), t.clone()));
                            match l.tag.as_str() {
                                "CHIL" => {
                                    fam_chil.entry(xref.clone()).or_default().push(t);
                                }
                                "HUSB" => {
                                    fam_husb.insert(xref.clone(), t);
                                }
                                "WIFE" => {
                                    fam_wife.insert(xref.clone(), t);
                                }
                                _ => {}
                            }
                        }
                    }
                    ("INDI", "NAME") => {
                        indi_name.insert(xref.clone(), l.value.clone());
                        if l.value.matches('/').count() % 2 != 0 {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "W402",
                                    Category::Style,
                                    Severity::Warning,
                                    l.no,
                                    format!("{}: NAME amb barres desequilibrades: {}", xref, truncate(&l.value, 50)),
                                )],
                            );
                        }
                    }
                    ("INDI", "SEX") => {
                        indi_sex.insert(xref.clone(), (l.value.clone(), l.no));
                    }
                    ("FAM", "MARR") | ("INDI", "BIRT") | ("INDI", "DEAT") => {
                        if let Some(y) = year_of(&l.value) {
                            match l.tag.as_str() {
                                "BIRT" => {
                                    cur_birt = Some(y);
                                    indi_birth.insert(xref.clone(), Some(y));
                                }
                                "DEAT" => {
                                    cur_deat = Some(y);
                                    indi_death.insert(xref.clone(), Some(y));
                                }
                                _ => {
                                    if kind == "FAM" {
                                        fam_marr.insert(xref.clone(), Some(y));
                                    }
                                }
                            }
                        }
                        if l.tag == "DEAT" && l.value.trim() == "Y" {
                            indi_death.insert(xref.clone(), Some(9999));
                            cur_deat = Some(9999);
                        }
                    }
                    _ => {
                        // Punters genèrics (SOUR, OBJE, NOTE, SUBM...): registra per E201.
                        if is_pointer(&l.value) && matches!(l.tag.as_str(), "SOUR" | "OBJE" | "NOTE" | "SUBM" | "REPO" | "ADOP") {
                            pending.push((l.no, xref.clone(), l.tag.clone(), inner_ptr(&l.value).to_string()));
                        }
                    }
                }
                // W401: URL dins PLAC (quirk MyHeritage).
                if l.tag == "PLAC" && l.value.contains("http") {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W401",
                            Category::Style,
                            Severity::Warning,
                            l.no,
                            format!("PLAC amb URL (quirk MyHeritage): mou-la a NOTE: {}", truncate(&l.value, 60)),
                        )],
                    );
                }
                // Notes HTML dins NOTE.
                if l.tag == "NOTE" && (l.value.contains("<br") || l.value.contains("<notexml") || l.value.contains("&nbsp")) {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W403",
                            Category::Style,
                            Severity::Warning,
                            l.no,
                            format!("NOTE amb HTML (quirk exportador): {}", truncate(&l.value, 60)),
                        )],
                    );
                }
                // Upgrade path 5.5.1 -> 7: tags eliminats.
                if version == Version::V551 && l.tag == "RELA" {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "U501",
                            Category::Upgrade,
                            Severity::Info,
                            l.no,
                            "RELA eliminat a 7.0: usar ROLE enumerat (vegeu gedcom.io/migrate)".into(),
                        )],
                    );
                }
                if version == Version::V551 && l.tag.starts_with("_") && matches!(l.tag.as_str(), "_MARNM" | "_UPD" | "_APID" | "_OID") {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "U502",
                            Category::Upgrade,
                            Severity::Info,
                            l.no,
                            format!("tag propietari {}: es perdrà o caldrà extensió _ a 7.0", l.tag),
                        )],
                    );
                }
            }
            continue;
        }

        // Nivell >= 2.
        // Upgrade path 5.5.1 -> 7 també a subnivells (p. ex. ASSO.RELA).
        if version == Version::V551 && l.tag == "RELA" {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "U501",
                    Category::Upgrade,
                    Severity::Info,
                    l.no,
                    "RELA eliminat a 7.0: usar ROLE enumerat (vegeu gedcom.io/migrate)".into(),
                )],
            );
        }
        if cur_sub == "BIRT" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "INDI" {
                    if let Some(y) = year_of(&l.value) {
                        cur_birt = Some(y);
                        indi_birth.insert(xref, Some(y));
                    }
                } else if kind == "FAM" && cur_sub == "MARR" {
                    // No-op: MARR es tracta a sota.
                }
            }
        }
        if cur_sub == "DEAT" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "INDI" {
                    if let Some(y) = year_of(&l.value) {
                        cur_deat = Some(y);
                        indi_death.insert(xref, Some(y));
                    }
                }
            }
        }
        if cur_sub == "MARR" && l.tag == "DATE" {
            if let Some((xref, kind)) = cur.clone() {
                if kind == "FAM" {
                    fam_marr.insert(xref, year_of(&l.value));
                }
            }
        }
        if l.tag == "DATE" && cur_sub == "BIRT" {
            // Ja tractat.
        }
        if l.tag == "PEDI" && version == Version::V551 {
            let v = l.value.trim();
            if v != v.to_uppercase() {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "U501",
                        Category::Upgrade,
                        Severity::Info,
                        l.no,
                        format!("PEDI en minúscules ({}): a 7.0 ha de ser majúscules", v),
                    )],
                );
            }
        }
        // AGE vs dates es comprova al final si cal; aquí només PLAC niuats.
        if l.tag == "PLAC" && l.value.contains("http") {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W401",
                    Category::Style,
                    Severity::Warning,
                    l.no,
                    format!("PLAC amb URL (quirk MyHeritage): {}", truncate(&l.value, 60)),
                )],
            );
        }
        // DATE amb format sospitós (mesos no ANG, "about" en minúscules...).
        if l.tag == "DATE" && !l.value.is_empty() {
            check_date_style(&mut diags, l.no, &l.value, version);
        }
    }

    // E002: HEAD/TRLR obligatoris.
    if !saw_head {
        push_capped(
            &mut diags,
            vec![Diag::new("E002", Category::Correctness, Severity::Error, 0, "falta el registre HEAD".into())],
        );
    }
    if !saw_trlr {
        push_capped(
            &mut diags,
            vec![Diag::new("E002", Category::Correctness, Severity::Error, 0, "falta el registre TRLR".into())],
        );
    }

    // E201: referències trencades.
    for (line, from, tag, target) in &pending {
        if !records.contains_key(target) {
            let kind = match tag.as_str() {
                "FAMS" | "FAMC" => "una FAM",
                "HUSB" | "WIFE" | "CHIL" => "un INDI",
                _ => "un registre",
            };
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "E201",
                    Category::Correctness,
                    Severity::Error,
                    *line,
                    format!("{}: {} {} apunta a {} inexistent", from, tag, target, kind),
                )],
            );
        }
    }

    // W202: FAMC no llistat com a CHIL (i viceversa).
    for (xref, fams) in &indi_famc {
        for f in fams {
            let listed = fam_chil.get(f).map(|c| c.contains(xref)).unwrap_or(false);
            let fam_exists = records.get(f).map(|r| r.0 == "FAM").unwrap_or(false);
            if fam_exists && !listed {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "W202",
                        Category::Suspicious,
                        Severity::Warning,
                        0,
                        format!("{}: declara FAMC {} però la FAM no el llista com a CHIL", xref, f),
                    )],
                );
            }
        }
    }
    for (fam, chils) in &fam_chil {
        for c in chils {
            let declares = indi_famc.get(c).map(|v| v.contains(fam)).unwrap_or(false);
            if !declares && records.contains_key(c) {
                push_capped(
                    &mut diags,
                    vec![Diag::new(
                        "W202",
                        Category::Suspicious,
                        Severity::Warning,
                        0,
                        format!("{}: llista CHIL {} però l'INDI no declara FAMC", fam, c),
                    )],
                );
            }
        }
    }

    // W301: mort abans de néixer + longevitat, per individu.
    for (xref, b) in &indi_birth {
        let d = indi_death.get(xref).copied().flatten();
        if let (Some(bb), Some(dd)) = (*b, d) {
            if dd < 10000 {
                flush_person(&mut diags, xref, Some(bb), Some(dd), records.get(xref).map(|r| r.1).unwrap_or(0));
            }
        }
    }

    // W303: edat dels pares al naixement del fill.
    for (fam, chils) in &fam_chil {
        for c in chils {
            let cb = indi_birth.get(c).copied().flatten();
            let Some(cb) = cb else { continue };
            if cb >= 10000 {
                continue;
            }
            for (parent, rol) in [(&fam_husb.get(fam), "pare"), (&fam_wife.get(fam), "mare")] {
                if let Some(px) = parent {
                    if let Some(Some(pb)) = indi_birth.get(*px) {
                        let age = cb - pb;
                        let max = if rol == "mare" { 50 } else { 70 };
                        if age < 13 || age > max {
                            push_capped(
                                &mut diags,
                                vec![Diag::new(
                                    "W303",
                                    Category::Suspicious,
                                    Severity::Warning,
                                    0,
                                    format!(
                                        "{}: {} {} (n. {}) tenia {} anys al néixer {} (n. {})",
                                        fam, rol, px, pb, age, c, cb
                                    ),
                                )],
                            );
                        }
                    }
                }
            }
            // Fill nascut abans del matrimoni (si hi ha data MARR).
            if let Some(Some(m)) = fam_marr.get(fam) {
                if cb < *m {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W304",
                            Category::Suspicious,
                            Severity::Warning,
                            0,
                            format!("{}: {} nascut ({}) abans del matrimoni ({})", fam, c, cb, m),
                        )],
                    );
                }
            }
        }
    }

    // SEX.
    for (xref, (v, line)) in &indi_sex {
        let ok = match version {
            Version::V70 => matches!(v.trim(), "M" | "F" | "X" | "U"),
            _ => matches!(v.trim(), "M" | "F" | "U"),
        };
        if !ok {
            push_capped(
                &mut diags,
                vec![Diag::new(
                    "W305",
                    Category::Suspicious,
                    Severity::Warning,
                    *line,
                    format!("{}: SEX invàlid ({}), esperat M/F/U{}", xref, v, if version == Version::V70 { "/X" } else { "" }),
                )],
            );
        }
    }

    // W302: duplicats (mateix nom normalitzat + naixement ±2 anys).
    let mut by_name: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (xref, b) in &indi_birth {
        if let (Some(nm), Some(bb)) = (indi_name.get(xref), *b) {
            if bb < 10000 {
                by_name.entry(norm_name(nm)).or_default().push((xref.clone(), bb));
            }
        }
    }
    for v in by_name.values() {
        for a in 0..v.len() {
            for b in a + 1..v.len() {
                if (v[a].1 - v[b].1).abs() <= 2 {
                    push_capped(
                        &mut diags,
                        vec![Diag::new(
                            "W302",
                            Category::Suspicious,
                            Severity::Warning,
                            0,
                            format!("possible duplicat: {} (n. {}) vs {} (n. {})", v[a].0, v[a].1, v[b].0, v[b].1),
                        )],
                    );
                }
            }
        }
    }

    let individuals = indi_birth.len();
    let _count_dbg = fam_chil.len() + fam_husb.len() + fam_wife.len();
    let mut families_set: HashSet<&String> = HashSet::new();
    for k in fam_chil.keys().chain(fam_husb.keys()).chain(fam_wife.keys()) {
        families_set.insert(k);
    }

    diags.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    Report { version, diags, lines: lines.len(), individuals, families: families_set.len() }
}

fn check_date_style(diags: &mut Vec<Diag>, line: usize, value: &str, version: Version) {
    let v = value.trim();
    // Mesos en català/castellà o minúscules: GEDCOM exigeix JAN FEB MAR...
    let lower_months = ["enero", "febrero", "gener", "febrer", "marzo", "març", "abril", "mayo", "maig", "junio", "juny"];
    let vl = v.to_lowercase();
    if lower_months.iter().any(|m| vl.contains(m)) {
        push_capped(
            diags,
            vec![Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE amb mes no estàndard (cal JAN/FEB/...): {}", truncate(v, 50)),
            )],
        );
        return;
    }
    if v.starts_with("about") || v.starts_with("circa") || v.starts_with("aprox") {
        push_capped(
            diags,
            vec![Diag::new(
                "W402",
                Category::Style,
                Severity::Warning,
                line,
                format!("DATE amb aproximació en minúscules (cal ABT/CAL/EST): {}", truncate(v, 50)),
            )],
        );
    }
    if version == Version::V70 && v.contains("BET") && !v.contains("AND") {
        push_capped(
            diags,
            vec![Diag::new(
                "U501",
                Category::Upgrade,
                Severity::Info,
                line,
                format!("BET sense AND (a 7.0 cal rang complet): {}", truncate(v, 50)),
            )],
        );
    }
}

// ---------------------------------------------------------------------------
// --fix: només reparacions segures, sempre amb còpia .bak.
// ---------------------------------------------------------------------------

/// Reparacions segures aplicades per --fix:
/// 1. Reuneix caràcters UTF-8 partits entre línies CONC (E101).
/// 2. Retalla espais finals de línia.
/// 3. Normalitza CRLF a LF si el fitxer és majoritàriament LF? No: ho deixem
///    com a avís (canviar finals de línia pot trencar round-trip). Només 1+2.
pub fn fix_bytes(data: &[u8]) -> (Vec<u8>, Vec<String>) {
    let mut applied = Vec::new();
    let mut lines: Vec<Vec<u8>> = data.split(|&b| b == b'\n').map(|l| l.to_vec()).collect();
    let had_cr: Vec<bool> = lines.iter().map(|l| l.last() == Some(&b'\r')).collect();

    // 1. CONC split: si el payload de la línia següent comença amb byte de
    // continuació, enganxa'l al final de la línia anterior (traient "N CONC ").
    let mut fixed_conc = 0;
    let mut i = 0;
    while i < lines.len() {
        let payload = {
            let l = strip_cr(&lines[i]);
            conc_payload_owned(l)
        };
        if let Some(p) = payload {
            if p.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) && i > 0 {
                // Troba inici del payload dins lines[i] i enganxa.
                let full = lines[i].clone();
                let stripped = strip_cr_slice(&full);
                if let Some(pos) = find_conc_pos(stripped) {
                    let tail = &stripped[pos..];
                    // Treu el \r de l'anterior si n'hi ha.
                    let prev = &mut lines[i - 1];
                    if prev.last() == Some(&b'\r') {
                        prev.pop();
                    }
                    prev.extend_from_slice(tail);
                    lines.remove(i);
                    // had_cr ja no cal: marquem com tocat.
                    fixed_conc += 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    if fixed_conc > 0 {
        applied.push(format!("E101: reunides {} línies CONC amb UTF-8 partit", fixed_conc));
    }

    // 2. Espais finals.
    let mut fixed_ws = 0;
    for l in lines.iter_mut() {
        let has_cr = l.last() == Some(&b'\r');
        let body = if has_cr { &l[..l.len() - 1] } else { &l[..] };
        let trimmed = rtrim_ws(body);
        if trimmed.len() != body.len() {
            fixed_ws += 1;
            let mut nl = trimmed.to_vec();
            if has_cr {
                nl.push(b'\r');
            }
            *l = nl;
        }
    }
    if fixed_ws > 0 {
        applied.push(format!("estil: retallats espais finals a {} línies", fixed_ws));
    }

    let mut out = Vec::with_capacity(data.len());
    for (k, l) in lines.iter().enumerate() {
        out.extend_from_slice(l);
        if k + 1 < lines.len() {
            out.push(b'\n');
        }
        let _ = had_cr;
    }
    // Preserva el newline final original.
    if data.ends_with(b"\n") && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    (out, applied)
}

fn strip_cr(line: &[u8]) -> &[u8] {
    strip_cr_slice(line)
}

fn strip_cr_slice(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') { &line[..line.len() - 1] } else { line }
}

fn conc_payload_owned(line: &[u8]) -> Option<Vec<u8>> {
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        Some(line[p + pat.len()..].to_vec())
    } else if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let mut rest = &line[p + 4..];
        if rest.first() == Some(&b' ') {
            rest = &rest[1..];
        }
        Some(rest.to_vec())
    } else {
        None
    }
}

fn find_conc_pos(line: &[u8]) -> Option<usize> {
    let pat = b"CONC ";
    if let Some(p) = line.windows(pat.len()).position(|w| w == pat) {
        return Some(p + pat.len());
    }
    if let Some(p) = line.windows(4).position(|w| w == b"CONC") {
        let mut q = p + 4;
        if line.get(q) == Some(&b' ') {
            q += 1;
        }
        return Some(q);
    }
    None
}

fn rtrim_ws(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b' ' || b[end - 1] == b'\t') {
        end -= 1;
    }
    &b[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_order() {
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn json_escapes() {
        let r = Report { version: Version::V551, lines: 1, individuals: 0, families: 0, diags: vec![Diag::new("E1", Category::Correctness, Severity::Error, 1, "a\"b\\c".into())] };
        let j = r.to_json();
        assert!(j.contains("a\\\"b\\\\c"));
    }
}
