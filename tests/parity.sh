#!/usr/bin/env bash
# Paritat gedlint vs gedcheck.py + regressió de forats (P0/P1).
# Sense falsos verds: pipefail, sense pipes que emmascarin exit, JSON validat.
set -euo pipefail
BIN="${1:-./target/debug/gedlint}"
FIX="/Users/pere/Genealogia-Montpeo/arbre/Montpeo_arbre_netejat.ged"
REF_PY="/Users/pere/Genealogia-Montpeo/informes/eines/gedcheck.py"
TDIR="$(mktemp -d -t gedlint-parity.XXXXXX)"
trap 'rm -rf "$TDIR"' EXIT

[ -x "$BIN" ] || cargo build --quiet
pass() { echo "ok: $1"; }
fail() { echo "FAIL: $1"; exit 1; }
# helper: exit esperat sense que set -e mati l'script
expect_exit() { # expect_exit <codi> <outfile> -- <cmd...>
  local want="$1" out="$2"; shift 2
  [ "${1:-}" = "--" ] && shift
  local got=0
  "$@" > "$out" 2>&1 || got=$?
  [ "$got" -eq "$want" ] || fail "exit $want (obtingut $got): $* -- vegeu $out"
}

# 0. smoke sintètic sempre (fins i tot sense GEDCOM privat)
expect_exit 1 "$TDIR/min.out" -- "$BIN" tests/fixtures/minimal.ged
grep -q "W301" "$TDIR/min.out" || fail "minimal W301"

# 1. truncate multibyte no fa panic (P0)
python3 -c "open('$TDIR/trunc.ged','w').write('0 HEAD\n0 @I1@ INDI\n1 BIRT\n2 PLAC '+'x'*59+'\u00e9'+' http://e.com/'+'y'*100+'\n0 TRLR\n')"
expect_exit 1 "$TDIR/trunc.out" -- "$BIN" "$TDIR/trunc.ged"
grep -q "W401" "$TDIR/trunc.out" || fail "trunc W401"
expect_exit 1 "$TDIR/trunc.json" -- "$BIN" --format json "$TDIR/trunc.ged"
python3 -c "import json; json.load(open('$TDIR/trunc.json'))" || fail "trunc JSON"

# 2. fitxer inexistent: exit 2 sense panic (P0)
expect_exit 2 "$TDIR/missing.out" -- "$BIN" "$TDIR/no-existeix.ged"
grep -q "panic" "$TDIR/missing.out" && fail "missing panic"

# 3. --fix línia 0: sense F001 fals (P0)
python3 -c "open('$TDIR/l0.ged','wb').write(b'\xa9X HEAD\n0 @I1@ INDI\n0 TRLR\n')"
expect_exit 2 "$TDIR/l0.out" -- "$BIN" --fix "$TDIR/l0.ged"
grep -q "F001" "$TDIR/l0.out" && fail "F001 fals línia 0"
[ -f "$TDIR/l0.ged.bak" ] && fail ".bak no calia a línia 0"

# 4. --fix net: sense .bak, bytes idèntics (P0 EOF fantasma)
sha256sum tests/fixtures/minimal.ged | cut -d' ' -f1 > "$TDIR/pre.sha"
cp tests/fixtures/minimal.ged "$TDIR/clean.ged"
expect_exit 1 "$TDIR/clean.out" -- "$BIN" --fix "$TDIR/clean.ged"
[ -f "$TDIR/clean.ged.bak" ] && fail ".bak en fitxer net"
sha256sum "$TDIR/clean.ged" | cut -d' ' -f1 > "$TDIR/post.sha"
cmp -s "$TDIR/pre.sha" "$TDIR/post.sha" || fail "fix net canvia bytes"

