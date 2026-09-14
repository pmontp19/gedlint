//! The rule registry: one static record per rule, and the lookups over it.
//!
//! `RULES` is the single source of truth for rule identity and documentation.
//! The CLI's `--explain` reads it, and so will the configuration validator,
//! the generated documentation and the web viewer's finding card: without it
//! each of those would keep its own copy and they would drift.
//!
//! `why` and `remedy` are written for a genealogist, not for a spec reader:
//! they name the software that breaks and how the breakage looks, because
//! that is the only part of a diagnostic a user can act on.

use crate::diag::{escape_json, Category, Severity};
use crate::fix::Applicability;

/// Everything known about one rule, independently of any file being linted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMeta {
    /// Stable identifier, e.g. "W202". Public API (JSON, annotations).
    pub code: &'static str,
    /// Kebab-case name, unique inside its ruleset, e.g. "asymmetric-famc-chil".
    pub name: &'static str,
    /// Domain the rule belongs to. "core" is everything normative shipped today.
    pub ruleset: &'static str,
    /// Why the rule exists (intent axis). Public API, do not extend or rename.
    pub category: Category,
    /// Severity used when the configuration says nothing.
    pub default_severity: Severity,
    /// False for every rule in a non-core ruleset.
    pub default_enabled: bool,
    /// The repair `--fix` carries for this rule, if any. `None` means the
    /// rule reports and the user decides; `Some(Safe)` is applied by a bare
    /// `--fix`, `Some(MaybeIncorrect)` only under `--fix --unsafe`.
    pub fixable: Option<Applicability>,
    /// One imperative line: what the file should do instead.
    pub title: &'static str,
    /// What breaks in consumer software, concretely.
    pub why: &'static str,
    /// What the user should do about it.
    pub remedy: &'static str,
    /// Optional (before, after) GEDCOM snippet pair for the documentation
    /// page and the web viewer's finding card. `None` until the examples
    /// are written, rule by rule; the renderers must handle both.
    pub example: Option<(&'static str, &'static str)>,
}

