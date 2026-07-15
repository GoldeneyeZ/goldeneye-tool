use crate::types::QueryError;

use super::super::{
    MAX_VARIABLE_HOPS,
    ast::{EdgeDirection, EdgeMatch, EdgePattern, MatchPattern, NodePattern, Operand},
    lexer::{Symbol, TokenKind},
    unsupported,
};
use super::Parser;

impl Parser {
    pub(super) fn parse_pattern_chain(&mut self) -> Result<Vec<MatchPattern>, QueryError> {
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
        let labels = self.parse_node_labels()?;
        let properties = self.parse_node_properties()?;
        self.expect_symbol(Symbol::RightParen)?;
        Ok(NodePattern {
            alias,
            labels,
            properties,
        })
    }

    fn parse_node_labels(&mut self) -> Result<Vec<String>, QueryError> {
        let mut labels = Vec::new();
        if self.consume_symbol(Symbol::Colon) {
            labels.push(self.parse_identifier("node label")?);
            while self.consume_symbol(Symbol::Pipe) {
                self.consume_symbol(Symbol::Colon);
                labels.push(self.parse_identifier("node label")?);
            }
        }
        Ok(labels)
    }

    fn parse_node_properties(
        &mut self,
    ) -> Result<Vec<(String, crate::types::QueryValue)>, QueryError> {
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
        Ok(properties)
    }

    fn parse_edge_pattern(&mut self) -> Result<EdgePattern, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let (alias, kinds) = self.parse_edge_identity()?;
        let (min_hops, max_hops) = self.parse_hop_range()?;
        self.expect_symbol(Symbol::RightBracket)?;
        Ok(EdgePattern {
            alias,
            kinds,
            direction: EdgeDirection::Outbound,
            min_hops,
            max_hops,
        })
    }

    fn parse_edge_identity(&mut self) -> Result<(Option<String>, Vec<String>), QueryError> {
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
        Ok((alias, kinds))
    }

    fn parse_hop_range(&mut self) -> Result<(usize, usize), QueryError> {
        if !self.consume_symbol(Symbol::Star) {
            return Ok((1, 1));
        }
        let first = if matches!(self.peek(), Some(TokenKind::Number(_))) {
            Some(self.parse_usize("relationship hop count")?)
        } else {
            None
        };
        let (mut min_hops, mut max_hops) = if self.consume_symbol(Symbol::Dot) {
            self.expect_symbol(Symbol::Dot)?;
            let maximum = if matches!(self.peek(), Some(TokenKind::Number(_))) {
                self.parse_usize("relationship maximum hop count")?
            } else {
                MAX_VARIABLE_HOPS
            };
            (first.unwrap_or(1), maximum)
        } else if let Some(hops) = first {
            (hops, hops)
        } else {
            (1, MAX_VARIABLE_HOPS)
        };
        self.clamp_hop_range(&mut min_hops, &mut max_hops)?;
        Ok((min_hops, max_hops))
    }

    fn clamp_hop_range(
        &mut self,
        min_hops: &mut usize,
        max_hops: &mut usize,
    ) -> Result<(), QueryError> {
        if *min_hops > MAX_VARIABLE_HOPS || *max_hops > MAX_VARIABLE_HOPS {
            self.warnings.push(format!(
                "variable-length relationship bound {min_hops}..{max_hops} was clamped to {MAX_VARIABLE_HOPS} hops"
            ));
        }
        *min_hops = (*min_hops).min(MAX_VARIABLE_HOPS);
        *max_hops = (*max_hops).min(MAX_VARIABLE_HOPS);
        if *max_hops < *min_hops {
            return Err(self.error("relationship maximum hop count is below minimum"));
        }
        Ok(())
    }
}