# 5. fronteres numèriques mare 13/14/55/56
mkfam() { printf '0 HEAD\n0 @M@ INDI\n1 NAME M /T/\n2 GIVN M\n2 SURN T\n1 SEX F\n1 BIRT\n2 DATE 1 JAN %s\n0 @C@ INDI\n1 NAME C /T/\n2 GIVN C\n2 SURN T\n1 BIRT\n2 DATE 1 JAN %s\n0 @F1@ FAM\n1 WIFE @M@\n1 CHIL @C@\n0 TRLR\n' "$1" "$2" > "$3"; }
mkfam 1980 1993 "$TDIR/m13.ged"; expect_exit 1 "$TDIR/m13.out" -- "$BIN" "$TDIR/m13.ged"
grep -q "0 errors, 1 avisos" "$TDIR/m13.out" || fail "mare 13 warn"
mkfam 1980 1994 "$TDIR/m14.ged"; expect_exit 0 "$TDIR/m14.out" -- "$BIN" "$TDIR/m14.ged"
grep -q "0 errors, 0 avisos" "$TDIR/m14.out" || fail "mare 14 net"
mkfam 1925 1980 "$TDIR/m55.ged"; expect_exit 0 "$TDIR/m55.out" -- "$BIN" "$TDIR/m55.ged"
grep -q "0 errors, 0 avisos" "$TDIR/m55.out" || fail "mare 55 net"
mkfam 1924 1980 "$TDIR/m56.ged"; expect_exit 1 "$TDIR/m56.out" -- "$BIN" "$TDIR/m56.ged"
grep -q "0 errors, 1 avisos" "$TDIR/m56.out" || fail "mare 56 warn"

# 6. regressió flush: E301 a l'últim INDI
printf '0 HEAD\n0 @I9@ INDI\n1 NAME U /T/\n2 GIVN U\n2 SURN T\n1 BIRT\n2 DATE 1 JAN 2000\n1 DEAT\n2 DATE 1 JAN 1990\n0 TRLR\n' > "$TDIR/last.ged"
expect_exit 2 "$TDIR/last.out" -- "$BIN" "$TDIR/last.ged"
grep -q "E301" "$TDIR/last.out" || fail "E301 últim INDI"

# 7. W307 case-sensitiu (paritat Python [A-Z]{3})
printf '0 HEAD\n0 @I1@ INDI\n1 DEAT\n2 DATE 25 jan 1909\n0 @I2@ INDI\n1 DEAT\n2 DATE 25 JAN 1909\n0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n0 TRLR\n' > "$TDIR/case.ged"
expect_exit 0 "$TDIR/case.out" -- "$BIN" "$TDIR/case.ged"
grep -q "W307" "$TDIR/case.out" && fail "W307 FP minúscules"

# 8. BURI nivell 2 amb http (paritat Python PLAC|BURI)
printf '0 HEAD\n0 @I1@ INDI\n1 DEAT\n2 BURI http://example.com/y\n0 TRLR\n' > "$TDIR/buri.ged"
expect_exit 1 "$TDIR/buri.out" -- "$BIN" "$TDIR/buri.ged"
grep -q "W401" "$TDIR/buri.out" || fail "BURI W401"

# 9. JSON summary == text (el linter surt amb exit 1: no deixar que pipefail mati l'script)
T_OUT="$( { "$BIN" "$TDIR/m13.ged" 2>&1 || true; } | tail -1)"
J_OUT="$( "$BIN" --format json "$TDIR/m13.ged" 2>/dev/null || true)"
python3 - "$J_OUT" "$T_OUT" <<'EOF' || fail "JSON summary mismatch"
import json,sys,re
d=json.loads(sys.argv[1]); t=sys.argv[2]
m=re.search(r"INDI (\d+)  FAM (\d+)  SOUR (\d+) \| (\d+) errors, (\d+) avisos",t)
assert m, "text summary no parseja"
s=d["summary"]
assert (s["indi"],s["fam"],s["sour"],s["errors"],s["warnings"])==tuple(map(int,m.groups())), "mismatch"
EOF

# 10. oracle privat (si disponible)
if [ ! -f "$FIX" ]; then
  echo "SKIP oracle privat (sense GEDCOM), smoke ok"
  echo OK
  exit 0
fi
expect_exit 2 "$TDIR/oracle.out" -- "$BIN" "$FIX"
grep -q "1 errors, 23 avisos" "$TDIR/oracle.out" || fail "oracle 1E+23W"
expect_exit 2 "$TDIR/oracle.json" -- "$BIN" --format json "$FIX"
python3 -c "import json; d=json.load(open('$TDIR/oracle.json')); s=d['summary']; assert (s['indi'],s['fam'],s['sour'],s['errors'],s['warnings'])==(522,124,35,1,23), s" || fail "oracle JSON"
echo OK: paritat 1E+23W + 10 famílies regressió
