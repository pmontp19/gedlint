use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Sev {
    Error,
    Warning,
    Info,
}
impl Sev {
    fn tag(&self) -> &'static str {
        match self {
            Sev::Error => "ERROR",
            Sev::Warning => "WARN ",
            Sev::Info => "INFO ",
        }
    }
    fn json(&self) -> &'static str {
        match self {
            Sev::Error => "error",
            Sev::Warning => "warning",
            Sev::Info => "info",
        }
    }
}

#[derive(Debug)]
struct Diag {
    code: &'static str,
    sev: Sev,
    line: usize,
    msg: String,
}

#[derive(Debug, Default)]
struct Indi {
    givn: String,
    surn: String,
    sex: String,
    birt: String,
    birtplac: String,
    deat: String,
    deatplac: String,
    died: bool,
    age: String,
    fams: Vec<String>,
    famc: Vec<String>,
    adop: bool,
    line: usize,
}

#[derive(Debug, Default)]
struct Fam {
    husb: String,
    wife: String,
    chil: Vec<String>,
    marr: String,
    marrplac: String,
    div: bool,
    line: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fix = args.iter().any(|a| a == "--fix");
    let mut format_json = false;
    for a in &args {
        if a == "--format=json" || a == "--format json" {
            format_json = true;
        }
        if let Some(v) = a.strip_prefix("--format=") {
            if v == "json" {
                format_json = true;
            }
        }
    }
    // suport: --format json (dos args)
    for w in args.windows(2) {
        if w[0] == "--format" && w[1] == "json" {
            format_json = true;
        }
    }
    let path = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--") && *a != "json")
        .cloned();
    let Some(path) = path else {
        eprintln!("ús: gedlint [--fix] [--format json] <fitxer.ged>");
        std::process::exit(2);
    };

    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            if format_json {
                println!(
                    "{{\"file\":{},\"summary\":{{\"indi\":0,\"fam\":0,\"sour\":0,\"errors\":1,\"warnings\":0,\"infos\":0,\"urls_plac_buri\":0,\"div\":0,\"adop\":0}},\"diagnostics\":[{{\"code\":\"E000\",\"severity\":\"error\",\"line\":0,\"message\":{}}}]}}",
                    json_str(&path),
                    json_str(&format!("no es pot llegir: {e}"))
                );
            } else {
                eprintln!("ERROR [E000] -: no es pot llegir {path}: {e}");
            }
            std::process::exit(2);
        }
    };
    let mut diags: Vec<Diag> = Vec::new();

    // ---- E101: UTF-8 partit per CONC (byte de continuació a inici de línia)
    let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    let mut bad_lines: Vec<usize> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let l = if l.last() == Some(&b'\r') {
            &l[..l.len() - 1]
        } else {
            l
        };
        if l.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) {
            bad_lines.push(i);
            continue;
        }
        // cas real MyHeritage: "N CONC <byte continuació>..." (caràcter partit)
        if let Some(pos) = find_conc(l) {
            if l[pos..].first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) {
                bad_lines.push(i);
            }
        }
    }
    if std::str::from_utf8(&bytes).is_err() && bad_lines.is_empty() {
        // UTF-8 invàlid sense signatura CONC clara: marca igual E101 a línia 0
        diags.push(Diag {
            code: "E101",
            sev: Sev::Error,
            line: 0,
            msg: "el fitxer NO és UTF-8 vàlid (executa amb --fix)".into(),
        });
    }
    if !bad_lines.is_empty() {
        diags.push(Diag {
            code: "E101",
            sev: Sev::Error,
            line: bad_lines[0] + 1,
            msg: format!(
                "{} línies comencen amb byte de continuació UTF-8 (MyHeritage parteix caràcters entre línies CONC)",
                bad_lines.len()
            ),
        });
    }
    if fix && !bad_lines.is_empty() {
        let ends_nl = bytes.last() == Some(&b'\n');
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut repaired = 0;
        let last = lines.len().saturating_sub(1);
        for (i, l) in lines.iter().enumerate() {
            // evita fantasma EOF: split("a\n") -> ["a", ""]
            if i == last && l.is_empty() && ends_nl {
                continue;
            }
            let orig: &[u8] = if l.last() == Some(&b'\r') {
                &l[..l.len() - 1]
            } else {
                l
            };
            if bad_lines.contains(&i) {
                match find_conc(orig) {
                    Some(pos) if i > 0 && !out.is_empty() => {
                        out.pop(); // treu el \n previ per reenganxar el caràcter partit
                        out.extend_from_slice(&orig[pos..]);
                        out.push(b'\n');
                        repaired += 1;
                    }
                    _ => {
                        // línia 0 o dany sense signatura CONC: no reenganxar (corrompria
                        // el registre previ); conserva tal qual + avís manual
                        diags.push(Diag {
                            code: "W103",
                            sev: Sev::Warning,
                            line: i + 1,
                            msg: "dany UTF-8 no reparable automàticament (sense CONC reenganxable): revisió manual".into(),
                        });
                        out.extend_from_slice(orig);
                        out.push(b'\n');
                    }
                }
            } else {
                out.extend_from_slice(orig);
                out.push(b'\n');
            }
        }
        // restaura EOF original (sense \n fantasma)
        if !ends_nl && out.last() == Some(&b'\n') {
            out.pop();
        }
        if repaired == 0 {
            // res reparable: no escriure, no .bak, no F001 mentider
        } else {
            match write_repaired(&path, &out) {
                Ok(_) => diags.push(Diag {
                    code: "F001",
                    sev: Sev::Info,
                    line: 0,
                    msg: format!("--fix: {repaired} línies reenganxades (còpia .bak)"),
                }),
                Err(e) => diags.push(Diag {
                    code: "E102",
                    sev: Sev::Error,
                    line: 0,
                    msg: format!("--fix: no s'ha pogut escriure: {e}"),
                }),
            }
        }
    }
    // Nota: amb bad_lines el diagnòstic E101 específic ja s'ha emès a dalt;
    // el genèric de línia 0 només cobreix UTF-8 invàlid sense signatura CONC.

    // ---- text per anàlisi semàntica (lossy per poder analitzar encara que E101)
    let text = String::from_utf8_lossy(&bytes).to_string();
    let tl: Vec<&str> = text.lines().collect();

    // ---- parser dues passades: col·lecció primer, checks després (evita falsos positius forward-ref)
    let mut indi: HashMap<String, Indi> = HashMap::new();
    let mut fams: HashMap<String, Fam> = HashMap::new();
    let mut sour_count: usize = 0;
    let mut cur_xref: Option<String> = None;
    let mut cur_kind: Option<char> = None; // 'I' 'F' 'S'
    let mut cur_sub = String::new();
    let mut url_plac: usize = 0;

    for (i, l) in tl.iter().enumerate() {
        let line_no = i + 1;
        let l = l.trim_start();
        if l.is_empty() {
            continue;
        }
        let mut it = l.splitn(3, char::is_whitespace);
        let lvl: Option<u32> = it.next().and_then(|x| x.parse().ok());
        let Some(lvl) = lvl else { continue };
        let rest = it.next().unwrap_or("");
        let tail_raw = it.next().unwrap_or("").trim();
        let (ptr, tag, val) = if rest.starts_with('@') {
            match rest[1..].find('@').map(|p| p + 1) {
                Some(p) => {
                    let pstr = &rest[..p + 1];
                    // tail_raw comença amb TAG
            let (t, _v) = match tail_raw.split_once([' ', '\t']) {
                        Some((t, v)) => (t, v.trim()),
                        None => (tail_raw, ""),
                    };
                    (pstr, t, _v)
                }
                None => ("", rest, tail_raw),
            }
        } else {
            match tail_raw.split_once([' ', '\t']) {
                Some((t, v)) => {
                    let _ = (t, v);
                }
                None => {}
            };
            // Nota: rest és el TAG quan no hi ha ptr; simplifiquem:
            ("" as &str, rest, tail_raw)
        };
        let _ = val;

        if lvl == 0 {
            cur_xref = None;
            cur_kind = None;
            cur_sub.clear();
            if ptr.starts_with('@') {
                match tag {
                    "INDI" => {
                        cur_xref = Some(ptr.to_string());
                        cur_kind = Some('I');
                        indi.entry(ptr.to_string()).or_insert_with(|| Indi {
                            line: line_no,
                            ..Default::default()
                        });
                    }
                    "FAM" => {
                        cur_xref = Some(ptr.to_string());
                        cur_kind = Some('F');
                        fams.entry(ptr.to_string()).or_insert_with(|| Fam {
                            line: line_no,
                            ..Default::default()
                        });
                    }
                    "SOUR" => {
                        sour_count += 1;
                        cur_xref = Some(ptr.to_string());
                        cur_kind = Some('S');
                    }
                    _ => {}
                }
            }
            continue;
        }
        let (Some(xref), Some(kind)) = (cur_xref.clone(), cur_kind) else {
            continue;
        };
        if lvl == 1 {
            cur_sub = tag.to_string();
            match kind {
                'I' => {
                    if let Some(r) = indi.get_mut(&xref) {
                        match tag {
                            "FAMS" => r.fams.push(tail_raw.to_string()),
                            "FAMC" => r.famc.push(tail_raw.to_string()),
                            "ADOP" => r.adop = true,
                            "DEAT" => {
                                r.died = true;
                                if tail_raw == "Y" && r.deat.is_empty() {
                                    // mort sense data: sentinel per comparacions
                                }
                            }
                            "SEX" => r.sex = tail_raw.to_string(),
                            _ => {}
                        }
                    }
                }
                'F' => {
                    if let Some(r) = fams.get_mut(&xref) {
                        match tag {
                            "HUSB" => r.husb = tail_raw.to_string(),
                            "WIFE" => r.wife = tail_raw.to_string(),
                            "CHIL" => r.chil.push(tail_raw.to_string()),
                            "DIV" => r.div = true,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            // W401: URL dins PLAC/BURI (quirk MyHeritage)
            if (tag == "PLAC" || tag == "BURI") && tail_raw.contains("http") {
                url_plac += 1;
                diags.push(Diag {
                    code: "W401",
                    sev: Sev::Warning,
                    line: line_no,
                    msg: format!(
                        "PLAC amb URL (quirk MyHeritage): mou-la a NOTE: {}",
                        truncate(tail_raw, 60)
                    ),
                });
            }
            continue;
        }
        if lvl == 2 {
            match kind {
                'I' => {
                    if let Some(r) = indi.get_mut(&xref) {
                        match (cur_sub.as_str(), tag) {
                            ("NAME", "GIVN") => r.givn = tail_raw.to_string(),
                            ("NAME", "SURN") => r.surn = tail_raw.to_string(),
                            ("BIRT", "DATE") => r.birt = tail_raw.to_string(),
                            ("BIRT", "PLAC") => r.birtplac = tail_raw.to_string(),
                            ("DEAT", "DATE") => {
                                r.died = true;
                                r.deat = tail_raw.to_string();
                            }
                            ("DEAT", "PLAC") => r.deatplac = tail_raw.to_string(),
                            ("DEAT", "AGE") => r.age = tail_raw.to_string(),
                            _ => {}
                        }
                    }
                }
                'F' => {
                    if let Some(r) = fams.get_mut(&xref) {
                        if cur_sub == "MARR" {
                            match tag {
                                "DATE" => r.marr = tail_raw.to_string(),
                                "PLAC" => r.marrplac = tail_raw.to_string(),
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
            // nivell 2: PLAC/BURI sota BIRT/DEAT (MARR ja cobert a nivell 1);
            // paritat gedcheck: compta PLAC|BURI a qualsevol nivell
            if tag == "PLAC" || tag == "BURI" {
                if tail_raw.contains("http") && cur_sub != "MARR" {
                    url_plac += 1;
                    diags.push(Diag {
                        code: "W401",
                        sev: Sev::Warning,
                        line: line_no,
                        msg: format!(
                            "{tag} amb URL (quirk MyHeritage): mou-la a NOTE: {}",
                            truncate(tail_raw, 60)
                        ),
                    });
                }
            }
        }
    }

    // ---- E201: referències trencades (segona passada, sense falsos forward-ref)
    let mut e201: Vec<Diag> = Vec::new();
    for (fid, f) in &fams {
        if !f.husb.is_empty() && !indi.contains_key(&f.husb) {
            e201.push(Diag {
                code: "E201",
                sev: Sev::Error,
                line: f.line,
                msg: format!("{fid}: HUSB {} apunta a un INDI inexistent", f.husb),
            });
        }
        if !f.wife.is_empty() && !indi.contains_key(&f.wife) {
            e201.push(Diag {
                code: "E201",
                sev: Sev::Error,
                line: f.line,
                msg: format!("{fid}: WIFE {} apunta a un INDI inexistent", f.wife),
            });
        }
        for c in &f.chil {
            if !indi.contains_key(c) {
                e201.push(Diag {
                    code: "E201",
                    sev: Sev::Error,
                    line: f.line,
                    msg: format!("{fid}: CHIL {c} apunta a un INDI inexistent"),
                });
            }
        }
    }
    for (iid, r) in &indi {
        for x in &r.fams {
            if !fams.contains_key(x) {
                e201.push(Diag {
                    code: "E201",
                    sev: Sev::Error,
                    line: r.line,
                    msg: format!("{iid}: FAMS {x} apunta a una FAM inexistent"),
                });
            }
        }
        for x in &r.famc {
            if !fams.contains_key(x) {
                e201.push(Diag {
                    code: "E201",
                    sev: Sev::Error,
                    line: r.line,
                    msg: format!("{iid}: FAMC {x} apunta a una FAM inexistent"),
                });
            }
        }
    }
    diags.append(&mut e201);

    // ---- W202: FAMC no llistat com a CHIL (amb nota ADOP com gedcheck.py)
    for (iid, r) in &indi {
        for f in &r.famc {
            if let Some(fam) = fams.get(f) {
                if !fam.chil.contains(iid) {
                    diags.push(Diag {
                        code: "W202",
                        sev: Sev::Warning,
                        line: r.line,
                        msg: format!(
                            "{iid} {} {}: FAMC {f} no el llista com a fill (ADOP={})",
                            r.givn,
                            r.surn,
                            if r.adop { "True" } else { "False" }
                        ),
                    });
                }
            }
        }
    }

    // ---- W301 + E301: dates impossibles, longevitat, AGE vs dates (paritat gedcheck)
    for (iid, r) in &indi {
        let by = year(&r.birt);
        let dy = if r.died { year(&r.deat) } else { None };
        if let (Some(b), Some(d)) = (by, dy) {
            if d < b {
                diags.push(Diag {
                    code: "E301",
                    sev: Sev::Error,
                    line: r.line,
                    msg: format!("{iid} {} {}: mort abans de neixer", r.givn, r.surn),
                });
            }
            if d - b > 105 {
                diags.push(Diag {
                    code: "W301",
                    sev: Sev::Warning,
                    line: r.line,
                    msg: format!(
                        "{iid} {} {}: {} anys, verificar",
                        r.givn,
                        r.surn,
                        d - b
                    ),
                });
            }
            if !r.age.is_empty() {
                if let Some(a) = first_int(&r.age) {
                    if (d - b - a).abs() > 2 {
                        diags.push(Diag {
                            code: "W303",
                            sev: Sev::Warning,
                            line: r.line,
                            msg: format!(
                                "{iid} {} {}: AGE mort={} pero dates donen {}",
                                r.givn,
                                r.surn,
                                r.age,
                                d - b
                            ),
                        });
                    }
                }
            }
        }
    }

    // ---- W304/W305: edats pares + fill abans matrimoni + E302 nascut després mort mare
    for (fid, f) in &fams {
        let my = year(&f.marr);
        let husb = f.husb.clone();
        let wife = f.wife.clone();
        let hy = indi.get(&husb).and_then(|h| year(&h.birt));
        let wy = indi.get(&wife).and_then(|w| year(&w.birt));
        let wdy = indi
            .get(&wife)
            .filter(|w| w.died)
            .and_then(|w| year(&w.deat));
        let hg = indi.get(&husb).map(|h| h.givn.clone()).unwrap_or_default();
        for c in &f.chil {
            let Some(ci) = indi.get(c) else { continue };
            let cy = year(&ci.birt);
            let Some(cy) = cy else { continue };
            if let Some(my) = my {
                if cy < my - 2 {
                    diags.push(Diag {
                        code: "W304",
                        sev: Sev::Warning,
                        line: f.line,
                        msg: format!(
                            "{fid}: {c} {} {} (n. {}) nascut abans del matrimoni ({})",
                            ci.givn, ci.surn, ci.birt, f.marr
                        ),
                    });
                }
            }
            if let Some(wy) = wy {
                let d = cy - wy;
                if d < 14 || d > 55 {
                    let wg = indi
                        .get(&wife)
                        .map(|w| w.givn.clone())
                        .unwrap_or_default();
                    diags.push(Diag {
                        code: "W305",
                        sev: Sev::Warning,
                        line: f.line,
                        msg: format!("{fid}: mare {wg} tindria {d} anys al neixer {c}"),
                    });
                }
            }
            if let Some(hy) = hy {
                let d = cy - hy;
                if d < 14 || d > 78 {
                    diags.push(Diag {
                        code: "W305",
                        sev: Sev::Warning,
                        line: f.line,
                        msg: format!("{fid}: pare {hg} tindria {d} anys al neixer {c}"),
                    });
                }
            }
            if let Some(wdy) = wdy {
                if cy > wdy {
                    diags.push(Diag {
                        code: "E302",
                        sev: Sev::Error,
                        line: f.line,
                        msg: format!("{fid}: {c} nascut despres de la mort de la mare"),
                    });
                }
            }
        }
    }

    // ---- W302: duplicats (cognom+nom, naixement ±2, com gedcheck)
    {
        use std::collections::HashMap as M;
        let mut groups: M<(String, String), Vec<(&String, &Indi)>> = M::new();
        for (iid, r) in &indi {
            groups
                .entry((r.surn.to_lowercase(), r.givn.to_lowercase()))
                .or_default()
                .push((iid, r));
        }
        for ((surn, givn), g) in &groups {
            if g.len() < 2 {
                continue;
            }
            for a in 0..g.len() {
                for b in (a + 1)..g.len() {
                    let (i1, x1) = g[a];
                    let (i2, x2) = g[b];
                    if let (Some(b1), Some(b2)) = (year(&x1.birt), year(&x2.birt)) {
                        if (b1 - b2).abs() <= 2 {
                            diags.push(Diag {
                                code: "W302",
                                sev: Sev::Warning,
                                line: x1.line,
                                msg: format!(
                                    "Possible duplicat: {i1} vs {i2} ({surn} {givn})"
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // ---- W306: FAM duplicades (mateix marit + data matrimoni)
    {
        let mut byh: HashMap<(String, String), Vec<&String>> = HashMap::new();
        for (fid, f) in &fams {
            if !f.husb.is_empty() {
                byh.entry((f.husb.clone(), f.marr.clone()))
                    .or_default()
                    .push(fid);
            }
        }
        for ((husb, marr), fids) in &byh {
            if fids.len() > 1 {
                let list: Vec<&str> = fids.iter().map(|s| s.as_str()).collect();
                diags.push(Diag {
                    code: "W306",
                    sev: Sev::Warning,
                    line: 0,
                    msg: format!(
                        "Families duplicades possibles: {} (marit {husb}, data '{marr}')",
                        list.join(", ")
                    ),
                });
            }
        }
    }

    // ---- W307: marit i muller morts el mateix dia
    for (fid, f) in &fams {
        let h = indi.get(&f.husb);
        let w = indi.get(&f.wife);
        if let (Some(h), Some(w)) = (h, w) {
            if h.died && w.died {
                if let (Some(d1), Some(d2)) = (full_date(&h.deat), full_date(&w.deat)) {
                    if d1 == d2 {
                        diags.push(Diag {
                            code: "W307",
                            sev: Sev::Warning,
                            line: f.line,
                            msg: format!(
                                "{fid}: marit i muller morts el mateix dia ({}): verificar",
                                h.deat
                            ),
                        });
                    }
                }
            }
        }
    }

    // ---- sortida
    diags.sort_by(|x, y| {
        x.sev
            .cmp(&y.sev)
            .then(x.code.cmp(y.code))
            .then(x.line.cmp(&y.line))
    });
    let (mut e, mut w, mut inf) = (0, 0, 0);
    for d in &diags {
        match d.sev {
            Sev::Error => e += 1,
            Sev::Warning => w += 1,
            Sev::Info => inf += 1,
        }
    }

    let div_count = fams.values().filter(|f| f.div).count();
    let adop_count = indi.values().filter(|r| r.adop).count();

    if format_json {
        let mut out = String::from("{\"file\":");
        out.push_str(&json_str(&path));
        out.push_str(",\"summary\":{\"indi\":");
        out.push_str(&indi.len().to_string());
        out.push_str(",\"fam\":");
        out.push_str(&fams.len().to_string());
        out.push_str(",\"sour\":");
        out.push_str(&sour_count.to_string());
        out.push_str(",\"errors\":");
        out.push_str(&e.to_string());
        out.push_str(",\"warnings\":");
        out.push_str(&w.to_string());
        out.push_str(",\"infos\":");
        out.push_str(&inf.to_string());
        out.push_str(",\"urls_plac_buri\":");
        out.push_str(&url_plac.to_string());
        out.push_str(",\"div\":");
        out.push_str(&div_count.to_string());
        out.push_str(",\"adop\":");
        out.push_str(&adop_count.to_string());
        out.push_str("},\"diagnostics\":[");
        for (i, d) in diags.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"code\":");
            out.push_str(&json_str(d.code));
            out.push_str(",\"severity\":");
            out.push_str(&json_str(d.sev.json()));
            out.push_str(",\"line\":");
            out.push_str(&d.line.to_string());
            out.push_str(",\"message\":");
            out.push_str(&json_str(&d.msg));
            out.push('}');
        }
        out.push_str("]}");
        println!("{out}");
    } else {
        for d in &diags {
            let loc = if d.line > 0 {
                format!("línia {}", d.line)
            } else {
                "-".into()
            };
            println!("{} [{:0>4}] {loc}: {}", d.sev.tag(), d.code, d.msg);
        }
        println!("\n{path}: INDI {}  FAM {}  SOUR {sour_count} | {e} errors, {w} avisos, {inf} infos | URLs a PLAC/BURI: {url_plac}", indi.len(), fams.len());
        let divs: Vec<&String> = fams
            .iter()
            .filter(|(_, f)| f.div)
            .map(|(k, _)| k)
            .collect();
        let adops: Vec<&String> = indi
            .iter()
            .filter(|(_, r)| r.adop)
            .map(|(k, _)| k)
            .collect();
        if !divs.is_empty() || !adops.is_empty() {
            println!(
                "DIV: {:?}  ADOP: {:?}",
                divs.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                adops.iter().map(|s| s.as_str()).collect::<Vec<_>>()
            );
        }
    }
    // silencia warnings de camps no usats en stats
    let _ = HashSet::<String>::new();
    std::io::stdout().flush().ok();
    std::process::exit(if e > 0 {
        2
    } else if w > 0 {
        1
    } else {
        0
    });
}

fn year(s: &str) -> Option<i64> {
    // paritat gedcheck: \b(1[5-9]\d{2}|20\d{2})\b, primer (o mín si >1)
    let mut found: Vec<i64> = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        // \b de Python: \w = [A-Za-z0-9_]; el _ NO és frontera
        let is_boundary_before =
            i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let tok = &s[i..j];
            let is_boundary_after =
                j >= b.len() || !(b[j].is_ascii_alphanumeric() || b[j] == b'_');
            if tok.len() == 4 && is_boundary_before && is_boundary_after {
                if let Ok(y) = tok.parse::<i64>() {
                    if (1500..=2099).contains(&y) {
                        found.push(y);
                    }
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if found.is_empty() {
        return None;
    }
    if found.len() == 1 {
        return Some(found[0]);
    }
    found.into_iter().min()
}

fn first_int(s: &str) -> Option<i64> {
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            break;
        }
    }
    if cur.is_empty() {
        None
    } else {
        cur.parse().ok()
    }
}

fn full_date(s: &str) -> Option<(String, String, String)> {
    // cerca "DD MMM YYYY" dins la cadena (tolera prefixos AFT/BEF/ABT)
    let toks: Vec<&str> = s.split_whitespace().collect();
    if toks.len() < 3 {
        return None;
    }
    for w in toks.windows(3) {
        // paritat gedcheck: mes en MAJÚSCULES ([A-Z]{3}); "25 jan 1909" NO casa
        if w[0].chars().all(|c| c.is_ascii_digit())
            && w[0].len() <= 2
            && w[1].len() == 3
            && w[1].chars().all(|c| c.is_ascii_uppercase())
            && w[2].len() == 4
            && w[2].chars().all(|c| c.is_ascii_digit())
        {
            return Some((
                w[0].to_string(),
                w[1].to_string(),
                w[2].to_string(),
            ));
        }
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    // tall per chars, no bytes: &s[..n] fa panic si n cau dins un multibyte
    if s.chars().count() <= n {
        s.into()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

fn find_conc(l: &[u8]) -> Option<usize> {
    let pat = b"CONC ";
    l.windows(pat.len())
        .position(|w| w == pat)
        .map(|p| p + pat.len())
}

fn write_repaired(path: &str, data: &[u8]) -> std::io::Result<()> {
    fs::copy(path, format!("{path}.bak"))?;
    fs::write(path, data)
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
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
    o.push('"');
    o
}
