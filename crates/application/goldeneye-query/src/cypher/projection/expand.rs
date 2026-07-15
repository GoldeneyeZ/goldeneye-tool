use std::collections::BTreeSet;

use super::super::{
    ast::{MatchClause, Projection, ProjectionExpression, Reference, WithClause},
    pattern_node_aliases, reference_column,
};

pub(in crate::cypher) fn expand_star_projections(matches: &[MatchClause]) -> Vec<Projection> {
    let mut seen = BTreeSet::new();
    let aliases: Vec<&str> = matches
        .iter()
        .flat_map(|clause| clause.patterns.iter().flat_map(pattern_node_aliases))
        .filter(|alias| !alias.starts_with("__anon_node_"))
        .filter(|alias| seen.insert((*alias).to_owned()))
        .collect();
    aliases
        .into_iter()
        .flat_map(|alias| {
            ["name", "qualified_name", "label", "file_path"].map(|property| {
                let reference = Reference::Property {
                    alias: alias.to_owned(),
                    path: vec![property.to_owned()],
                };
                Projection {
                    column: reference_column(&reference),
                    expression: ProjectionExpression::Reference(reference),
                }
            })
        })
        .collect()
}

pub(in crate::cypher) fn expand_with_star_projections(clause: &WithClause) -> Vec<Projection> {
    clause
        .projections
        .iter()
        .map(|projection| Projection {
            expression: ProjectionExpression::Reference(Reference::Alias(
                projection.column.clone(),
            )),
            column: projection.column.clone(),
        })
        .collect()
}
