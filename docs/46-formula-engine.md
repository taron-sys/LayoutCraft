# 46 Formula engine (`layoutcraft-formula`)

Status: first cut, not wired into the importer. Unfinished parts are listed at the end.

## Sources

Implemented from public documentation only (no other implementation read):

- [MS-VSDX] 2.5 Formula Expressions and Evaluation, 2.5.1 Formula ABNF and Full Grammar Definition:
  https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/e617d7e0-b0d9-4019-890c-cb4de0e3c6bf
  (also the pages Formula Expression `.../e715b9f4-e36e-402b-b962-5894c4ad7532`, Reference Tokens `.../a5d209e8-6bf3-4212-acb3-509df1b76d7d`, Function Tokens `.../71b8cdb6-1885-4fa2-a750-16d6626054f4`)
- Microsoft Learn, Visio ShapeSheet reference: About Operators, About Units of Measure, About Cell References,
  About Error Values, About Date, Time, and Duration Values, About Formulas, About Strings, and the per-function
  pages listed from https://learn.microsoft.com/en-us/office/client-developer/visio/functions-visio-shapesheet-reference
  (all under https://learn.microsoft.com/en-us/office/client-developer/visio/).
- The repository's earlier evaluators (`formula.rs`, `colorexpr.rs`) and their recorded probe results, for behaviour the pages leave open.

## Design

`parse` (lexer with byte spans, recursive descent, bounded length/nesting/height) -> `Expr`; `evaluate(Expr, &dyn Env)` -> `Value`
(numbers carry a dimension: length, angle, duration exponents; internal units inches, radians, days); `Engine<Host>` keeps cells,
a dependency graph (edges from the syntax tree), iterative Tarjan components, evaluation dependencies-first, early cut-off when a
value is unchanged, and `#CIRC!` plus a report for cycles. The host resolves `CellRef` to its own key and supplies services
(theme, palette, shape properties).

## Choices where the sources are silent (marked `Unspecified in the sources` in code)

Circular references -> `Circular` error value; `Unsupported` error value; comparison tolerance 1e-9 (word forms compare 15 digits);
a bare number next to a dimensioned one takes its dimension; didot = 1/2660 m; `INTUP` rounds toward +infinity; `CHAR` = Latin-1;
string `=` converts to numbers as documented; HSL functions use the documented 0..240 scale (the old `colorexpr` uses 0..255: verify with the cell oracle before migrating).
Operand conversion follows About Operators (unconvertible text is 0) although About Error Values shows `#VALUE!` for `5 + "House"`.

## Functions

Table in code: `layoutcraft_formula::functions()` (status Full / Partial / Compat / Unsupported with a note). Unsupported or unknown
functions evaluate to the `Unsupported` error value. Date/time functions, document properties, geometry/path functions, actions and
format pictures are unsupported. Compat (not in the public reference, kept for the old evaluator): MOD, CHOOSE, SWITCH, AVG, AVERAGE, EXP, LOG, CELLISTHEMED.

## Migration plan

1. Differential run of `formula.rs`/`colorexpr.rs` against the engine over the private corpus cells. 2. Importer builds an `Engine`
from shape cells (host resolves `Sheet.N!`, names, masters). 3. Replace both evaluators; keep cached values as fallback for
`Unsupported`. 4. Verify HSL scale and `MOD`/`MODULUS` against Visio.

## Unfinished in this change

Not written yet: tests for functions, engine recalculation, the differential test against `formula.rs`, the hostile-input sweep,
the generated function table in this note, `Pages[]`/unit corner cases. Only parser and operator tests exist (34 tests).
