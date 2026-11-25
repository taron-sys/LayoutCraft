//! Building blocks to write the line rules by hand (`regex` is not available offline). A rule is a function that asks, at one position of
//! a line, "does the pattern match here?"; [`Line::any_start`] tries every position, as a regular expression search does.
//!
//! The semantics are those of the JavaScript regular expressions the rules came from, without the `u` flag: `\b` and `\w` are ASCII only,
//! the `i` flag folds ASCII letters only, and `\s` is the JavaScript whitespace set ([`crate::js_compat::is_space`]).

use crate::js_compat::is_space;

/// One line of text to search.
pub struct Line<'a>(pub &'a str);

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

impl Line<'_> {
    /// Every position where a match could start: each character boundary, and the end of the line.
    pub fn any_start(&self, matches_at: impl Fn(usize) -> bool) -> bool {
        self.0.char_indices().map(|(index, _)| index).chain(std::iter::once(self.0.len())).any(matches_at)
    }

    /// `\b`: a word character on exactly one side of `pos`.
    pub fn boundary(&self, pos: usize) -> bool {
        let bytes = self.0.as_bytes();
        let before = pos.checked_sub(1).and_then(|i| bytes.get(i)).is_some_and(|&b| is_word(b));
        let after = bytes.get(pos).is_some_and(|&b| is_word(b));
        before != after
    }

    /// The literal `text` (case-sensitive) at `pos`: the position after it.
    pub fn lit(&self, pos: usize, text: &str) -> Option<usize> {
        let found = self.0.as_bytes().get(pos..pos.checked_add(text.len())?)?;
        (found == text.as_bytes()).then_some(pos + text.len())
    }

    /// The literal `text` at `pos`, ASCII letters in either case (the `i` flag): the position after it.
    pub fn ilit(&self, pos: usize, text: &str) -> Option<usize> {
        let found = self.0.as_bytes().get(pos..pos.checked_add(text.len())?)?;
        found.eq_ignore_ascii_case(text.as_bytes()).then_some(pos + text.len())
    }

    /// The first of the alternatives `a|b|c` that matches at `pos` and lets `rest` succeed from the position after it: the end of the match.
    /// Like a regular expression, it goes on to the next alternative when `rest` fails.
    pub fn ialt(&self, pos: usize, alternatives: &[&str], rest: impl Fn(usize) -> Option<usize>) -> Option<usize> {
        alternatives.iter().filter_map(|alt| self.ilit(pos, alt)).find_map(rest)
    }

    /// One character that `\s` matches: the position after it.
    pub fn space(&self, pos: usize) -> Option<usize> {
        let c = self.0.get(pos..)?.chars().next()?;
        is_space(c).then_some(pos + c.len_utf8())
    }

    /// `\s+`: the position after the whole run of whitespace.
    pub fn spaces(&self, pos: usize) -> Option<usize> {
        let mut end = self.space(pos)?;
        while let Some(next) = self.space(end) {
            end = next;
        }
        Some(end)
    }

    /// `[-\s]?`: one hyphen or whitespace character if there is one.
    pub fn hyphen_or_space(&self, pos: usize) -> usize {
        self.lit(pos, "-").or_else(|| self.space(pos)).unwrap_or(pos)
    }

    /// `\w*`: the end of the run of word characters.
    pub fn word_run(&self, pos: usize) -> usize {
        let run = self.0.as_bytes().get(pos..).unwrap_or_default().iter().take_while(|&&b| is_word(b)).count();
        pos + run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boundary_lies_between_a_word_character_and_anything_else_and_underscore_is_a_word_character() {
        let line = Line("ab cd_e-\u{e9}");
        assert!(line.boundary(0) && line.boundary(2) && line.boundary(3));
        assert!(!line.boundary(1) && !line.boundary(5));
        assert!(line.boundary(7)); // between `e` and `-`
        assert!(!line.boundary(8)); // between `-` and `é`: neither is an ASCII word character
        assert!(!Line("").boundary(0));
    }

    #[test]
    fn literals_match_exactly_or_with_ascii_case_folding_and_never_run_past_the_line() {
        let line = Line("Clean room");
        assert_eq!(line.lit(0, "Clean"), Some(5));
        assert_eq!(line.lit(0, "clean"), None);
        assert_eq!(line.ilit(0, "cLEAN"), Some(5));
        assert_eq!(line.ilit(6, "roomy"), None);
        assert_eq!(Line("\u{e9}").ilit(0, "e"), None);
        assert_eq!(Line("a").ilit(1, "x"), None);
    }

    #[test]
    fn a_position_inside_a_multibyte_character_matches_nothing_and_does_not_panic() {
        let line = Line("\u{e9} a");
        assert_eq!(line.space(1), None);
        assert_eq!(line.spaces(1), None);
        assert_eq!(line.word_run(1), 1);
        assert_eq!(line.lit(1, "\u{a9}"), None);
        assert!(!line.any_start(|i| line.ilit(i, "\u{a9}").is_some()));
    }

    #[test]
    fn whitespace_runs_use_the_javascript_set() {
        let line = Line("a \u{a0}\u{feff}b\u{85}c");
        assert_eq!(line.spaces(1), Some(1 + 1 + 2 + 3));
        assert_eq!(line.spaces(0), None);
        assert_eq!(line.space(line.0.len() - 3), None); // U+0085 is not JavaScript whitespace
    }

    #[test]
    fn an_optional_separator_takes_at_most_one_hyphen_or_space() {
        assert_eq!(Line("a-b").hyphen_or_space(1), 2);
        assert_eq!(Line("a b").hyphen_or_space(1), 2);
        assert_eq!(Line("a  b").hyphen_or_space(1), 2);
        assert_eq!(Line("ab").hyphen_or_space(1), 1);
        assert_eq!(Line("a").hyphen_or_space(5), 5);
    }

    #[test]
    fn an_alternative_that_fails_its_continuation_gives_way_to_the_next_one() {
        let line = Line("username");
        let rest = |after: usize| line.boundary(after).then_some(after);
        assert_eq!(line.ialt(0, &["user", "username"], rest), Some(8));
        assert_eq!(line.ialt(0, &["user"], rest), None);
    }

    #[test]
    fn a_word_run_stops_at_the_first_character_that_is_not_ascii_alphanumeric_or_underscore() {
        assert_eq!(Line("ab_9-x").word_run(0), 4);
        assert_eq!(Line("-x").word_run(0), 0);
        assert_eq!(Line("\u{e9}x").word_run(0), 0);
    }
}
