use crate::types::QueryError;

use super::super::{
    MAX_MATCH_PATTERNS,
    ast::{MatchClause, WithClause},
    lexer::Symbol,
    unsupported,
};
use super::Parser;

impl Parser {
    pub(super) fn parse_match_clauses(&mut self) -> Result<Vec<MatchClause>, QueryError> {
        let mut clauses = Vec::new();
        let mut optional = false;
        loop {
            let mut patterns = self.parse_pattern_chain()?;
            while self.consume_symbol(Symbol::Comma) {
                patterns.extend(self.parse_pattern_chain()?);
            }
            let filter = if self.consume_keyword("WHERE") {
                Some(self.parse_or_expression()?)
            } else {
                None
            };
            clauses.push(MatchClause {
                patterns,
                filter,
                optional,
            });
            if clauses
                .iter()
                .map(|clause| clause.patterns.len())
                .sum::<usize>()
                > MAX_MATCH_PATTERNS
            {
                return Err(unsupported("query exceeds MATCH pattern safety cap"));
            }
            if self.consume_keyword("OPTIONAL") {
                self.expect_keyword("MATCH")?;
                optional = true;
            } else if self.consume_keyword("MATCH") {
                optional = false;
            } else {
                break;
            }
        }
        Ok(clauses)
    }

    pub(super) fn parse_with_clause(&mut self) -> Result<WithClause, QueryError> {
        let distinct = self.consume_keyword("DISTINCT");
        let projections = self.parse_projections()?;
        let mut filter = None;
        let mut order = Vec::new();
        let mut skip = 0;
        let mut limit = None;
        loop {
            if self.consume_keyword("WHERE") {
                if filter.is_some() {
                    return Err(self.error("duplicate WHERE after WITH"));
                }
                filter = Some(self.parse_or_expression()?);
            } else if self.consume_keyword("ORDER") {
                if !order.is_empty() {
                    return Err(self.error("duplicate ORDER BY after WITH"));
                }
                self.expect_keyword("BY")?;
                order = self.parse_order_clauses()?;
            } else if self.consume_keyword("SKIP") {
                if skip != 0 {
                    return Err(self.error("duplicate SKIP after WITH"));
                }
                skip = self.parse_usize("SKIP")?;
            } else if self.consume_keyword("LIMIT") {
                if limit.is_some() {
                    return Err(self.error("duplicate LIMIT after WITH"));
                }
                limit = Some(self.parse_usize("LIMIT")?);
            } else {
                break;
            }
        }
        Ok(WithClause {
            distinct,
            projections,
            filter,
            order,
            skip,
            limit,
        })
    }
}
