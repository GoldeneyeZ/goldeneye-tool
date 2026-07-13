use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use crate::{
    engine::node_summary,
    types::{EdgeSummary, QueryError, QueryGraphRequest, QueryGraphResult, QueryValue},
};

const MAX_QUERY_ROWS: usize = 10_000;

pub(crate) fn execute(
    request: &QueryGraphRequest,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Result<QueryGraphResult, QueryError> {
    if request.max_rows == 0 || request.max_rows > MAX_QUERY_ROWS {
        return Err(QueryError::InvalidQueryRowLimit {
            actual: request.max_rows,
            maximum: MAX_QUERY_ROWS,
        });
    }
    let tokens = lex(&request.query)?;
    reject_mutations(&tokens)?;
    let query = Parser::new(tokens, request.query.len()).parse()?;
    let degrees = graph_degrees(edges);
    let mut bindings = build_bindings(&query.pattern, nodes, edges);
    if let Some(expression) = &query.filter {
        let mut retained = Vec::with_capacity(bindings.len());
        for binding in bindings {
            if evaluate_expression(expression, &binding, &degrees)? {
                retained.push(binding);
            }
        }
        bindings = retained;
    }

    let columns = query
        .projections
        .iter()
        .map(|projection| projection.column.clone())
        .collect();
    let mut rows = materialize_rows(&query, bindings, &degrees)?;
    if query.distinct {
        let mut seen = BTreeSet::new();
        rows.retain(|row| seen.insert(row_key(row)));
    }
    let skipped = query.skip.min(rows.len());
    rows.drain(..skipped);
    let total = rows.len();
    let query_limit = query.limit.unwrap_or(usize::MAX);
    let materialized_limit = request.max_rows.min(query_limit);
    let truncated = total > materialized_limit;
    rows.truncate(materialized_limit);

    Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns,
        rows,
        total,
        truncated,
    })
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Identifier(String),
    String(String),
    Number(String),
    Symbol(Symbol),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symbol {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    Dot,
    Dash,
    ArrowRight,
    ArrowLeft,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Star,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    position: usize,
}

