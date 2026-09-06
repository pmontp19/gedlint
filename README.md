# gedlint (WIP)

Linter GEDCOM en Rust: ràpid, binari únic, streaming per fitxers grans, suport dual 5.5.1 (llegat, exports MyHeritage) + 7.0 (espec formal).

**Estat: POC esborrany que NO compila** (main.rs amb ~5 errors de sintaxi). És el punt de partida de la sessió futura: veure issue #2 a pmontp19/gedcom-family-tree per l'especificació completa.

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
