use crate::types::{QueryError, QueryValue};

use super::{
    MAX_MATCH_PATTERNS, MAX_PROJECTIONS, MAX_VARIABLE_HOPS,
    ast::{
        AggregateKind, CaseBranch, CaseExpression, CaseWhen, EdgeDirection, EdgeMatch, EdgePattern,
        Expression, MatchClause, MatchPattern, NodePattern, Operand, OrderClause, ParsedQuery,
        Predicate, PredicateOperator, Projection, ProjectionExpression, Reference, UnwindClause,
        WithClause,
    },
    function_column,
    lexer::{Symbol, Token, TokenKind},
    reference_column, syntax, unsupported,
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
        let unwind = if self.consume_keyword("UNWIND") {
            let expression = self.parse_operand()?;
            self.expect_keyword("AS")?;
            let alias = self.parse_identifier("UNWIND alias")?;
            Some(UnwindClause { expression, alias })
        } else {
            None
        };
        self.expect_keyword("MATCH")?;
        let matches = self.parse_match_clauses()?;
        let with_clause = if self.consume_keyword("WITH") {
            Some(self.parse_with_clause()?)
        } else {
            None
        };
        self.expect_keyword("RETURN")?;
        let distinct = self.consume_keyword("DISTINCT");
        let star = self.consume_symbol(Symbol::Star);
        let projections = if star {
            Vec::new()
        } else {
            self.parse_projections()?
        };
        let order = if self.consume_keyword("ORDER") {
            self.expect_keyword("BY")?;
            self.parse_order_clauses()?
        } else {
            Vec::new()
        };
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

    fn parse_match_clauses(&mut self) -> Result<Vec<MatchClause>, QueryError> {
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

    fn parse_with_clause(&mut self) -> Result<WithClause, QueryError> {
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

    fn parse_pattern_chain(&mut self) -> Result<Vec<MatchPattern>, QueryError> {
        let mut left = self.parse_node_pattern()?;
        let mut patterns = Vec::new();
        loop {
            let Some((edge, right)) = self.parse_pattern_relationship()? else {
                break;
            };
            if edge
                .alias
                .as_deref()
                .is_some_and(|alias| alias == left.alias || alias == right.alias)
            {
                return Err(unsupported("aliases in a relationship must be distinct"));
            }
            patterns.push(MatchPattern::Edge(Box::new(EdgeMatch {
                left: left.clone(),
                edge,
                right: right.clone(),
            })));
            left = right;
        }
        if patterns.is_empty() {
            patterns.push(MatchPattern::Node(left));
        }
        Ok(patterns)
    }

    fn parse_pattern_relationship(
        &mut self,
    ) -> Result<Option<(EdgePattern, NodePattern)>, QueryError> {
        let (edge, right) = if self.consume_symbol(Symbol::Dash) {
            let mut edge = self.parse_edge_pattern()?;
            edge.direction = if self.consume_symbol(Symbol::ArrowRight) {
                EdgeDirection::Outbound
            } else if self.consume_symbol(Symbol::Dash) {
                EdgeDirection::Undirected
            } else {
                return Err(self.error("expected -> or - after relationship"));
            };
            (edge, self.parse_node_pattern()?)
        } else if self.consume_symbol(Symbol::ArrowLeft) {
            let mut edge = self.parse_edge_pattern()?;
            self.expect_symbol(Symbol::Dash)?;
            edge.direction = EdgeDirection::Inbound;
            (edge, self.parse_node_pattern()?)
        } else {
            return Ok(None);
        };
        Ok(Some((edge, right)))
    }

    fn parse_node_pattern(&mut self) -> Result<NodePattern, QueryError> {
        self.expect_symbol(Symbol::LeftParen)?;
        let alias = if matches!(self.peek(), Some(TokenKind::Identifier(_))) {
            self.parse_identifier("node alias")?
        } else {
            let alias = format!("__anon_node_{}", self.anonymous_nodes);
            self.anonymous_nodes = self.anonymous_nodes.saturating_add(1);
            alias
        };
        let mut labels = Vec::new();
        if self.consume_symbol(Symbol::Colon) {
            labels.push(self.parse_identifier("node label")?);
            while self.consume_symbol(Symbol::Pipe) {
                self.consume_symbol(Symbol::Colon);
                labels.push(self.parse_identifier("node label")?);
            }
        }
        let mut properties = Vec::new();
        if self.consume_symbol(Symbol::LeftBrace) && !self.consume_symbol(Symbol::RightBrace) {
            loop {
                let key = self.parse_identifier("inline property name")?;
                self.expect_symbol(Symbol::Colon)?;
                let Operand::Literal(value) = self.parse_operand()? else {
                    return Err(self.error("inline properties require literal values"));
                };
                properties.push((key, *value));
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightBrace)?;
        }
        self.expect_symbol(Symbol::RightParen)?;
        Ok(NodePattern {
            alias,
            labels,
            properties,
        })
    }

    fn parse_edge_pattern(&mut self) -> Result<EdgePattern, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut alias = None;
        let mut kinds = Vec::new();
        if self.consume_symbol(Symbol::Colon) {
            kinds.push(self.parse_identifier("relationship kind")?);
        } else if matches!(self.peek(), Some(TokenKind::Identifier(_))) {
            alias = Some(self.parse_identifier("relationship alias")?);
            if self.consume_symbol(Symbol::Colon) {
                kinds.push(self.parse_identifier("relationship kind")?);
            }
        }
        while self.consume_symbol(Symbol::Pipe) {
            self.consume_symbol(Symbol::Colon);
            kinds.push(self.parse_identifier("relationship kind")?);
        }
        let mut min_hops = 1;
        let mut max_hops = 1;
        if self.consume_symbol(Symbol::Star) {
            max_hops = MAX_VARIABLE_HOPS;
            let first = if matches!(self.peek(), Some(TokenKind::Number(_))) {
                Some(self.parse_usize("relationship hop count")?)
            } else {
                None
            };
            if self.consume_symbol(Symbol::Dot) {
                self.expect_symbol(Symbol::Dot)?;
                min_hops = first.unwrap_or(1);
                if matches!(self.peek(), Some(TokenKind::Number(_))) {
                    max_hops = self.parse_usize("relationship maximum hop count")?;
                }
            } else if let Some(hops) = first {
                min_hops = hops;
                max_hops = hops;
            }
            if min_hops > MAX_VARIABLE_HOPS || max_hops > MAX_VARIABLE_HOPS {
                self.warnings.push(format!(
                    "variable-length relationship bound {min_hops}..{max_hops} was clamped to {MAX_VARIABLE_HOPS} hops"
                ));
            }
            min_hops = min_hops.min(MAX_VARIABLE_HOPS);
            max_hops = max_hops.min(MAX_VARIABLE_HOPS);
            if max_hops < min_hops {
                return Err(self.error("relationship maximum hop count is below minimum"));
            }
        }
        self.expect_symbol(Symbol::RightBracket)?;
        Ok(EdgePattern {
            alias,
            kinds,
            direction: EdgeDirection::Outbound,
            min_hops,
            max_hops,
        })
    }

    fn parse_or_expression(&mut self) -> Result<Expression, QueryError> {
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
        let (operator, right) = if self.consume_symbol(Symbol::Colon) {
            let mut labels = vec![self.parse_identifier("node label")?];
            while self.consume_symbol(Symbol::Pipe) {
                self.consume_symbol(Symbol::Colon);
                labels.push(self.parse_identifier("node label")?);
            }
            (PredicateOperator::HasLabel(labels), None)
        } else if self.consume_keyword("IS") {
            let negated = self.consume_keyword("NOT");
            self.expect_keyword("NULL")?;
            (
                if negated {
                    PredicateOperator::IsNotNull
                } else {
                    PredicateOperator::IsNull
                },
                None,
            )
        } else if self.consume_keyword("CONTAINS") {
            (PredicateOperator::Contains, Some(self.parse_operand()?))
        } else if self.consume_keyword("STARTS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::StartsWith, Some(self.parse_operand()?))
        } else if self.consume_keyword("ENDS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::EndsWith, Some(self.parse_operand()?))
        } else if self.consume_keyword("IN") {
            (PredicateOperator::In, Some(self.parse_list_operand()?))
        } else if self.consume_keyword("NOT") {
            self.expect_keyword("IN")?;
            (PredicateOperator::NotIn, Some(self.parse_list_operand()?))
        } else {
            let operator = if self.consume_symbol(Symbol::Equal) {
                PredicateOperator::Equal
            } else if self.consume_symbol(Symbol::NotEqual) {
                PredicateOperator::NotEqual
            } else if self.consume_symbol(Symbol::LessEqual) {
                PredicateOperator::LessEqual
            } else if self.consume_symbol(Symbol::GreaterEqual) {
                PredicateOperator::GreaterEqual
            } else if self.consume_symbol(Symbol::Less) {
                PredicateOperator::Less
            } else if self.consume_symbol(Symbol::Greater) {
                PredicateOperator::Greater
            } else if self.consume_symbol(Symbol::Regex) {
                PredicateOperator::Regex
            } else {
                return Err(self.error("expected predicate operator"));
            };
            (operator, Some(self.parse_operand()?))
        };
        Ok(Predicate {
            left,
            operator,
            right,
        })
    }

    fn parse_operand(&mut self) -> Result<Operand, QueryError> {
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

    fn parse_function_operand(&mut self) -> Result<Operand, QueryError> {
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

    fn parse_projections(&mut self) -> Result<Vec<Projection>, QueryError> {
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
        let aggregate = if self.consume_keyword("COUNT") {
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
        };
        let (expression, default_column) = if let Some((kind, name)) = aggregate {
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
            (
                ProjectionExpression::Aggregate {
                    kind,
                    target,
                    distinct,
                },
                column,
            )
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

    fn parse_order_clauses(&mut self) -> Result<Vec<OrderClause>, QueryError> {
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

    fn parse_reference(&mut self) -> Result<Reference, QueryError> {
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

    fn parse_identifier(&mut self, expected: &str) -> Result<String, QueryError> {
        match self.peek().cloned() {
            Some(TokenKind::Identifier(identifier)) => {
                self.index += 1;
                Ok(identifier)
            }
            _ => Err(self.error(&format!("expected {expected}"))),
        }
    }

    fn parse_usize(&mut self, clause: &str) -> Result<usize, QueryError> {
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

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), QueryError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {keyword}")))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let matches = matches!(
            self.peek(),
            Some(TokenKind::Identifier(identifier)) if identifier.eq_ignore_ascii_case(keyword)
        );
        if matches {
            self.index += 1;
        }
        matches
    }

    fn expect_symbol(&mut self, symbol: Symbol) -> Result<(), QueryError> {
        if self.consume_symbol(symbol) {
            Ok(())
        } else {
            Err(self.error("unexpected token"))
        }
    }

    fn consume_symbol(&mut self, symbol: Symbol) -> bool {
        let matches = matches!(self.peek(), Some(TokenKind::Symbol(actual)) if *actual == symbol);
        if matches {
            self.index += 1;
        }
        matches
    }

    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.index).map(|token| &token.kind)
    }

    fn peek_function_call(&self) -> bool {
        matches!(self.peek(), Some(TokenKind::Identifier(_)))
            && matches!(
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                Some(TokenKind::Symbol(Symbol::LeftParen))
            )
    }

    fn error(&self, message: &str) -> QueryError {
        let position = self
            .tokens
            .get(self.index)
            .map_or(self.end_position, |token| token.position);
        syntax(position, message)
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
