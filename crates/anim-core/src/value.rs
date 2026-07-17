//! Evaluation values. In M1 these are symbolic: a human-readable recipe string
//! plus a stable hash. Golden tests assert on both. M2 swaps the payload for
//! real GPU texture handles while keeping the same evaluator shape — the hash
//! survives as the cache/render-dirty key.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageDesc {
    pub hash: u64,
    pub recipe: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Image(ImageDesc),
}

impl Value {
    pub fn image(recipe: String) -> Self {
        let hash = fnv1a(recipe.as_bytes());
        Self::Image(ImageDesc { hash, recipe })
    }

    pub fn recipe(&self) -> &str {
        match self {
            Self::Image(d) => &d.recipe,
        }
    }

    pub fn hash(&self) -> u64 {
        match self {
            Self::Image(d) => d.hash,
        }
    }
}

/// FNV-1a: implemented inline so hashes are stable across Rust versions
/// (std's DefaultHasher makes no such promise, and these hashes end up in
/// golden tests and, later, render caches).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
