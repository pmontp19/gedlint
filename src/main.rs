use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;

#[derive(Debug)]
struct Diag { code: &'static str, sev: Sev, line: usize, msg: String }
#[derive(Debug, PartialEq, PartialOrd)]
enum Sev { Error, Warning, Info }
impl Sev { fn tag(&self) -> &'static str { match self { Sev::Error => "ERROR", Sev::Warning => "WARN ", Sev::Info => "INFO " } } }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut fix = false;
    let path = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned();
    fix = args.iter().any(|a| a == "--fix");
    let Some(path) = path else { eprintln!("ús: gedlint [--fix] <fitxer.ged>"); std::process::exit(2) };

    let bytes = fs::read(&path).expect("no es pot llegir");
    let mut diags: Vec<Diag> = Vec::new();

    // ---- E101: UTF-8 invalid (byte de continuacio a inici de linia = seq parted per CONC)
    let mut fixed = bytes.clone();
    let mut fixes = 0;
    let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    let mut bad_lines: Vec<usize> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let l = if l.last() == Some(&b'\r') { &l[..l.len()-1] } else { l };
        if l.first().map(|b| (0x80..=0xBF).contains(b)).unwrap_or(false) {
            bad_lines.push(i);
        }
    }
    if !bad_lines.is_empty() {
        diags.push(Diag{code:"E101", sev:Sev::Error, line: bad_lines[0]+1,
            msg: format!("{} línies comencen amb byte de continuació UTF-8 (MyHeritage parteix caràcters entre línies CONC)", bad_lines.len())});
    }
    // reparacio: ajunta prefix trencat + continuacio
    if fix && !bad_lines.is_empty() {
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut skip_next_is_cont = false;
        for (i, l) in lines.iter().enumerate() {
            let mut l = if l.last() == Some(&b'\r') { &l[..l.len()-1] } else { *l };
            if bad_lines.contains(&i) && i > 0 {
                // treu "N CONC " del inici i enganxa
                if let Some(pos) = find_conc(l) { l = &l[pos..]; }
                out.pop(); // treu \r residual? no: treu el \n previ
                out.extend_from_slice(l);
            } else {
                out.extend_from_slice(l);
                out.push(b'\n');
            }
            let _ = skip_next_is_cont; let _ = &mut fixes;
        }
        // repara també prefixes multibyte incomplets a final de linia + CONC seguent (generic)
        fixed = out;
        let _ = write_repaired(&path, &fixed, &mut fixes);
        diags.push(Diag{code:"F001", sev:Sev::Info, line:0, msg:"--fix: fitxer reparat escrit (còpia .bak)".into()});
    } else if !bad_lines.is_empty() {
        // sense --fix: comprova si decodede utf8 falla
        if std::str::from_utf8(&bytes).is_err() {
            diags.push(Diag{code:"E101", sev:Sev::Error, line:0, msg:"el fitxer NO és UTF-8 vàlid (executa amb --fix)".into()});
        }
    }
    let _ = &mut fixed; let _ = fixes;

    // ---- text per analisi semantica
    let text = String::from_utf8_lossy(&bytes).to_string();
    let tl: Vec<&str> = text.lines().collect();

