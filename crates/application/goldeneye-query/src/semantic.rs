// Semantic hashing and quantization use deliberately bounded numeric narrowing; vector,
// checksum, and determinism tests cover the representation boundaries.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use xxhash_rust::xxh3::{xxh3_64, xxh3_64_with_seed};

mod pretrained;
mod tokenization;

pub use pretrained::{PretrainedModel, PretrainedModelError};
pub use tokenization::tokenize_identifier;

pub const SEMANTIC_DIM: usize = 768;
pub const SEMANTIC_SPARSE_NON_ZERO: usize = 8;
pub const SEMANTIC_WINDOW: usize = 5;
pub const SEMANTIC_MAX_OCCURRENCES: usize = 512;
pub const SEMANTIC_EDGE_THRESHOLD: f32 = 0.75;
pub const SEMANTIC_MAX_EDGES: usize = 10;
pub const SEMANTIC_DENOMINATOR_EPSILON: f32 = 1.0e-10;
pub const PRETRAINED_TOKEN_COUNT: usize = 40_856;
pub const PRETRAINED_DIM: usize = SEMANTIC_DIM;
pub const PRETRAINED_VECTOR_SHA256: &str =
    "c76bba4c5032323ded6202053af5afdbbac12f6d920c691b3b3b4cd708f99e83";
pub const PRETRAINED_TOKENS_SHA256: &str =
    "b2d1cc1524bc934c157d9b64afa1d45cf0739c5d9db7e8806ddce7ed48232819";

const RANDOM_INDEX_SEED_BASE: u64 = 0x5249_4E44;