/// Every rule the engine can emit, sorted by code.
pub const RULES: &[RuleMeta] = &[
    RuleMeta {
        code: "E001",
        name: "invalid-line-level",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: Some(Applicability::Safe),
        example: None,
        title: "Start every line with a level number, one deeper at most",
        why: "The level number at the start of a line is the only thing that says what the line belongs to. \
A line without one (a note or a source transcription that wrapped onto its own line is the usual cause) \
or a line that jumps two levels at once leaves the importer with nowhere to put the text: Gramps and \
webtrees list it in the import log and drop it, and lenient importers hang it off the previous person. \
Either way the note, place or source is missing from the tree you end up with.",
        remedy: "Continuation text belongs on a CONT line one level deeper than the line it continues: \
\"1 NOTE first part\" followed by \"2 CONT second part\". `gedlint --fix` adds that prefix for you. \
For a level jump, add the missing intermediate line or lower the level so it grows by one at most.",
    },
    RuleMeta {
        code: "E002",
        name: "head-trlr-envelope",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Open the file with HEAD and close it with TRLR",
        why: "A GEDCOM file is an envelope: HEAD first, TRLR last. Readers take the version and the character \
set from the header before anything else, so a file that starts with a record or ends without TRLR is \
commonly refused outright with \"not a GEDCOM file\", and a program that does open it treats whatever \
follows TRLR as if it were not there.",
        remedy: "Move the HEAD record to the very first line and leave a single \"0 TRLR\" as the last one. \
Delete anything after TRLR, or move it above TRLR when it is a real record. Two exports concatenated \
into one file is the usual origin.",
    },
    RuleMeta {
        code: "E003",
        name: "duplicate-xref",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Give every record its own identifier",
        why: "The @I123@ identifier is how one record points at another. When two records share it, every \
pointer that names it becomes ambiguous and importers resolve it to whichever of the two they read last: \
children end up attached to the wrong parents, or one of the two people is swallowed by the other.",
        remedy: "Renumber one of the two records to an unused identifier and update the pointers that meant \
that one. If the two records are the same person exported twice, merge them in your genealogy program \
and export again.",
    },
    RuleMeta {
        code: "E004",
        name: "malformed-xref",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Write identifiers as @XREF@, with no spaces",
        why: "An identifier must be an @...@ token with no spaces and no nesting. A malformed one matches \
nothing, so every link into that record breaks at once: the person keeps their name but arrives in the \
new program without parents, children or sources.",
        remedy: "Rewrite it as @ plus letters, digits or underscores plus @, for example @I42@, and make \
sure every pointer that references the record uses exactly the same spelling. Hand-editing a GEDCOM is \
where this normally comes from.",
    },
    RuleMeta {
        code: "E005",
        name: "orphan-continuation",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: Some(Applicability::Safe),
        example: None,
        title: "Anchor every CONT and CONC to the line it continues",
        why: "CONT and CONC continue the value of the line directly above them, one level up, and they never \
nest inside each other. One that hangs from nothing, or from another CONT or CONC, continues nothing: \
strict importers drop the text and lenient ones paste it into a neighbouring field. Long notes and \
record transcriptions are what usually come out mangled. A CONT nested under a CONC has only one \
possible meaning: it continues the same value the CONC continues, and only its level number is wrong.",
        remedy: "Put the CONT or CONC line exactly one level below the value line it belongs to, and never \
chain one under another: \"1 NOTE line one\", \"2 CONT line two\", \"2 CONT line three\". `gedlint --fix` \
rewrites the level of a nested continuation for you, and only the number in column one changes, so you \
can judge the result against the text. A CONT or CONC with no line above it at all is left for you to \
place, because only you know which value it was meant to continue.",
    },
    RuleMeta {
        code: "E007",
        name: "conc-in-gedcom7",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Drop CONC from 7.0 files and carry long values on CONT",
        why: "GEDCOM 7 removed CONC and reserved the tag (spec 1.3), so a 7.0 reader is entitled to refuse the \
file outright or to skip the line. Skipping it costs you the tail of the value: a long note or place name \
arrives cut off exactly where the exporter chose to split it, and nothing says so.",
        remedy: "Join the CONC fragments back into the value of their parent line, which 7.0 allows because \
it has no line-length limit, and keep CONT only where you want a real line break. Staying on 5.5.1 is the \
other valid answer if you are not migrating yet.",
    },
    RuleMeta {
        code: "E008",
        name: "duplicate-singleton",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep the one-per-record fields to a single instance",
        why: "The specification allows exactly one SEX per person, one HUSB and one WIFE per family, one GEDC \
in the header, and one of each detail such as DATE, PLAC or CAUS inside a given event block. A second copy is almost always a merge \
leftover, and importers silently keep whichever they read first or last: the birth date your program \
shows afterwards may not be the one you meant to keep.",
        remedy: "Decide which value is right, delete the other, and give genuinely different information its \
own block. A second marriage is a second MARR event, not a second DATE inside the first one.",
    },
    RuleMeta {
        code: "E009",
        name: "missing-required-substructure",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Supply the substructures the specification requires",
        why: "Some lines are not optional. The header needs GEDC with its VERS, which is how a reader learns \
whether the file is 5.5.1 or 7.0; in 7.0 a custom EVEN or FACT needs a TYPE and an LDS ordinance STAT \
needs a DATE. Without the version the importer guesses, and guessing wrong changes how dates, names and \
the character encoding are read for the entire file.",
        remedy: "Add the missing lines: \"1 GEDC\" with \"2 VERS 5.5.1\" (or 7.0) inside HEAD, a \"2 TYPE ...\" \
under each custom EVEN or FACT, and a DATE under each ordinance STAT.",
    },
    RuleMeta {
        code: "E101",
        name: "invalid-utf8",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: Some(Applicability::Safe),
        example: None,
        title: "Keep every character whole and the file in valid UTF-8",
        why: "MyHeritage cuts long values at a fixed byte count, and when the cut falls inside an accented \
character its two halves end up on different CONC lines. What is left is not valid UTF-8: where \
\"José\" belongs the importer shows a replacement box or a truncated \"Jos\", and some readers refuse the \
file outright. The same rule fires when the file is not UTF-8 at all while presenting itself as UTF-8.",
        remedy: "`gedlint --fix` rejoins the split characters, which is a safe and invertible repair. If the \
whole file is in a legacy encoding instead, convert it to UTF-8, or declare the real encoding in \
\"1 CHAR\" so readers stop decoding it as UTF-8.",
    },
    RuleMeta {
        code: "E201",
        name: "broken-reference",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Point only at records that exist in the same file",
        why: "A pointer such as \"1 FAMC @F12@\" promises that @F12@ is in the file. When it is not, the link \
is simply lost on import: the child arrives without parents, the citation without its source, the person \
without their photograph. A partial export of a larger tree, or a record deleted by hand, is the classic \
cause.",
        remedy: "Either add the missing record to the file or delete the pointer that names it. In 7.0, \
@VOID@ is the correct way to say \"deliberately nothing here\". When the file came from a filtered \
export, re-export the whole tree instead.",
    },
    RuleMeta {
        code: "E202",
        name: "ancestral-cycle",
        ruleset: "core",
        category: Category::Correctness,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Break the loop where a person becomes their own ancestor",
        why: "A directed cycle through parent links (a child attached as their own grandparent after a bad merge or a mistyped identifier) has no valid reading: chart generators recurse forever or stop at an arbitrary depth, relationship calculators report contradictory paths, and exporters either loop or drop whole branches without warning.",
        remedy: "Follow the chain named in the message and unlink the wrong parent or child connection in your genealogy program, then export again. When two records describe the same person, merge them instead of linking them as parent and child.",
    },
    RuleMeta {
        code: "U501",
        name: "removed-in-gedcom7",
        ruleset: "core",
        category: Category::Upgrade,
        default_severity: Severity::Info,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Replace the 5.5.1 constructs that GEDCOM 7.0 dropped",
        why: "These lines are valid 5.5.1 and are not errors today: they only have no home in GEDCOM 7. RELA \
became the enumerated ROLE, HEAD.CHAR disappeared because 7.0 is always UTF-8, PEDI values are uppercase, \
and a BET range has to be complete and in chronological order. A 7.0 reader given them ignores or reports \
the line, so the relationship description or the date range stops travelling with your tree. One of the \
checks is not about the migration at all: a BET range whose years run backwards is reported on a 5.5.1 \
file too, because no reader expects a range to end before it starts.",
        remedy: "When you migrate, swap RELA for an enumerated ROLE with a PHRASE holding the free text, \
delete \"1 CHAR\", uppercase the PEDI value, and write ranges as \"BET <earlier> AND <later>\". The \
migration guide is at gedcom.io/migrate. Apart from putting a backwards range the right way round, \
nothing has to change while you stay on 5.5.1.",
    },
    RuleMeta {
        code: "U502",
        name: "undocumented-vendor-tag",
        ruleset: "core",
        category: Category::Upgrade,
        default_severity: Severity::Info,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Declare the vendor extension tags that a 7.0 file carries",
        why: "Tags beginning with an underscore are private extensions: _MARNM for a married name, _UPD for \
MyHeritage's last-changed stamp, _APID for an Ancestry source link. They survive into 7.0 as undocumented \
extensions, which means every other program is entitled to drop them, and a married name recorded only in \
_MARNM is exactly what disappears when the tree moves to another program.",
        remedy: "Keep the tags, and when you move to 7.0 declare them in a \"1 SCHMA\" block in the header so \
readers know what they mean. Copy anything you cannot afford to lose into a standard structure first: a \
married name fits a second \"1 NAME\" with \"2 TYPE MARRIED\" under it.",
    },
    RuleMeta {
        code: "W102",
        name: "encoding-artifact",
        ruleset: "core",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep the bytes plain: no BOM, one line ending, no controls",
        why: "A byte-order mark in front of \"0 HEAD\" stops many 5.5.x readers from recognizing the first \
line at all, so the file is rejected as malformed (7.0 recommends the BOM, and it is not flagged there). \
CRLF and LF mixed in one file confuse older parsers and make every later diff unreadable. Stray control \
characters ride along inside names and notes and surface as boxes or as broken text.",
        remedy: "Save 5.5.1 files as UTF-8 without BOM, pick one line ending for the whole file, and remove \
the control characters from the values that carry them. None of that is automatic. Independently of this \
warning, and whether or not it fired, `gedlint --fix` always rewrites classic Mac CR line endings to LF: \
that is whole-file preprocessing, not a repair of this rule.",
    },
    RuleMeta {
        code: "W202",
        name: "asymmetric-famc-chil",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep FAMC and CHIL pointing back at each other",
        why: "A parent-child link is written twice: the child says \"1 FAMC @F1@\" and the family answers with \
\"1 CHIL @I1@\". With only one half present, what you see depends on which half your program reads first: \
the child shows up in the family's list but has no parents on their own page, or the reverse. The missing \
half is usually gone for good at the next export.",
        remedy: "Add the line that is missing. Every \"1 FAMC @Fx@\" in a person needs a matching \"1 CHIL\" \
in family @Fx@, and every CHIL needs its FAMC. Re-linking the child to the family inside your genealogy \
program normally restores both halves at once.",
    },
    RuleMeta {
        code: "W301",
        name: "implausible-lifespan",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Check deaths before births and lifespans over 105 years",
        why: "A death before a birth, or a life longer than 105 years, is nearly always a mistyped year, a \
date read from the wrong column of a parish register, or two different people merged into one. Nothing \
breaks on import, but the mistake spreads: every age, generation gap and timeline your program draws is \
computed from these two dates.",
        remedy: "Open the person and compare both dates against the source. If the record really is \
exceptional and correct, leave it alone: this rule asks you to verify, it does not claim the data is \
wrong.",
    },
    RuleMeta {
        code: "W302",
        name: "possible-duplicate-individual",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Review same-name people born within two years of each other",
        why: "Two records with the same name and birth years two years apart or less are the classic result \
of importing the same branch twice, or of a merge that matched nothing. Left alone they split one \
person's sources, children and photographs across two half-empty pages, and neither page tells the whole \
story.",
        remedy: "Compare the two records. If they are the same person, merge them in your genealogy program, \
which keeps both sets of sources, and export again. If they are a nephew named after their uncle, which \
happens constantly, nothing needs to change.",
    },
    RuleMeta {
        code: "W303",
        name: "implausible-parent-age",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Check parents implausibly young or old at a child's birth",
        why: "A parent under 13, a mother over 50 or a father over 70 at a child's birth usually means the \
child is attached to the wrong generation, most often a grandparent linked as a parent. The tree then has one generation too few, and \
every descendant chart and relationship calculation drawn from it is wrong from that point down.",
        remedy: "Check whether the child belongs to this family or to the next generation, and check the two \
birth years for a transposed digit. Late fathers and very young mothers are real, so verify the record \
rather than assume the link is wrong.",
    },
    RuleMeta {
        code: "W304",
        name: "child-born-before-marriage",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Check children born before the marriage date of their family",
        why: "This is often simply true and needs no change at all. It is flagged because the other common \
cause is a wrong marriage year, or a child of an earlier union attached to the later family, which puts \
that child in the wrong household in every report and chart your program prints.",
        remedy: "Verify the marriage date and which family the child belongs to. When the birth really does \
precede the marriage, leave it as it stands, or record the earlier union as its own family so the child \
sits where they belong.",
    },
    RuleMeta {
        code: "W305",
        name: "invalid-sex-value",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Use the SEX values the version allows: M, F, U, and X in 7.0",
        why: "SEX carries one letter: M, F or U in 5.5.1, plus X in 7.0. Anything else, a whole word or a \
blank value included, is not understood, so importers store U instead. The person then shows up with a \
neutral icon and drops out of the \"sons\" and \"daughters\" listings your program builds from this field.",
        remedy: "Write \"1 SEX M\", \"1 SEX F\" or \"1 SEX U\", and \"1 SEX X\" only in a 7.0 file. Anything \
you want to say in words about a person's gender belongs in a NOTE, not in this field.",
    },
    RuleMeta {
        code: "W306",
        name: "invalid-enum-value",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Spell enumerated values the way the specification lists them",
        why: "Fields such as PEDI, ROLE, QUAY, RESN, NAME.TYPE, FAMC.STAT, an ordinance STAT, HEAD.CHAR and \
the media type of a FILE take their value from a fixed list. A value outside the list is dropped rather \
than adapted: an adoption recorded as \"2 PEDI adopted child\" imports as an ordinary birth relationship, \
and the fact that the child was adopted is gone from the tree.",
        remedy: "Use the listed value named in the message; 7.0 wants the exact uppercase spelling, 5.5.1 \
accepts any case. When none of them fits, that is what OTHER is for: put it there and write the real \
wording in a PHRASE beside it.",
    },
    RuleMeta {
        code: "W307",
        name: "conflicting-duplicate-event",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Reconcile single events recorded twice with different dates",
        why: "A person is born, christened, baptized, confirmed and buried once each, and a given marriage \
or divorce happens on a single date, so one of those events twice with two different dates is the \
fingerprint of a merge where neither date won. Programs keep one of them, usually the first, so the date \
you end up looking at may not be the researched one and the other is lost at the next export. A \
remarriage, MARR then DIV then MARR, is a legitimate sequence and is not flagged.",
        remedy: "Keep the date you can source, delete the other, and put the rejected one in a NOTE if it is \
worth remembering. When both dates are right because they describe two different occasions, give each its \
own event block with its own PLAC and SOUR.",
    },
    RuleMeta {
        code: "W308",
        name: "child-born-after-parent-death",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Check children born after a parent died",
        why: "A birth after the mother's death, or more than a year after the father's death, usually means the child is attached to the wrong parents or a death year lost a digit. Nothing breaks on import, but every descendant timeline and relationship path drawn below that link inherits the wrong household.",
        remedy: "Compare the child's birth date against both parents' death dates in the sources. Fathers allow one year of slack for a posthumous birth; anything beyond that, or any birth after the mother's death, belongs to another family or needs a corrected year.",
    },
    RuleMeta {
        code: "W309",
        name: "child-born-before-parent-birth",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Flag children older than their own parents",
        why: "A child born in the same year as a parent or earlier cannot be that couple's child: the link almost always joins two generations, typically a grandparent recorded as a parent. Charts built from it lose a generation and every relationship below that point is computed one step wrong.",
        remedy: "Move the child to the family where they belong, usually one generation down, or correct the birth year that carries the transposed digit. W303 already warns on wide gaps; this error marks the gap that cannot happen.",
    },
    RuleMeta {
        code: "W310",
        name: "chronological-event-sequence",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep baptism, burial and death in chronological order",
        why: "A christening or baptism before the birth, a burial before the death, or a baptism after the death is nearly always a date attached to the wrong event or the wrong person after a merge. Timelines and age calculations built from these dates place the person's whole life in the wrong order.",
        remedy: "Open the person and compare each event date against its source, moving the stray date to the event it belongs to. Approximate, ranged, BEF and AFT qualified dates are not flagged, because an inexact date near a boundary proves nothing.",
    },
    RuleMeta {
        code: "W311",
        name: "marriage-before-birth-or-after-death",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Error,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep marriages inside both spouses' lifetimes",
        why: "A marriage dated before either spouse was born or after either spouse died usually attaches the wrong couple to the family or carries a marriage year from a different union. Family reports then show a household that could never have existed, with children assigned to it.",
        remedy: "Verify the marriage date and that the two spouses are the right pair. When the date belongs to an earlier or later union, record that union as its own family instead of reusing this one. Each marriage in the family is checked, and BEF or AFT qualified dates are not flagged against the side their uncertainty covers.",
    },
    RuleMeta {
        code: "W312",
        name: "spouse-role-sex-discordance",
        ruleset: "core",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Match HUSB and WIFE roles to the recorded sex",
        why: "A family whose HUSB is recorded female while its WIFE is recorded male is usually a swapped pair from manual editing or a bad import. Programs that list sons and daughters or draw pedigree symbols from these two fields then place the parents on the wrong sides of every chart. A single mismatched side alone is never flagged, because a same-sex marriage is legitimately recorded with two men or two women in these slots.",
        remedy: "Swap the two spouse links back, or correct the SEX value that is wrong. Records without a usable SEX value are never flagged, so only a jointly inverted pair needs review.",
    },
    RuleMeta {
        code: "W401",
        name: "url-in-place",
        ruleset: "core",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Keep URLs out of PLAC and put the link where links belong",
        why: "MyHeritage writes the address of its place catalogue into the place name itself. The place then \
imports as the literal text \"Sabadell, https://...\", which matches nothing that anyone else wrote for \
the same town: your place index fills up with near-duplicates, and no map lookup or place merge resolves \
them.",
        remedy: "Trim the value back to the place name and move the link into a NOTE, or into a \"2 WWW\" \
line in 7.0. The name should read the way a person would write it: \"Sabadell, Valles Occidental, \
Barcelona, Spain\".",
    },
    RuleMeta {
        code: "W402",
        name: "nonstandard-name-or-date",
        ruleset: "core",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Write NAME and DATE values in the shape the format defines",
        why: "A surname is delimited by a pair of slashes and a date is \"DD MMM YYYY\" with an English \
three-letter month. With a slash missing the importer reads the whole string as a given name, so the \
person files under the wrong letter and has no surname in any index. A date written \"gener 1901\" or \
\"about 1901\" is stored as unparsed text: it does not sort, does not filter, and never lands on a \
timeline.",
        remedy: "Balance the slashes around the surname, as in \"1 NAME Joan /Puig i Ferrer/\", and write \
dates as \"12 JAN 1901\", using ABT, CAL or EST for approximations and \"BET x AND y\" for ranges. Keep \
the original wording in a PHRASE or a NOTE when it matters.",
    },
    RuleMeta {
        code: "W403",
        name: "html-in-note",
        ruleset: "core",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: true,
        fixable: None,
        example: None,
        title: "Store notes as plain text, not as HTML",
        why: "Exporters that keep notes in a rich-text editor write the markup out exactly as it stands, so \
<br>, &nbsp; and whole <notexml> wrappers end up inside the note. GEDCOM notes are plain text: the \
receiving program prints the tags literally, and a note that looked like three tidy paragraphs on screen \
arrives as one paragraph full of angle brackets.",
        remedy: "Convert the markup to plain text before exporting: a <br> becomes a real line break on a \
CONT line and a &nbsp; becomes an ordinary space. Delete the wrapper elements entirely.",
    },

    RuleMeta {
        code: "W601",
        name: "no-comma-in-surname",
        ruleset: "hispanic-naming",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: Some(Applicability::Safe),
        example: None,
        title: "Remove the single comma separating two surnames",
        why: "Iberian surnames are space-separated; the comma is an export artifact that triggers reverse-indexing in Anglo-centric software. \
The rule reads the surname slot of \"1 NAME\" and the \"2 SURN\" subtag, and reports a record once: the same surname written wrong in both places is one defect, not two.",
        remedy: "Remove the comma. The rule's repair does this for you automatically when it finds exactly two tokens and one comma.",
    },
    RuleMeta {
        code: "W602",
        name: "no-married-name",
        ruleset: "hispanic-naming",
        category: Category::Style,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: None,
        example: None,
        title: "Review married names imported into the _MARNM tag",
        why: "Women do not take a husband's surname in Iberian and Latin American naming, so its presence is almost always an import artifact from Anglo-centric software.",
        remedy: "Verify the name. If it is an artifact, delete the `_MARNM` tag. This requires manual review because deleting the tag destroys data.",
    },
    RuleMeta {
        code: "W603",
        name: "no-abbreviated-given-name",
        ruleset: "hispanic-naming",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        example: None,
        title: "Expand abbreviated given names",
        why: "Abbreviated given names like Mª, Ma., Fco., Jph. are ambiguous and make searching difficult in consumer software, as users rarely guess the exact abbreviation used when searching for a person in an index. \
The rule reads the whole \"1 NAME\" value and the \"2 GIVN\" subtag, and reports a record once.",
        remedy: "Write out the full name instead so that consumer software can index it properly and users can find the person without guessing abbreviations.",
    },
    RuleMeta {
        code: "W701",
        name: "polluted-name",
        ruleset: "hygiene",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        example: None,
        title: "Remove notes, occupations or nicknames from name fields",
        why: "Consumer software indexing names cannot separate notes or occupations from actual names. A parenthetical, asterisk or digit breaks the name index. \
The rule reads \"1 NAME\" and the \"2 SURN\" subtag, and reports a record once.",
        remedy: "Move the notes, occupations or nicknames to their proper tags like NOTE, OCCU or NICK.",
    },
    RuleMeta {
        code: "W702",
        name: "all-caps-name",
        ruleset: "hygiene",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: Some(Applicability::MaybeIncorrect),
        example: None,
        title: "Avoid all-caps surnames",
        why: "Fully capitalized surnames are a long-standing genealogical convention rather than a defect, and title casing is lossy for MCDONALD, O'BRIEN, DE LA O and most Iberian particles. \
The rule reads the surname slot of \"1 NAME\" and the \"2 SURN\" subtag, and reports a record once; a given name is out of scope, so the repair never title-cases one.",
        remedy: "Write names in standard capitalization. You can apply a potentially unsafe automatic repair using `--fix --unsafe`.",
    },
    RuleMeta {
        code: "W703",
        name: "malformed-place",
        ruleset: "hygiene",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: Some(Applicability::Safe),
        example: None,
        title: "Remove doubled commas from place names",
        why: "Doubled commas inside PLAC break the place index in consumer software, causing near-duplicates and breaking map lookups completely.",
        remedy: "Remove the extra commas from the place name. The automatic repair `gedlint --fix` does this for you automatically.",
    },
    RuleMeta {
        code: "W704",
        name: "impossible-sibling-spacing",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: None,
        example: None,
        title: "Review siblings born less than eight months apart",
        why: "Two children of the same mother born one to eight months apart, on exact dates, are in practice either one person entered twice with slightly different dates or children of two different unions merged into a single family. Left alone, the duplicate splits sources across two pages and the merged household misleads every sibling chart.",
        remedy: "Compare the two birth dates against the parish register. Merge the pair when they are one person, or split the later union into its own family when two mothers were merged. Twins sharing the same day are never flagged, and families with no recorded mother are skipped because spacing cannot be measured without her.",
    },
    RuleMeta {
        code: "W705",
        name: "alive-but-too-old",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: None,
        title: "Mark very old people without a death date as deceased",
        why: "A person born more than a century before the newest date in the file and still without a death date is almost always dead with the death missing from the record. While they stay listed as living, every search for the very old and every count of the living in your program includes people who cannot be alive, and record hints keep arriving for them.",
        remedy: "Find the death date in the parish register or the civil records and add it, or record the person as deceased when the exact date is unknown. When the person really is alive and exceptional, leave the record as it stands: this check asks for verification, not for invention. The age limit is the max-alive-years threshold.",
    },
    RuleMeta {
        code: "W706",
        name: "large-spouse-age-difference",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: None,
        title: "Check spouses whose births are decades apart",
        why: "Spouses born a generation or more apart are usually a mistyped year or a second union recorded on the first family: one birth year of 1991 instead of 1919 moves a whole household by seventy years. Programs draw the couple side by side, so the mistake hides in plain sight until a descendant chart makes the gap visible.",
        remedy: "Compare both birth years against their sources and check whether the marriage belongs to an earlier or later union of one of the spouses. Adjust the max-spouse-gap threshold in [lints.thresholds] when your tree legitimately holds wide gaps, so only the certain errors keep reporting.",
    },
    RuleMeta {
        code: "W707",
        name: "married-too-young",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Warning,
        default_enabled: false,
        fixable: None,
        title: "Check marriages before either spouse grew up",
        why: "A marriage dated when a spouse was still a child usually carries the wrong marriage year or attaches the wrong couple to the family: parish registers sometimes date the banns where the marriage belongs. The household your program prints from that date joins two people who could not yet have married, and any children attached to it inherit the wrong start.",
        remedy: "Verify the marriage date and that the two spouses are the right pair, moving an early union to its own family when it belongs there. A spouse who died in childhood is flagged the same way. The minimum age is the min-marriage-age threshold, so raise it or lower it to match what your sources consider marriageable.",
    },
    RuleMeta {
        code: "W708",
        name: "siblings-same-first-name",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        title: "Review siblings who share a first name",
        why: "Two children of one family with the same first name are often the same person entered twice with slightly different dates, splitting sources and photographs across two half-empty pages. A namesake after an infant death is the legitimate counterpart, which happens constantly and is why this finding stays a notice rather than a warning.",
        remedy: "Compare the two records, including death dates: merge them in your genealogy program when they are one person, and leave them alone when the elder namesake died young. Nothing needs to change for cultural naming patterns that reuse names deliberately.",
    },
    RuleMeta {
        code: "W709",
        name: "disconnected-individual",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        title: "Reconnect or remove people linked to no family",
        why: "A person with no parents, spouses or children floats outside every pedigree, descendant chart and relationship calculation your program draws: they are invisible in all of them. Detached leftovers usually arrive when a branch is deleted or moved and one record stays behind, exactly what this check collects.",
        remedy: "Attach the person to their family, or delete the leftover record when it duplicates someone already in the tree. Forests of genuinely separate trees in one file can silence this check in [lints.rules] without losing anything else.",
    },
    RuleMeta {
        code: "W710",
        name: "name-spacing-affixes-short-years",
        ruleset: "hygiene",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        title: "Clean spacing, affixes, short years and missing sex in names",
        why: "Double spaces, leading blanks, a Dr. inside the given name or a Jr. inside the surname break name indexes: searches miss the person unless the user guesses the exact quirk. A two-digit birth year sorts nowhere and a missing sex drops the person from every sons and daughters listing, so these small defects share one check.",
        remedy: "Remove the extra spaces, move prefixes and suffixes to their own fields, write all four year digits, and record M, F or U for every person. Each half of this check is independent: fix what is wrong in your sources and leave the rest, since house names and particles are legitimate text.",
    },
    RuleMeta {
        code: "W711",
        name: "children-different-surnames",
        ruleset: "hygiene",
        category: Category::Suspicious,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        title: "Review families whose children carry different surnames",
        why: "Children of one household with different surnames often mean a child of an earlier union recorded on the later family, or a second family merged into the first: every sibling chart printed from that household then mixes two lines. Remarriage, adoption and double surnames make this common enough to stay a notice, but the split is worth one look.",
        remedy: "Check which union each child belongs to and move misplaced children to their own family, or correct the surname that carries the typo. When the mix is real, for example after a remarriage, leave it and silence the single finding in the baseline.",
    },
    RuleMeta {
        code: "W712",
        name: "place-resembles-cause-or-date",
        ruleset: "hygiene",
        category: Category::Style,
        default_severity: Severity::Info,
        default_enabled: false,
        fixable: None,
        title: "Move causes of death and dates out of place names",
        why: "A death place holding Holocaust or died of pneumonia belongs in the cause field, not on the map: geocoders fail on it and place indexes fill with entries no other tree shares. A place holding a year or a month is the mirror mistake, a date typed into the wrong field that then never lands on any timeline.",
        remedy: "Move the cause into CAUS and the date into DATE, leaving the place as a person would write it. Only a human knows which field the text belongs in, so this check never repairs itself: correct the line by hand.",
    },
];

