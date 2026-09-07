# Golden fixtures (issue 9: external dataset audit)

Official public corpora used as regression tests. Never add private tree
data to this directory (see AGENTS.md).

| Files | Source | Terms |
|---|---|---|
| `minimal70.ged`, `maximal70.ged`, `remarriage1.ged`, `remarriage2.ged`, `same-sex-marriage.ged`, `escapes.ged` | FamilySearch/GEDCOM.io, `testfiles/gedcom70` (github.com/FamilySearch/GEDCOM.io) | official 7.0 reference test files; no explicit license file in the source repo, used unmodified for interop testing |
| `TGC551.ged`, `TGC551LF.ged` | GEDCOM Committee 5.5.1 coverage suite (H. Eichmann & J. A. Nairn, 1997-2001) via github.com/cmosher01/Gedcom-Tests | file header grants: "Feel free to copy and use this GEDCOM file for any non-commercial purpose" |

`TGC551.ged` keeps the original classic-Mac CR line terminators (that is the
point: it exercises bare-CR parsing); `TGC551LF.ged` is the LF twin.

Tamura Jones torture tests (Long26CC.ged, Children1200.ged, MARRSELF.GED...)
are not vendored: tamurajones.net blocks automated downloads. Their relevant
patterns (NAME split across CONC) are covered synthetically in
`tests/rules.rs` (`w402_name_slashes_across_conc`).
