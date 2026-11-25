//! The line rules of the provenance guard: a checklist of phrases, not a legal analysis. A line that matches a rule's pattern is a
//! finding unless the rule's `allow` function accepts the line. The patterns are hand-written matchers (see `matcher.rs`); the regular
//! expression each one stands for is given in its comment.

use super::matcher::Line;

/// One checklist entry.
pub struct Rule {
    /// What a finding says.
    pub name: &'static str,
    /// Does the pattern occur anywhere in the line?
    pub pattern: fn(&str) -> bool,
    /// A line that the rule accepts although the pattern occurs, such as one that forbids the claim.
    pub allow: Option<fn(&str) -> bool>,
}

impl Rule {
    /// Is the line a finding of this rule?
    pub fn flags(&self, line: &str) -> bool {
        (self.pattern)(line) && !self.allow.is_some_and(|allow| allow(line))
    }
}

/// The rules in the order in which they report.
pub const RULES: [Rule; 9] = [
    Rule { name: "unverified clean-room claim", pattern: clean_room, allow: Some(is_negated) },
    Rule { name: "binary-analysis terminology in a public file", pattern: analysis_tool_terms, allow: Some(is_negated) },
    Rule { name: "binary-analysis terminology in a public file", pattern: analysis_tool_name_rea, allow: Some(is_negated) },
    Rule { name: "Microsoft binary or class name in a public file", pattern: microsoft_internal_names, allow: None },
    Rule { name: "claim of legal clearance", pattern: legal_clearance, allow: None },
    Rule { name: "exclusive public-source claim", pattern: exclusive_public_source, allow: Some(is_negated) },
    Rule { name: "exclusive-source claim", pattern: exclusive_written_from, allow: Some(is_negated) },
    Rule { name: "private working path", pattern: private_path, allow: Some(is_placeholder_path) },
    Rule { name: "reference to a private note", pattern: private_note, allow: None },
];

/// `/\b(not|no|never|do not|don't|must not)\b/i`: the line forbids or denies something, so it is a rule and not a claim.
fn is_negated(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        text.boundary(i) && text.ialt(i, &["not", "no", "never", "do not", "don't", "must not"], |end| text.boundary(end).then_some(end)).is_some()
    })
}

/// `/\bclean[-\s]?room\b/i`
fn clean_room(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        let matched = || {
            text.boundary(i).then_some(())?;
            let after_clean = text.ilit(i, "clean")?;
            let end = text.ilit(text.hyphen_or_space(after_clean), "room")?;
            text.boundary(end).then_some(())
        };
        matched().is_some()
    })
}

/// `/\b(Ghidra|IDA Pro|pseudocode|disassembl\w*|decompil\w*)\b/i`
fn analysis_tool_terms(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        let whole_word = |end: usize| text.boundary(end).then_some(end);
        let to_word_end = |end: usize| Some(text.word_run(end));
        text.boundary(i)
            && (text.ialt(i, &["ghidra", "ida pro", "pseudocode"], whole_word).is_some()
                || text.ialt(i, &["disassembl", "decompil"], to_word_end).is_some_and(|end| text.boundary(end)))
    })
}

/// `/\bREA\b/` (case-sensitive: the name of a reverse-engineering tool, not the word "area")
fn analysis_tool_name_rea(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| text.boundary(i) && text.lit(i, "REA").is_some_and(|end| text.boundary(end)))
}

/// `/\b(VISLIB|VISXDATA|VISBRGR|CDataExplorer|IVDataRecordset)\b/` (case-sensitive)
fn microsoft_internal_names(line: &str) -> bool {
    let text = Line(line);
    const NAMES: [&str; 5] = ["VISLIB", "VISXDATA", "VISBRGR", "CDataExplorer", "IVDataRecordset"];
    text.any_start(|i| text.boundary(i) && NAMES.iter().filter_map(|name| text.lit(i, name)).any(|end| text.boundary(end)))
}

