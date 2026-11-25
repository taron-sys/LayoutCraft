//! Recursive-descent parser.
//!
//! Operator precedence, tightest first (Microsoft Learn, "About Operators", "Order of
//! evaluation"): unary `+` `-` and postfix `%`; `^`; `*` `/`; `+` `-`; `&`; the ordering
//! comparisons `<` `<=` `>` `>=` (also `LT` `LE` `GT` `GE`); then `=` `<>` (also `EQ` `NE`).
//! All binary operators associate to the left. Because the unary operators bind tighter than `^`,
//! `-2^2` is `(-2)^2`; and `2^3^2` is `(2^3)^2`. Both match what the repository's earlier
//! evaluator recorded from the application.

use crate::ast::*;
use crate::error::{ParseError, ParseErrorKind, Span};
use crate::lexer::{tokenize, Tok, Token};
use crate::{MAX_DEPTH, MAX_FORMULA_BYTES, MAX_NESTING};

/// A parsed node and the height of the tree below it (a leaf has height 1).
type Node = (Expr, usize);

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    nesting: usize,
}

/// One `name` or `name[bracket]` piece of a `!`-separated reference.
struct Segment {
    name: String,
    quoted: bool,
    bracket: Option<String>,
    span: Span,
}

/// Parses a formula (a leading `=` is ignored).
pub fn parse(source: &str) -> Result<Expr, ParseError> {
    if source.len() > MAX_FORMULA_BYTES {
        return Err(ParseError::new(
            ParseErrorKind::TooLong {
                max: MAX_FORMULA_BYTES,
            },
            Span::new(MAX_FORMULA_BYTES, source.len()),
        ));
    }
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        nesting: 0,
    };
    if parser.peek() == &Tok::End {
        return Err(ParseError::new(ParseErrorKind::Empty, parser.span()));
    }
    let (expr, _) = parser.expression()?;
    if parser.peek() != &Tok::End {
        return Err(ParseError::new(
            ParseErrorKind::TrailingInput,
            parser.span(),
        ));
    }
    Ok(expr)
}

