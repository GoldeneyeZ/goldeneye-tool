use std::collections::BTreeMap;

use goldeneye_domain::NodeId;

use super::super::{ast::Operand, unsupported};
use super::{
    binding::Binding,
    reference::{evaluate_reference, query_value_to_json},
};
use crate::types::{QueryError, QueryValue};

pub(in crate::cypher) fn evaluate_operand(
    operand: &Operand,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match operand {
        Operand::Literal(value) => Ok(value.as_ref().clone()),
        Operand::List(values) => Ok(QueryValue::Json(serde_json::Value::Array(
            values
                .iter()
                .map(|value| {
                    evaluate_operand(value, binding, degrees).and_then(query_value_to_json)
                })
                .collect::<Result<_, _>>()?,
        ))),
        Operand::Reference(reference) => evaluate_reference(reference, binding, degrees),
        Operand::Function { name, arguments } => {
            evaluate_scalar_function(name, arguments, binding, degrees)
        }
    }
}

pub(in crate::cypher) fn evaluate_scalar_function(
    name: &str,
    arguments: &[Operand],
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    let values = arguments
        .iter()
        .map(|argument| evaluate_operand(argument, binding, degrees))
        .collect::<Result<Vec<_>, _>>()?;
    let normalized = name.to_ascii_lowercase();
    let value = match normalized.as_str() {
        "coalesce" => values
            .into_iter()
            .find(|value| !matches!(value, QueryValue::Null))
            .unwrap_or(QueryValue::Null),
        "tolower" => unary_string(&values, str::to_lowercase),
        "toupper" => unary_string(&values, str::to_uppercase),
        "tostring" => values
            .first()
            .and_then(query_value_string)
            .map_or(QueryValue::Null, QueryValue::String),
        "tointeger" => values.first().map_or(QueryValue::Null, query_value_integer),
        "tofloat" => values.first().map_or(QueryValue::Null, query_value_float),
        "toboolean" => values.first().map_or(QueryValue::Null, query_value_boolean),
        "size" | "length" => values.first().map_or(QueryValue::Null, value_size),
        "reverse" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::String(value) => QueryValue::String(value.chars().rev().collect()),
                QueryValue::Json(serde_json::Value::Array(values)) => {
                    let mut values = values.clone();
                    values.reverse();
                    QueryValue::Json(serde_json::Value::Array(values))
                }
                _ => QueryValue::Null,
            }),
        "trim" => unary_string(&values, |value| value.trim().to_owned()),
        "ltrim" => unary_string(&values, |value| value.trim_start().to_owned()),
        "rtrim" => unary_string(&values, |value| value.trim_end().to_owned()),
        "substring" => substring_value(&values),
        "left" => edge_slice_value(&values, true),
        "right" => edge_slice_value(&values, false),
        "replace" => match (values.first(), values.get(1), values.get(2)) {
            (
                Some(QueryValue::String(value)),
                Some(QueryValue::String(from)),
                Some(QueryValue::String(to)),
            ) => QueryValue::String(value.replace(from, to)),
            _ => QueryValue::Null,
        },
        "split" => match (values.first(), values.get(1)) {
            (Some(QueryValue::String(value)), Some(QueryValue::String(separator))) => {
                let parts: Vec<String> = if separator.is_empty() {
                    value
                        .chars()
                        .map(|character| character.to_string())
                        .collect()
                } else {
                    value.split(separator).map(str::to_owned).collect()
                };
                QueryValue::Json(serde_json::Value::Array(
                    parts.into_iter().map(serde_json::Value::String).collect(),
                ))
            }
            _ => QueryValue::Null,
        },
        "labels" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => {
                    QueryValue::Json(serde_json::Value::Array(vec![serde_json::Value::String(
                        node.label.clone(),
                    )]))
                }
                _ => QueryValue::Json(serde_json::Value::Array(Vec::new())),
            }),
        "type" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Edge(edge) => QueryValue::String(edge.kind.clone()),
                _ => QueryValue::Null,
            }),
        "id" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => QueryValue::String(node.id.clone()),
                QueryValue::Edge(edge) => QueryValue::String(edge.discriminator.clone()),
                _ => QueryValue::Null,
            }),
        "properties" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => QueryValue::Json(
                    serde_json::to_value(&node.properties).unwrap_or(serde_json::Value::Null),
                ),
                QueryValue::Edge(edge) => QueryValue::Json(
                    serde_json::to_value(&edge.properties).unwrap_or(serde_json::Value::Null),
                ),
                _ => QueryValue::Json(serde_json::Value::Object(serde_json::Map::new())),
            }),
        "keys" => values.first().map_or(QueryValue::Null, entity_keys),
        _ => return Err(unsupported(&format!("unsupported function {name}"))),
    };
    Ok(value)
}

fn unary_string(values: &[QueryValue], transform: impl FnOnce(&str) -> String) -> QueryValue {
    match values.first() {
        Some(QueryValue::String(value)) => QueryValue::String(transform(value)),
        _ => QueryValue::Null,
    }
}

