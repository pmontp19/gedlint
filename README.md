# gedlint

Linter GEDCOM en Rust: binari únic, zero deps, exit codes 0/1/2.

**Estat: compila i amb paritat gedcheck.py (1E+23W sobre Montpeo_arbre_netejat.ged: 522 INDI / 124 FAM / 35 SOUR).**

## Ús

```sh
cargo build --release
./target/release/gedlint arbre.ged
./target/release/gedlint --format json arbre.ged
./target/release/gedlint --fix arbre.ged   # repara E101, guarda .bak
```

## GitHub Action

```yaml
- uses: pmontp19/gedlint@v1
  with:
    path: arbre/Montpeo_arbre_netejat.ged
    fail-on: error
```

Releases: `gh release create vX.Y.Z` amb `gedlint-{linux,darwin}-{x86_64,aarch64}`.


## Disseny (decisions preses)

- **Linter, no només validador**: el validador comprova conformitat amb l'espec; el linter afegeix categories (correctness / suspicious / style), severitats, configuració i `--fix` (reparacions segures). Model clippy/eslint.
- **Rust**: binari únic sense dependències, streaming per exports de 100MB+, i compilable a WASM per reutilitzar el motor al viewer web (gedcom-family-tree).
- **GEDCOM 7**: la spec 7.0 (gedcom.io) defineix estructures amb URI i cardinalitat: les regles estructurals es poden derivar quasi automàticament. Suport dual: detectar HEAD.GEDC.VERS i aplicar el conjunt de regles per versió (5.5.1 per als exports MyHeritage reals; 7.0 per a FamilySearch modern). Regla "upgrade path": avisar d'estructures no compatibles amb 7.
- **Zero dependències al POC** (sense clap: args a mà) per mesurar la velocitat base.

## Regles del POC (esborrany, no compila)

- E101: UTF-8 invàlid / caràcters partits entre línies CONC (bug real de MyHeritage) + `--fix` que el repara
- E201: referències trencades (FAMS/FAMC/HUSB/WIFE/CHIL)
- W202: FAMC no llistat com a CHIL
- W301: mort abans de neixer, longevitats >105
- W302: duplicats (nom + naixement ±2 anys)
- W401: URLs dins PLAC (quirk MyHeritage)
- Exit codes: 0 net, 1 avisos, 2 errors

## Fixtures reals

pmontp19/Genealogia-Montpeo (privat): GEDCOM MyHeritage amb tots els quirks documentats (UTF-8 partit ×3, @@escapats, URLs a PLAC, SEX U, duplicats, edats impossibles, entrada de 112 anys).