/// `/\b((is|are|has been|have been|fully)\s+legally?\s+(cleared|approved|safe)|no\s+legal\s+risk|lawyer[-\s]?approved|copyright[-\s]?safe)\b/i`
fn legal_clearance(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| text.boundary(i) && legal_clearance_phrases(&text, i).is_some_and(|end| text.boundary(end)))
}

/// The four alternatives of the legal-clearance pattern at `i`: the end of the first phrase that matches.
fn legal_clearance_phrases(text: &Line, i: usize) -> Option<usize> {
    let after_legal = |at: usize| {
        let at = text.spaces(at)?;
        let at = text.ilit(at, "legall")?; // `legally?`: the `?` makes only the final y optional
        let at = text.ilit(at, "y").unwrap_or(at);
        text.ialt(text.spaces(at)?, &["cleared", "approved", "safe"], Some)
    };
    let said_to_be_cleared = text.ialt(i, &["is", "are", "has been", "have been", "fully"], after_legal);
    let no_risk = || {
        let at = text.spaces(text.ilit(i, "no")?)?;
        let at = text.spaces(text.ilit(at, "legal")?)?;
        text.ilit(at, "risk")
    };
    let lawyer = || text.ilit(text.hyphen_or_space(text.ilit(i, "lawyer")?), "approved");
    let copyright = || text.ilit(text.hyphen_or_space(text.ilit(i, "copyright")?), "safe");
    said_to_be_cleared.or_else(no_risk).or_else(lawyer).or_else(copyright)
}

/// `/\b(only|solely)\b[^.\n]{0,50}\bpublic\s+(documentation|docs|sources)\b/i`: "public" may follow within 50 UTF-16 code units of
/// the same sentence.
fn exclusive_public_source(line: &str) -> bool {
    const GAP_UNITS: usize = 50;
    let text = Line(line);
    text.any_start(|i| {
        text.boundary(i)
            && text
                .ialt(i, &["only", "solely"], |end| text.boundary(end).then_some(end))
                .is_some_and(|end| gap_positions(line, end, GAP_UNITS).any(|at| public_docs_at(&text, at)))
    })
}

/// `\bpublic\s+(documentation|docs|sources)\b` at `at`.
fn public_docs_at(text: &Line, at: usize) -> bool {
    let matched = || {
        text.boundary(at).then_some(())?;
        let after_public = text.spaces(text.ilit(at, "public")?)?;
        text.ialt(after_public, &["documentation", "docs", "sources"], |end| text.boundary(end).then_some(end))
    };
    matched().is_some()
}

/// `[^.\n]{0,limit}`: every position the gap can end at when it starts at `start`: it stops at a full stop or a line break and may not
/// be longer than `limit` UTF-16 code units (the unit of a JavaScript string).
fn gap_positions(line: &str, start: usize, limit: usize) -> impl Iterator<Item = usize> + '_ {
    let rest = line.get(start..).unwrap_or_default();
    let mut used = 0;
    let steps = rest.char_indices().map_while(move |(offset, c)| {
        used += c.len_utf16();
        (c != '.' && c != '\n' && used <= limit).then_some(start + offset + c.len_utf8())
    });
    std::iter::once(start).chain(steps)
}

/// `/\b(written|developed)\s+(only|solely)\s+from\b/i`
fn exclusive_written_from(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        let matched = || {
            text.boundary(i).then_some(())?;
            let after_verb = text.ialt(i, &["written", "developed"], Some)?;
            let after_only = text.ialt(text.spaces(after_verb)?, &["only", "solely"], Some)?;
            let end = text.ilit(text.spaces(after_only)?, "from")?;
            text.boundary(end).then_some(())
        };
        matched().is_some()
    })
}

