//! Tokeniser. Never panics: every failure is a [`ParseError`] with the offending span.
//!
//! Number syntax follows the grammar in [MS-VSDX] section 2.5.1 (`double-value`) plus the unit
//! suffix forms of "About Units of Measure": `1.25 in`, `45deg`, `0.7 pt.`, `5[in,P]`.

use crate::error::{ParseError, ParseErrorKind, Span};
use crate::unit::Unit;
use crate::value::Rgb;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Tok {
    Number {
        value: f64,
        unit: Option<Unit>,
    },
    Str(String),
    Color(Rgb),
    /// An identifier, possibly with dots inside (`Geometry1.X1`, `Sheet.5`).
    Word(String),
    /// `'name'`
    Quoted(String),
    /// `[text]` directly after a word.
    Bracketed(String),
    LParen,
    RParen,
    Comma,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    End,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    pub span: Span,
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.rest().chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn take_while(&mut self, mut keep: impl FnMut(char) -> bool) -> &'a str {
        let start = self.pos;
        while self.peek().is_some_and(&mut keep) {
            self.bump();
        }
        self.src.get(start..self.pos).unwrap_or("")
    }

    fn err(&self, kind: ParseErrorKind, start: usize) -> ParseError {
        ParseError::new(kind, Span::new(start, self.pos.max(start)))
    }

    fn next_token(&mut self) -> Result<Token, ParseError> {
        self.take_while(char::is_whitespace);
        let start = self.pos;
        let Some(c) = self.peek() else {
            return Ok(Token {
                tok: Tok::End,
                span: Span::new(start, start),
            });
        };
        let tok = match c {
            '"' => self.string(start)?,
            '\'' => self.quoted(start)?,
            '#' => self.color(start)?,
            '0'..='9' => self.number(start)?,
            '.' if self.peek_second().is_some_and(|d| d.is_ascii_digit()) => self.number(start)?,
            c if is_word_start(c) => self.word(start)?,
            _ => self.operator(start, c)?,
        };
        Ok(Token {
            tok,
            span: Span::new(start, self.pos),
        })
    }

    fn operator(&mut self, start: usize, c: char) -> Result<Tok, ParseError> {
        self.bump();
        Ok(match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            '!' => Tok::Bang,
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '^' => Tok::Caret,
            '&' => Tok::Amp,
            '%' => Tok::Percent,
            '=' => Tok::Eq,
            '<' => {
                if self.eat('>') {
                    Tok::Ne
                } else if self.eat('=') {
                    Tok::Le
                } else {
                    Tok::Lt
                }
            }
            '>' => {
                if self.eat('=') {
                    Tok::Ge
                } else {
                    Tok::Gt
                }
            }
            other => return Err(self.err(ParseErrorKind::UnexpectedChar(other), start)),
        })
    }

    /// `"text"` with `""` for a quote inside.
    fn string(&mut self, start: usize) -> Result<Tok, ParseError> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(ParseErrorKind::UnterminatedString, start)),
                Some('"') => {
                    if self.eat('"') {
                        text.push('"');
                    } else {
                        return Ok(Tok::Str(text));
                    }
                }
                Some(c) => text.push(c),
            }
        }
    }

    /// `'name'` with `''` for a quote inside.
    fn quoted(&mut self, start: usize) -> Result<Tok, ParseError> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(ParseErrorKind::UnterminatedQuotedName, start)),
                Some('\'') => {
                    if self.eat('\'') {
                        text.push('\'');
                    } else {
                        return Ok(Tok::Quoted(text));
                    }
                }
                Some(c) => text.push(c),
            }
        }
    }

    fn color(&mut self, start: usize) -> Result<Tok, ParseError> {
        self.bump();
        let digits = self.take_while(|c| c.is_ascii_hexdigit());
        let text = self.src.get(start..self.pos).unwrap_or("");
        match Rgb::from_hex(text) {
            Some(rgb) if digits.len() == 6 && !self.peek().is_some_and(is_word_char) => {
                Ok(Tok::Color(rgb))
            }
            _ => Err(self.err(ParseErrorKind::BadColor, start)),
        }
    }

    fn word(&mut self, start: usize) -> Result<Tok, ParseError> {
        loop {
            self.take_while(is_word_char);
            if self.peek() == Some('.') && self.peek_second().is_some_and(is_word_char) {
                self.bump();
            } else {
                break;
            }
        }
        let word = self.src.get(start..self.pos).unwrap_or("").to_string();
        Ok(Tok::Word(word))
    }

    /// Reads `[...]` that directly follows a word; called by the driver, not by `word`.
    fn bracketed(&mut self, start: usize) -> Result<Tok, ParseError> {
        self.bump();
        let content_start = self.pos;
        while let Some(c) = self.peek() {
            if c == ']' {
                let content = self
                    .src
                    .get(content_start..self.pos)
                    .unwrap_or("")
                    .to_string();
                self.bump();
                return Ok(Tok::Bracketed(content));
            }
            self.bump();
        }
        Err(self.err(ParseErrorKind::UnterminatedBracket, start))
    }

    fn number(&mut self, start: usize) -> Result<Tok, ParseError> {
        self.take_while(|c| c.is_ascii_digit());
        if self.peek() == Some('.') {
            self.bump();
            self.take_while(|c| c.is_ascii_digit());
        }
        // An exponent needs at least one digit, so `2em` stays "2 elapsed minutes".
        if matches!(self.peek(), Some('e' | 'E')) {
            let rest = self.rest();
            let mut chars = rest.chars().skip(1);
            let after = match chars.next() {
                Some('+' | '-') => chars.next(),
                other => other,
            };
            if after.is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                self.take_while(|c| c.is_ascii_digit());
            }
        }
        let text = self.src.get(start..self.pos).unwrap_or("");
        let value: f64 = text
            .parse()
            .map_err(|_| self.err(ParseErrorKind::BadNumber, start))?;
        if !value.is_finite() {
            return Err(self.err(ParseErrorKind::BadNumber, start));
        }
        let unit = if self.peek() == Some('[') {
            Some(self.implicit_unit(start)?)
        } else {
            self.unit_suffix()?
        };
        Ok(Tok::Number { value, unit })
    }

    /// `[unit,flag]` after a number: the unit is what matters, the flag only says which default
    /// measurement system the cell displays in.
    fn implicit_unit(&mut self, start: usize) -> Result<Unit, ParseError> {
        let Tok::Bracketed(content) = self.bracketed(self.pos)? else {
            return Err(self.err(ParseErrorKind::BadNumber, start));
        };
        let mut parts = content.splitn(2, ',');
        let name = parts.next().unwrap_or("").trim();
        let flag = parts.next().map(str::trim);
        let flag_ok =
            flag.is_none_or(|f| f.len() == 1 && f.bytes().all(|b| b"aAdDeEpPtT".contains(&b)));
        match Unit::parse(name) {
            Some(unit) if flag_ok => Ok(unit),
            Some(_) => Err(self.err(ParseErrorKind::BadNumber, start)),
            None => Err(self.err(ParseErrorKind::UnknownUnit(name.to_string()), start)),
        }
    }

    /// An optional unit after a number, with or without white space between.
    fn unit_suffix(&mut self) -> Result<Option<Unit>, ParseError> {
        let number_end = self.pos;
        let spaced = self.peek().is_some_and(char::is_whitespace);
        self.take_while(char::is_whitespace);
        let name_start = self.pos;
        match self.peek() {
            Some('\u{b0}') => {
                self.bump();
                return Ok(Some(Unit::Degree));
            }
            // Arc minutes and seconds are accepted only when attached to the number: with a gap,
            // `'` starts a quoted name and `"` a string.
            Some('\'') if !spaced => {
                self.bump();
                return Ok(Some(Unit::ArcMinute));
            }
            Some('"') if !spaced => {
                self.bump();
                return Ok(Some(Unit::ArcSecond));
            }
            _ => {}
        }
        let run = self.take_while(|c| c.is_ascii_alphabetic() || c == '_');
        if run.is_empty() {
            self.pos = number_end;
            return Ok(None);
        }
        let with_dot = self.peek() == Some('.') && !self.peek_second().is_some_and(is_word_char);
        let name = if with_dot {
            format!("{run}.")
        } else {
            run.to_string()
        };
        match Unit::parse(&name) {
            Some(unit) => {
                if with_dot {
                    self.bump();
                }
                Ok(Some(unit))
            }
            None if !spaced => {
                Err(self.err(ParseErrorKind::UnknownUnit(run.to_string()), name_start))
            }
            // Something else follows (`5 GT 2`): not a unit, leave it for the parser.
            None => {
                self.pos = number_end;
                Ok(None)
            }
        }
    }
}

/// Splits `src` into tokens. A leading `=` is skipped (the editor inserts it automatically).
pub(crate) fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut lexer = Lexer { src, pos: 0 };
    lexer.take_while(char::is_whitespace);
    lexer.eat('=');
    let mut tokens = Vec::new();
    loop {
        let mut token = lexer.next_token()?;
        // `[` binds to the word before it only when nothing separates them.
        if matches!(token.tok, Tok::Word(_) | Tok::Quoted(_)) && lexer.peek() == Some('[') {
            let bracket_start = lexer.pos;
            let bracketed = lexer.bracketed(bracket_start)?;
            tokens.push(token);
            token = Token {
                tok: bracketed,
                span: Span::new(bracket_start, lexer.pos),
            };
        }
        let done = token.tok == Tok::End;
        tokens.push(token);
        if done {
            return Ok(tokens);
        }
    }
}
