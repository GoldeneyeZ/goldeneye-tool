use std::cmp::Ordering;

use super::super::row_key;
use crate::types::QueryValue;

pub(in crate::cypher) fn values_equal(left: &QueryValue, right: &QueryValue) -> bool {
    compare_numeric(left, right).map_or_else(|| left == right, Ordering::is_eq)
}

pub(in crate::cypher) fn compare_values(left: &QueryValue, right: &QueryValue) -> Ordering {
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
    if let QueryValue::String(value) = left
        && is_numeric_value(right)
    {
        return parse_numeric_value(value).and_then(|value| compare_numeric(&value, right));
    }
    if let QueryValue::String(value) = right
        && is_numeric_value(left)
    {
        return parse_numeric_value(value).and_then(|value| compare_numeric(left, &value));
    }
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

const fn is_numeric_value(value: &QueryValue) -> bool {
    matches!(
        value,
        QueryValue::Integer(_) | QueryValue::Unsigned(_) | QueryValue::Float(_)
    )
}

fn parse_numeric_value(value: &str) -> Option<QueryValue> {
    if value.contains('.') || value.contains('e') || value.contains('E') {
        return value.parse().ok().map(QueryValue::Float);
    }
    value
        .parse::<i64>()
        .map(QueryValue::Integer)
        .ok()
        .or_else(|| value.parse::<u64>().map(QueryValue::Unsigned).ok())
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

pub(super) fn string_pair(
    left: &QueryValue,
    right: &QueryValue,
    predicate: impl Fn(&str, &str) -> bool,
) -> bool {
    match (left, right) {
        (QueryValue::String(left), QueryValue::String(right)) => predicate(left, right),
        _ => false,
    }
}