/// `/(docs-private\b|C:\\Users\\|\/home\/[A-Za-z0-9._-]+\/|\barchlinuxuser\b)/i`: a path of a working machine that must not leak.
fn private_path(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        let docs_private = text.ilit(i, "docs-private").is_some_and(|end| text.boundary(end));
        let windows_home = text.ilit(i, r"C:\Users\").is_some();
        let linux_home = text.ilit(i, "/home/").is_some_and(|after| {
            let name_len = line.as_bytes().get(after..).unwrap_or_default().iter().take_while(|b| is_user_name_byte(**b)).count();
            name_len > 0 && text.lit(after + name_len, "/").is_some()
        });
        let machine_name = text.boundary(i) && text.ilit(i, "archlinuxuser").is_some_and(|end| text.boundary(end));
        docs_private || windows_home || linux_home || machine_name
    })
}

/// `[A-Za-z0-9._-]`
fn is_user_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

/// `/\/(home|users)\/(me|user|username|example|you)\b/i`: `/home/me/...` is a placeholder in documentation and tests, not a leaked path.
fn is_placeholder_path(line: &str) -> bool {
    let text = Line(line);
    text.any_start(|i| {
        let user =
            |after_prefix: usize| text.ialt(after_prefix, &["me", "user", "username", "example", "you"], |end| text.boundary(end).then_some(end));
        let home = text.ilit(i, "/home/").or_else(|| text.ilit(i, "/users/"));
        home.and_then(user).is_some()
    })
}