fn node(kind: ExprKind, span: Span, height: usize) -> Node {
    (Expr { kind, span }, height)
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.tokens.get(self.pos).map_or(&Tok::End, |t| &t.tok)
    }

    fn peek_at(&self, offset: usize) -> &Tok {
        self.tokens
            .get(self.pos + offset)
            .map_or(&Tok::End, |t| &t.tok)
    }

    fn span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or(Span::default(), |t| t.span)
    }

    fn previous_end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(0, |t| t.span.end)
    }

    fn advance(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned().unwrap_or(Token {
            tok: Tok::End,
            span: Span::default(),
        });
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, what: &'static str) -> Result<(), ParseError> {
        if self.eat(tok) {
            Ok(())
        } else {
            Err(ParseError::new(
                ParseErrorKind::Expected { what },
                self.span(),
            ))
        }
    }

    fn too_deep(&self) -> ParseError {
        ParseError::new(ParseErrorKind::TooDeep { max: MAX_DEPTH }, self.span())
    }

    fn binary(&self, op: BinaryOp, lhs: Node, rhs: Node) -> Result<Node, ParseError> {
        let height = lhs.1.max(rhs.1) + 1;
        if height > MAX_DEPTH {
            return Err(self.too_deep());
        }
        let span = lhs.0.span.to(rhs.0.span);
        Ok(node(
            ExprKind::Binary {
                op,
                lhs: Box::new(lhs.0),
                rhs: Box::new(rhs.0),
            },
            span,
            height,
        ))
    }

    fn expression(&mut self) -> Result<Node, ParseError> {
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(self.too_deep());
        }
        let result = self.equality();
        self.nesting -= 1;
        result
    }

    /// The word forms of the comparison operators, when the next token is one.
    fn word_operator(&self, names: [&str; 2]) -> Option<usize> {
        match self.peek() {
            Tok::Word(word) if !matches!(self.peek_at(1), Tok::Bang | Tok::Bracketed(_)) => names
                .iter()
                .position(|name| word.eq_ignore_ascii_case(name)),
            _ => None,
        }
    }

    fn equality(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.ordering()?;
        loop {
            let (cmp, precision) = match self.peek() {
                Tok::Eq => (CmpOp::Eq, Precision::Tolerant),
                Tok::Ne => (CmpOp::Ne, Precision::Tolerant),
                _ => match self.word_operator(["EQ", "NE"]) {
                    Some(0) => (CmpOp::Eq, Precision::Exact),
                    Some(_) => (CmpOp::Ne, Precision::Exact),
                    None => return Ok(lhs),
                },
            };
            self.advance();
            let rhs = self.ordering()?;
            lhs = self.binary(BinaryOp::Compare(cmp, precision), lhs, rhs)?;
        }
    }

    fn ordering(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.concat()?;
        loop {
            let (cmp, precision) = match self.peek() {
                Tok::Lt => (CmpOp::Lt, Precision::Tolerant),
                Tok::Le => (CmpOp::Le, Precision::Tolerant),
                Tok::Gt => (CmpOp::Gt, Precision::Tolerant),
                Tok::Ge => (CmpOp::Ge, Precision::Tolerant),
                _ => {
                    if let Some(which) = self.word_operator(["LT", "LE"]) {
                        (
                            if which == 0 { CmpOp::Lt } else { CmpOp::Le },
                            Precision::Exact,
                        )
                    } else if let Some(which) = self.word_operator(["GT", "GE"]) {
                        (
                            if which == 0 { CmpOp::Gt } else { CmpOp::Ge },
                            Precision::Exact,
                        )
                    } else {
                        return Ok(lhs);
                    }
                }
            };
            self.advance();
            let rhs = self.concat()?;
            lhs = self.binary(BinaryOp::Compare(cmp, precision), lhs, rhs)?;
        }
    }

    fn concat(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.additive()?;
        while self.eat(&Tok::Amp) {
            let rhs = self.additive()?;
            lhs = self.binary(BinaryOp::Concat, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinaryOp::Add,
                Tok::Minus => BinaryOp::Sub,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.term()?;
            lhs = self.binary(op, lhs, rhs)?;
        }
    }

    fn term(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.power()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinaryOp::Mul,
                Tok::Slash => BinaryOp::Div,
                _ => return Ok(lhs),
            };
            self.advance();
            let rhs = self.power()?;
            lhs = self.binary(op, lhs, rhs)?;
        }
    }

    fn power(&mut self) -> Result<Node, ParseError> {
        let mut lhs = self.unary()?;
        while self.eat(&Tok::Caret) {
            let rhs = self.unary()?;
            lhs = self.binary(BinaryOp::Pow, lhs, rhs)?;
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Node, ParseError> {
        let op = match self.peek() {
            Tok::Plus => UnaryOp::Plus,
            Tok::Minus => UnaryOp::Minus,
            _ => return self.postfix(),
        };
        let start = self.advance().span;
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(self.too_deep());
        }
        let operand = self.unary();
        self.nesting -= 1;
        let (operand, height) = operand?;
        if height + 1 > MAX_DEPTH {
            return Err(self.too_deep());
        }
        let span = start.to(operand.span);
        Ok(node(
            ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            span,
            height + 1,
        ))
    }

    fn postfix(&mut self) -> Result<Node, ParseError> {
        let mut inner = self.primary()?;
        while self.peek() == &Tok::Percent {
            let percent = self.advance().span;
            let height = inner.1 + 1;
            if height > MAX_DEPTH {
                return Err(self.too_deep());
            }
            let span = inner.0.span.to(percent);
            inner = node(
                ExprKind::Unary {
                    op: UnaryOp::Percent,
                    operand: Box::new(inner.0),
                },
                span,
                height,
            );
        }
        Ok(inner)
    }

    fn primary(&mut self) -> Result<Node, ParseError> {
        let token = self.span();
        match self.peek().clone() {
            Tok::Number { value, unit } => {
                self.advance();
                Ok(node(ExprKind::Number { value, unit }, token, 1))
            }
            Tok::Str(text) => {
                self.advance();
                Ok(node(ExprKind::Str(text), token, 1))
            }
            Tok::Color(rgb) => {
                self.advance();
                Ok(node(ExprKind::Color(rgb), token, 1))
            }
            Tok::LParen => {
                self.advance();
                let (mut inner, height) = self.expression()?;
                self.expect(&Tok::RParen, "')'")?;
                inner.span = Span::new(token.start, self.previous_end());
                Ok((inner, height))
            }
            Tok::Word(word) => self.word(word),
            Tok::Quoted(_) => self.reference(),
            _ => Err(ParseError::new(
                ParseErrorKind::Expected { what: "a value" },
                token,
            )),
        }
    }

    fn word(&mut self, word: String) -> Result<Node, ParseError> {
        let span = self.span();
        if self.peek_at(1) == &Tok::LParen {
            return self.call(word);
        }
        // Not followed by anything that continues a reference: a keyword constant.
        let continues = matches!(self.peek_at(1), Tok::Bang | Tok::Bracketed(_));
        if !continues {
            if word.eq_ignore_ascii_case("TRUE") {
                self.advance();
                return Ok(node(ExprKind::Bool(true), span, 1));
            }
            if word.eq_ignore_ascii_case("FALSE") {
                self.advance();
                return Ok(node(ExprKind::Bool(false), span, 1));
            }
            // A bare `PI` means `PI()` (the earlier evaluator accepted it) and a bare `ISTHEMED`
            // means `ISTHEMED()` (the ISTHEMED page writes its second example that way).
            if word.eq_ignore_ascii_case("PI") || word.eq_ignore_ascii_case("ISTHEMED") {
                self.advance();
                return Ok(node(
                    ExprKind::Call {
                        name: word,
                        name_span: span,
                        args: Vec::new(),
                    },
                    span,
                    1,
                ));
            }
        }
        self.reference()
    }

    fn call(&mut self, name: String) -> Result<Node, ParseError> {
        let name_span = self.advance().span;
        if name.contains('.') {
            return Err(ParseError::new(
                ParseErrorKind::BadReference {
                    why: "a function name has no dots",
                },
                name_span,
            ));
        }
        self.advance(); // '('
        self.nesting += 1;
        if self.nesting > MAX_NESTING {
            return Err(self.too_deep());
        }
        let result = self.arguments();
        self.nesting -= 1;
        let (args, height) = result?;
        let span = Span::new(name_span.start, self.previous_end());
        Ok(node(
            ExprKind::Call {
                name,
                name_span,
                args,
            },
            span,
            height + 1,
        ))
    }

    /// The arguments up to and including the closing parenthesis, and the tallest argument.
    fn arguments(&mut self) -> Result<(Vec<Expr>, usize), ParseError> {
        let mut args = Vec::new();
        let mut tallest = 0;
        if self.eat(&Tok::RParen) {
            return Ok((args, tallest));
        }
        loop {
            if matches!(self.peek(), Tok::Comma | Tok::RParen) {
                let here = self.span().start;
                args.push(Expr {
                    kind: ExprKind::Empty,
                    span: Span::new(here, here),
                });
            } else {
                let (arg, height) = self.expression()?;
                tallest = tallest.max(height);
                args.push(arg);
            }
            if self.eat(&Tok::RParen) {
                break;
            }
            self.expect(&Tok::Comma, "',' or ')'")?;
        }
        if tallest + 1 > MAX_DEPTH {
            return Err(self.too_deep());
        }
        Ok((args, tallest))
    }

    fn segment(&mut self) -> Result<Segment, ParseError> {
        let token = self.advance();
        let (name, quoted) = match token.tok {
            Tok::Word(word) => (word, false),
            Tok::Quoted(name) => (name, true),
            _ => {
                return Err(ParseError::new(
                    ParseErrorKind::Expected { what: "a name" },
                    token.span,
                ));
            }
        };
        let mut span = token.span;
        let mut bracket = None;
        if let Tok::Bracketed(content) = self.peek().clone() {
            span = span.to(self.advance().span);
            bracket = Some(content);
        }
        Ok(Segment {
            name,
            quoted,
            bracket,
            span,
        })
    }

    fn reference(&mut self) -> Result<Node, ParseError> {
        let mut qualifiers = Vec::new();
        let mut last = self.segment()?;
        while self.peek() == &Tok::Bang {
            self.advance();
            qualifiers.push(last);
            last = self.segment()?;
        }
        let span = qualifiers
            .first()
            .map_or(last.span, |q| q.span)
            .to(last.span);
        let reference = build_reference(&qualifiers, &last, span)?;
        Ok(node(ExprKind::Ref(reference), span, 1))
    }
}