const ABBREVIATIONS: &[(&str, &str)] = &[
    ("err", "error"),
    ("exc", "exception"),
    ("ex", "exception"),
    ("ctx", "context"),
    ("cfg", "config"),
    ("conf", "configuration"),
    ("env", "environment"),
    ("opt", "option"),
    ("opts", "options"),
    ("req", "request"),
    ("res", "response"),
    ("resp", "response"),
    ("rsp", "response"),
    ("hdr", "header"),
    ("hdrs", "headers"),
    ("str", "string"),
    ("fmt", "format"),
    ("msg", "message"),
    ("txt", "text"),
    ("lbl", "label"),
    ("desc", "description"),
    ("buf", "buffer"),
    ("arr", "array"),
    ("vec", "vector"),
    ("lst", "list"),
    ("dict", "dictionary"),
    ("tbl", "table"),
    ("stk", "stack"),
    ("que", "queue"),
    ("fn", "function"),
    ("func", "function"),
    ("cb", "callback"),
    ("proc", "procedure"),
    ("ctor", "constructor"),
    ("dtor", "destructor"),
    ("db", "database"),
    ("col", "column"),
    ("tbl", "table"),
    ("stmt", "statement"),
    ("txn", "transaction"),
    ("trx", "transaction"),
    ("repo", "repository"),
    ("auth", "authentication"),
    ("authz", "authorization"),
    ("perm", "permission"),
    ("cred", "credential"),
    ("tok", "token"),
    ("pwd", "password"),
    ("val", "value"),
    ("num", "number"),
    ("int", "integer"),
    ("bool", "boolean"),
    ("flt", "float"),
    ("dbl", "double"),
    ("idx", "index"),
    ("iter", "iterator"),
    ("elem", "element"),
    ("cnt", "count"),
    ("len", "length"),
    ("sz", "size"),
    ("pos", "position"),
    ("off", "offset"),
    ("cap", "capacity"),
    ("init", "initialize"),
    ("deinit", "deinitialize"),
    ("alloc", "allocate"),
    ("dealloc", "deallocate"),
    ("del", "delete"),
    ("rm", "remove"),
    ("impl", "implementation"),
    ("iface", "interface"),
    ("abs", "abstract"),
    ("decl", "declaration"),
    ("param", "parameter"),
    ("arg", "argument"),
    ("attr", "attribute"),
    ("prop", "property"),
    ("ret", "return"),
    ("src", "source"),
    ("dst", "destination"),
    ("tgt", "target"),
    ("orig", "original"),
    ("prev", "previous"),
    ("cur", "current"),
    ("tmp", "temporary"),
    ("temp", "temporary"),
    ("conn", "connection"),
    ("sess", "session"),
    ("sock", "socket"),
    ("addr", "address"),
    ("url", "uniform"),
    ("srv", "server"),
    ("cli", "client"),
    ("svc", "service"),
    ("ep", "endpoint"),
    ("mgr", "manager"),
    ("ctrl", "controller"),
    ("hdlr", "handler"),
    ("sched", "scheduler"),
    ("disp", "dispatcher"),
    ("reg", "registry"),
    ("chan", "channel"),
    ("sem", "semaphore"),
    ("mtx", "mutex"),
    ("wg", "waitgroup"),
    ("sig", "signal"),
    ("evt", "event"),
    ("sub", "subscriber"),
    ("pub", "publisher"),
    ("spec", "specification"),
    ("mock", "mock"),
    ("stub", "stub"),
    ("assert", "assertion"),
    ("log", "logging"),
    ("lvl", "level"),
    ("dbg", "debug"),
    ("wrn", "warning"),
    ("inf", "info"),
    ("ts", "timestamp"),
    ("dur", "duration"),
    ("ttl", "timetolive"),
    ("ver", "version"),
    ("ns", "namespace"),
    ("pkg", "package"),
    ("mod", "module"),
    ("lib", "library"),
    ("dep", "dependency"),
    ("ref", "reference"),
    ("ptr", "pointer"),
    ("obj", "object"),
    ("doc", "document"),
    ("cmd", "command"),
    ("ops", "operations"),
    ("util", "utility"),
    ("hlp", "helper"),
    ("ext", "extension"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticVector {
    values: [f32; SEMANTIC_DIM],
}

impl Default for SemanticVector {
    fn default() -> Self {
        Self::zero()
    }
}

impl SemanticVector {
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            values: [0.0; SEMANTIC_DIM],
        }
    }

    #[must_use]
    pub const fn from_array(values: [f32; SEMANTIC_DIM]) -> Self {
        Self { values }
    }

    #[must_use]
    pub const fn values(&self) -> &[f32; SEMANTIC_DIM] {
        &self.values
    }

    #[must_use]
    pub fn cosine(&self, other: &Self) -> f32 {
        cosine(&self.values, &other.values)
    }

    pub fn normalize(&mut self) {
        let magnitude = self
            .values
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        if magnitude < SEMANTIC_DENOMINATOR_EPSILON {
            return;
        }
        let inverse = 1.0 / magnitude;
        for value in &mut self.values {
            *value *= inverse;
        }
    }

    pub fn add_scaled(&mut self, source: &Self, scale: f32) {
        for (destination, source) in self.values.iter_mut().zip(source.values) {
            *destination += scale * source;
        }
    }

    pub fn diffuse(&mut self, neighbors: &[Self], alpha: f32) {
        if neighbors.is_empty() {
            return;
        }
        let mut mean = [0.0_f32; SEMANTIC_DIM];
        for neighbor in neighbors {
            for (sum, value) in mean.iter_mut().zip(neighbor.values) {
                *sum += value;
            }
        }
        let inverse_count = 1.0 / neighbors.len() as f32;
        let retained = 1.0 - alpha;
        for (value, mean) in self.values.iter_mut().zip(mean) {
            *value = retained * *value + alpha * mean * inverse_count;
        }
        self.normalize();
    }

    #[must_use]
    pub fn for_token(token: &str, model: Option<&PretrainedModel>) -> Self {
        if let Some(vector) = model.and_then(|model| model.vector(token)) {
            let mut values = [0.0; SEMANTIC_DIM];
            for (destination, source) in values.iter_mut().zip(vector) {
                *destination = f32::from(*source) / 127.0;
            }
            return Self { values };
        }
        Self::sparse_random_index(token)
    }

    #[must_use]
    pub fn sparse_random_index(token: &str) -> Self {
        let mut result = Self::zero();
        let token_seed = xxh3_64(token.as_bytes());
        for index in 0_i32..SEMANTIC_SPARSE_NON_ZERO as i32 {
            let hash = xxh3_64_with_seed(
                &index.to_le_bytes(),
                token_seed.wrapping_add(RANDOM_INDEX_SEED_BASE),
            );
            let position = hash as usize % SEMANTIC_DIM;
            let sign = if hash & 1 == 1 { 1.0 } else { -1.0 };
            result.values[position] += sign;
        }
        result
    }
}

