//! The function table: one row per function named by the public reference or the grammar.

use super::{color, host, logic, math, text, EagerFn, FuncInfo, Imp, LazyFn, Status};

/// Any number of arguments.
const ANY: u8 = u8::MAX;

const DATE: &str =
    "date and time values need calendar conversion; no date library is a dependency of this crate";
const DOC: &str = "reads a document, page or shape property that only the host can supply";
const ACTION: &str = "performs an action or has a side effect instead of computing a value";
const GEOMETRY: &str = "geometry or path computation that needs shape transforms or geometry rows; not implemented yet";
const FORMAT: &str = "format pictures, currency and text widening are not implemented";
const CUSTOM: &str = "calls a custom function or evaluates text as a formula; not implemented";
const LOCALE: &str = "depends on the user's locale settings; not implemented";
const GRAMMAR_ONLY: &str =
    "named in the [MS-VSDX] formula grammar (2.5.1) but no public source defines its behaviour";

const fn row(
    name: &'static str,
    status: Status,
    min: u8,
    max: u8,
    note: &'static str,
    imp: Imp,
) -> FuncInfo {
    FuncInfo {
        name,
        status,
        min_args: min,
        max_args: max,
        note,
        imp,
    }
}

const fn eager(name: &'static str, min: u8, max: u8, f: EagerFn) -> FuncInfo {
    row(name, Status::Full, min, max, "", Imp::Eager(f))
}

const fn partial(name: &'static str, min: u8, max: u8, f: EagerFn, note: &'static str) -> FuncInfo {
    row(name, Status::Partial, min, max, note, Imp::Eager(f))
}

const fn compat(name: &'static str, min: u8, max: u8, f: EagerFn, note: &'static str) -> FuncInfo {
    row(name, Status::Compat, min, max, note, Imp::Eager(f))
}

const fn raw(name: &'static str, min: u8, max: u8, f: EagerFn) -> FuncInfo {
    row(name, Status::Full, min, max, "", Imp::Raw(f))
}

const fn lazy(name: &'static str, min: u8, max: u8, f: LazyFn) -> FuncInfo {
    row(name, Status::Full, min, max, "", Imp::Lazy(f))
}

const fn lazy_partial(
    name: &'static str,
    min: u8,
    max: u8,
    f: LazyFn,
    note: &'static str,
) -> FuncInfo {
    row(name, Status::Partial, min, max, note, Imp::Lazy(f))
}

const fn lazy_compat(
    name: &'static str,
    min: u8,
    max: u8,
    f: LazyFn,
    note: &'static str,
) -> FuncInfo {
    row(name, Status::Compat, min, max, note, Imp::Lazy(f))
}

const fn unsupported(name: &'static str, note: &'static str) -> FuncInfo {
    row(name, Status::Unsupported, 0, ANY, note, Imp::None)
}

const LEGACY: &str =
    "not in the public reference; accepted because the repository's earlier evaluator evaluated it";

