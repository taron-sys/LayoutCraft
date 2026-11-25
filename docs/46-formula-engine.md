# 46 Formula engine (`layoutcraft-formula`)

Status: draft pull request. The crate is not wired into the importer yet (see "Not done").

The crate evaluates ShapeSheet-style cell formulas: it parses them, evaluates them with units, and
keeps a graph of cells that it recalculates incrementally. It has no runtime dependency, so it builds
offline and for WebAssembly.

## Sources

These public pages are what the code cites; the module documentation says which one each rule comes
from, and `Unspecified in the sources:` marks every place where they say nothing. This note makes no
provenance claim beyond those citations (see `docs/14-v1-scope-provenance.md` and `PROVENANCE.md`).

- [MS-VSDX] 2.5 Formula Expressions and Evaluation and 2.5.1 Formula ABNF and Full Grammar Definition:
  https://learn.microsoft.com/en-us/openspecs/sharepoint_protocols/ms-vsdx/e617d7e0-b0d9-4019-890c-cb4de0e3c6bf
  (also Formula Expression `.../e715b9f4-e36e-402b-b962-5894c4ad7532`, Reference Tokens
  `.../a5d209e8-6bf3-4212-acb3-509df1b76d7d`, Function Tokens `.../71b8cdb6-1885-4fa2-a750-16d6626054f4`).
- Microsoft Learn, Visio ShapeSheet reference: About Operators, About Units of Measure, About Cell
  References, About Error Values, About Date, Time, and Duration Values, About Strings, and one page
  per function (the Markdown sources of these pages are in the public repository
  `MicrosoftDocs/office-developer-client-docs`, folder `docs/visio/`). All under
  https://learn.microsoft.com/en-us/office/client-developer/visio/.
- The repository's earlier evaluators (`layoutcraft-vsdx` `formula.rs` and `colorexpr.rs`) for the few
  compatibility names that no page lists (`MOD`, `CHOOSE`, `SWITCH`, `AVG`, `AVERAGE`, `EXP`, `LOG`).
  They are run against this crate in the differential test; nothing was copied from them.

