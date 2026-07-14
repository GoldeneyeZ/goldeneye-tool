use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const AST_PROFILE_DIMS: usize = 25;
pub const AST_PROFILE_MAX_ENCODED_LEN: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstProfile {
    pub if_count: u16,
    pub for_count: u16,
    pub while_count: u16,
    pub switch_count: u16,
    pub try_count: u16,
    pub return_count: u16,
    pub max_nesting_depth: u16,
    pub avg_nesting_depth_x10: u16,
    pub comparison_ops: u16,
    pub arithmetic_ops: u16,
    pub logical_ops: u16,
    pub assignment_count: u16,
    pub string_literals: u16,
    pub number_literals: u16,
    pub bool_literals: u16,
    pub param_count: u16,
    pub params_in_returns: u16,
    pub params_in_conditions: u16,
    pub variable_reassigns: u16,
    pub unique_operators: u16,
    pub unique_operands: u16,
    pub total_operators: u16,
    pub total_operands: u16,
    pub body_lines: u16,
    pub body_tokens: u16,
}

impl AstProfile {
    #[must_use]
    pub const fn as_array(self) -> [u16; AST_PROFILE_DIMS] {
        [
            self.if_count,
            self.for_count,
            self.while_count,
            self.switch_count,
            self.try_count,
            self.return_count,
            self.max_nesting_depth,
            self.avg_nesting_depth_x10,
            self.comparison_ops,
            self.arithmetic_ops,
            self.logical_ops,
            self.assignment_count,
            self.string_literals,
            self.number_literals,
            self.bool_literals,
            self.param_count,
            self.params_in_returns,
            self.params_in_conditions,
            self.variable_reassigns,
            self.unique_operators,
            self.unique_operands,
            self.total_operators,
            self.total_operands,
            self.body_lines,
            self.body_tokens,
        ]
    }

    #[must_use]
    pub fn to_vector(self) -> [f32; AST_PROFILE_DIMS] {
        let fields = self.as_array();
        let denominators = [
            100.0, 100.0, 100.0, 100.0, 100.0, 100.0, 20.0, 200.0, 100.0, 100.0,
            100.0, 100.0, 100.0, 100.0, 100.0, 20.0, 100.0, 100.0, 100.0, 200.0,
            200.0, 200.0, 200.0, 2_000.0, 2_000.0,
        ];
        std::array::from_fn(|index| f32::from(fields[index]) / denominators[index])
    }
}

impl fmt::Display for AstProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let fields = self.as_array();
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                formatter.write_str(",")?;
            }
            write!(formatter, "{field}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AstProfileParseError {
    #[error("expected exactly {AST_PROFILE_DIMS} comma-separated fields, found {0}")]
    FieldCount(usize),
    #[error("field {index} is not an unsigned 16-bit integer: {value}")]
    InvalidField { index: usize, value: String },
}

impl FromStr for AstProfile {
    type Err = AstProfileParseError;

    fn from_str(encoded: &str) -> Result<Self, Self::Err> {
        let parts = encoded.split(',').collect::<Vec<_>>();
        if parts.len() != AST_PROFILE_DIMS {
            return Err(AstProfileParseError::FieldCount(parts.len()));
        }
        let mut fields = [0_u16; AST_PROFILE_DIMS];
        for (index, part) in parts.into_iter().enumerate() {
            fields[index] = part.parse().map_err(|_| AstProfileParseError::InvalidField {
                index,
                value: part.to_owned(),
            })?;
        }
        Ok(Self {
            if_count: fields[0],
            for_count: fields[1],
            while_count: fields[2],
            switch_count: fields[3],
            try_count: fields[4],
            return_count: fields[5],
            max_nesting_depth: fields[6],
            avg_nesting_depth_x10: fields[7],
            comparison_ops: fields[8],
            arithmetic_ops: fields[9],
            logical_ops: fields[10],
            assignment_count: fields[11],
            string_literals: fields[12],
            number_literals: fields[13],
            bool_literals: fields[14],
            param_count: fields[15],
            params_in_returns: fields[16],
            params_in_conditions: fields[17],
            variable_reassigns: fields[18],
            unique_operators: fields[19],
            unique_operands: fields[20],
            total_operators: fields[21],
            total_operands: fields[22],
            body_lines: fields[23],
            body_tokens: fields[24],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_csv_round_trips_all_25_fields() {
        let profile = AstProfile {
            if_count: 1,
            for_count: 2,
            while_count: 3,
            switch_count: 4,
            try_count: 5,
            return_count: 6,
            max_nesting_depth: 7,
            avg_nesting_depth_x10: 8,
            comparison_ops: 9,
            arithmetic_ops: 10,
            logical_ops: 11,
            assignment_count: 12,
            string_literals: 13,
            number_literals: 14,
            bool_literals: 15,
            param_count: 16,
            params_in_returns: 17,
            params_in_conditions: 18,
            variable_reassigns: 19,
            unique_operators: 20,
            unique_operands: 21,
            total_operators: 22,
            total_operands: 23,
            body_lines: 24,
            body_tokens: 25,
        };
        let encoded = profile.to_string();

        assert!(encoded.len() < AST_PROFILE_MAX_ENCODED_LEN);
        assert_eq!(encoded.parse::<AstProfile>(), Ok(profile));
    }

    #[test]
    fn profile_vector_uses_upstream_scales_without_clamping() {
        let profile = AstProfile {
            if_count: 500,
            max_nesting_depth: 20,
            avg_nesting_depth_x10: 200,
            body_tokens: 5_000,
            ..AstProfile::default()
        };
        let vector = profile.to_vector();

        assert_eq!(vector[0], 5.0);
        assert_eq!(vector[6], 1.0);
        assert_eq!(vector[7], 1.0);
        assert_eq!(vector[24], 2.5);
    }

    #[test]
    fn profile_parser_rejects_malformed_or_incomplete_input() {
        assert!("not,a,profile".parse::<AstProfile>().is_err());
        assert!("0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0".parse::<AstProfile>().is_err());
    }
}
