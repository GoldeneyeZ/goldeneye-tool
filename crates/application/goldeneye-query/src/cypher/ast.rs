use crate::types::QueryValue;

#[derive(Debug)]
pub(super) struct ParsedQuery {
    pub(super) unwind: Option<UnwindClause>,
    pub(super) matches: Vec<MatchClause>,
    pub(super) with_clause: Option<WithClause>,
    pub(super) distinct: bool,
    pub(super) star: bool,
    pub(super) projections: Vec<Projection>,
    pub(super) order: Vec<OrderClause>,
    pub(super) skip: usize,
    pub(super) limit: Option<usize>,
    pub(super) warnings: Vec<String>,
}

#[derive(Debug)]
pub(super) struct UnwindClause {
    pub(super) expression: Operand,
    pub(super) alias: String,
}

#[derive(Debug)]
pub(super) struct MatchClause {
    pub(super) patterns: Vec<MatchPattern>,
    pub(super) filter: Option<Expression>,
    pub(super) optional: bool,
}

#[derive(Debug)]
pub(super) struct WithClause {
    pub(super) distinct: bool,
    pub(super) projections: Vec<Projection>,
    pub(super) filter: Option<Expression>,
    pub(super) order: Vec<OrderClause>,
    pub(super) skip: usize,
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub(super) enum MatchPattern {
    Node(NodePattern),
    Edge(Box<EdgeMatch>),
}

#[derive(Debug, Clone)]
pub(super) struct EdgeMatch {
    pub(super) left: NodePattern,
    pub(super) edge: EdgePattern,
    pub(super) right: NodePattern,
}

#[derive(Debug, Clone)]
pub(super) struct NodePattern {
    pub(super) alias: String,
    pub(super) labels: Vec<String>,
    pub(super) properties: Vec<(String, QueryValue)>,
}

#[derive(Debug, Clone)]
pub(super) struct EdgePattern {
    pub(super) alias: Option<String>,
    pub(super) kinds: Vec<String>,
    pub(super) direction: EdgeDirection,
    pub(super) min_hops: usize,
    pub(super) max_hops: usize,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum EdgeDirection {
    Outbound,
    Inbound,
    Undirected,
}

#[derive(Debug)]
pub(super) enum Expression {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Xor(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Exists(Vec<MatchPattern>),
    Predicate(Box<Predicate>),
}

#[derive(Debug)]
pub(super) struct Predicate {
    pub(super) left: Operand,
    pub(super) operator: PredicateOperator,
    pub(super) right: Option<Operand>,
}

#[derive(Debug)]
pub(super) enum PredicateOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Regex,
    In,
    NotIn,
    Contains,
    StartsWith,
    EndsWith,
    HasLabel(Vec<String>),
    IsNull,
    IsNotNull,
}

#[derive(Debug)]
pub(super) enum Operand {
    Literal(Box<QueryValue>),
    List(Vec<Self>),
    Reference(Reference),
    Function { name: String, arguments: Vec<Self> },
}

#[derive(Debug, Clone)]
pub(super) enum Reference {
    Alias(String),
    Property { alias: String, path: Vec<String> },
    EdgeType(String),
}

#[derive(Debug)]
pub(super) struct Projection {
    pub(super) expression: ProjectionExpression,
    pub(super) column: String,
}

#[derive(Debug)]
pub(super) enum ProjectionExpression {
    Reference(Reference),
    Function {
        name: String,
        arguments: Vec<Operand>,
    },
    Case(CaseExpression),
    Aggregate {
        kind: AggregateKind,
        target: Option<Reference>,
        distinct: bool,
    },
}

#[derive(Debug)]
pub(super) struct CaseExpression {
    pub(super) subject: Option<Operand>,
    pub(super) branches: Vec<CaseBranch>,
    pub(super) fallback: Option<Operand>,
}

#[derive(Debug)]
pub(super) struct CaseBranch {
    pub(super) when: CaseWhen,
    pub(super) then: Operand,
}

#[derive(Debug)]
pub(super) enum CaseWhen {
    Predicate(Expression),
    Value(Operand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AggregateKind {
    Count,
    Sum,
    Average,
    Minimum,
    Maximum,
    Collect,
}

#[derive(Debug)]
pub(super) struct OrderClause {
    pub(super) reference: Reference,
    pub(super) descending: bool,
}
