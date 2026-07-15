mod clauses;
mod expression;
mod pattern;
mod projection;

use crate::types::QueryError;

use super::{
    ast::{ParsedQuery, Projection, UnwindClause},
    lexer::{Symbol, Token, TokenKind},
    syntax,
};

pub(super) struct Parser {
    tokens: Vec<Token>,
    index: usize,
    end_position: usize,
    anonymous_nodes: usize,
    warnings: Vec<String>,
}

impl Parser {
    pub(super) const fn new(tokens: Vec<Token>, end_position: usize) -> Self {
        Self {
            tokens,
            index: 0,
            end_position,
            anonymous_nodes: 0,
            warnings: Vec::new(),
        }
    }

    pub(super) fn parse(mut self) -> Result<ParsedQuery, QueryError> {
        let unwind = self.parse_unwind()?;
        self.expect_keyword("MATCH")?;
        let matches = self.parse_match_clauses()?;
        let with_clause = if self.consume_keyword("WITH") {
            Some(self.parse_with_clause()?)
        } else {
            None
        };
        let (distinct, star, projections) = self.parse_return()?;
        let order = self.parse_order()?;
        let (skip, limit) = self.parse_pagination()?;
        if self.peek().is_some() {
            return Err(self.error("unsupported trailing clause"));
        }
        Ok(ParsedQuery {
            unwind,
            matches,
            with_clause,
            distinct,
            star,
            projections,
            order,
            skip,
            limit,
            warnings: self.warnings,
        })
    }

    fn parse_unwind(&mut self) -> Result<Option<UnwindClause>, QueryError> {
        if !self.consume_keyword("UNWIND") {
            return Ok(None);
        }
        let expression = self.parse_operand()?;
        self.expect_keyword("AS")?;
        let alias = self.parse_identifier("UNWIND alias")?;
        Ok(Some(UnwindClause { expression, alias }))
    }

    fn parse_return(&mut self) -> Result<(bool, bool, Vec<Projection>), QueryError> {
        self.expect_keyword("RETURN")?;
        let distinct = self.consume_keyword("DISTINCT");
        let star = self.consume_symbol(Symbol::Star);
        let projections = if star {
            Vec::new()
        } else {
            self.parse_projections()?
        };
        Ok((distinct, star, projections))
    }

    fn parse_order(&mut self) -> Result<Vec<super::ast::OrderClause>, QueryError> {
        if !self.consume_keyword("ORDER") {
            return Ok(Vec::new());
        }
        self.expect_keyword("BY")?;
        self.parse_order_clauses()
    }

    fn parse_pagination(&mut self) -> Result<(usize, Option<usize>), QueryError> {
        let mut skip = 0;
        let mut limit = None;
        loop {
            if self.consume_keyword("SKIP") {
                if skip != 0 {
                    return Err(self.error("duplicate SKIP clause"));
                }
                skip = self.parse_usize("SKIP")?;
            } else if self.consume_keyword("LIMIT") {
                if limit.is_some() {
                    return Err(self.error("duplicate LIMIT clause"));
                }
                limit = Some(self.parse_usize("LIMIT")?);
            } else {
                break;
            }
        }
        Ok((skip, limit))
    }

    pub(super) fn parse_identifier(&mut self, expected: &str) -> Result<String, QueryError> {
        match self.peek().cloned() {
            Some(TokenKind::Identifier(identifier)) => {
                self.index += 1;
                Ok(identifier)
            }
            _ => Err(self.error(&format!("expected {expected}"))),
        }
    }

    pub(super) fn parse_usize(&mut self, clause: &str) -> Result<usize, QueryError> {
        match self.peek().cloned() {
            Some(TokenKind::Number(value)) if !value.contains('.') => {
                self.index += 1;
                value
                    .parse()
                    .map_err(|_| self.error(&format!("invalid {clause} value")))
            }
            _ => Err(self.error(&format!("{clause} requires a non-negative integer"))),
        }
    }

    pub(super) fn expect_keyword(&mut self, keyword: &str) -> Result<(), QueryError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {keyword}")))
        }
    }

    pub(super) fn consume_keyword(&mut self, keyword: &str) -> bool {
        let matches = matches!(
            self.peek(),
            Some(TokenKind::Identifier(identifier)) if identifier.eq_ignore_ascii_case(keyword)
        );
        if matches {
            self.index += 1;
        }
        matches
    }

    pub(super) fn expect_symbol(&mut self, symbol: Symbol) -> Result<(), QueryError> {
        if self.consume_symbol(symbol) {
            Ok(())
        } else {
            Err(self.error("unexpected token"))
        }
    }

    pub(super) fn consume_symbol(&mut self, symbol: Symbol) -> bool {
        let matches = matches!(self.peek(), Some(TokenKind::Symbol(actual)) if *actual == symbol);
        if matches {
            self.index += 1;
        }
        matches
    }

    pub(super) fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.index).map(|token| &token.kind)
    }

    pub(super) fn peek_function_call(&self) -> bool {
        matches!(self.peek(), Some(TokenKind::Identifier(_)))
            && matches!(
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                Some(TokenKind::Symbol(Symbol::LeftParen))
            )
    }

    pub(super) fn error(&self, message: &str) -> QueryError {
        let position = self
            .tokens
            .get(self.index)
            .map_or(self.end_position, |token| token.position);
        syntax(position, message)
    }
}
