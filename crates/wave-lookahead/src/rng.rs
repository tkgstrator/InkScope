//! Four-word RNG operations recovered from the 11.3.0 executable.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeedWords([u32; 4]);

impl SeedWords {
    /// The shared game state replaces an all-zero scene seed with `(1, 0, 0, 0)`.
    pub fn from_scene(mut words: [u32; 4]) -> Self {
        if words == [0; 4] {
            words[0] = 1;
        }
        Self(words)
    }

    pub fn as_array(self) -> [u32; 4] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XorShift128([u32; 4]);

impl XorShift128 {
    pub fn from_words(words: [u32; 4]) -> Self {
        Self(words)
    }

    pub fn as_array(self) -> [u32; 4] {
        self.0
    }

    pub fn next32(&mut self) -> u32 {
        let [s0, s1, s2, s3] = self.0;
        let t = s0 ^ (s0 << 11);
        let value = s3 ^ (s3 >> 19) ^ t ^ (t >> 8);
        self.0 = [s1, s2, s3, value];
        value
    }
}

/// Derive an independent four-word state from the scene seed and a domain.
pub fn derive(seed: SeedWords, domain: u32) -> XorShift128 {
    let source = seed.as_array();
    let t = source[0] ^ (source[0] << 11);
    let mut value = source[3] ^ (source[3] >> 19) ^ t ^ (t >> 8);
    value = value.wrapping_add(domain);
    expand_seed_word(value)
}

/// Expand one 32-bit input into a four-word state using the director's rounds.
pub(crate) fn expand_seed_word(mut value: u32) -> XorShift128 {
    let mut words = [0; 4];
    for (i, word) in words.iter_mut().enumerate() {
        value = 0x6c07_8965u32
            .wrapping_mul(value ^ (value >> 30))
            .wrapping_add((i + 1) as u32);
        *word = value;
    }
    XorShift128::from_words(words)
}