#[must_use]
pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut left_magnitude = 0.0_f32;
    let mut right_magnitude = 0.0_f32;
    for (&left, &right) in left.iter().zip(right) {
        dot += left * right;
        left_magnitude += left * left;
        right_magnitude += right * right;
    }
    let denominator = left_magnitude.sqrt() * right_magnitude.sqrt();
    if denominator < SEMANTIC_DENOMINATOR_EPSILON {
        0.0
    } else {
        dot / denominator
    }
}

#[must_use]
pub fn module_proximity(path_a: &str, path_b: &str) -> f32 {
    let shared = path_a
        .bytes()
        .zip(path_b.bytes())
        .take_while(|(left, right)| left == right)
        .filter(|(byte, _)| *byte == b'/')
        .count();
    let maximum_components = path_a
        .bytes()
        .filter(|byte| *byte == b'/')
        .count()
        .max(path_b.bytes().filter(|byte| *byte == b'/').count());
    if maximum_components == 0 {
        1.0
    } else {
        1.0 + shared as f32 / maximum_components as f32 * 0.10
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use super::*;

    #[test]
    fn tokenizer_matches_camel_delimiter_and_abbreviation_rules() {
        assert_eq!(
            tokenize_identifier("handleHttp_request.ctx-err", 16),
            [
                "handle", "http", "request", "ctx", "err", "context", "error"
            ]
        );
        assert_eq!(tokenize_identifier("HTTPServer", 8), ["httpserver"]);
        assert_eq!(tokenize_identifier("a_b_c", 2), ["a", "b"]);
    }

    #[test]
    fn cosine_normalization_and_diffusion_preserve_bounds() {
        let mut left = SemanticVector::zero();
        left.values[0] = 3.0;
        left.values[1] = 4.0;
        left.normalize();
        assert!((left.values[0] - 0.6).abs() < 1.0e-6);
        assert!((left.values[1] - 0.8).abs() < 1.0e-6);
        assert!((left.cosine(&left) - 1.0).abs() < 1.0e-6);

        let mut right = SemanticVector::zero();
        right.values[2] = 1.0;
        assert_eq!(left.cosine(&right), 0.0);
        left.diffuse(&[right], 0.5);
        assert!((left.cosine(&left) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn sparse_random_index_is_deterministic_and_bounded() {
        let first = SemanticVector::sparse_random_index("context");
        let second = SemanticVector::sparse_random_index("context");
        let other = SemanticVector::sparse_random_index("request");

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(
            first.values.iter().filter(|value| **value != 0.0).count() <= SEMANTIC_SPARSE_NON_ZERO
        );
    }

    #[test]
    fn bundled_model_passes_integrity_and_shape_guards() {
        let model = PretrainedModel::load_bundled().expect("audited runtime model");
        assert_eq!(model.token_count(), PRETRAINED_TOKEN_COUNT);
        assert_eq!(model.lookup_token_count(), PRETRAINED_TOKEN_COUNT - 11);
        let error = model.vector("error").expect("error token");
        assert_eq!(
            &error[..16],
            &[3, 5, -1, -3, 4, -1, -1, -6, -5, 10, 0, -2, 0, 2, 4, 1]
        );
    }

    #[test]
    fn module_proximity_matches_upstream_prefix_ratio() {
        assert_eq!(module_proximity("a.rs", "b.rs"), 1.0);
        assert!((module_proximity("src/a.rs", "src/b.rs") - 1.1).abs() < 1.0e-6);
        assert_eq!(module_proximity("src/a.rs", "tests/b.rs"), 1.0);
    }
}