/// `/docs\/(08|10|11|16|23|24|25|26|27|28|29|30|31|33|37|40)-/` (case-sensitive): the numbered notes that stay private.
fn private_note(line: &str) -> bool {
    const NOTES: [&str; 16] = ["08", "10", "11", "16", "23", "24", "25", "26", "27", "28", "29", "30", "31", "33", "37", "40"];
    let text = Line(line);
    text.any_start(|i| {
        let Some(after) = text.lit(i, "docs/") else { return false };
        let number = line.as_bytes().get(after..after + 2).and_then(|bytes| std::str::from_utf8(bytes).ok());
        number.is_some_and(|n| NOTES.contains(&n)) && text.lit(after + 2, "-").is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of the rules that flag `line`, in report order.
    fn flagged(line: &str) -> Vec<&'static str> {
        RULES.iter().filter(|rule| rule.flags(line)).map(|rule| rule.name).collect()
    }

    fn assert_flagged(line: &str, rule: &str) {
        assert!(flagged(line).contains(&rule), "`{line}` should be flagged as `{rule}`, got {:?}", flagged(line));
    }

    fn assert_clean(line: &str) {
        assert!(flagged(line).is_empty(), "`{line}` should pass, but is flagged as {:?}", flagged(line));
    }

    const CLEAN_ROOM: &str = "unverified clean-room claim";
    const TOOLS: &str = "binary-analysis terminology in a public file";
    const MICROSOFT: &str = "Microsoft binary or class name in a public file";
    const LEGAL: &str = "claim of legal clearance";
    const PUBLIC_SOURCE: &str = "exclusive public-source claim";
    const WRITTEN_FROM: &str = "exclusive-source claim";
    const PRIVATE_PATH: &str = "private working path";
    const PRIVATE_NOTE: &str = "reference to a private note";

    #[test]
    fn the_clean_room_claim_is_found_with_a_hyphen_a_space_or_nothing_between_the_words_in_any_case() {
        for line in ["a clean-room build", "a Clean Room build", "cleanroom", "CLEAN\u{a0}ROOM", "x clean\troom y"] {
            assert_flagged(line, CLEAN_ROOM);
        }
    }

    #[test]
    fn the_clean_room_claim_needs_word_boundaries_and_at_most_one_separator() {
        for line in ["unclean room", "clean-roomy", "clean  room", "clean--room", "cleaner room", "clean_room"] {
            assert_clean(line);
        }
    }

    #[test]
    fn a_line_that_denies_a_claim_is_allowed_but_only_a_whole_word_denial_counts() {
        assert_clean("we do not claim a clean-room process");
        assert_clean("Never write clean room");
        assert_clean("There is no clean room");
        assert_clean("we don't say clean room");
        assert_clean("you must not say clean room");
        assert_flagged("a clean room, see the notes", CLEAN_ROOM);
        assert_flagged("a clean room (nothing else)", CLEAN_ROOM);
        assert_flagged("a clean room, nope", CLEAN_ROOM);
        assert_flagged("a clean room, know-how", CLEAN_ROOM);
    }

    #[test]
    fn analysis_tools_and_their_words_are_found_as_whole_words_in_any_case() {
        for line in [
            "with Ghidra today",
            "IDA Pro was used",
            "ida pro",
            "the pseudocode",
            "we disassemble it",
            "disassembly of x",
            "decompiled output",
            "DECOMPILER",
        ] {
            assert_flagged(line, TOOLS);
        }
        // `disassembl` alone is flagged too: the `\w*` after the stem may be empty
        assert_flagged("disassembl", TOOLS);
        for line in ["Ghidras", "IDA  Pro", "IDAPro", "pseudocodes", "xdecompile", "undecompiled"] {
            assert_clean(line);
        }
    }

    #[test]
    fn the_rea_tool_name_is_case_sensitive_and_a_whole_word() {
        assert_flagged("run REA on it", TOOLS);
        assert_flagged("(REA)", TOOLS);
        for line in ["area", "REAL", "Rea", "rea", "AREA", "REA_x"] {
            assert_clean(line);
        }
        assert_clean("do not use REA");
    }

    #[test]
    fn the_tool_terms_are_allowed_on_a_line_that_forbids_them() {
        assert_clean("you must not use a decompiler");
        assert_clean("no disassembly");
    }

    #[test]
    fn microsoft_internal_names_are_found_case_sensitively_as_whole_words_and_cannot_be_allowed() {
        for name in ["VISLIB", "VISXDATA", "VISBRGR", "CDataExplorer", "IVDataRecordset"] {
            assert_flagged(&format!("calls {name}."), MICROSOFT);
            assert_flagged(&format!("never {name}"), MICROSOFT);
            assert_clean(&format!("{name}s"));
            assert_clean(&format!("X{name}"));
        }
        assert_clean("vislib");
    }

    #[test]
    fn legal_clearance_claims_are_found_in_every_form() {
        for line in [
            "this is legally cleared",
            "it has been legally approved",
            "they are legally safe",
            "have been legally approved",
            "fully legally safe",
            "is legall safe",
            "There is no legal risk.",
            "No  Legal\tRisk",
            "lawyer-approved",
            "Lawyer approved",
            "lawyerapproved",
            "copyright-safe",
            "copyright safe",
            "COPYRIGHTSAFE",
        ] {
            assert_flagged(line, LEGAL);
        }
    }

    #[test]
    fn legal_clearance_needs_the_whole_phrase_with_word_boundaries_and_has_no_allow_exception() {
        for line in [
            "this is illegally cleared",
            "legally cleared",
            "is legally clearedx",
            "is legallly safe",
            "is legal safe",
            "fully legal approved",
            "this is legally safely",
            "no legal risks",
            "xno legal risk",
            "lawyer--approved",
            "copyright safety",
        ] {
            assert_clean(line);
        }
        assert_flagged("we never say it is legally safe", LEGAL);
    }

    #[test]
    fn an_exclusive_public_source_claim_is_found_within_fifty_units_of_the_same_sentence() {
        for line in [
            "built only from public documentation",
            "Solely using public docs",
            "ONLY public sources",
            "only the very latest public sources",
            "only\tpublic\tdocs",
        ] {
            assert_flagged(line, PUBLIC_SOURCE);
        }
        // the gap is exactly 50 units: 48 letters, a space on each side
        let fifty = format!("only {} public docs", "a".repeat(48));
        assert_flagged(&fifty, PUBLIC_SOURCE);
        let fifty_one = format!("only {} public docs", "a".repeat(49));
        assert_clean(&fifty_one);
    }

    #[test]
    fn the_public_source_gap_ends_at_a_full_stop_and_counts_utf16_units() {
        assert_clean("only that. public docs");
        assert_clean("only public.docs");
        assert_clean("onlypublic docs");
        assert_clean("only public documentations");
        assert_clean("only publicdocs");
        // 24 astral characters are 48 units, 26 are 52 units
        let inside = format!("only {} public docs", "\u{1f600}".repeat(24));
        assert_flagged(&inside, PUBLIC_SOURCE);
        let outside = format!("only {} public docs", "\u{1f600}".repeat(26));
        assert_clean(&outside);
        assert_clean("not only public docs");
        assert_clean("only public docs, not private"); // a denial anywhere in the line allows it
    }

    #[test]
    fn written_only_from_is_found_and_allowed_when_the_line_denies_it() {
        for line in ["written only from specs", "Developed solely  from specs", "developed\tonly\tfrom"] {
            assert_flagged(line, WRITTEN_FROM);
        }
        for line in ["written from specs", "writtenonly from", "written only fromage", "unwritten only from"] {
            assert_clean(line);
        }
        assert_clean("it is not written only from specs");
    }

    #[test]
    fn private_working_paths_are_found() {
        for line in [
            "see docs-private/notes",
            "DOCS-PRIVATE",
            "at C:\\Users\\someone\\x",
            "c:\\users\\x",
            "/home/alice/work",
            "/HOME/Alice.B-c_d/",
            "on archlinuxuser's box",
            "xdocs-private",
        ] {
            assert_flagged(line, PRIVATE_PATH);
        }
    }

    #[test]
    fn near_misses_of_a_private_path_pass() {
        for line in [
            "docs-privates",
            "docs-private_x",
            "C:\\Usersx\\",
            "/home/",
            "/home//x",
            "/home/alice",
            "/home/a b/",
            "myarchlinuxuser",
            "archlinuxusers",
            "C:/Users/x",
        ] {
            assert_clean(line);
        }
    }

    #[test]
    fn placeholder_paths_are_allowed_only_when_the_name_is_a_whole_placeholder_word() {
        for line in
            ["/home/me/drawings", "/home/user/x", "/Users/USERNAME/x", "/home/example/x", "/users/you", "/home/me", "C:/users/me/x and /home/you."]
        {
            assert!(!flagged(line).contains(&PRIVATE_PATH), "{line}");
        }
        assert_flagged("/home/meg/x", PRIVATE_PATH);
        assert_flagged("/home/username2/x", PRIVATE_PATH);
        assert_flagged("/home/users/x", PRIVATE_PATH);
        assert_flagged("/home/mx/x", PRIVATE_PATH);
        // the exception covers the whole line, as the Node check did
        assert_clean("/home/me/x and /home/alice/y");
    }

    #[test]
    fn references_to_private_notes_are_found_by_number_and_a_following_hyphen() {
        for number in ["08", "10", "11", "16", "23", "24", "25", "26", "27", "28", "29", "30", "31", "33", "37", "40"] {
            assert_flagged(&format!("see docs/{number}-legal.md"), PRIVATE_NOTE);
        }
        for line in [
            "docs/09-file-reference.md",
            "docs/41-legal.md",
            "docs/32-mcp.md",
            "docs/08.md",
            "docs/08",
            "Docs/08-x",
            "docs/8-x",
            "docs/0",
            "docs/\u{e9}\u{e9}-x",
        ] {
            assert_clean(line);
        }
        assert_flagged("a doc: docs/40-x", PRIVATE_NOTE);
        assert_flagged("do not use docs/40-x", PRIVATE_NOTE); // this rule has no allow exception
    }
}
