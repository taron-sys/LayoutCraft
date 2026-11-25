//! Prints the function table of `docs/46-formula-engine.md` as Markdown:
//! `cargo run -p layoutcraft-formula --example function_table`.
//!
//! The test column comes from the case lists the integration tests run (`tests/it/cases.rs` is
//! compiled into this example), so the table cannot name a test that does not exist.

#[path = "../tests/it/cases.rs"]
mod cases;

use layoutcraft_formula::{functions, Status};

/// Functions that have a page under "Functions (Visio ShapeSheet Reference)".
const PAGES: &str = "
    ABS ACOS AND ANG360 ANGLEALONGPATH ANGLETOLOC ANGLETOPAR ARG ASIN ATAN ATAN2 BITAND BITNOT BITOR
    BITXOR BKGPAGENAME BLEND BLOB BLUE BOUND BOUNDINGBOXDIST BOUNDINGBOXRECT CALLOUTCOUNT
    CALLOUTTARGETREF CALLTHIS CATEGORY CEILING CHAR COMPANY CONTAINERCOUNT CONTAINERMEMBERCOUNT
    CONTAINERSHEETREF COS COSH CREATOR CY DATA1 DATA2 DATA3 DATE DATETIME DATEVALUE DAY DAYOFYEAR
    DECIMALSEP DEFAULTEVENT DEG DEPENDSON DESCRIPTION DIRECTORY DISTTOPATH DOCCREATION DOCLASTEDIT
    DOCLASTPRINT DOCLASTSAVE DOCMD DOOLEVERB EVALCELL EVALTEXT FIELDPICTURE FILENAME FIND FLOOR FONT
    FONTTOID FORMAT FORMATEX FORMULAEXISTS GETREF GETVAL GOTOPAGE GRAVITY GREEN GUARD HASCATEGORY HELP
    HOUR HSL HUE HUEDIFF HYPERLINK HYPERLINKBASE ID IF IFERROR INDEX INT INTERSECTX INTERSECTY INTUP
    IS1D ISERR ISERRNA ISERROR ISERRVALUE ISTHEMED KEYWORDS LANGUAGE LEFT LEN LISTMEMBERCOUNT LISTORDER
    LISTSEP LISTSHEETREF LN LOC LOCALFORMULAEXISTS LOCTOLOC LOCTOPAR LOG10 LOOKUP LOWER LUM LUMDIFF
    MAGNITUDE MANAGER MASTERNAME MAX MID MIN MINUTE MODULUS MONTH MSOSHADE MSOTINT NA NAME
    NEARESTPOINTONPATH NOT NOW NURBS OPENFILE OPENGROUPWIN OPENSHEETWIN OPENTEXTWIN OR PAGECOUNT
    PAGENAME PAGENUMBER PAR PATHLENGTH PATHSEGMENT PI PLAYSOUND PNT PNTX PNTY POINTALONGPATH POLYLINE
    POW QUEUEMARKEREVENT RAD RAND RECTSECT RED REF REPLACE REPT REWIDEN RGB RIGHT ROUND RUNADDON
    RUNADDONWARGS RUNMACRO SAT SATDIFF SECOND SEGMENTCOUNT SETATREF SETATREFEVAL SETATREFEXPR SETF
    SHADE SHAPETEXT SHEETREF SIGN SIN SINH SQRT STRSAME STRSAMEEX SUBJECT SUBSTITUTE SUM TAN TANH
    TEXTHEIGHT TEXTWIDTH THEME THEMECBV THEMEGUARD THEMERESTORE THEMEVAL TIME TIMEVALUE TINT TITLE TONE
    TRIM TRUNC TYPE TYPEDESC UNICHAR UPPER USE USERUI VERSION WEEKDAY YEAR
";

/// Markdown table cells must not contain a bare pipe.
fn cell(text: &str) -> String {
    text.replace('|', "\\|")
}

fn main() {
    let cases = cases::all_cases();
    println!("| function | status | arguments | test | source and notes |");
    println!("|---|---|---|---|---|");
    for info in functions() {
        let status = match info.status {
            Status::Full => "implemented",
            Status::Partial => "implemented (partial)",
            Status::Compat => "implemented (compat)",
            Status::Unsupported => "unsupported",
        };
        let max = if info.max_args == u8::MAX {
            "...".to_string()
        } else {
            info.max_args.to_string()
        };
        let arguments = if info.status == Status::Unsupported {
            "-".to_string()
        } else if max == info.min_args.to_string() {
            max
        } else {
            format!("{}-{max}", info.min_args)
        };
        let mine: Vec<&cases::Case> = cases.iter().filter(|c| c.function == info.name).collect();
        let test = match mine.first() {
            Some(first) => {
                let chosen = mine
                    .iter()
                    .find(|c| !c.edge && !c.bare_host)
                    .copied()
                    .unwrap_or(first);
                format!(
                    "`functions::{}`: {} cases, e.g. `{}` is `{}`",
                    first.test,
                    mine.len(),
                    cell(chosen.formula),
                    cell(&chosen.want.show())
                )
            }
            None => "`functions::the_function_table_has_one_row_per_name_in_upper_case` (calling it gives the unsupported error)".to_string(),
        };
        let source = if info.status == Status::Compat {
            "not on a public page; kept for the earlier evaluator"
        } else if PAGES.split_whitespace().any(|name| name == info.name) {
            "function page"
        } else if info.name == "CELLISTHEMED" {
            "named in the grammar; the ISTHEMED page says ISTHEMED replaces it"
        } else {
            "[MS-VSDX] 2.5.1 function token only"
        };
        let note = if info.note.is_empty() {
            String::new()
        } else {
            format!("; {}", cell(info.note))
        };
        println!(
            "| `{}` | {status} | {arguments} | {test} | {source}{note} |",
            info.name
        );
    }
}