fn lex(input: &str) -> Result<Vec<Token>, QueryError> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character.is_whitespace() {
            index += character.len_utf8();
            continue;
        }
        if character == '\'' || character == '"' {
            let (value, next) = lex_string(input, index, character)?;
            tokens.push(Token {
                kind: TokenKind::String(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '`' {
            let (value, next) = lex_backtick_identifier(input, index)?;
            tokens.push(Token {
                kind: TokenKind::Identifier(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character.is_ascii_digit() {
            let next = lex_number_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Number(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '_' || character.is_alphabetic() {
            let next = lex_identifier_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Identifier(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        let (symbol, consumed) = lex_symbol(input, index)?;
        tokens.push(Token {
            kind: TokenKind::Symbol(symbol),
            position: index,
        });
        index += consumed;
    }
    Ok(tokens)
}

fn lex_string(input: &str, start: usize, quote: char) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + quote.len_utf8();
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == quote {
            return Ok((value, index + character.len_utf8()));
        }
        if character == '\\' {
            index += character.len_utf8();
            let escaped = input[index..]
                .chars()
                .next()
                .ok_or_else(|| syntax(start, "unterminated string escape"))?;
            value.push(match escaped {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            });
            index += escaped.len_utf8();
            continue;
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(syntax(start, "unterminated string literal"))
}

fn lex_backtick_identifier(input: &str, start: usize) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + 1;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == '`' {
            return Ok((value, index + 1));
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(syntax(start, "unterminated backtick identifier"))
}

fn lex_number_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut index = start;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index + 1 < bytes.len() && bytes[index] == b'.' && bytes[index + 1].is_ascii_digit() {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    index
}

fn lex_identifier_end(input: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, character) in input[start..].char_indices() {
        if offset == 0 || character == '_' || character.is_alphanumeric() {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn lex_symbol(input: &str, index: usize) -> Result<(Symbol, usize), QueryError> {
    let rest = &input[index..];
    let pair = rest.get(..2);
    if let Some(symbol) = pair.and_then(|pair| match pair {
        "->" => Some(Symbol::ArrowRight),
        "<-" => Some(Symbol::ArrowLeft),
        "<>" | "!=" => Some(Symbol::NotEqual),
        "<=" => Some(Symbol::LessEqual),
        ">=" => Some(Symbol::GreaterEqual),
        _ => None,
    }) {
        return Ok((symbol, 2));
    }
    let symbol = match rest.as_bytes()[0] {
        b'(' => Symbol::LeftParen,
        b')' => Symbol::RightParen,
        b'[' => Symbol::LeftBracket,
        b']' => Symbol::RightBracket,
        b':' => Symbol::Colon,
        b',' => Symbol::Comma,
        b'.' => Symbol::Dot,
        b'-' => Symbol::Dash,
        b'=' => Symbol::Equal,
        b'<' => Symbol::Less,
        b'>' => Symbol::Greater,
        b'*' => Symbol::Star,
        _ => return Err(syntax(index, "unsupported character")),
    };
    Ok((symbol, 1))
}

fn reject_mutations(tokens: &[Token]) -> Result<(), QueryError> {
    const MUTATING: &[&str] = &[
        "ALTER", "CALL", "CREATE", "DELETE", "DETACH", "DROP", "FOREACH", "INSERT", "LOAD",
        "MERGE", "REMOVE", "SET", "UPDATE",
    ];
    for token in tokens {
        let TokenKind::Identifier(identifier) = &token.kind else {
            continue;
        };
        if let Some(keyword) = MUTATING
            .iter()
            .find(|keyword| identifier.eq_ignore_ascii_case(keyword))
        {
            return Err(QueryError::MutatingQuery {
                keyword: (*keyword).to_owned(),
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedQuery {
    pattern: MatchPattern,
    filter: Option<Expression>,
    distinct: bool,
    projections: Vec<Projection>,
    order: Vec<OrderClause>,
    skip: usize,
    limit: Option<usize>,
}

#[derive(Debug)]
enum MatchPattern {
    Node(NodePattern),
    Edge(Box<EdgeMatch>),
}

#[derive(Debug)]
struct EdgeMatch {
    left: NodePattern,
    edge: EdgePattern,
    right: NodePattern,
}

#[derive(Debug)]
struct NodePattern {
    alias: String,
    label: Option<String>,
}

#[derive(Debug)]
struct EdgePattern {
    alias: Option<String>,
    kind: Option<String>,
    direction: EdgeDirection,
}

#[derive(Debug, Clone, Copy)]
enum EdgeDirection {
    Outbound,
    Inbound,
    Undirected,
}

#[derive(Debug)]
enum Expression {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Predicate(Box<Predicate>),
}

#[derive(Debug)]
struct Predicate {
    left: Operand,
    operator: PredicateOperator,
    right: Option<Operand>,
}

#[derive(Debug)]
enum PredicateOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Contains,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}

#[derive(Debug)]
enum Operand {
    Literal(Box<QueryValue>),
    Reference(Reference),
}

#[derive(Debug, Clone)]
enum Reference {
    Alias(String),
    Property { alias: String, path: Vec<String> },
    EdgeType(String),
}

#[derive(Debug)]
struct Projection {
    expression: ProjectionExpression,
    column: String,
}

#[derive(Debug)]
enum ProjectionExpression {
    Reference(Reference),
    Count(Option<String>),
}

#[derive(Debug)]
struct OrderClause {
    reference: Reference,
    descending: bool,
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    end_position: usize,
}

impl Parser {
    const fn new(tokens: Vec<Token>, end_position: usize) -> Self {
        Self {
            tokens,
            index: 0,
            end_position,
        }
    }

    fn parse(mut self) -> Result<ParsedQuery, QueryError> {
        self.expect_keyword("MATCH")?;
        let pattern = self.parse_pattern()?;
        let filter = if self.consume_keyword("WHERE") {
            Some(self.parse_or_expression()?)
        } else {
            None
        };
        self.expect_keyword("RETURN")?;
        let distinct = self.consume_keyword("DISTINCT");
        let projections = self.parse_projections()?;
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
            pattern,
            filter,
            distinct,
            projections,
            order,
            skip,
            limit,
        })
    }

    fn parse_pattern(&mut self) -> Result<MatchPattern, QueryError> {
        let left = self.parse_node_pattern()?;
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
            return Ok(MatchPattern::Node(left));
        };
        if left.alias == right.alias
            || edge
                .alias
                .as_deref()
                .is_some_and(|alias| alias == left.alias || alias == right.alias)
        {
            return Err(unsupported("aliases in a one-hop pattern must be distinct"));
        }
        Ok(MatchPattern::Edge(Box::new(EdgeMatch {
            left,
            edge,
            right,
        })))
    }

    fn parse_node_pattern(&mut self) -> Result<NodePattern, QueryError> {
        self.expect_symbol(Symbol::LeftParen)?;
        let alias = self.parse_identifier("node alias")?;
        let label = if self.consume_symbol(Symbol::Colon) {
            Some(self.parse_identifier("node label")?)
        } else {
            None
        };
        self.expect_symbol(Symbol::RightParen)?;
        Ok(NodePattern { alias, label })
    }

    fn parse_edge_pattern(&mut self) -> Result<EdgePattern, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut alias = None;
        let mut kind = None;
        if self.consume_symbol(Symbol::Colon) {
            kind = Some(self.parse_identifier("relationship kind")?);
        } else if matches!(self.peek(), Some(TokenKind::Identifier(_))) {
            alias = Some(self.parse_identifier("relationship alias")?);
            if self.consume_symbol(Symbol::Colon) {
                kind = Some(self.parse_identifier("relationship kind")?);
            }
        }
        if self.consume_symbol(Symbol::Star) {
            return Err(unsupported(
                "variable-length relationships are not supported",
            ));
        }
        self.expect_symbol(Symbol::RightBracket)?;
        Ok(EdgePattern {
            alias,
            kind,
            direction: EdgeDirection::Outbound,
        })
    }

    fn parse_or_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_and_expression()?;
        while self.consume_keyword("OR") {
            expression =
                Expression::Or(Box::new(expression), Box::new(self.parse_and_expression()?));
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
        if self.consume_symbol(Symbol::LeftParen) {
            let expression = self.parse_or_expression()?;
            self.expect_symbol(Symbol::RightParen)?;
            return Ok(expression);
        }
        Ok(Expression::Predicate(Box::new(self.parse_predicate()?)))
    }

    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let left = self.parse_operand()?;
        let (operator, right) = if self.consume_keyword("IS") {
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

    fn parse_projections(&mut self) -> Result<Vec<Projection>, QueryError> {
        let mut projections = Vec::new();
        loop {
            projections.push(self.parse_projection()?);
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
        let (expression, default_column) = if self.consume_keyword("COUNT") {
            self.expect_symbol(Symbol::LeftParen)?;
            let target = if self.consume_symbol(Symbol::Star) {
                None
            } else {
                Some(self.parse_identifier("count alias")?)
            };
            self.expect_symbol(Symbol::RightParen)?;
            let column = format!("count({})", target.as_deref().unwrap_or("*"));
            (ProjectionExpression::Count(target), column)
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

fn reference_column(reference: &Reference) -> String {
    match reference {
        Reference::Alias(alias) => alias.clone(),
        Reference::Property { alias, path } => format!("{alias}.{}", path.join(".")),
        Reference::EdgeType(alias) => format!("type({alias})"),
    }
}

fn syntax(position: usize, message: &str) -> QueryError {
    QueryError::CypherSyntax {
        position,
        message: message.to_owned(),
    }
}

fn unsupported(message: &str) -> QueryError {
    QueryError::UnsupportedQuery {
        message: message.to_owned(),
    }
}

#[derive(Clone)]
struct Binding<'a> {
    nodes: BTreeMap<String, &'a GraphNode>,
    edges: BTreeMap<String, &'a GraphEdge>,
}

fn build_bindings<'a>(
    pattern: &MatchPattern,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
) -> Vec<Binding<'a>> {
    match pattern {
        MatchPattern::Node(pattern) => nodes
            .iter()
            .filter(|node| node_matches(node, pattern))
            .map(|node| Binding {
                nodes: BTreeMap::from([(pattern.alias.clone(), node)]),
                edges: BTreeMap::new(),
            })
            .collect(),
        MatchPattern::Edge(pattern) => {
            let EdgeMatch { left, edge, right } = pattern.as_ref();
            let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
                nodes.iter().map(|node| (&node.id, node)).collect();
            let mut bindings = Vec::new();
            for graph_edge in edges.iter().filter(|graph_edge| {
                edge.kind
                    .as_deref()
                    .is_none_or(|kind| graph_edge.kind.as_str() == kind)
            }) {
                let Some(source) = nodes_by_id.get(&graph_edge.source).copied() else {
                    continue;
                };
                let Some(target) = nodes_by_id.get(&graph_edge.target).copied() else {
                    continue;
                };
                match edge.direction {
                    EdgeDirection::Outbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        source,
                        target,
                        graph_edge,
                    ),
                    EdgeDirection::Inbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        target,
                        source,
                        graph_edge,
                    ),
                    EdgeDirection::Undirected => {
                        push_edge_binding(
                            &mut bindings,
                            left,
                            right,
                            edge,
                            source,
                            target,
                            graph_edge,
                        );
                        if source.id != target.id {
                            push_edge_binding(
                                &mut bindings,
                                left,
                                right,
                                edge,
                                target,
                                source,
                                graph_edge,
                            );
                        }
                    }
                }
            }
            bindings
        }
    }
}

fn push_edge_binding<'a>(
    bindings: &mut Vec<Binding<'a>>,
    left_pattern: &NodePattern,
    right_pattern: &NodePattern,
    edge_pattern: &EdgePattern,
    left: &'a GraphNode,
    right: &'a GraphNode,
    edge: &'a GraphEdge,
) {
    if !node_matches(left, left_pattern) || !node_matches(right, right_pattern) {
        return;
    }
    let mut edges = BTreeMap::new();
    if let Some(alias) = &edge_pattern.alias {
        edges.insert(alias.clone(), edge);
    }
    bindings.push(Binding {
        nodes: BTreeMap::from([
            (left_pattern.alias.clone(), left),
            (right_pattern.alias.clone(), right),
        ]),
        edges,
    });
}

fn node_matches(node: &GraphNode, pattern: &NodePattern) -> bool {
    pattern
        .label
        .as_deref()
        .is_none_or(|label| node.label.as_str() == label)
}

fn graph_degrees(edges: &[GraphEdge]) -> BTreeMap<NodeId, (usize, usize)> {
    let mut degrees = BTreeMap::new();
    for edge in edges {
        degrees.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
        degrees.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
    }
    degrees
}

fn evaluate_expression(
    expression: &Expression,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<bool, QueryError> {
    match expression {
        Expression::And(left, right) => Ok(evaluate_expression(left, binding, degrees)?
            && evaluate_expression(right, binding, degrees)?),
        Expression::Or(left, right) => Ok(evaluate_expression(left, binding, degrees)?
            || evaluate_expression(right, binding, degrees)?),
        Expression::Not(inner) => Ok(!evaluate_expression(inner, binding, degrees)?),
        Expression::Predicate(predicate) => evaluate_predicate(predicate, binding, degrees),
    }
}

fn evaluate_predicate(
    predicate: &Predicate,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<bool, QueryError> {
    let left = evaluate_operand(&predicate.left, binding, degrees)?;
    if matches!(predicate.operator, PredicateOperator::IsNull) {
        return Ok(matches!(left, QueryValue::Null));
    }
    if matches!(predicate.operator, PredicateOperator::IsNotNull) {
        return Ok(!matches!(left, QueryValue::Null));
    }
    let right = evaluate_operand(
        predicate
            .right
            .as_ref()
            .expect("binary predicate has right operand"),
        binding,
        degrees,
    )?;
    if matches!(left, QueryValue::Null) || matches!(right, QueryValue::Null) {
        return Ok(false);
    }
    Ok(match predicate.operator {
        PredicateOperator::Equal => values_equal(&left, &right),
        PredicateOperator::NotEqual => !values_equal(&left, &right),
        PredicateOperator::Less => compare_values(&left, &right) == Ordering::Less,
        PredicateOperator::LessEqual => compare_values(&left, &right) != Ordering::Greater,
        PredicateOperator::Greater => compare_values(&left, &right) == Ordering::Greater,
        PredicateOperator::GreaterEqual => compare_values(&left, &right) != Ordering::Less,
        PredicateOperator::Contains => {
            string_pair(&left, &right, |left, right| left.contains(right))
        }
        PredicateOperator::StartsWith => {
            string_pair(&left, &right, |left, right| left.starts_with(right))
        }
        PredicateOperator::EndsWith => {
            string_pair(&left, &right, |left, right| left.ends_with(right))
        }
        PredicateOperator::IsNull | PredicateOperator::IsNotNull => unreachable!(),
    })
}

fn evaluate_operand(
    operand: &Operand,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match operand {
        Operand::Literal(value) => Ok(value.as_ref().clone()),
        Operand::Reference(reference) => evaluate_reference(reference, binding, degrees),
    }
}

fn evaluate_reference(
    reference: &Reference,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match reference {
        Reference::Alias(alias) => {
            if let Some(node) = binding.nodes.get(alias) {
                return Ok(QueryValue::Node(node_summary(
                    node,
                    None,
                    degrees,
                    Vec::new(),
                )));
            }
            if let Some(edge) = binding.edges.get(alias) {
                return Ok(QueryValue::Edge(edge_summary(edge)));
            }
            Err(unsupported(&format!("unknown alias {alias}")))
        }
        Reference::Property { alias, path } => {
            if let Some(node) = binding.nodes.get(alias) {
                return Ok(node_property(node, path, degrees));
            }
            if let Some(edge) = binding.edges.get(alias) {
                return Ok(edge_property(edge, path));
            }
            Err(unsupported(&format!("unknown alias {alias}")))
        }
        Reference::EdgeType(alias) => binding
            .edges
            .get(alias)
            .map(|edge| QueryValue::String(edge.kind.as_str().to_owned()))
            .ok_or_else(|| unsupported(&format!("unknown relationship alias {alias}"))),
    }
}

fn node_property(
    node: &GraphNode,
    path: &[String],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> QueryValue {
    let Some(property) = path.first().map(String::as_str) else {
        return QueryValue::Null;
    };
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or((0, 0));
    let fixed = match property {
        "id" | "node_id" => Some(QueryValue::String(node.id.as_str().to_owned())),
        "project" | "project_id" => Some(QueryValue::String(node.project.as_str().to_owned())),
        "label" => Some(QueryValue::String(node.label.as_str().to_owned())),
        "name" => Some(QueryValue::String(node.name.clone())),
        "qualified_name" | "qn" => {
            Some(QueryValue::String(node.qualified_name.as_str().to_owned()))
        }
        "file" | "file_path" => Some(node.file_path.as_ref().map_or(QueryValue::Null, |path| {
            QueryValue::String(path.as_str().to_owned())
        })),
        "start_byte" => Some(optional_u64(node.source_span.map(|span| span.bytes.start))),
        "end_byte" => Some(optional_u64(node.source_span.map(|span| span.bytes.end))),
        "start_line" => Some(optional_u64(
            node.source_span.map(|span| span.start.row + 1),
        )),
        "end_line" => Some(optional_u64(node.source_span.map(|span| span.end.row + 1))),
        "generation" => Some(unsigned_value(node.generation.value())),
        "in_degree" => Some(unsigned_value(u64::try_from(in_degree).unwrap_or(u64::MAX))),
        "out_degree" => Some(unsigned_value(
            u64::try_from(out_degree).unwrap_or(u64::MAX),
        )),
        "degree" => Some(unsigned_value(
            u64::try_from(in_degree.saturating_add(out_degree)).unwrap_or(u64::MAX),
        )),
        _ => None,
    };
    if path.len() == 1 {
        return fixed.unwrap_or_else(|| {
            node.properties
                .get(property)
                .map_or(QueryValue::Null, json_value)
        });
    }
    if property == "properties" {
        return json_path(&node.properties, &path[1..]);
    }
    QueryValue::Null
}

fn edge_property(edge: &GraphEdge, path: &[String]) -> QueryValue {
    let Some(property) = path.first().map(String::as_str) else {
        return QueryValue::Null;
    };
    let fixed = match property {
        "source" | "source_id" => Some(QueryValue::String(edge.source.as_str().to_owned())),
        "target" | "target_id" => Some(QueryValue::String(edge.target.as_str().to_owned())),
        "kind" | "type" => Some(QueryValue::String(edge.kind.as_str().to_owned())),
        "discriminator" => Some(QueryValue::String(edge.discriminator.as_str().to_owned())),
        "generation" => Some(unsigned_value(edge.generation.value())),
        _ => None,
    };
    if path.len() == 1 {
        return fixed.unwrap_or_else(|| {
            edge.properties
                .get(property)
                .map_or(QueryValue::Null, json_value)
        });
    }
    if property == "properties" {
        return json_path(&edge.properties, &path[1..]);
    }
    QueryValue::Null
}

fn json_path(properties: &BTreeMap<String, serde_json::Value>, path: &[String]) -> QueryValue {
    let Some((first, rest)) = path.split_first() else {
        return QueryValue::Json(serde_json::to_value(properties).unwrap_or_default());
    };
    let Some(mut value) = properties.get(first) else {
        return QueryValue::Null;
    };
    for segment in rest {
        let Some(next) = value.as_object().and_then(|object| object.get(segment)) else {
            return QueryValue::Null;
        };
        value = next;
    }
    json_value(value)
}

fn json_value(value: &serde_json::Value) -> QueryValue {
    match value {
        serde_json::Value::Null => QueryValue::Null,
        serde_json::Value::Bool(value) => QueryValue::Bool(*value),
        serde_json::Value::Number(value) => value.as_i64().map_or_else(
            || {
                value.as_u64().map_or_else(
                    || value.as_f64().map_or(QueryValue::Null, QueryValue::Float),
                    QueryValue::Unsigned,
                )
            },
            QueryValue::Integer,
        ),
        serde_json::Value::String(value) => QueryValue::String(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            QueryValue::Json(value.clone())
        }
    }
}

fn optional_u64(value: Option<u64>) -> QueryValue {
    value.map_or(QueryValue::Null, unsigned_value)
}

fn unsigned_value(value: u64) -> QueryValue {
    i64::try_from(value).map_or(QueryValue::Unsigned(value), QueryValue::Integer)
}

fn edge_summary(edge: &GraphEdge) -> EdgeSummary {
    EdgeSummary {
        source_id: edge.source.as_str().to_owned(),
        target_id: edge.target.as_str().to_owned(),
        kind: edge.kind.as_str().to_owned(),
        discriminator: edge.discriminator.as_str().to_owned(),
        generation: edge.generation.value(),
        properties: edge.properties.clone(),
    }
}

fn values_equal(left: &QueryValue, right: &QueryValue) -> bool {
    compare_numeric(left, right).map_or_else(|| left == right, Ordering::is_eq)
}

fn compare_values(left: &QueryValue, right: &QueryValue) -> Ordering {
    if let Some(ordering) = compare_numeric(left, right) {
        return ordering;
    }
    match (left, right) {
        (QueryValue::String(left), QueryValue::String(right)) => left.cmp(right),
        (QueryValue::Bool(left), QueryValue::Bool(right)) => left.cmp(right),
        _ => row_key(std::slice::from_ref(left)).cmp(&row_key(std::slice::from_ref(right))),
    }
}

fn compare_numeric(left: &QueryValue, right: &QueryValue) -> Option<Ordering> {
    Some(match (left, right) {
        (QueryValue::Integer(left), QueryValue::Integer(right)) => left.cmp(right),
        (QueryValue::Unsigned(left), QueryValue::Unsigned(right)) => left.cmp(right),
        (QueryValue::Float(left), QueryValue::Float(right)) => left.total_cmp(right),
        (QueryValue::Integer(left), QueryValue::Unsigned(right)) => compare_i64_u64(*left, *right),
        (QueryValue::Unsigned(left), QueryValue::Integer(right)) => {
            compare_i64_u64(*right, *left).reverse()
        }
        (QueryValue::Integer(left), QueryValue::Float(right)) => compare_i64_f64(*left, *right),
        (QueryValue::Float(left), QueryValue::Integer(right)) => {
            compare_i64_f64(*right, *left).reverse()
        }
        (QueryValue::Unsigned(left), QueryValue::Float(right)) => compare_u64_f64(*left, *right),
        (QueryValue::Float(left), QueryValue::Unsigned(right)) => {
            compare_u64_f64(*right, *left).reverse()
        }
        _ => return None,
    })
}

fn compare_i64_u64(signed: i64, unsigned: u64) -> Ordering {
    u64::try_from(signed).map_or(Ordering::Less, |signed| signed.cmp(&unsigned))
}

fn compare_i64_f64(integer: i64, float: f64) -> Ordering {
    if float.is_nan() {
        return Ordering::Less;
    }
    if float.is_sign_negative() && float.to_bits() & i64::MAX.cast_unsigned() != 0 {
        if integer >= 0 {
            Ordering::Greater
        } else {
            compare_positive_u64_f64(integer.unsigned_abs(), -float).reverse()
        }
    } else if integer < 0 {
        Ordering::Less
    } else {
        compare_positive_u64_f64(integer.unsigned_abs(), float)
    }
}

fn compare_u64_f64(integer: u64, float: f64) -> Ordering {
    if float.is_nan() {
        return Ordering::Less;
    }
    if float.is_sign_negative() && float.to_bits() & i64::MAX.cast_unsigned() != 0 {
        Ordering::Greater
    } else {
        compare_positive_u64_f64(integer, float)
    }
}

fn compare_positive_u64_f64(integer: u64, float: f64) -> Ordering {
    let bits = float.to_bits();
    let exponent_bits = u16::try_from((bits >> 52) & 0x7ff).expect("f64 exponent fits u16");
    if exponent_bits == 0x7ff {
        return Ordering::Less;
    }
    if bits & i64::MAX.cast_unsigned() == 0 {
        return integer.cmp(&0);
    }
    if exponent_bits == 0 {
        return if integer == 0 {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    let exponent = i32::from(exponent_bits) - 1023;
    if exponent < 0 {
        return if integer == 0 {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    if exponent >= 64 {
        return Ordering::Less;
    }

    let mantissa = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    let (whole, has_fraction) = if exponent >= 52 {
        let shift = u32::try_from(exponent - 52).expect("nonnegative f64 shift");
        (mantissa << shift, false)
    } else {
        let shift = u32::try_from(52 - exponent).expect("positive f64 shift");
        let fraction_mask = (1_u64 << shift) - 1;
        (mantissa >> shift, mantissa & fraction_mask != 0)
    };
    integer.cmp(&whole).then_with(|| {
        if has_fraction {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    })
}

fn string_pair(
    left: &QueryValue,
    right: &QueryValue,
    predicate: impl Fn(&str, &str) -> bool,
) -> bool {
    match (left, right) {
        (QueryValue::String(left), QueryValue::String(right)) => predicate(left, right),
        _ => false,
    }
}

fn materialize_rows(
    query: &ParsedQuery,
    bindings: Vec<Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Vec<QueryValue>>, QueryError> {
    let count_projections = query
        .projections
        .iter()
        .filter(|projection| matches!(projection.expression, ProjectionExpression::Count(_)))
        .count();
    if count_projections > 0 {
        if query.projections.len() != 1 || count_projections != 1 {
            return Err(unsupported(
                "COUNT cannot be mixed with non-aggregate projections",
            ));
        }
        let ProjectionExpression::Count(target) = &query.projections[0].expression else {
            unreachable!();
        };
        if let Some(alias) = target {
            let alias_known = bindings.first().is_none_or(|binding| {
                binding.nodes.contains_key(alias) || binding.edges.contains_key(alias)
            });
            if !alias_known {
                return Err(unsupported(&format!("unknown count alias {alias}")));
            }
        }
        let count = i64::try_from(bindings.len())
            .map_err(|_| unsupported("aggregate count exceeds signed integer range"))?;
        return Ok(vec![vec![QueryValue::Integer(count)]]);
    }

    let mut rows = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let mut order_values = Vec::with_capacity(query.order.len());
        for clause in &query.order {
            order_values.push(evaluate_reference(&clause.reference, &binding, degrees)?);
        }
        let mut values = Vec::with_capacity(query.projections.len());
        for projection in &query.projections {
            let ProjectionExpression::Reference(reference) = &projection.expression else {
                unreachable!();
            };
            values.push(evaluate_reference(reference, &binding, degrees)?);
        }
        rows.push((order_values, values));
    }
    rows.sort_by(|(left_order, left_row), (right_order, right_row)| {
        for (index, clause) in query.order.iter().enumerate() {
            let mut ordering = compare_values(&left_order[index], &right_order[index]);
            if clause.descending {
                ordering = ordering.reverse();
            }
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        row_key(left_row).cmp(&row_key(right_row))
    });
    Ok(rows.into_iter().map(|(_, values)| values).collect())
}

fn row_key(row: &[QueryValue]) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| format!("{row:?}"))
}
