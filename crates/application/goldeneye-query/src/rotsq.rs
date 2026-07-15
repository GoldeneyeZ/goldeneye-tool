// The audited fixed-width quantizer intentionally narrows between its f32, f64, and packed
// integer representations. Error-bound tests below validate those conversions.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::sync::OnceLock;

use xxhash_rust::xxh3::xxh3_64_with_seed;

pub const ROTSQ_INPUT_DIM: usize = 768;
pub const ROTSQ_DIM: usize = 1_024;
pub const ROTSQ_BITS: usize = 4;
pub const ROTSQ_LEVELS: i32 = 15;
pub const ROTSQ_CODE_BYTES: usize = ROTSQ_DIM / 2;

const DIAGONAL_SEED: u64 = 0x5bd1_e995;
const INVERSE_SQRT_DIMENSION: f32 = 1.0 / 32.0;

#[derive(Clone, Debug, PartialEq)]
pub struct RotatedScalarCode {
    codes: [u8; ROTSQ_CODE_BYTES],
    scale: f32,
    offset: f32,
    code_sum: i32,
}

impl RotatedScalarCode {
    #[must_use]
    pub fn encode(input: &[f32; ROTSQ_INPUT_DIM]) -> Self {
        let diagonal = rotation_diagonal();
        let mut rotated = [0.0_f32; ROTSQ_DIM];
        for index in 0..ROTSQ_INPUT_DIM {
            rotated[index] = input[index] * diagonal[index];
        }
        fast_walsh_hadamard(&mut rotated);

        let mut low = rotated[0] * INVERSE_SQRT_DIMENSION;
        let mut high = low;
        for value in &mut rotated {
            *value *= INVERSE_SQRT_DIMENSION;
            low = low.min(*value);
            high = high.max(*value);
        }

        let range = high - low;
        let scale = if range > 0.0 {
            range / ROTSQ_LEVELS as f32
        } else {
            1.0
        };
        let mut codes = [0_u8; ROTSQ_CODE_BYTES];
        let mut code_sum = 0_i32;
        for (index, value) in rotated.into_iter().enumerate() {
            let quantized = (((value - low) / scale + 0.5) as i32).clamp(0, ROTSQ_LEVELS);
            code_sum += quantized;
            if index & 1 == 1 {
                codes[index >> 1] |= (quantized << 4) as u8;
            } else {
                codes[index >> 1] |= quantized as u8;
            }
        }
        Self {
            codes,
            scale,
            offset: low,
            code_sum,
        }
    }

    #[must_use]
    pub const fn codes(&self) -> &[u8; ROTSQ_CODE_BYTES] {
        &self.codes
    }

    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    #[must_use]
    pub const fn offset(&self) -> f32 {
        self.offset
    }

    #[must_use]
    pub const fn code_sum(&self) -> i32 {
        self.code_sum
    }

    #[must_use]
    pub fn inner_product(&self, other: &Self) -> f32 {
        let mut code_dot = 0_i64;
        for (&left, right) in self.codes.iter().zip(other.codes) {
            code_dot += i64::from(left & 0x0f) * i64::from(right & 0x0f);
            code_dot += i64::from(left >> 4) * i64::from(right >> 4);
        }
        let dimension = ROTSQ_DIM as f64;
        let estimate = dimension * f64::from(self.offset) * f64::from(other.offset)
            + f64::from(self.offset) * f64::from(other.scale) * f64::from(other.code_sum)
            + f64::from(other.offset) * f64::from(self.scale) * f64::from(self.code_sum)
            + f64::from(self.scale) * f64::from(other.scale) * code_dot as f64;
        estimate as f32
    }

    #[must_use]
    pub fn decode_rotated(&self) -> [f32; ROTSQ_DIM] {
        let mut decoded = [0.0; ROTSQ_DIM];
        for (index, byte) in self.codes.iter().copied().enumerate() {
            decoded[index * 2] = self.offset + self.scale * f32::from(byte & 0x0f);
            decoded[index * 2 + 1] = self.offset + self.scale * f32::from(byte >> 4);
        }
        decoded
    }
}

fn rotation_diagonal() -> &'static [f32; ROTSQ_DIM] {
    static DIAGONAL: OnceLock<[f32; ROTSQ_DIM]> = OnceLock::new();
    DIAGONAL.get_or_init(|| {
        std::array::from_fn(|index| {
            let index = i32::try_from(index).expect("rotation dimension fits i32");
            if xxh3_64_with_seed(&index.to_le_bytes(), DIAGONAL_SEED) & 1 == 1 {
                1.0
            } else {
                -1.0
            }
        })
    })
}

fn fast_walsh_hadamard(values: &mut [f32; ROTSQ_DIM]) {
    let mut length = 1;
    while length < ROTSQ_DIM {
        for block in (0..ROTSQ_DIM).step_by(length * 2) {
            for index in block..block + length {
                let left = values[index];
                let right = values[index + length];
                values[index] = left + right;
                values[index + length] = left - right;
            }
        }
        length <<= 1;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn zero_vector_has_deterministic_zero_estimate() {
        let code = RotatedScalarCode::encode(&[0.0; ROTSQ_INPUT_DIM]);
        assert_eq!(code.codes, [0; ROTSQ_CODE_BYTES]);
        assert_eq!(code.code_sum, 0);
        assert_eq!(code.inner_product(&code), 0.0);
        assert_eq!(code, RotatedScalarCode::encode(&[0.0; ROTSQ_INPUT_DIM]));
    }

    #[test]
    fn quantized_inner_products_stay_inside_audited_error_bounds() {
        const VECTOR_COUNT: usize = 64;
        let mut state = 0x00c0_ffee_u32;
        let mut vectors = Vec::with_capacity(VECTOR_COUNT);
        let mut codes = Vec::with_capacity(VECTOR_COUNT);
        for _ in 0..VECTOR_COUNT {
            let mut vector = [0.0_f32; ROTSQ_INPUT_DIM];
            let mut norm = 0.0_f64;
            for value in &mut vector {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *value = (state & 0x00ff_ffff) as f32 / 0x007f_ffff_u32 as f32 - 1.0;
                norm += f64::from(*value) * f64::from(*value);
            }
            let inverse = if norm > 0.0 {
                (1.0 / norm.sqrt()) as f32
            } else {
                0.0
            };
            for value in &mut vector {
                *value *= inverse;
            }
            codes.push(RotatedScalarCode::encode(&vector));
            vectors.push(vector);
        }

        let mut maximum_error = 0.0_f64;
        let mut error_sum = 0.0_f64;
        let mut pair_count = 0_usize;
        for left in 0..VECTOR_COUNT {
            for right in left..VECTOR_COUNT {
                let exact = vectors[left]
                    .iter()
                    .zip(vectors[right])
                    .map(|(left, right)| f64::from(*left) * f64::from(right))
                    .sum::<f64>();
                let estimated = f64::from(codes[left].inner_product(&codes[right]));
                let error = (estimated - exact).abs();
                maximum_error = maximum_error.max(error);
                error_sum += error;
                pair_count += 1;
            }
        }

        let mean_error = error_sum / pair_count as f64;
        assert!((f64::from(codes[0].inner_product(&codes[0])) - 1.0).abs() < 0.05);
        assert!(mean_error < 0.01, "mean error was {mean_error}");
        assert!(maximum_error < 0.04, "maximum error was {maximum_error}");
    }
}
