//! The few JavaScript behaviours the ported Node tools relied on, so that the Rust ports give the same bytes:
//! the `\s` and `String.prototype.trim` whitespace set, UTF-16 lengths, and `localeCompare` for ASCII names.

use std::cmp::Ordering;

/// A character that `\s` matches in a JavaScript regular expression and that `String.prototype.trim` removes.
/// It differs from `char::is_whitespace`: it contains U+FEFF and not U+0085.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `text.trim()`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `text.replace(/\s+/g, ' ')`.
pub fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `text.slice(0, units)`: the first `units` UTF-16 code units. A surrogate pair that does not fit is cut in half in JavaScript and
/// prints as U+FFFD, so that is what is returned for it.
pub fn slice_utf16(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if used + width <= units {
            out.push(c);
            used += width;
        } else {
            if used < units {
                out.push('\u{fffd}');
            }
            break;
        }
    }
    out
}

/// The primary order of the printable ASCII characters in the default (CLDR root) collation that `localeCompare` uses: punctuation, then
/// digits, then letters. It was read from `Array.prototype.sort` with `localeCompare` over all of them (upper case follows lower case).
const ASCII_ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";

/// Primary weight (a letter weighs the same in both cases) and tertiary weight (lower case before upper case) of one character.
/// Characters the table does not know (control characters, non-ASCII) sort after the letters by code point;
/// crate names and versions are ASCII.
fn weights(c: char) -> (u32, u8) {
    let primary = match ASCII_ORDER.chars().position(|x| x == c.to_ascii_lowercase()) {
        Some(index) => index as u32,
        None => 1000 + u32::from(c),
    };
    (primary, u8::from(c.is_ascii_uppercase()))
}

/// `a.localeCompare(b)` for the ASCII strings of crate names and versions (the default collation of Node with ICU, not numeric):
/// the primary weights decide first, the case decides only between strings that are otherwise equal.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| s.chars().map(|c| weights(c).0).collect::<Vec<_>>();
    let tertiary = |s: &str| s.chars().map(|c| weights(c).1).collect::<Vec<_>>();
    primary(a).cmp(&primary(b)).then_with(|| tertiary(a).cmp(&tertiary(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whitespace_set_is_that_of_javascript_not_of_rust() {
        assert!(is_space('\u{feff}') && is_space('\u{a0}') && is_space('\u{2028}'));
        assert!(!is_space('\u{85}'));
        assert_eq!(trim("\u{feff} a b\u{a0}\n"), "a b");
        assert_eq!(trim("\u{85}x"), "\u{85}x");
    }

    #[test]
    fn runs_of_whitespace_collapse_to_one_space() {
        assert_eq!(collapse_spaces("a \t\u{a0} b\n\nc"), "a b c");
        assert_eq!(collapse_spaces(""), "");
    }

    #[test]
    fn a_slice_counts_utf16_units_and_cuts_a_pair_to_the_replacement_character() {
        assert_eq!(slice_utf16("abcdef", 3), "abc");
        assert_eq!(slice_utf16("ab\u{1f600}", 3), "ab\u{fffd}");
        assert_eq!(slice_utf16("ab\u{1f600}", 4), "ab\u{1f600}");
        assert_eq!(slice_utf16("ab", 10), "ab");
    }

    #[test]
    fn punctuation_sorts_before_digits_and_digits_before_letters_with_underscore_before_hyphen() {
        // the order that put windows_x86_64_gnu before windows-sys in the Node tool
        assert_eq!(locale_compare("windows_x86_64_gnu", "windows-sys"), Ordering::Less);
        assert_eq!(locale_compare("a_", "a1"), Ordering::Less);
        assert_eq!(locale_compare("a1", "ab"), Ordering::Less);
        assert_eq!(locale_compare("a-b", "ab"), Ordering::Less);
    }

    #[test]
    fn case_only_decides_between_names_that_are_otherwise_equal() {
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("A", "b"), Ordering::Less);
        assert_eq!(locale_compare("Ab", "aB"), Ordering::Greater);
        assert_eq!(locale_compare("same", "same"), Ordering::Equal);
    }

    #[test]
    fn versions_compare_character_by_character_as_text() {
        assert_eq!(locale_compare("0.2.10", "0.2.9"), Ordering::Less);
        assert_eq!(locale_compare("1.0.0", "1.0.0-rc.1"), Ordering::Less);
        assert_eq!(locale_compare("1.0.0-rc.1", "1.0.0+x"), Ordering::Less);
    }
}