Expected values in the tests are the examples and statements of those pages (the page is named in the
test group's comment). Where a page's printed example is wrong, the test says so (decisions D11, D12).

## Design

`parse` (lexer with byte spans, recursive descent with bounded length, nesting and height) gives an
`Expr`. `evaluate(Expr, &dyn Env)` gives a `Value`: numbers carry a dimension (exponents of length,
angle and duration; internal units are inches, radians and days, as the units page states). Errors are
values, never panics. The host implements `Env` (cells) and `Services` (theme, palette, shape
properties); the crate knows nothing about `.vsdx` packages.

`Engine<Host>` keeps the cells, their dependency graph (edges are read from the syntax tree, so a
reference in a branch that is not taken still counts) and their values. `recalculate` re-evaluates
exactly the cells an edit can affect, dependencies first (iterative Tarjan components, so a chain of
100 000 cells needs no deep recursion), and stops following an edge when a cell's new value equals its
old one. Cells on a circular reference get `#CIRC!`, and the `Report` lists each cycle.

Limits (constants in `lib.rs`): formula text 64 KiB, nesting 64 levels (parentheses, calls, prefix
operators), tree height 256, strings built by a formula 1 MiB.

## Operators

Tightest first, as the "Order of evaluation" table of About Operators lists them; operators of one
level group from the left.

| level | operators |
|---|---|
| 1 | unary `+`, unary `-`, postfix `%` |
| 2 | `^` |
| 3 | `*` `/` |
| 4 | `+` `-` |
| 5 | `&` |
| 6 | `<` `<=` `>` `>=` and `LT` `LE` `GT` `GE` |
| 7 | `=` `<>` and `EQ` `NE` |

Symbolic comparisons treat numbers within 1e-9 as equal (D3); the word forms compare the 15 stored
digits. Text is converted to a number (0 when it is not one) wherever an operator needs a number, and
anything is converted to text by `&`; two different words therefore compare equal with `=`, as the
page says. Dimensions: `+`, `-` and comparisons need equal dimensions (a bare number takes the other
operand's, D4), `*` and `/` add and subtract exponents, `^` needs a whole exponent for a dimensioned
base, and exponents outside -128..=127 are `#DIM!`. Dates: date +/- duration is a date, date - date is
a duration, duration +/- duration is a duration, date + date is a date (the dates page's table).

Error values: `#DIV/0!`, `#VALUE!`, `#REF!`, `#NUM!`, `#N/A!` and `#DIM!` as in About Error Values; the
engine adds `#UNSUPPORTED!` (a function known by name but not evaluated) and `#CIRC!`, which the
public pages do not define and which must never be written to a file. An error operand is the result
(the left one when both fail); `ISERR`, `ISERRNA`, `ISERROR`, `ISERRVALUE` and `IFERROR` look at it.

## Units of measure

Internal units: inches (square inches for areas), radians, days. A suffix may follow a number with or
without a space and with an optional trailing dot (`0.7 pt.`); case is ignored. Only the spellings of
the units page are accepted; anything else after a number is a parse error, not a guess (D9).

| unit | spellings | dimension | factor to internal | note |
|---|---|---|---|---|
| inch | `in`, `IN_F` | length | 1 | `IN_F` differs only in how a cell displays |
| foot | `ft` | length | 12 | |
| yard | `yd` | length | 36 | |
| mile | `mi`, `MI_F` | length | 63 360 | |
| nautical mile | `nm` | length | 1852 m | international definition |
| millimetre, centimetre, metre, kilometre | `mm`, `cm`, `m`, `km` | length | 0.001, 0.01, 1, 1000 m | 1 in = 0.0254 m |
| point | `pt` | length | 1/72 | value not on the page (D7) |
| pica | `p` | length | 1/6 | value not on the page (D7) |
| didot | `d` | length | 1/2660 m | value not on the page (D7) |
| cicero | `c` | length | 12 didots | value not on the page (D7) |
| degree | `deg`, `°` | angle | pi/180 | |
| radian | `rad` | angle | 1 | |
| arc minute, arc second | `'`, `"` (attached to the number) | angle | pi/10 800, pi/648 000 | |
| elapsed week, day, hour, minute, second | `ew`/`eweek`, `ed`/`eday`, `eh`/`ehour`, `em`/`eminute`, `es`/`esecond` | duration | 7, 1, 1/24, 1/1440, 1/86 400 | the dates page lists the long spellings |
| acre | `ACRES` | area | 43 560 ft² | |
| hectare | `HECTARES`, `HECTARE`, `HA` | area | 10 000 m² | |
| square inch, foot, yard, mile, mm, cm, m, km | `SQ IN`, `SQ. IN.`, `IN^2`, `IN.^2`, `IN ^2` (same for `ft yd mi mm cm m km`) | area | square of the length unit | multidimensional table of the units page |
| percent | `%` after a number | none | 0.01 | the postfix operator |

Implicit units: `5[in,P]`, `5 [in, P]` (flag `a d e p t` in either case) take the unit named and keep
the number as written; `DL`, `DP`, `DT` are inches, `DA` radians and `DE` days, "internal units", as the
page says. A number with no unit is dimensionless; a dimensionless number in a place that needs a
length is taken to be in internal units (D4).

## Decision list

Choices the sources do not settle, or settle in two ways. Each is also marked in the code.

- D1 A cell on a circular reference is `#CIRC!`, as is everything that reads it. The edges come from the
  syntax tree, so a reference in an untaken branch (`IF(0, B, 1)`) still makes a cycle.
- D2 `#UNSUPPORTED!` for functions not evaluated; the engine never invents a number for them.
- D3 Symbolic comparisons use an absolute tolerance of 1e-9 (the documented default of the `SIGN` fuzz).
- D4 A dimensionless operand of `+`, `-`, a comparison, `MAX`, `MIN`, `SUM` takes the dimension of the
  others.
- D5 `^` groups from the left (`2^3^2` is 64), like every other binary operator; the operators page
  gives the level but not the grouping. Unary signs bind tighter than `^` (`-2^2` is 4), as its table says.
- D6 `5 + "House"` is 5 (About Operators: unconvertible text is zero), although About Error Values shows
  `#VALUE!` for it. Both pages are cited; the operators page describes the rule, the error page a table
  example.
- D7 Point, pica, didot and cicero sizes are the usual typographic ones; the units page names the units
  but gives no sizes. Didot is 1/2660 m.
- D8 `INTUP` rounds towards +infinity (`INTUP(-3.2)` is -3, the page's example); `CHAR` takes the number
  as a Latin-1 code point (the page says "ANSI" without a code page); `UNICHAR` refuses surrogates.
- D9 Unit spellings are limited to the documented ones (the earlier evaluator accepted any letters). A
  length unit followed by `^2` is the area unit (`3 in^2` is 3 square inches, not 9), as the multidimensional
  table lists `IN^2` as a unit; other powers stay powers of the quantity (`5.2cm^1.5` is `#DIM!`, the
  error page's own example). `1 in ^ 2 in` is therefore a parse error, not `#DIM!`.
- D10 Colours are 8-bit sRGB values. `HSL`, `HUE`, `SAT`, `LUM` use the 0..240 scale of their pages, so a
  colour with low saturation does not survive `HUE(HSL(h, s, l))` exactly (the HUE page's example
  `HUE(HSL(10, 20, 30))` returns 10; here it returns 13, which the test bounds). The HSL page lists black's
  luminosity as 24, which contradicts its other rows; black is 0 here. A number used as a colour is a
  palette index the host resolves.
- D11 `TINT`, `SHADE`, `TONE` add to or subtract from luminosity (saturation) in 0..=240 steps (their
  pages). `MSOTINT`/`MSOSHADE` mix with white or black by a percentage; the pages give no value, so the
  linear mix of the earlier evaluator is kept.
- D12 Two page examples are not reproduced: `ATAN2(1.25, 2.25)` is printed as 29.0456 degrees, but the
  arctangent of 1.25/2.25 is 29.0546 degrees (digits transposed on the page); `TANH(-45)` is printed as
  -0.6558, which is the value for -45 degrees, not -45 radians (which gives -1). The tests use the
  mathematically correct values.
- D13 A word comparison operator (`EQ`, `LT`, ...) is an operator wherever an operator can stand,
  also when `(` follows it (`1 EQ (2)`).
- D14 A chain of more than about 255 binary operators (`1+1+...`) exceeds the tree-height limit and is a
  parse error; `SUM(...)` takes any number of arguments. Nesting beyond 64 levels is a parse error.
- D15 Positions and lengths in string functions count Unicode scalar values; a result longer than 1 MiB
  is `#VALUE!`.
- D16 `BOUND` evaluates to its first argument (0 when omitted, as in the page's own example) and needs
  5, 8, 11, ... arguments. `GUARD`, `THEMEGUARD`, `SETATREF*` evaluate their argument; protecting the
  cell or redirecting an edit is the host's business. `DEPENDSON` is FALSE and its arguments are not
  evaluated (their references still create edges).
- D17 `ISERR` is TRUE for every error but `#N/A!`, including `#UNSUPPORTED!` and `#CIRC!`.
- D18 A colour is TRUE in a condition; as an operand of arithmetic it is 0; two colours compare for
  equality only.
- D19 `THEMEVAL(name, default)` returns the default when the host has no value for `name` ("document is
  not themed"); with no default it is `#N/A!`.
- D20 `ADD`, `SUB`, `MUL`, `DIV`, `UMINUS`, `UPLUS`, `PCT` are function tokens of the grammar with no
  page; they evaluate like the operator of the same name.
- D21 `HUEDIFF`, `SATDIFF`, `LUMDIFF` are first minus second (the pages give no sign).
- D22 `GRAVITY` includes the two limits.
- D23 `BITAND` and the other bit functions use the low 16 bits of the truncated argument.
- D24 `SETATREFEXPR()` is 0 (its page); `INDEX` returns `errorvalue` exactly as given.
- D25 `SHAPETEXT` honours flags 4, 16, 32 and 64; flags 1, 2 and 8 need field and hyphenation data and give
  `#UNSUPPORTED!`. `NAME`, `PAGENAME`, `MASTERNAME` ignore their language argument.
- D26 `POW` follows the special cases of its page; `POW(-2, -3)` (both negative) is `#NUM!`.
- D27 `ISTHEMED` may be written without parentheses (the second example on its page does), as `PI` may.
- D28 Dates: arithmetic is implemented, the date functions are not (no calendar library).

## Differences from the earlier evaluators

`tests/it/differential.rs` runs every formula of the unit tests of `layoutcraft-vsdx` `formula.rs` and
`colorexpr.rs` through both and requires the same result; it also compares 3 000 generated arithmetic
formulas. These differences are intended, and each is pinned by a test:

| formula | earlier evaluator | this crate | source |
|---|---|---|---|
| `LUM` of a colour | 0..255 scale (`luminance("#ffffff")` is 255) | 0..240 (`LUM(#ffffff)` is 240) | LUM, HSL, TINT pages |
| `MODULUS(-7, 3)` | not known | 2 (sign of the divisor); `MOD(-7,3)` stays 1 | MODULUS page |
| `TINT(#808080, 50)` | mixes 50 % with white | raises luminosity by 50 of 240 steps | TINT page |
| `SHADE(c, n)` | mixes with black | lowers luminosity by n of 240 | SHADE page |
| `0.1+0.2=0.3` | 0 (exact) | 1 (tolerance); `EQ` is exact to 15 digits | About Operators |
| `0 = 1 < 2` | 1 (all comparisons on one level) | 0 (`<` binds tighter than `=`) | About Operators, order table |
| `SIGN(0.00000000001)` | 1 | 0 (fuzz 1e-9) | SIGN page |
| `FLOOR(-1.2, 0.5)`, `CEILING(-1.2, 0.5)` | a number | `#NUM!` (opposite signs) | FLOOR, CEILING pages |
| `SQRT(-1)`, `ASIN(2)`, `ACOS(2)` | NaN | `#NUM!` | SQRT, ASIN, ACOS pages |
| `POW(0, -1)` | infinity | 0 | POW page |
| `5xyz`, `96px` | accepted (`px` is 1/96 in) | parse error | units page |
| `IF(1, 5)` | 5 | `#VALUE!` (three arguments) | IF page |
| `BITXOR(1, 2, 3)` | folds all arguments | `#VALUE!` (two arguments) | BITXOR page |
| `SETATREFEXPR()` | cannot evaluate | 0 | SETATREFEXPR page |
| `EVAL(5)` | 5 | `#UNSUPPORTED!` (no such function page) | function list |

Not a difference: `MOD`, `CHOOSE`, `SWITCH`, `AVG`, `AVERAGE`, `EXP`, `LOG` keep their results (they are
`compat` rows below), `-2^2` is 4 and `2^3^2` is 64 in both.

## Functions

`status`: implemented (the reference's behaviour), partial (a documented option or side effect is not
modelled, see the note), compat (not on a public page, kept for the earlier evaluator) or unsupported
(known by name, evaluates to `#UNSUPPORTED!`). `arguments` is the accepted count (`...` is any number;
a wrong count is `#VALUE!`). Every implemented function has at least one normal and one edge or error
case in `tests/it/cases.rs`; the `test` column names the test that runs them. This table is printed by
`cargo run -p layoutcraft-formula --example function_table`; do not edit it by hand.

| function | status | arguments | test | source and notes |
|---|---|---|---|---|
| `ABS` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 5 cases, e.g. `ABS(-1.24)` is `1.24` | function page |
| `ADD` | implemented (partial) | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `ADD(1, 2)` is `3` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `SUB` | implemented (partial) | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `SUB(5, 2)` is `3` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `MUL` | implemented (partial) | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `MUL(3, 4)` is `12` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `DIV` | implemented (partial) | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `DIV(8, 2)` is `4` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `UMINUS` | implemented (partial) | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `UMINUS(3)` is `-3` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `UPLUS` | implemented (partial) | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `UPLUS(3)` is `3` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `PCT` | implemented (partial) | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `PCT(50)` is `0.5` | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] grammar (2.5.1) but no public page defines it; evaluates like the operator of the same name |
| `CEILING` | implemented | 1-2 | `functions::math_functions_give_the_reference_values_and_errors`: 9 cases, e.g. `CEILING(3.7)` is `4` | function page |
| `FLOOR` | implemented | 1-2 | `functions::math_functions_give_the_reference_values_and_errors`: 6 cases, e.g. `FLOOR(3.7)` is `3` | function page |
| `INT` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 4 cases, e.g. `INT(7.2)` is `7` | function page |
| `INTUP` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `INTUP(3.2)` is `4` | function page |
| `ROUND` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 10 cases, e.g. `ROUND(123.654, 2)` is `123.65` | function page |
| `TRUNC` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 6 cases, e.g. `TRUNC(123.654, 2)` is `123.65` | function page |
| `SIGN` | implemented | 1-2 | `functions::math_functions_give_the_reference_values_and_errors`: 7 cases, e.g. `SIGN(-5)` is `-1` | function page |
| `SQRT` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 5 cases, e.g. `SQRT(2)` is `1.4142135623730951` | function page |
| `POW` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 11 cases, e.g. `POW(5, 2)` is `25` | function page |
| `LN` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `LN(42)` is `3.7377` | function page |
| `LOG10` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `LOG10(42)` is `1.6232` | function page |
| `PI` | implemented | 0 | `functions::math_functions_give_the_reference_values_and_errors`: 4 cases, e.g. `PI()` is `3.141592653589793` | function page |
| `MODULUS` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 7 cases, e.g. `MODULUS(5, 1.4)` is `0.8` | function page |
| `MAX` | implemented | 1-... | `functions::math_functions_give_the_reference_values_and_errors`: 5 cases, e.g. `MAX(13 in, 1 ft, 20 cm)` is `13 in` | function page |
| `MIN` | implemented | 1-... | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `MIN(13 in, 1 ft, 20 cm)` is `7.874015748031496 in` | function page |
| `SUM` | implemented | 1-... | `functions::math_functions_give_the_reference_values_and_errors`: 5 cases, e.g. `SUM(5, 7, 12)` is `24` | function page |
| `MAGNITUDE` | implemented | 4 | `functions::math_functions_give_the_reference_values_and_errors`: 4 cases, e.g. `MAGNITUDE(1, 3, 1, 4)` is `5` | function page |
| `BITAND` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `BITAND(12, 6)` is `4` | function page |
| `BITOR` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `BITOR(12, 6)` is `14` | function page |
| `BITXOR` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `BITXOR(12, 6)` is `10` | function page |
| `BITNOT` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `BITNOT(6)` is `65529` | function page |
| `MOD` | implemented (compat) | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 4 cases, e.g. `MOD(7, 3)` is `1` | not on a public page; kept for the earlier evaluator; the legacy remainder: \|a\| mod \|b\| with the divisor's sign; MODULUS is the documented one |
| `EXP` | implemented (compat) | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `EXP(LN(5))` is `5` | not on a public page; kept for the earlier evaluator; not in the public reference; accepted because the repository's earlier evaluator evaluated it |
| `LOG` | implemented (compat) | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `LOG(1000)` is `3` | not on a public page; kept for the earlier evaluator; base-10 logarithm, as the earlier evaluator treated it |
| `AVG` | implemented (compat) | 1-... | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `AVG(1, 2, 3)` is `2` | not on a public page; kept for the earlier evaluator; not in the public reference; accepted because the repository's earlier evaluator evaluated it |
| `AVERAGE` | implemented (compat) | 1-... | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `AVERAGE(2 in, 4 in)` is `3 in` | not on a public page; kept for the earlier evaluator; not in the public reference; accepted because the repository's earlier evaluator evaluated it |
| `SIN` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 4 cases, e.g. `SIN(45 deg)` is `0.7071067811865476` | function page |
| `COS` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `COS(45 deg)` is `0.7071067811865476` | function page |
| `TAN` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `TAN(45 deg)` is `1` | function page |
| `SINH` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `SINH(45 deg)` is `0.8687` | function page |
| `COSH` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `COSH(45 deg)` is `1.3246` | function page |
| `TANH` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `TANH(-45 deg)` is `-0.6558` | function page |
| `ASIN` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `ASIN(1)` is `1.5707963267948966 rad` | function page |
| `ACOS` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `ACOS(0)` is `1.5707963267948966 rad` | function page |
| `ATAN` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `ATAN(1)` is `0.7853981633974483 rad` | function page |
| `ATAN2` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 6 cases, e.g. `ATAN2(1.25, 2.25)` is `29.0546 deg` | function page |
| `DEG` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `DEG(PI() / 4)` is `45` | function page |
| `RAD` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `RAD(45)` is `0.7853981633974483` | function page |
| `ANG360` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 6 cases, e.g. `ANG360(395 deg)` is `35 deg` | function page |
| `GRAVITY` | implemented | 1-3 | `functions::math_functions_give_the_reference_values_and_errors`: 7 cases, e.g. `GRAVITY(2)` is `3.141592653589793 rad` | function page |
| `PNT` | implemented | 2 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `PNTX(PNT(7, 12))` is `7` | function page |
| `PNTX` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 3 cases, e.g. `PNTX(PNT(7, 12))` is `7` | function page |
| `PNTY` | implemented | 1 | `functions::math_functions_give_the_reference_values_and_errors`: 2 cases, e.g. `PNTY(PNT(7, 12))` is `12` | function page |
| `IF` | implemented | 3 | `functions::logic_functions_select_lazily_and_report_errors`: 11 cases, e.g. `IF(Height > 1.25 in, 5, 7)` is `5` | function page |
| `IFERROR` | implemented | 2 | `functions::logic_functions_select_lazily_and_report_errors`: 5 cases, e.g. `IFERROR(1/0, 5)` is `5` | function page |
| `AND` | implemented | 1-... | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `AND(Height > 1, Width > 1)` is `true` | function page |
| `OR` | implemented | 1-... | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `OR(Height > 1, Width > 9)` is `true` | function page |
| `NOT` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `NOT(Height > 0.75 in)` is `false` | function page |
| `ISERR` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 5 cases, e.g. `ISERR(1/0)` is `true` | function page |
| `ISERRNA` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `ISERRNA(NA())` is `true` | function page |
| `ISERROR` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `ISERROR(NA())` is `true` | function page |
| `ISERRVALUE` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `ISERRVALUE(PNT(1, 2) + 1)` is `true` | function page |
| `NA` | implemented | 0 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `NA()` is `#N/A!` | function page |
| `REF` | implemented | 0 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `REF()` is `#REF!` | function page |
| `CHOOSE` | implemented (compat) | 2-... | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `CHOOSE(2, 10, 20, 1/0)` is `20` | not on a public page; kept for the earlier evaluator; not in the public reference; 1-based selection as the earlier evaluator did |
| `SWITCH` | implemented (compat) | 3-... | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `SWITCH(3, 1, 10, 3, 30, 99)` is `30` | not on a public page; kept for the earlier evaluator; not in the public reference; value of the first equal case, else the trailing default |
| `GUARD` | implemented (partial) | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `GUARD(5)` is `5` | function page; evaluates to its argument; protecting the cell from edits is up to the host |
| `BOUND` | implemented (partial) | 5-... | `functions::logic_functions_select_lazily_and_report_errors`: 5 cases, e.g. `BOUND(5, 0, FALSE, 2, 8)` is `5` | function page; evaluates to its value argument; the range is applied when the host writes the cell |
| `DEPENDSON` | implemented (partial) | 1-... | `functions::logic_functions_select_lazily_and_report_errors`: 3 cases, e.g. `DEPENDSON(Width, Height)` is `false` | function page; always FALSE; the dependencies come from the references in the formula |
| `SETATREF` | implemented (partial) | 1-3 | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `SETATREF(Width)` is `4 in` | function page; evaluates the reference; redirecting edits is up to the host |
| `SETATREFEVAL` | implemented (partial) | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 2 cases, e.g. `SETATREFEVAL(2 + 3)` is `5` | function page; evaluates its argument; redirecting edits is up to the host |
| `SETATREFEXPR` | implemented (partial) | 0-1 | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `SETATREFEXPR(5)` is `5` | function page; evaluates its argument (0 if omitted); storing edits is up to the host |
| `FORMULAEXISTS` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 5 cases, e.g. `FORMULAEXISTS(Width)` is `true` | function page |
| `LOCALFORMULAEXISTS` | implemented | 1 | `functions::logic_functions_select_lazily_and_report_errors`: 4 cases, e.g. `LOCALFORMULAEXISTS(Width)` is `true` | function page |
| `LEFT` | implemented | 1-2 | `functions::text_functions_give_the_reference_strings_and_errors`: 7 cases, e.g. `LEFT("January 1, 2004", 3)` is `"Jan"` | function page |
| `RIGHT` | implemented | 1-2 | `functions::text_functions_give_the_reference_strings_and_errors`: 5 cases, e.g. `RIGHT("January 1, 2004", 4)` is `"2004"` | function page |
| `MID` | implemented | 3 | `functions::text_functions_give_the_reference_strings_and_errors`: 6 cases, e.g. `MID("SSN 999-99-9999", 5, 11)` is `"999-99-9999"` | function page |
| `LEN` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 4 cases, e.g. `LEN("Seattle, WA")` is `11` | function page |
| `LOWER` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 3 cases, e.g. `LOWER("mIxEd CAse")` is `"mixed case"` | function page |
| `UPPER` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 2 cases, e.g. `UPPER("mIxEd CAse")` is `"MIXED CASE"` | function page |
| `TRIM` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 3 cases, e.g. `TRIM("January 1, 2003 ")` is `"January 1, 2003"` | function page |
| `FIND` | implemented | 2-4 | `functions::text_functions_give_the_reference_strings_and_errors`: 10 cases, e.g. `FIND("2003", "January 1, 2003")` is `12` | function page |
| `SUBSTITUTE` | implemented | 3-5 | `functions::text_functions_give_the_reference_strings_and_errors`: 8 cases, e.g. `SUBSTITUTE("1 January 2003", "January", "JAN")` is `"1 JAN 2003"` | function page |
| `REPLACE` | implemented | 4 | `functions::text_functions_give_the_reference_strings_and_errors`: 5 cases, e.g. `REPLACE("01/03/2002", 9, 2, "03")` is `"01/03/2003"` | function page |
| `REPT` | implemented | 2 | `functions::text_functions_give_the_reference_strings_and_errors`: 6 cases, e.g. `REPT("*", 5)` is `"*****"` | function page |
| `CHAR` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 5 cases, e.g. `CHAR(9)` is `"\t"` | function page |
| `UNICHAR` | implemented | 1 | `functions::text_functions_give_the_reference_strings_and_errors`: 5 cases, e.g. `UNICHAR(65)` is `"A"` | function page |
| `INDEX` | implemented | 2-4 | `functions::text_functions_give_the_reference_strings_and_errors`: 8 cases, e.g. `INDEX(3, "cat;rat;;goat")` is `"goat"` | function page |
| `LOOKUP` | implemented | 2-3 | `functions::text_functions_give_the_reference_strings_and_errors`: 6 cases, e.g. `LOOKUP("rat", "cat;rat;;goat")` is `1` | function page |
| `STRSAME` | implemented | 2-3 | `functions::text_functions_give_the_reference_strings_and_errors`: 6 cases, e.g. `STRSAME("cat", "cat")` is `true` | function page |
| `STRSAMEEX` | implemented (partial) | 4 | `functions::text_functions_give_the_reference_strings_and_errors`: 4 cases, e.g. `STRSAMEEX("cat", "CAT", 0, 1)` is `true` | function page; only the ignore-case flag (1) is modelled; the locale is ignored |
| `SHAPETEXT` | implemented (partial) | 1-2 | `functions::text_functions_give_the_reference_strings_and_errors`: 10 cases, e.g. `SHAPETEXT(TheText)` is `"Hello"` | function page; flags 4, 16, 32 and 64 are modelled; 1, 2 and 8 need field and hyphenation data and give the unsupported error |
| `TEXTWIDTH` | implemented | 1-2 | `functions::text_functions_give_the_reference_strings_and_errors`: 3 cases, e.g. `TEXTWIDTH(TheText)` is `0.5 in` | function page |
| `TEXTHEIGHT` | implemented | 1-2 | `functions::text_functions_give_the_reference_strings_and_errors`: 2 cases, e.g. `TEXTHEIGHT(TheText, Width - 0.5 in)` is `0.2 in` | function page |
| `RGB` | implemented | 3 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `RGB(0, 0, 255)` is `#0000ff` | function page |
| `HSL` | implemented | 3 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 11 cases, e.g. `HSL(160, 240, 120)` is `#0000ff` | function page |
| `HUE` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 6 cases, e.g. `HUE(HSL(10, 240, 120))` is `10` | function page |
| `SAT` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 4 cases, e.g. `SAT(HSL(10, 240, 120))` is `240` | function page |
| `LUM` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 4 cases, e.g. `LUM(HSL(10, 20, 30))` is `30` | function page |
| `RED` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 4 cases, e.g. `RED(RGB(10, 20, 30))` is `10` | function page |
| `GREEN` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `GREEN(RGB(10, 20, 30))` is `20` | function page |
| `BLUE` | implemented | 1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `BLUE(RGB(10, 20, 30))` is `30` | function page |
| `HUEDIFF` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `HUEDIFF(RGB(0, 0, 255), RGB(255, 0, 0))` is `160` | function page |
| `SATDIFF` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 2 cases, e.g. `SATDIFF(RGB(0, 0, 255), RGB(128, 128, 128))` is `240` | function page |
| `LUMDIFF` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `LUMDIFF(RGB(255, 255, 255), RGB(0, 0, 0))` is `240` | function page |
| `TINT` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 8 cases, e.g. `LUM(TINT(RGB(255, 0, 0), 60))` is `180` | function page |
| `SHADE` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `LUM(SHADE(RGB(255, 0, 0), 60))` is `60` | function page |
| `TONE` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 4 cases, e.g. `SAT(TONE(RGB(255, 0, 0), 120))` is `120` | function page |
| `MSOTINT` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 6 cases, e.g. `MSOTINT(RGB(100, 100, 100), 100)` is `#ffffff` | function page |
| `MSOSHADE` | implemented | 2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `MSOSHADE(RGB(100, 100, 100), 100)` is `#000000` | function page |
| `BLEND` | implemented | 3 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 6 cases, e.g. `BLEND(RGB(0, 0, 0), RGB(255, 255, 255), 0)` is `#000000` | function page |
| `THEMEVAL` | implemented | 0-2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 11 cases, e.g. `THEMEVAL()` is `#202020` | function page |
| `THEME` | implemented | 0-2 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 2 cases, e.g. `THEME("Light")` is `#f0f0f0` | function page |
| `THEMEGUARD` | implemented (partial) | 0-1 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 2 cases, e.g. `THEMEGUARD(THEME("Light"))` is `#f0f0f0` | function page; evaluates to its argument; guarding is up to the host |
| `ISTHEMED` | implemented | 0 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 3 cases, e.g. `ISTHEMED()` is `true` | function page |
| `CELLISTHEMED` | implemented (compat) | 0 | `functions::colour_functions_follow_the_240_scale_and_report_errors`: 2 cases, e.g. `CELLISTHEMED()` is `true` | not on a public page; kept for the earlier evaluator; the older name of ISTHEMED, listed in the grammar |
| `ID` | implemented | 0 | `functions::host_functions_read_the_services_or_report_unsupported`: 2 cases, e.g. `ID()` is `7` | function page |
| `NAME` | implemented (partial) | 0-1 | `functions::host_functions_read_the_services_or_report_unsupported`: 3 cases, e.g. `NAME()` is `"Box.7"` | function page; the language argument (0 local, 750 universal) is ignored: the host supplies one name |
| `PAGENAME` | implemented (partial) | 0-1 | `functions::host_functions_read_the_services_or_report_unsupported`: 2 cases, e.g. `PAGENAME()` is `"Page-1"` | function page; the language argument (0 local, 750 universal) is ignored: the host supplies one name |
| `MASTERNAME` | implemented (partial) | 0-1 | `functions::host_functions_read_the_services_or_report_unsupported`: 2 cases, e.g. `MASTERNAME()` is `"<no master>"` | function page; the language argument (0 local, 750 universal) is ignored: the host supplies one name |
| `IS1D` | implemented | 0 | `functions::host_functions_read_the_services_or_report_unsupported`: 2 cases, e.g. `IS1D()` is `false` | function page |
| `RAND` | implemented | 0 | `functions::host_functions_read_the_services_or_report_unsupported`: 2 cases, e.g. `RAND()` is `0.25` | function page |
| `ANGLEALONGPATH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `ANGLETOLOC` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `ANGLETOPAR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `ARG` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; calls a custom function or evaluates text as a formula; not implemented |
| `BKGPAGENAME` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `BLOB` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `BOUNDINGBOXDIST` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `BOUNDINGBOXRECT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `CALLOUTCOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CALLOUTTARGETREF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CALLTHIS` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `CAT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `CATEGORY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `COMPANY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CONTAINERCOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CONTAINERMEMBERCOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CONTAINERSHEETREF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CREATOR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `CY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `DATA1` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DATA2` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DATA3` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DATE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `DATETIME` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `DATEVALUE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `DAY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `DAYOFYEAR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `DECIMALSEP` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; depends on the user's locale settings; not implemented |
| `DEFAULTEVENT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `DESCRIPTION` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DIRECTORY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DISTTOPATH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `DOCCREATION` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DOCLASTEDIT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DOCLASTPRINT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DOCLASTSAVE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `DOCMD` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `DOOLEVERB` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `EEQ` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `EGE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `EGT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `ELE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `ELT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `ENE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `EVALCELL` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; calls a custom function or evaluates text as a formula; not implemented |
| `EVALTEXT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; calls a custom function or evaluates text as a formula; not implemented |
| `FEQ` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FGE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FGT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FIELDPICTURE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `FILENAME` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `FLE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FLT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FNE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `FONT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `FONTTOID` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `FORMAT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `FORMATEX` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `GETREF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `GETVAL` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `GOTOPAGE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `HASCATEGORY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `HELP` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `HOUR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `HYPERLINK` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `HYPERLINKBASE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `INTERSECTX` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `INTERSECTY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `KEYWORDS` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `LANGUAGE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; depends on the user's locale settings; not implemented |
| `LISTMEMBERCOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `LISTORDER` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `LISTSEP` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; depends on the user's locale settings; not implemented |
| `LISTSHEETREF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `LOC` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; depends on the user's locale settings; not implemented |
| `LOCTOLOC` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `LOCTOPAR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `MANAGER` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `MINUTE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `MONTH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `NEARESTPOINTONPATH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `NOW` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `NURBS` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `OPENFILE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `OPENGROUPWIN` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `OPENSHEETWIN` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `OPENTEXTWIN` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `PAGECOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `PAGENUMBER` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `PAR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `PATHLENGTH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `PATHSEGMENT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `PLAYSOUND` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `POINTALONGPATH` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `POLYLINE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `QUEUEMARKEREVENT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `RECTSECT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `REWIDEN` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; format pictures, currency and text widening are not implemented |
| `RUNADDON` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `RUNADDONWARGS` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `RUNMACRO` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `SECOND` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `SEGMENTCOUNT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; geometry or path computation that needs shape transforms or geometry rows; not implemented yet |
| `SETF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `SHEETREF` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `SUBJECT` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `THEMECBV` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; needs the gradient stops of the active theme |
| `THEMEPROP` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | [MS-VSDX] 2.5.1 function token only; named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour |
| `THEMERESTORE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; stores a value for the host to restore; no value is computed |
| `TIME` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `TIMEVALUE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `TITLE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `TYPE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `TYPEDESC` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `USE` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `USERUI` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; performs an action or has a side effect instead of computing a value |
| `VERSION` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; reads a document, page or shape property that only the host can supply |
| `WEEKDAY` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |
| `YEAR` | unsupported | - | `functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error) | function page; date and time values need calendar conversion; no date library is a dependency of this crate |

## Shared code (craft-libs)

Recommendation for the maintainer; nothing from `storytold/craft-libs` is used in this change, and no
dependency on it is added (LayoutCraft builds offline from the crates in `rust/Cargo.lock`).

Checked (read through `gh api`, workspace of about 190 crates under `crates/`; every crate named below is
`MIT OR Apache-2.0`, `unsafe_code` forbidden by the workspace, and the format crates carry the same
`deny(clippy::unwrap_used, ...)` lints as this crate). The repository README says GridCraft owns its
workbook model and that craft-libs only holds `craft-gridcraft-format`, a JSON reader and writer, so
GridCraft's calculation code is not in craft-libs. The v0.1.0 tag covers five unrelated crates (ui,
settings, quit, atomic, print); anything else would be a `rev` dependency.

| area | candidate | fit | recommendation |
|---|---|---|---|
| lexer, parser, evaluator | none in craft-libs. Nearest: `crates/xls/src/expr.rs` (BIFF8 formula AST and printer, `pub(crate)`, no parser or evaluator), `crates/odf/src/text/cell_formula.rs` (8-function converter) | the Excel and ODF dialects differ from this one: unit suffixes (`1.5 in`), `Sheet.5!Width`, `Pages[..]!`, `#rrggbb` literals, word comparisons, `#DIM!` | do not reuse; nothing to reuse |
| dependency graph, recalculation | none | `craft-sheet` has `CellRef`/`Range` (A1 text), `Value`, `CellError` (no `#DIM!`; `#[non_exhaustive]`) and sheet-name quoting, but no graph | do not reuse |
| number formatting | `crates/drawingml/src/numfmt.rs`: `format(code, v, lang)`, 15 significant digits; `crates/xls/src/expr.rs`: private `number_to_text` | `numfmt` renders Excel format codes and pulls in `chrono`, `craft-xml`, `kurbo`, `log`, `serde`; this crate needs plain "15 digits, no trailing zeros" text (`format_number`, about ten lines) | do not reuse now; revisit when the `FORMAT`/`FORMATEX` pictures (unsupported here) are implemented, since `numfmt` could render them |
| unit conversion | `crates/docx/src/units.rs` (`measure`, `f32`, points, OOXML cap), `crates/latex/src/units.rs` | neither is a dimension system; this crate needs `f64`, exponents and the 24 units above | do not reuse |
| colour (HSL/RGB) | `crates/color` (`rgb_to_hsl` on `f32` 0..1, video focus, `serde`), `crates/math/src/rgb.rs` (Office colour transforms, `from_hsl` on `f64`), `crates/svg` (CSS 0..360) | none uses the 0..240 integer scale or Visio's tint/shade/blend | do not reuse; `color.rs` here is about 150 lines |

What adopting any of it would take: a git dependency pinned by `rev`, an entry in
`THIRD-PARTY-NOTICES.txt`, the new transitive crates in the offline cache, and a wasm check. Open
questions for the maintainer: does GridCraft's engine (in its own repository) have a lexer or
recalculation graph worth comparing, and under which licence; and should this crate be offered to
craft-libs (it holds format libraries, not engines, so it may not fit there).

## Tests

All under `tests/it/` (one integration binary, as the testing rules ask):

| file | what it guards |
|---|---|
| `parse.rs` | lexer and parser: precedence, grouping, every reference form, spans, limits |
| `arithmetic.rs` | every operator, order of evaluation, operand conversion, tolerance, dimensions, errors, dates |
| `units.rs` | the unit table, implicit and area units, dimension range, number display |
| `cases.rs`, `functions.rs` | one case table for every implemented function (a normal and an edge or error case each), table consistency, argument counts, the grammar's function tokens |
| `references.rs`, `host.rs` | every reference form through a test host with shapes, pages, masters, styles |
| `engine.rs` | dependency order, incremental recalculation, early cut-off, cycles, removing and redefining cells, 100 000-cell chains on a 256 KiB stack, fan-out, bulk removal, the regression cases below |
| `differential.rs` | the earlier evaluators (see above) |
| `hostile.rs` | mutated and random formulas, every function with random arguments, deep nesting, huge numbers, long strings, unicode, random engine edits checked against a calculation from scratch |

The hostile sweeps use a fixed-seed generator and run `CRAFT_HOSTILE_DIVISOR`-scaled case counts; a hang
fails through a deadline that is not itself asserted.

Bugs the tests found (each has a named regression test or case):

- a word comparison operator before `(` (`1 EQ (2)`) was not read as an operator;
- the implicit unit form did not accept the space the units page writes (`5 [in, P]`);
- `ROUND`/`TRUNC` with a huge negative digit count overflowed an `i64` negation (a panic in debug builds);
- a function with "any number" of arguments refused more than 255;
- a constant set to the value it already had re-evaluated all its readers;
- a formula that failed to parse was not reported as changed, and replacing it before the next
  recalculation hid the change from its readers;
- a cell that left a cycle kept `#CIRC!` when its input stayed circular, and so did a reader of a cell that
  moved from one cycle to another;
- the engine's bookkeeping was quadratic in the number of readers of a cell and in bulk removals;
- a shape named `ThePage`, `TheDoc` or `Styles` was displayed as the container and read back as it;
- `BOUND` refused the omitted first argument of its page's own example; `SHAPETEXT` ignored its flags;
  `ISTHEMED` without parentheses (as its page writes it) was a cell reference.

## Checks

```sh
cargo test -p layoutcraft-formula
cargo clippy -p layoutcraft-formula --lib --tests --examples --no-deps -- -D warnings
cargo fmt -p layoutcraft-formula --check
LAYOUTCRAFT_PUBLIC_BUILD=1 cargo test --workspace
```

Clippy is run with `--lib --tests --examples --no-deps` for this crate. `--all-targets` over the
workspace fails today on warnings that already exist in `layoutcraft-vsdx` (unused code, `chunks_exact`,
`is_multiple_of`), which this crate's dev-dependency on it would otherwise inherit; `--no-deps` lints
only this crate.

## Not done

- Not wired into the importer: the host that resolves `Sheet.N!`, names and masters and supplies theme
  and palette is a separate change, as is replacing `formula.rs` and `colorexpr.rs`.
- Unsupported (the table above lists each): date and time functions, document and page properties,
  geometry and path functions, actions and add-on calls, format pictures, currency, locale functions,
  `EVALTEXT`/`EVALCELL`, the `THEMECBV`/`THEMERESTORE` pair, `THEMEPROP`, and the grammar-only tokens
  `CAT`, `EEQ`..`ENE`, `FEQ`..`FNE`. Partial: `SHAPETEXT` flags 1, 2, 8; `STRSAMEEX` flags other than 1;
  the language argument of `NAME`, `PAGENAME`, `MASTERNAME`; `GUARD`, `BOUND`, `SETATREF*`, `THEMEGUARD`
  (value only; the host acts on them).
- Open against the product: the HSL scale of the colour pages (decision D10) and `MOD` against `MODULUS`
  are verified only against the public pages; checking them against Visio by a COM probe on a
  maintainer's machine, as `AGENTS.md` rule 26 asks, is still to do.