fn query_value_string(value: &QueryValue) -> Option<String> {
    Some(match value {
        QueryValue::Null => return None,
        QueryValue::Bool(value) => value.to_string(),
        QueryValue::Integer(value) => value.to_string(),
        QueryValue::Unsigned(value) => value.to_string(),
        QueryValue::Float(value) => value.to_string(),
        QueryValue::String(value) => value.clone(),
        QueryValue::Json(value) => value.to_string(),
        QueryValue::Node(value) => serde_json::to_string(value).ok()?,
        QueryValue::Edge(value) => serde_json::to_string(value).ok()?,
    })
}

fn query_value_integer(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Integer(value) => QueryValue::Integer(*value),
        QueryValue::Unsigned(value) => {
            i64::try_from(*value).map_or(QueryValue::Null, QueryValue::Integer)
        }
        QueryValue::Float(value)
            if value.is_finite() && *value >= i64::MIN as f64 && *value <= i64::MAX as f64 =>
        {
            QueryValue::Integer(value.trunc() as i64)
        }
        QueryValue::String(value) => value
            .parse::<i64>()
            .map_or(QueryValue::Null, QueryValue::Integer),
        QueryValue::Bool(value) => QueryValue::Integer(i64::from(*value)),
        _ => QueryValue::Null,
    }
}

fn query_value_float(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Integer(value) => QueryValue::Float(*value as f64),
        QueryValue::Unsigned(value) => QueryValue::Float(*value as f64),
        QueryValue::Float(value) => QueryValue::Float(*value),
        QueryValue::String(value) => value
            .parse::<f64>()
            .map_or(QueryValue::Null, QueryValue::Float),
        QueryValue::Bool(value) => QueryValue::Float(if *value { 1.0 } else { 0.0 }),
        _ => QueryValue::Null,
    }
}

fn query_value_boolean(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Bool(value) => QueryValue::Bool(*value),
        QueryValue::Integer(value) => QueryValue::Bool(*value != 0),
        QueryValue::Unsigned(value) => QueryValue::Bool(*value != 0),
        QueryValue::Float(value) => QueryValue::Bool(*value != 0.0),
        QueryValue::String(value) if value.eq_ignore_ascii_case("true") => QueryValue::Bool(true),
        QueryValue::String(value) if value.eq_ignore_ascii_case("false") => QueryValue::Bool(false),
        _ => QueryValue::Null,
    }
}

fn value_size(value: &QueryValue) -> QueryValue {
    let size = match value {
        QueryValue::String(value) => value.chars().count(),
        QueryValue::Json(serde_json::Value::Array(values)) => values.len(),
        QueryValue::Json(serde_json::Value::Object(values)) => values.len(),
        _ => return QueryValue::Null,
    };
    i64::try_from(size).map_or(QueryValue::Null, QueryValue::Integer)
}

fn substring_value(values: &[QueryValue]) -> QueryValue {
    let (Some(QueryValue::String(value)), Some(start)) =
        (values.first(), values.get(1).and_then(value_index))
    else {
        return QueryValue::Null;
    };
    let characters: Vec<char> = value.chars().collect();
    if start >= characters.len() {
        return QueryValue::String(String::new());
    }
    let length = values
        .get(2)
        .and_then(value_index)
        .unwrap_or(characters.len() - start);
    QueryValue::String(
        characters[start..characters.len().min(start.saturating_add(length))]
            .iter()
            .collect(),
    )
}

fn edge_slice_value(values: &[QueryValue], from_left: bool) -> QueryValue {
    let (Some(QueryValue::String(value)), Some(length)) =
        (values.first(), values.get(1).and_then(value_index))
    else {
        return QueryValue::Null;
    };
    let characters: Vec<char> = value.chars().collect();
    let take = length.min(characters.len());
    let start = if from_left {
        0
    } else {
        characters.len() - take
    };
    QueryValue::String(characters[start..start + take].iter().collect())
}

fn value_index(value: &QueryValue) -> Option<usize> {
    match value {
        QueryValue::Integer(value) => usize::try_from(*value).ok(),
        QueryValue::Unsigned(value) => usize::try_from(*value).ok(),
        QueryValue::Float(value) if value.is_finite() && *value >= 0.0 => {
            usize::try_from(value.trunc() as u64).ok()
        }
        QueryValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn entity_keys(value: &QueryValue) -> QueryValue {
    let keys = match value {
        QueryValue::Node(node) => {
            let mut keys = vec!["name", "qualified_name", "label"];
            if node.file_path.is_some() {
                keys.push("file_path");
            }
            if node.start_line.is_some() {
                keys.push("start_line");
            }
            if node.end_line.is_some() {
                keys.push("end_line");
            }
            let mut keys: Vec<String> = keys.into_iter().map(str::to_owned).collect();
            keys.extend(node.properties.keys().cloned());
            keys
        }
        QueryValue::Edge(edge) => {
            let mut keys = vec![
                "source_id".to_owned(),
                "target_id".to_owned(),
                "kind".to_owned(),
                "discriminator".to_owned(),
            ];
            keys.extend(edge.properties.keys().cloned());
            keys
        }
        _ => Vec::new(),
    };
    QueryValue::Json(serde_json::Value::Array(
        keys.into_iter().map(serde_json::Value::String).collect(),
    ))
}