    // ---- parser: registres i referencies
    let mut indi: HashMap<String, (Option<i64>, Option<i64>)> = HashMap::new(); // xref -> (birt, deat)
    let mut fams: HashSet<String> = HashSet::new();
    let mut sour: HashSet<String> = HashSet::new();
    let mut fam_chil: HashMap<String, Vec<String>> = HashMap::new();
    let mut indi_famc: HashMap<String, Vec<String>> = HashMap::new();
    let mut indi_fams: HashMap<String, Vec<String>> = HashMap::new();
    let mut indi_name: HashMap<String, String> = HashMap::new();
    let mut cur: Option<(String, &'static str)> = None;
    let mut cur_sub = String::new();
    let mut birt: Option<i64> = None; let mut deat: Option<i64> = None;
    let mut died = false;

    for (i, l) in tl.iter().enumerate() {
        let line_no = i + 1;
        let mut it = l.splitn(3, char::is_whitespace);
        let lvl: Option<u32> = it.next().and_then(|x| x.parse().ok());
        let Some(lvl) = lvl else { continue };
        let rest = it.next().unwrap_or("");
        let (ptr, tail) = if rest.starts_with('@') {
            match rest.find('@', 1) {
                Some(p) => (&rest[..p+1], rest[p+1..].trim_start()),
                None => ("", rest),
            }
        } else { ("", rest) };
        let (tag, val) = match tail.split_once(' ') { Some((t,v)) => (t, v.trim()), None => (tail, "") };
        if lvl == 0 {
            if let Some((xref, kind)) = &cur {
                if let (Some(b), Some(d)) = (birt, deat) { if d < b {
                    diags.push(Diag{code:"W301", sev:Sev::Warning, line:line_no,
                        msg:format!("{xref} ({kind}): mort ({deat:?}) abans de neixer ({birt:?})")} });
                } }
                if let Some(d) = deat { if let Some(b) = birt { if d - b > 105 {
                    diags.push(Diag{code:"W301", sev:Sev::Warning, line:line_no,
                        msg:format!("{xref} ({kind}): {}/{} = {} anys, verificar", birt, deat, d-b)} });
                } }
            }
            cur = None; birt = None; deat = None; died = false;
            if ptr.starts_with('@') {
                match tag {
                    "INDI" => { cur = Some((ptr.to_string(), "INDI")); indi.entry(ptr.to_string()).or_insert((None, None)); }
                    "FAM" => { cur = Some((ptr.to_string(), "FAM")); fams.insert(ptr.to_string()); }
                    "SOUR" => { cur = Some((ptr.to_string(), "SOUR")); sour.insert(ptr.to_string()); }
                    _ => {}
                }
            }
            cur_sub.clear();
            continue;
        }
        if lvl == 1 {
            cur_sub = tag.to_string();
            if let Some((xref, kind)) = &cur {
                match (kind.as_ref(), tag) {
                    ("INDI", "FAMS") => indi_fams.entry(xref.clone()).or_default().push(val.to_string()),
                    ("INDI", "FAMC") => indi_famc.entry(xref.clone()).or_default().push(val.to_string()),
                    ("INDI", "NAME") => { indi_name.insert(xref.clone(), val.to_string()); }
                    ("FAM", "HUSB") | ("FAM", "WIFE") | ("FAM", "CHIL") => {
                        if !indi.contains_key(val) {
                            diags.push(Diag{code:"E201", sev:Sev::Error, line:line_no,
                                msg:format!("{xref}: {tag} {val} apunta a un INDI inexistent")});
                        }
                        if tag == "CHIL" { fam_chil.entry(xref.clone()).or_default().push(val.to_string()); }
                    }
                    ("INDI", "BIRT") => { if let Some(y) = year(val) { birt = Some(y); if let Some(e) = indi.get_mut(xref) { e.0 = Some(y); } } }
                    ("INDI", "DEAT") => { died = true; if let Some(y) = year(val) { deat = Some(y); if let Some(e) = indi.get_mut(xref) { e.1 = Some(y); } } }
                    _ => {}
                }
                if (kind.as_ref() == "INDI" && (tag == "FAMS" || tag == "FAMC")) && !fams.contains_key(val) && !tag.is_empty() {
                    diags.push(Diag{code:"E201", sev:Sev::Error, line:line_no,
                        msg:format!("{xref}: {tag} {val} apunta a una FAM inexistent")});
                }
                if kind.as_ref() == "INDI" && tag == "DEAT" && val == "Y" { died = true; if let Some(e) = indi.get_mut(xref) { if e.1.is_none() { e.1 = Some(i64::MAX/2); } } }
            }
            continue;
        }
        if lvl == 2 && cur_sub == "DEAT" && tag == "DATE" {
            if let Some((xref, _)) = &cur {
                if let Some(y) = year(val) { deat = Some(y); if let Some(e) = indi.get_mut(xref) { e.1 = Some(y); } }
                else if died { if let Some(e) = indi.get_mut(xref) { if e.1.is_none() { e.1 = Some(i64::MAX/2); } } }
            }
        }
        if lvl == 2 && cur_sub == "BIRT" && tag == "DATE" {
            if let Some((xref, _)) = &cur {
                if let Some(y) = year(val) { birt = Some(y); if let Some(e) = indi.get_mut(xref) { e.0 = Some(y); } }
            }
        }
        // W401: URL dins PLAC (quirk MyHeritage)
        if tag == "PLAC" && val.contains("http") {
            diags.push(Diag{code:"W401", sev:Sev::Warning, line:line_no,
                msg:format!("PLAC amb URL (quirk MyHeritage): mou-la a NOTE: {}", truncate(val, 60))});
        }
        let _ = died;
    }