static TABLE: &[FuncInfo] = &[
    // Arithmetic and rounding.
    eager("ABS", 1, 1, math::abs),
    eager("ADD", 2, 2, math::add),
    eager("SUB", 2, 2, math::sub),
    eager("MUL", 2, 2, math::mul),
    eager("DIV", 2, 2, math::div),
    eager("UMINUS", 1, 1, math::uminus),
    eager("UPLUS", 1, 1, math::uplus),
    eager("PCT", 1, 1, math::pct),
    eager("CEILING", 1, 2, math::ceiling),
    eager("FLOOR", 1, 2, math::floor),
    eager("INT", 1, 1, math::int),
    eager("INTUP", 1, 1, math::intup),
    eager("ROUND", 2, 2, math::round),
    eager("TRUNC", 2, 2, math::trunc),
    eager("SIGN", 1, 2, math::sign),
    eager("SQRT", 1, 1, math::sqrt),
    eager("POW", 2, 2, math::pow),
    eager("LN", 1, 1, math::ln),
    eager("LOG10", 1, 1, math::log10),
    eager("PI", 0, 0, math::pi),
    eager("MODULUS", 2, 2, math::modulus),
    eager("MAX", 1, ANY, math::max),
    eager("MIN", 1, ANY, math::min),
    eager("SUM", 1, ANY, math::sum),
    eager("MAGNITUDE", 4, 4, math::magnitude),
    eager("BITAND", 2, 2, math::bit_and),
    eager("BITOR", 2, 2, math::bit_or),
    eager("BITXOR", 2, 2, math::bit_xor),
    eager("BITNOT", 1, 1, math::bit_not),
    compat(
        "MOD",
        2,
        2,
        math::legacy_mod,
        "the legacy remainder: |a| mod |b| with the divisor's sign; MODULUS is the documented one",
    ),
    compat("EXP", 1, 1, math::exp, LEGACY),
    compat(
        "LOG",
        1,
        1,
        math::log10,
        "base-10 logarithm, as the earlier evaluator treated it",
    ),
    compat("AVG", 1, ANY, math::average, LEGACY),
    compat("AVERAGE", 1, ANY, math::average, LEGACY),
    // Angles and trigonometry.
    eager("SIN", 1, 1, math::sin),
    eager("COS", 1, 1, math::cos),
    eager("TAN", 1, 1, math::tan),
    eager("SINH", 1, 1, math::sinh),
    eager("COSH", 1, 1, math::cosh),
    eager("TANH", 1, 1, math::tanh),
    eager("ASIN", 1, 1, math::asin),
    eager("ACOS", 1, 1, math::acos),
    eager("ATAN", 1, 1, math::atan),
    eager("ATAN2", 2, 2, math::atan2),
    eager("DEG", 1, 1, math::deg),
    eager("RAD", 1, 1, math::rad),
    eager("ANG360", 1, 1, math::ang360),
    eager("GRAVITY", 1, 3, math::gravity),
    // Points.
    eager("PNT", 2, 2, math::pnt),
    eager("PNTX", 1, 1, math::pnt_x),
    eager("PNTY", 1, 1, math::pnt_y),
    // Logic and errors.
    lazy("IF", 3, 3, logic::if_),
    lazy("IFERROR", 2, 2, logic::if_error),
    eager("AND", 1, ANY, logic::and),
    eager("OR", 1, ANY, logic::or),
    eager("NOT", 1, 1, logic::not),
    raw("ISERR", 1, 1, logic::is_err),
    raw("ISERRNA", 1, 1, logic::is_err_na),
    raw("ISERROR", 1, 1, logic::is_error),
    raw("ISERRVALUE", 1, 1, logic::is_err_value),
    eager("NA", 0, 0, logic::na),
    eager("REF", 0, 0, logic::ref_error),
    lazy_compat(
        "CHOOSE",
        2,
        ANY,
        logic::choose,
        "not in the public reference; 1-based selection as the earlier evaluator did",
    ),
    lazy_compat(
        "SWITCH",
        3,
        ANY,
        logic::switch,
        "not in the public reference; value of the first equal case, else the trailing default",
    ),
    partial(
        "GUARD",
        1,
        1,
        logic::guard,
        "evaluates to its argument; protecting the cell from edits is up to the host",
    ),
    partial(
        "BOUND",
        5,
        ANY,
        logic::bound,
        "evaluates to its value argument; the range is applied when the host writes the cell",
    ),
    lazy_partial(
        "DEPENDSON",
        1,
        ANY,
        logic::depends_on,
        "always FALSE; the dependencies come from the references in the formula",
    ),
    lazy_partial(
        "SETATREF",
        1,
        3,
        logic::set_at_ref,
        "evaluates the reference; redirecting edits is up to the host",
    ),
    lazy_partial(
        "SETATREFEVAL",
        1,
        1,
        logic::set_at_ref_eval,
        "evaluates its argument; redirecting edits is up to the host",
    ),
    lazy_partial(
        "SETATREFEXPR",
        0,
        1,
        logic::set_at_ref_expr,
        "evaluates its argument (0 if omitted); storing edits is up to the host",
    ),
    lazy("FORMULAEXISTS", 1, 1, logic::formula_exists_any),
    lazy("LOCALFORMULAEXISTS", 1, 1, logic::local_formula_exists),
    // Text.
    eager("LEFT", 1, 2, text::left),
    eager("RIGHT", 1, 2, text::right),
    eager("MID", 3, 3, text::mid),
    eager("LEN", 1, 1, text::len),
    eager("LOWER", 1, 1, text::lower),
    eager("UPPER", 1, 1, text::upper),
    eager("TRIM", 1, 1, text::trim),
    eager("FIND", 2, 4, text::find),
    eager("SUBSTITUTE", 3, 5, text::substitute),
    eager("REPLACE", 4, 4, text::replace),
    eager("REPT", 2, 2, text::rept),
    eager("CHAR", 1, 1, text::char_),
    eager("UNICHAR", 1, 1, text::unichar),
    eager("INDEX", 2, 4, text::index),
    eager("LOOKUP", 2, 3, text::lookup),
    eager("STRSAME", 2, 3, text::str_same),
    partial(
        "STRSAMEEX",
        4,
        4,
        text::str_same_ex,
        "only the ignore-case flag (1) is modelled; the locale is ignored",
    ),
    eager("SHAPETEXT", 1, 2, text::shape_text),
    eager("TEXTWIDTH", 1, 2, text::text_width),
    eager("TEXTHEIGHT", 1, 2, text::text_height),
    // Colour and theme.
    eager("RGB", 3, 3, color::rgb),
    eager("HSL", 3, 3, color::hsl),
    eager("HUE", 1, 1, color::hue),
    eager("SAT", 1, 1, color::sat),
    eager("LUM", 1, 1, color::lum),
    eager("RED", 1, 1, color::red),
    eager("GREEN", 1, 1, color::green),
    eager("BLUE", 1, 1, color::blue),
    eager("HUEDIFF", 2, 2, color::hue_diff),
    eager("SATDIFF", 2, 2, color::sat_diff),
    eager("LUMDIFF", 2, 2, color::lum_diff),
    eager("TINT", 2, 2, color::tint),
    eager("SHADE", 2, 2, color::shade),
    eager("TONE", 2, 2, color::tone),
    eager("MSOTINT", 2, 2, color::mso_tint),
    eager("MSOSHADE", 2, 2, color::mso_shade),
    eager("BLEND", 3, 3, color::blend),
    eager("THEMEVAL", 0, 2, color::theme_val),
    eager("THEME", 0, 2, color::theme_val),
    partial(
        "THEMEGUARD",
        0,
        1,
        color::theme_guard,
        "evaluates to its argument; guarding is up to the host",
    ),
    eager("ISTHEMED", 0, 0, color::is_themed),
    compat(
        "CELLISTHEMED",
        0,
        0,
        color::is_themed,
        "the older name of ISTHEMED, listed in the grammar",
    ),
    // The shape being evaluated.
    eager("ID", 0, 0, host::id),
    eager("NAME", 0, 1, host::name),
    eager("PAGENAME", 0, 1, host::page_name),
    eager("MASTERNAME", 0, 1, host::master_name),
    eager("IS1D", 0, 0, host::is_1d),
    eager("RAND", 0, 0, host::rand),
    // Not evaluated.
    unsupported("ANGLEALONGPATH", GEOMETRY),
    unsupported("ANGLETOLOC", GEOMETRY),
    unsupported("ANGLETOPAR", GEOMETRY),
    unsupported("ARG", CUSTOM),
    unsupported("BKGPAGENAME", DOC),
    unsupported("BLOB", FORMAT),
    unsupported("BOUNDINGBOXDIST", GEOMETRY),
    unsupported("BOUNDINGBOXRECT", GEOMETRY),
    unsupported("CALLOUTCOUNT", DOC),
    unsupported("CALLOUTTARGETREF", DOC),
    unsupported("CALLTHIS", ACTION),
    unsupported("CAT", GRAMMAR_ONLY),
    unsupported("CATEGORY", DOC),
    unsupported("COMPANY", DOC),
    unsupported("CONTAINERCOUNT", DOC),
    unsupported("CONTAINERMEMBERCOUNT", DOC),
    unsupported("CONTAINERSHEETREF", DOC),
    unsupported("CREATOR", DOC),
    unsupported("CY", FORMAT),
    unsupported("DATA1", DOC),
    unsupported("DATA2", DOC),
    unsupported("DATA3", DOC),
    unsupported("DATE", DATE),
    unsupported("DATETIME", DATE),
    unsupported("DATEVALUE", DATE),
    unsupported("DAY", DATE),
    unsupported("DAYOFYEAR", DATE),
    unsupported("DECIMALSEP", LOCALE),
    unsupported("DEFAULTEVENT", ACTION),
    unsupported("DESCRIPTION", DOC),
    unsupported("DIRECTORY", DOC),
    unsupported("DISTTOPATH", GEOMETRY),
    unsupported("DOCCREATION", DOC),
    unsupported("DOCLASTEDIT", DOC),
    unsupported("DOCLASTPRINT", DOC),
    unsupported("DOCLASTSAVE", DOC),
    unsupported("DOCMD", ACTION),
    unsupported("DOOLEVERB", ACTION),
    unsupported("EEQ", GRAMMAR_ONLY),
    unsupported("EGE", GRAMMAR_ONLY),
    unsupported("EGT", GRAMMAR_ONLY),
    unsupported("ELE", GRAMMAR_ONLY),
    unsupported("ELT", GRAMMAR_ONLY),
    unsupported("ENE", GRAMMAR_ONLY),
    unsupported("EVALCELL", CUSTOM),
    unsupported("EVALTEXT", CUSTOM),
    unsupported("FEQ", GRAMMAR_ONLY),
    unsupported("FGE", GRAMMAR_ONLY),
    unsupported("FGT", GRAMMAR_ONLY),
    unsupported("FIELDPICTURE", FORMAT),
    unsupported("FILENAME", DOC),
    unsupported("FLE", GRAMMAR_ONLY),
    unsupported("FLT", GRAMMAR_ONLY),
    unsupported("FNE", GRAMMAR_ONLY),
    unsupported("FONT", DOC),
    unsupported("FONTTOID", DOC),
    unsupported("FORMAT", FORMAT),
    unsupported("FORMATEX", FORMAT),
    unsupported("GETREF", DOC),
    unsupported("GETVAL", DOC),
    unsupported("GOTOPAGE", ACTION),
    unsupported("HASCATEGORY", DOC),
    unsupported("HELP", ACTION),
    unsupported("HOUR", DATE),
    unsupported("HYPERLINK", ACTION),
    unsupported("HYPERLINKBASE", DOC),
    unsupported("INTERSECTX", GEOMETRY),
    unsupported("INTERSECTY", GEOMETRY),
    unsupported("KEYWORDS", DOC),
    unsupported("LANGUAGE", LOCALE),
    unsupported("LISTMEMBERCOUNT", DOC),
    unsupported("LISTORDER", DOC),
    unsupported("LISTSEP", LOCALE),
    unsupported("LISTSHEETREF", DOC),
    unsupported("LOC", LOCALE),
    unsupported("LOCTOLOC", GEOMETRY),
    unsupported("LOCTOPAR", GEOMETRY),
    unsupported("MANAGER", DOC),
    unsupported("MINUTE", DATE),
    unsupported("MONTH", DATE),
    unsupported("NEARESTPOINTONPATH", GEOMETRY),
    unsupported("NOW", DATE),
    unsupported("NURBS", GEOMETRY),
    unsupported("OPENFILE", ACTION),
    unsupported("OPENGROUPWIN", ACTION),
    unsupported("OPENSHEETWIN", ACTION),
    unsupported("OPENTEXTWIN", ACTION),
    unsupported("PAGECOUNT", DOC),
    unsupported("PAGENUMBER", DOC),
    unsupported("PAR", GEOMETRY),
    unsupported("PATHLENGTH", GEOMETRY),
    unsupported("PATHSEGMENT", GEOMETRY),
    unsupported("PLAYSOUND", ACTION),
    unsupported("POINTALONGPATH", GEOMETRY),
    unsupported("POLYLINE", GEOMETRY),
    unsupported("QUEUEMARKEREVENT", ACTION),
    unsupported("RECTSECT", GEOMETRY),
    unsupported("REWIDEN", FORMAT),
    unsupported("RUNADDON", ACTION),
    unsupported("RUNADDONWARGS", ACTION),
    unsupported("RUNMACRO", ACTION),
    unsupported("SECOND", DATE),
    unsupported("SEGMENTCOUNT", GEOMETRY),
    unsupported("SETF", ACTION),
    unsupported("SHEETREF", DOC),
    unsupported("SUBJECT", DOC),
    unsupported("THEMECBV", "needs the gradient stops of the active theme"),
    unsupported("THEMEPROP", GRAMMAR_ONLY),
    unsupported(
        "THEMERESTORE",
        "stores a value for the host to restore; no value is computed",
    ),
    unsupported("TIME", DATE),
    unsupported("TIMEVALUE", DATE),
    unsupported("TITLE", DOC),
    unsupported("TYPE", DOC),
    unsupported("TYPEDESC", DOC),
    unsupported("USE", ACTION),
    unsupported("USERUI", ACTION),
    unsupported("VERSION", DOC),
    unsupported("WEEKDAY", DATE),
    unsupported("YEAR", DATE),
];

/// Every known function, in table order.
pub fn functions() -> &'static [FuncInfo] {
    TABLE
}
