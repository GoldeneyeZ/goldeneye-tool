use std::collections::BTreeMap;

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use super::super::{ast::Reference, unsupported};
use super::binding::Binding;
use crate::{
    engine::node_summary,
    types::{EdgeSummary, QueryError, QueryValue},
};

pub(in crate::cypher) fn evaluate_reference(
    reference: &Reference,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match reference {
        Reference::Alias(alias) => {
            if let Some(value) = binding.values.get(alias) {
                return Ok(value.clone());
            }
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
            if let Some(value) = binding.values.get(alias) {
                return Ok(value_property(value, path));
            }
            if let Some(node) = binding.nodes.get(alias) {
                return Ok(node_property(node, path, degrees));
            }
            if let Some(edge) = binding.edges.get(alias) {
                return Ok(edge_property(edge, path));
            }
            Err(unsupported(&format!("unknown alias {alias}")))
        }
        Reference::EdgeType(alias) => {
            if let Some(QueryValue::Edge(edge)) = binding.values.get(alias) {
                return Ok(QueryValue::String(edge.kind.clone()));
            }
            binding
                .edges
                .get(alias)
                .map(|edge| QueryValue::String(edge.kind.as_str().to_owned()))
                .ok_or_else(|| unsupported(&format!("unknown relationship alias {alias}")))
        }
    }
}

fn value_property(value: &QueryValue, path: &[String]) -> QueryValue {
    let Some((first, rest)) = path.split_first() else {
        return value.clone();
    };
    let json = match value {
        QueryValue::Json(value) => value,
        QueryValue::Node(node) => {
            return match first.as_str() {
                "id" | "node_id" if rest.is_empty() => QueryValue::String(node.id.clone()),
                "name" if rest.is_empty() => QueryValue::String(node.name.clone()),
                "qualified_name" | "qn" if rest.is_empty() => {
                    QueryValue::String(node.qualified_name.clone())
                }
                "label" if rest.is_empty() => QueryValue::String(node.label.clone()),
                "file" | "file_path" if rest.is_empty() => node
                    .file_path
                    .clone()
                    .map_or(QueryValue::Null, QueryValue::String),
                property if rest.is_empty() => node
                    .properties
                    .get(property)
                    .map_or(QueryValue::Null, json_value),
                _ => QueryValue::Null,
            };
        }
        QueryValue::Edge(edge) => {
            return match first.as_str() {
                "source" | "source_id" if rest.is_empty() => {
                    QueryValue::String(edge.source_id.clone())
                }
                "target" | "target_id" if rest.is_empty() => {
                    QueryValue::String(edge.target_id.clone())
                }
                "kind" | "type" if rest.is_empty() => QueryValue::String(edge.kind.clone()),
                "discriminator" if rest.is_empty() => {
                    QueryValue::String(edge.discriminator.clone())
                }
                property if rest.is_empty() => edge
                    .properties
                    .get(property)
                    .map_or(QueryValue::Null, json_value),
                _ => QueryValue::Null,
            };
        }
        _ => return QueryValue::Null,
    };
    let mut current = json;
    for segment in std::iter::once(first).chain(rest) {
        let Some(next) = current.as_object().and_then(|object| object.get(segment)) else {
            return QueryValue::Null;
        };
        current = next;
    }
    json_value(current)
}

pub(super) fn node_property(
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

pub(super) fn json_value(value: &serde_json::Value) -> QueryValue {
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

pub(in crate::cypher) fn query_value_to_json(
    value: QueryValue,
) -> Result<serde_json::Value, QueryError> {
    Ok(match value {
        QueryValue::Null => serde_json::Value::Null,
        QueryValue::Bool(value) => serde_json::Value::Bool(value),
        QueryValue::Integer(value) => serde_json::Value::Number(value.into()),
        QueryValue::Unsigned(value) => serde_json::Value::Number(value.into()),
        QueryValue::Float(value) => serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        QueryValue::String(value) => serde_json::Value::String(value),
        QueryValue::Json(value) => value,
        QueryValue::Node(value) => serde_json::to_value(value)
            .map_err(|error| unsupported(&format!("cannot serialize node value: {error}")))?,
        QueryValue::Edge(value) => serde_json::to_value(value)
            .map_err(|error| unsupported(&format!("cannot serialize edge value: {error}")))?,
    })
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