    // E201: FAMC/FAMS coherencia
    for (xref, famlist) in &indi_famc {
        for f in famlist {
            if let Some(chil) = fam_chil.get(f) {
                if !chil.contains(xref) {
                    diags.push(Diag{code:"W202", sev:Sev::Warning, line:0,
                        msg:format!("{xref}: declara FAMC {f} però la FAM no el llista com a fill")});
                }
            }
        }
    }
    for (xref, _) in &indi_fams {
        if let Some(i) = indi.get(xref) { let _ = i; }
    }
    let _ = indi_fams; let _ = indi_name;

    // W302: duplicats (mateix nom + naixement proper)
    let mut by_name: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (xref, (b, _)) in &indi {
        if let Some(nm) = indi_name.get(xref) {
            let key = nm.to_lowercase().replace('/', " ").split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some(b) = b { by_name.entry(key).or_default().push((xref.clone(), *b)); }
        }
    }
    for (_k, v) in &by_name {
        for a in 0..v.len() { for b in a+1..v.len() {
            if (v[a].1 - v[b].1).abs() <= 2 {
                diags.push(Diag{code:"W302", sev:Sev::Warning, line:0,
                    msg:format!("possible duplicat: {} (n. {}) vs {} (n. {})", v[a].0, v[a].1, v[b].0, v[b].1)});
            }
        }}
    }

    // sortida
    diags.sort_by(|x,y| y.sev.cmp(&x.sev).then(x.line.cmp(&y.line)));
    let (mut e, mut w, mut info) = (0,0,0);
    for d in &diags {
        match d.sev { Sev::Error => e+=1, Sev::Warning => w+=1, Sev::Info => info+=1 }
        let loc = if d.line > 0 { format!("línia {}", d.line) } else { "-".into() };
        println!("{} [{:0>4}] {loc}: {}", d.sev.tag(), d.code, d.msg);
    }
    println!("\n{} diagnostics: {e} errors, {w} avisos, {info} infos", path);
    std::process::exit(if e > 0 {2} else if w > 0 {1} else {0});
}

fn year(s: &str) -> Option<i64> {
    let re = |s: &str| -> Option<i64> {
        let mut out = None;
        for tok in s.split_whitespace() {
            if tok.len() == 4 && tok.chars().all(|c| c.is_ascii_digit()) { out = tok.parse().ok(); }
        }
        out
    };
    if let Some(m) = re(s) { return Some(m); }
    let nums: Vec<i64> = s.split(|c: char| !c.is_ascii_digit()).filter_map(|t| t.parse().ok()).collect();
    nums.first().copied()
}
fn truncate(s: &str, n: usize) -> String { if s.len() <= n { s.into() } else { format!("{}...", &s[..n]) } }
fn find_conc(l: &[u8]) -> Option<usize> {
    let pat = b"CONC ";
    l.windows(pat.len()).position(|w| w == pat).map(|p| p + pat.len())
}
fn write_repaired(path: &str, data: &[u8], fixes: &mut i64) -> std::io::Result<()> {
    fs::copy(path, format!("{path}.bak"))?;
    *fixes += 1;
    fs::write(path, data)
}