fn bad(why: &'static str, span: Span) -> ParseError {
    ParseError::new(ParseErrorKind::BadReference { why }, span)
}

fn build_reference(
    qualifiers: &[Segment],
    cell: &Segment,
    span: Span,
) -> Result<CellRef, ParseError> {
    let mut rest = qualifiers;
    let mut container = Container::Current;
    if let Some((first, tail)) = rest.split_first() {
        if !first.quoted {
            let name = first.name.as_str();
            let bracket = first.bracket.as_deref().map(str::trim);
            let found = if name.eq_ignore_ascii_case("ThePage") && bracket.is_none() {
                Some(Container::ThePage)
            } else if name.eq_ignore_ascii_case("TheDoc") && bracket.is_none() {
                Some(Container::TheDoc)
            } else if name.eq_ignore_ascii_case("Styles") && bracket.is_none() {
                Some(Container::Styles)
            } else if name.eq_ignore_ascii_case("Pages") && bracket.is_some_and(|b| !b.is_empty()) {
                bracket.map(|b| Container::Page(b.to_string()))
            } else if name.eq_ignore_ascii_case("Masters") && bracket.is_some_and(|b| !b.is_empty())
            {
                bracket.map(|b| Container::Master(b.to_string()))
            } else {
                None
            };
            if let Some(found) = found {
                container = found;
                rest = tail;
            }
        }
    }
    let target = match rest {
        [] => SheetTarget::Itself,
        [one] => sheet_target(one)?,
        [_, extra, ..] => return Err(bad("too many sheet qualifiers before '!'", extra.span)),
    };
    if cell.quoted {
        return Err(bad("a cell name cannot be quoted", cell.span));
    }
    let index = match &cell.bracket {
        None => None,
        Some(text) => Some(
            text.trim()
                .parse::<u32>()
                .map_err(|_| bad("a row index is a whole number", cell.span))?,
        ),
    };
    let path: Vec<String> = cell.name.split('.').map(str::to_string).collect();
    if path.iter().any(String::is_empty) {
        return Err(bad("empty name in cell path", span));
    }
    Ok(CellRef {
        sheet: SheetRef { container, target },
        path,
        index,
    })
}

fn sheet_target(segment: &Segment) -> Result<SheetTarget, ParseError> {
    if segment.bracket.is_some() {
        return Err(bad("unexpected '[' after a sheet name", segment.span));
    }
    if segment.quoted {
        return Ok(SheetTarget::Name(segment.name.clone()));
    }
    match segment.name.rsplit_once('.') {
        None => Ok(SheetTarget::Name(segment.name.clone())),
        Some((name, digits)) if digits.bytes().all(|b| b.is_ascii_digit()) => {
            let id: u32 = digits
                .parse()
                .map_err(|_| bad("sheet ID is too large", segment.span))?;
            if name.eq_ignore_ascii_case("Sheet") {
                Ok(SheetTarget::Id(id))
            } else if name.contains('.') {
                Err(bad("a sheet name with dots must be quoted", segment.span))
            } else {
                Ok(SheetTarget::NamedId {
                    name: name.to_string(),
                    id,
                })
            }
        }
        Some(_) => Err(bad("a sheet name with dots must be quoted", segment.span)),
    }
}