/// Look up a rule by its code. Exact match: codes are uppercase by
/// construction, and configuration keys must resolve unambiguously.
pub fn rule(code: &str) -> Option<&'static RuleMeta> {
    RULES.iter().find(|r| r.code == code)
}

/// Look up a rule by the `<ruleset>/<name>` pair configuration addresses it by.
pub fn rule_by_name(ruleset: &str, name: &str) -> Option<&'static RuleMeta> {
    RULES
        .iter()
        .find(|r| r.ruleset == ruleset && r.name == name)
}

/// Every ruleset present in `RULES`, in first-appearance order. Lets a
/// consumer group the table without hardcoding the list of rulesets.
pub fn rulesets() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for r in RULES {
        if !out.contains(&r.ruleset) {
            out.push(r.ruleset);
        }
    }
    out
}

/// The registry as JSON (issue 15): the web viewer's finding card renders
/// `title` / `why` / `remedy` straight from here, so the page and the CLI's
/// `--explain` cannot drift. Same serialization conventions as
/// `Report::to_json`: severity strings, `escape_json` for text.
pub fn rules_to_json() -> String {
    rules_to_json_for(RULES)
}

/// The registry as JSON over a given slice; `rules_to_json` is the shipped
/// form. Parameterized so tests can pin the JSON shape of a filled-in
/// example without inventing records in the shipped table.
pub fn rules_to_json_for(rules: &[RuleMeta]) -> String {
    let mut out = String::with_capacity(rules.len() * 640);
    out.push('[');
    for (i, r) in rules.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"code\":\"");
        out.push_str(r.code);
        out.push_str("\",\"name\":\"");
        out.push_str(r.name);
        out.push_str("\",\"ruleset\":\"");
        out.push_str(r.ruleset);
        out.push_str("\",\"category\":\"");
        out.push_str(r.category.as_str());
        out.push_str("\",\"default_severity\":\"");
        out.push_str(r.default_severity.tag().trim());
        out.push_str("\",\"default_enabled\":");
        out.push_str(if r.default_enabled { "true" } else { "false" });
        out.push_str(",\"fixable\":");
        match r.fixable {
            None => out.push_str("null"),
            Some(a) => {
                out.push('"');
                out.push_str(a.as_str());
                out.push('"');
            }
        }
        out.push_str(",\"title\":\"");
        out.push_str(&escape_json(r.title));
        out.push_str("\",\"why\":\"");
        out.push_str(&escape_json(r.why));
        out.push_str("\",\"remedy\":\"");
        out.push_str(&escape_json(r.remedy));
        out.push_str("\",\"example\":");
        match r.example {
            None => out.push_str("null"),
            Some((before, after)) => {
                out.push_str("{\"before\":\"");
                out.push_str(&escape_json(before));
                out.push_str("\",\"after\":\"");
                out.push_str(&escape_json(after));
                out.push_str("\"}");
            }
        }
        out.push('}');
    }
    out.push(']');
    out
}
