# gedlint

Linter GEDCOM en Rust: ràpid, binari únic, streaming per fitxers grans, suport dual 5.5.1 (llegat, exports MyHeritage) + 7.0 (espec formal). Compilable a WASM per reutilitzar el motor al viewer web.

Spec completa: pmontp19/gedcom-family-tree issue #2 (revisió de #1).

## Estat

MVP funcional: `cargo test` (23 tests), `cargo clippy` net, release validat a 4.5MB en 0.09s, `cargo check --target wasm32-unknown-unknown` OK.

## Ús

```
gedlint [--fix] [--format text|json] [--severity error|warning|info] [--no-color] [--quiet] <fitxer.ged>
```

Exit codes: 0 net, 1 avisos, 2 errors.

`--fix` només aplica reparacions segures (E101 CONC partit, espais finals) i sempre escriu còpia `.bak`.

## Disseny

- **Linter, no només validador**: categories (correctness / suspicious / style / upgrade), severitats configurables, `--fix`. Model clippy/eslint.
- **`src/lib.rs`**: motor pur (`lint_str`, `lint_bytes`, `lint_reader`, `fix_bytes`, `Report::to_json`). Sense fs ni process: compila a WASM sense canvis. Parsing en streaming (`BufRead` línia a línia).
- **`src/main.rs`**: capa prima CLI (args a mà, zero dependències, colors ANSI manuals).
- **Versió**: es detecta via `HEAD.GEDC.VERS` i s'aplica el joc de regles per versió. Regles `U5xx` marquen l'upgrade path 5.5.1 cap a 7.0 (vegeu https://gedcom.io/migrate/).
- **Tests**: `tests/rules.rs`, un fixture mínim per regla + fixture 7.0 propi.

## Regles

Estructurals: E001 salt de nivell, E002 HEAD/TRLR, E003 xref duplicat, E004 xref malformat, E005 CONT/CONC orfe.
Codificació: E101 UTF-8 invàlid / CONC partit (bug MyHeritage) amb `--fix`, W102 BOM / CRLF mixt / controls.
Referencials: E201 refs trencades, W202 FAMC/CHIL creuat.
Semàntiques: W301 mort abans de néixer / longevitat >105, W302 duplicats (nom + naixement ±2 anys), W303 edat pares, W304 fill abans del matrimoni, W305 SEX.
Estil/quirks MyHeritage: W401 URL dins PLAC, W402 NAME/DATE no estàndard, W403 NOTE amb HTML.
Upgrade: U501 RELA/PEDI/BET (canvis 7.0), U502 tags propietaris `_MARNM`/`_UPD`/Ancestry.

## Estat de l'art (resum recerca, set 2026)

Validadors existents: Chronoplex GEDCOM Validator (Windows/.NET, tancat), GED-inline (Java MIT, 5.5/5.5.1/7.0, referència + ged-inline.org), gedcomtools Python (MIT, G5+G7+GX, `validate7`), js-gedcom (validació 7 completa), C# ArmidaleSoftware, go-gedcom (ABNF). Linters amb `--fix`: sashaperigo/gedcom-tools (Ancestry, Python), zupulint (26 checks, privacitat). Rust: pirtleshell/rust-gedcom (abandonat 2021, només parse 5.5.1), crate `ged_io` (lectura/escriptura). Forat: cap linter Rust amb model clippy (categories/severitats/config/--fix), streaming+WASM i dual 5/7 amb semàntica genealògica. Aquí entra gedlint.

## Fixtures reals

pmontp19/Genealogia-Montpeo (privat): GEDCOM MyHeritage amb UTF-8 partit, PLAC amb URLs, SEX U, duplicats, edats impossibles, entrada de 112 anys.
