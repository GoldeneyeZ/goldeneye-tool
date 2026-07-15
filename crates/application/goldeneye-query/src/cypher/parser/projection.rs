use crate::types::QueryError;

use super::super::{
    MAX_PROJECTIONS,
    ast::{
        AggregateKind, CaseBranch, CaseExpression, CaseWhen, Operand, OrderClause, Projection,
        ProjectionExpression, Reference,
    },
    function_column,
    lexer::Symbol,
    reference_column, unsupported,
};
use super::Parser;

impl Parser {
    pub(super) fn parse_projections(&mut self) -> Result<Vec<Projection>, QueryError> {
        let mut projections = Vec::new();
        loop {
            projections.push(self.parse_projection()?);
            if projections.len() > MAX_PROJECTIONS {
                return Err(unsupported("query exceeds projection safety cap"));
            }
            if !self.consume_symbol(Symbol::Comma) {
                break;
            }
        }
        if projections.is_empty() {
            return Err(self.error("RETURN requires at least one expression"));
        }
        Ok(projections)
    }

    fn parse_projection(&mut self) -> Result<Projection, QueryError> {
        let (expression, default_column) = if let Some((kind, name)) = self.parse_aggregate_kind() {
            self.parse_aggregate_projection(kind, name)?
        } else if self.consume_keyword("CASE") {
            let expression = self.parse_case_expression()?;
            (ProjectionExpression::Case(expression), "case".to_owned())
        } else if self.peek_function_call() {
            let Operand::Function { name, arguments } = self.parse_function_operand()? else {
                unreachable!();
            };
            let column = function_column(&name, &arguments);
            (ProjectionExpression::Function { name, arguments }, column)
        } else {
            let reference = self.parse_reference()?;
            let column = reference_column(&reference);
            (ProjectionExpression::Reference(reference), column)
        };
        let column = if self.consume_keyword("AS") {
            self.parse_identifier("projection alias")?
        } else {
            default_column
        };
        Ok(Projection { expression, column })
    }

    fn parse_aggregate_kind(&mut self) -> Option<(AggregateKind, &'static str)> {
        if self.consume_keyword("COUNT") {
            Some((AggregateKind::Count, "count"))
        } else if self.consume_keyword("SUM") {
            Some((AggregateKind::Sum, "sum"))
        } else if self.consume_keyword("AVG") {
            Some((AggregateKind::Average, "avg"))
        } else if self.consume_keyword("MIN") {
            Some((AggregateKind::Minimum, "min"))
        } else if self.consume_keyword("MAX") {
            Some((AggregateKind::Maximum, "max"))
        } else if self.consume_keyword("COLLECT") {
            Some((AggregateKind::Collect, "collect"))
        } else {
            None
        }
    }

    fn parse_aggregate_projection(
        &mut self,
        kind: AggregateKind,
        name: &str,
    ) -> Result<(ProjectionExpression, String), QueryError> {
        self.expect_symbol(Symbol::LeftParen)?;
        let distinct = self.consume_keyword("DISTINCT");
        let target = if self.consume_symbol(Symbol::Star) {
            None
        } else {
            Some(self.parse_reference()?)
        };
        self.expect_symbol(Symbol::RightParen)?;
        if target.is_none() && !matches!(kind, AggregateKind::Count) {
            return Err(self.error("only COUNT accepts *"));
        }
        let target_column = target
            .as_ref()
            .map_or_else(|| "*".to_owned(), reference_column);
        let distinct_prefix = if distinct { "DISTINCT " } else { "" };
        let column = format!("{name}({distinct_prefix}{target_column})");
        Ok((
            ProjectionExpression::Aggregate {
                kind,
                target,
                distinct,
            },
            column,
        ))
    }

    fn parse_case_expression(&mut self) -> Result<CaseExpression, QueryError> {
        let subject = if self.consume_keyword("WHEN") {
            None
        } else {
            let subject = self.parse_operand()?;
            self.expect_keyword("WHEN")?;
            Some(subject)
        };
        let mut branches = Vec::new();
        loop {
            let when = if subject.is_some() {
                CaseWhen::Value(self.parse_operand()?)
            } else {
                CaseWhen::Predicate(self.parse_or_expression()?)
            };
            self.expect_keyword("THEN")?;
            let then = self.parse_operand()?;
            branches.push(CaseBranch { when, then });
            if !self.consume_keyword("WHEN") {
                break;
            }
        }
        let fallback = if self.consume_keyword("ELSE") {
            Some(self.parse_operand()?)
        } else {
            None
        };
        self.expect_keyword("END")?;
        Ok(CaseExpression {
            subject,
            branches,
            fallback,
        })
    }

    pub(super) fn parse_order_clauses(&mut self) -> Result<Vec<OrderClause>, QueryError> {
        let mut clauses = Vec::new();
        loop {
            let reference = self.parse_reference()?;
            let descending = if self.consume_keyword("DESC") {
                true
            } else {
                self.consume_keyword("ASC");
                false
            };
            clauses.push(OrderClause {
                reference,
                descending,
            });
            if !self.consume_symbol(Symbol::Comma) {
                break;
            }
        }
        Ok(clauses)
    }

    pub(super) fn parse_reference(&mut self) -> Result<Reference, QueryError> {
        if self.consume_keyword("TYPE") {
            self.expect_symbol(Symbol::LeftParen)?;
            let alias = self.parse_identifier("relationship alias")?;
            self.expect_symbol(Symbol::RightParen)?;
            return Ok(Reference::EdgeType(alias));
        }
        let alias = self.parse_identifier("alias")?;
        let mut path = Vec::new();
        while self.consume_symbol(Symbol::Dot) {
            path.push(self.parse_identifier("property")?);
        }
        if path.is_empty() {
            Ok(Reference::Alias(alias))
        } else {
            Ok(Reference::Property { alias, path })
        }
    }
}
