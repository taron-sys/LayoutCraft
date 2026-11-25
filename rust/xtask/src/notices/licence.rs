//! Which licences are acceptable: the SPDX expression of a crate is split like the Node tool split it, and an expression is fine when
//! one of its `OR` alternatives uses only permissive licences.

use crate::js_compat::{is_space, trim};

/// The licence identifiers that are accepted without a note (exact, case-sensitive).
const PERMISSIVE: [&str; 17] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unlicense",
    "0BSD",
    "CC0-1.0",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "BSL-1.0",
    "MIT-0",
    "OFL-1.1",
    "LicenseRef-UFL-1.0",
    "Ubuntu-font-1.0",
    "NCSA",
];

/// The words of one alternative: the licence identifiers, without the operators `OR`, `AND`, `WITH` and `/` and the parentheses.
/// (So `Apache-2.0 WITH LLVM-exception` has the words `Apache-2.0` and `LLVM-exception`, and the second one is not on the list.)
fn words(expression: &str) -> Vec<&str> {
    expression
        .split(|c: char| is_space(c) || c == '(' || c == ')')
        .filter(|word| !word.is_empty() && !matches!(*word, "OR" | "AND" | "WITH" | "/"))
        .collect()
}

/// `expression.split(/\s+OR\s+|\//)`: the alternatives of an expression, cut at ` OR ` and at every slash (`MIT/Apache-2.0`).
fn alternatives(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < expression.len() {
        let rest = &expression[at..];
        if let Some(after) = rest.strip_prefix('/') {
            parts.push(&expression[start..at]);
            at = expression.len() - after.len();
            start = at;
        } else if let Some(separator_len) = or_separator_len(rest) {
            parts.push(&expression[start..at]);
            at += separator_len;
            start = at;
        } else {
            at += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    parts.push(&expression[start..]);
    parts
}

/// `\s+OR\s+` at the start of `text`: its length in bytes.
fn or_separator_len(text: &str) -> Option<usize> {
    let after_space = text.trim_start_matches(is_space);
    let after_or = after_space.strip_prefix("OR")?;
    let after_all = after_or.trim_start_matches(is_space);
    let has_space_before = after_space.len() < text.len();
    let has_space_after = after_all.len() < after_or.len();
    (has_space_before && has_space_after).then(|| text.len() - after_all.len())
}

/// Is the expression acceptable? One alternative whose words are all permissive is enough. An empty expression (a crate with only a
/// licence file, or none) has no word that fails, so it passes here; the list of crates shows what it has.
pub fn is_acceptable(expression: &str) -> bool {
    alternatives(expression)
        .into_iter()
        .map(|alternative| trim(&alternative.replace(['(', ')'], "")).to_string())
        .any(|alternative| words(&alternative).iter().all(|word| PERMISSIVE.contains(word)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_permissive_alternative_of_an_or_expression_is_enough() {
        assert!(is_acceptable("MIT"));
        assert!(is_acceptable("MIT OR Apache-2.0"));
        assert!(is_acceptable("MIT/Apache-2.0"));
        assert!(is_acceptable("GPL-3.0-only OR MIT"));
        assert!(is_acceptable("(MIT OR Apache-2.0) OR GPL-3.0"));
        assert!(is_acceptable("MIT OR   Apache-2.0"));
    }

    #[test]
    fn an_and_expression_without_an_or_needs_every_licence_to_be_permissive() {
        assert!(is_acceptable("MIT AND Apache-2.0"));
        // the OR split comes first, so the AND is dropped with the other alternative (the Node tool did the same)
        assert!(is_acceptable("(MIT OR Apache-2.0) AND GPL-3.0"));
        assert!(!is_acceptable("MIT AND GPL-3.0"));
        assert!(!is_acceptable("MPL-2.0"));
        assert!(!is_acceptable("Apache-2.0 WITH LLVM-exception"));
    }

    #[test]
    fn identifiers_are_matched_exactly_and_including_case() {
        assert!(!is_acceptable("mit"));
        assert!(!is_acceptable("MIT-1"));
        assert!(!is_acceptable("MIT OR2 GPL-3.0"));
        assert!(is_acceptable("0BSD"));
        assert!(!is_acceptable("BSD-4-Clause"));
    }

    #[test]
    fn an_empty_expression_passes_because_no_word_of_it_fails() {
        assert!(is_acceptable(""));
        assert!(is_acceptable("  "));
    }

    #[test]
    fn the_alternatives_are_cut_at_or_and_at_every_slash_but_only_at_or_between_whitespace() {
        assert_eq!(alternatives("A OR B"), ["A", "B"]);
        assert_eq!(alternatives("A/B"), ["A", "B"]);
        assert_eq!(alternatives("A / B"), ["A ", " B"]);
        assert_eq!(alternatives("A  OR \t B OR C"), ["A", "B", "C"]);
        assert_eq!(alternatives("A OR"), ["A OR"]);
        assert_eq!(alternatives("OR B"), ["OR B"]);
        assert_eq!(alternatives("AOR B"), ["AOR B"]);
        assert_eq!(alternatives("A ORB"), ["A ORB"]);
        assert_eq!(alternatives(""), [""]);
        assert_eq!(alternatives("\u{e9} OR \u{e9}"), ["\u{e9}", "\u{e9}"]);
    }

    #[test]
    fn the_words_of_an_alternative_drop_operators_and_parentheses() {
        assert_eq!(words("(MIT OR Apache-2.0) AND  X / Y"), ["MIT", "Apache-2.0", "X", "Y"]);
        assert_eq!(words("A WITH B"), ["A", "B"]);
        assert_eq!(words("/B /"), ["/B"]);
        assert_eq!(words("ORX"), ["ORX"]);
    }
}
