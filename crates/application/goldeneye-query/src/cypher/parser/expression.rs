use crate::types::{QueryError, QueryValue};

use super::super::{
    ast::{Expression, Operand, Predicate, PredicateOperator},
    lexer::{Symbol, TokenKind},
    syntax,
};
use super::Parser;

impl Parser {
    pub(super) fn parse_or_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_xor_expression()?;
        while self.consume_keyword("OR") {
            expression =
                Expression::Or(Box::new(expression), Box::new(self.parse_xor_expression()?));
        }
        Ok(expression)
    }

    fn parse_xor_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_and_expression()?;
        while self.consume_keyword("XOR") {
            expression =
                Expression::Xor(Box::new(expression), Box::new(self.parse_and_expression()?));
        }
        Ok(expression)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_not_expression()?;
        while self.consume_keyword("AND") {
            expression =
                Expression::And(Box::new(expression), Box::new(self.parse_not_expression()?));
        }
        Ok(expression)
    }

    fn parse_not_expression(&mut self) -> Result<Expression, QueryError> {
        if self.consume_keyword("NOT") {
            return Ok(Expression::Not(Box::new(self.parse_not_expression()?)));
        }
        if self.consume_keyword("EXISTS") {
            self.expect_symbol(Symbol::LeftBrace)?;
            let patterns = self.parse_pattern_chain()?;
            self.expect_symbol(Symbol::RightBrace)?;
            return Ok(Expression::Exists(patterns));
        }
        if self.consume_symbol(Symbol::LeftParen) {
            let expression = self.parse_or_expression()?;
            self.expect_symbol(Symbol::RightParen)?;
            return Ok(expression);
        }
        Ok(Expression::Predicate(Box::new(self.parse_predicate()?)))
    }

    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let left = self.parse_operand()?;
        let (operator, right) = self.parse_predicate_tail()?;
        Ok(Predicate {
            left,
            operator,
            right,
        })
    }

    fn parse_predicate_tail(&mut self) -> Result<(PredicateOperator, Option<Operand>), QueryError> {
        if self.consume_symbol(Symbol::Colon) {
            return Ok((
                PredicateOperator::HasLabel(self.parse_predicate_labels()?),
                None,
            ));
        }
        if self.consume_keyword("IS") {
            let negated = self.consume_keyword("NOT");
            self.expect_keyword("NULL")?;
            let operator = if negated {
                PredicateOperator::IsNotNull
            } else {
                PredicateOperator::IsNull
            };
            return Ok((operator, None));
        }
        self.parse_binary_predicate()
    }

    fn parse_predicate_labels(&mut self) -> Result<Vec<String>, QueryError> {
        let mut labels = vec![self.parse_identifier("node label")?];
        while self.consume_symbol(Symbol::Pipe) {
            self.consume_symbol(Symbol::Colon);
            labels.push(self.parse_identifier("node label")?);
        }
        Ok(labels)
    }

    fn parse_binary_predicate(
        &mut self,
    ) -> Result<(PredicateOperator, Option<Operand>), QueryError> {
        let (operator, right) = if self.consume_keyword("CONTAINS") {
            (PredicateOperator::Contains, self.parse_operand()?)
        } else if self.consume_keyword("STARTS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::StartsWith, self.parse_operand()?)
        } else if self.consume_keyword("ENDS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::EndsWith, self.parse_operand()?)
        } else if self.consume_keyword("IN") {
            (PredicateOperator::In, self.parse_list_operand()?)
        } else if self.consume_keyword("NOT") {
            self.expect_keyword("IN")?;
            (PredicateOperator::NotIn, self.parse_list_operand()?)
        } else {
            (self.parse_comparison_operator()?, self.parse_operand()?)
        };
        Ok((operator, Some(right)))
    }

    fn parse_comparison_operator(&mut self) -> Result<PredicateOperator, QueryError> {
        if self.consume_symbol(Symbol::Equal) {
            Ok(PredicateOperator::Equal)
        } else if self.consume_symbol(Symbol::NotEqual) {
            Ok(PredicateOperator::NotEqual)
        } else if self.consume_symbol(Symbol::LessEqual) {
            Ok(PredicateOperator::LessEqual)
        } else if self.consume_symbol(Symbol::GreaterEqual) {
            Ok(PredicateOperator::GreaterEqual)
        } else if self.consume_symbol(Symbol::Less) {
            Ok(PredicateOperator::Less)
        } else if self.consume_symbol(Symbol::Greater) {
            Ok(PredicateOperator::Greater)
        } else if self.consume_symbol(Symbol::Regex) {
            Ok(PredicateOperator::Regex)
        } else {
            Err(self.error("expected predicate operator"))
        }
    }

    pub(super) fn parse_operand(&mut self) -> Result<Operand, QueryError> {
        if matches!(self.peek(), Some(TokenKind::Symbol(Symbol::LeftBracket))) {
            return self.parse_list_operand();
        }
        if self.peek_function_call() {
            return self.parse_function_operand();
        }
        if self.consume_keyword("TRUE") {
            return Ok(Operand::Literal(Box::new(QueryValue::Bool(true))));
        }
        if self.consume_keyword("FALSE") {
            return Ok(Operand::Literal(Box::new(QueryValue::Bool(false))));
        }
        if self.consume_keyword("NULL") {
            return Ok(Operand::Literal(Box::new(QueryValue::Null)));
        }
        if let Some(TokenKind::String(value)) = self.peek().cloned() {
            self.index += 1;
            return Ok(Operand::Literal(Box::new(QueryValue::String(value))));
        }
        let negative = self.consume_symbol(Symbol::Dash);
        if let Some(TokenKind::Number(value)) = self.peek().cloned() {
            self.index += 1;
            return parse_number(&value, negative).map(|value| Operand::Literal(Box::new(value)));
        }
        if negative {
            return Err(self.error("expected number after unary minus"));
        }
        Ok(Operand::Reference(self.parse_reference()?))
    }

    pub(super) fn parse_function_operand(&mut self) -> Result<Operand, QueryError> {
        let name = self.parse_identifier("function name")?;
        self.expect_symbol(Symbol::LeftParen)?;
        let mut arguments = Vec::new();
        if !self.consume_symbol(Symbol::RightParen) {
            loop {
                arguments.push(self.parse_operand()?);
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightParen)?;
        }
        Ok(Operand::Function { name, arguments })
    }

    fn parse_list_operand(&mut self) -> Result<Operand, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut values = Vec::new();
        if !self.consume_symbol(Symbol::RightBracket) {
            loop {
                values.push(self.parse_operand()?);
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightBracket)?;
        }
        Ok(Operand::List(values))
    }
}

fn parse_number(value: &str, negative: bool) -> Result<QueryValue, QueryError> {
    let sign = if negative { "-" } else { "" };
    let number = format!("{sign}{value}");
    if value.contains('.') {
        return number
            .parse::<f64>()
            .map(QueryValue::Float)
            .map_err(|_| syntax(0, "invalid floating-point literal"));
    }
    number
        .parse::<i64>()
        .map(QueryValue::Integer)
        .map_err(|_| syntax(0, "invalid integer literal"))
}
