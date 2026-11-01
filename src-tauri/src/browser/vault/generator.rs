//! Password and passphrase generator with strength estimation.
//!
//! Ported from Maho (Project-Maho/maho-browser@c2830251): maho/crates/maho-core/src/vault_generator.rs
//! with its types from maho/crates/maho-types/src/vault_generator.rs and the bundled EFF short
//! wordlist (wordlist.txt, 1296 words), under the owner's reuse grant.

use std::fmt;
use std::sync::OnceLock;

use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use rand::Rng;

use serde::{Deserialize, Serialize};

pub const MIN_PASSWORD_LENGTH: u32 = 8;
pub const MAX_PASSWORD_LENGTH: u32 = 128;
pub const DEFAULT_PASSWORD_LENGTH: u32 = 20;

pub const MIN_PASSPHRASE_WORDS: u32 = 3;
pub const MAX_PASSPHRASE_WORDS: u32 = 12;
pub const DEFAULT_PASSPHRASE_WORDS: u32 = 5;
pub const DEFAULT_PASSPHRASE_SEPARATOR: &str = "-";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PasswordGeneratorMode {
    #[default]
    Password,
    Passphrase,
}

fn default_mode() -> PasswordGeneratorMode {
    PasswordGeneratorMode::Password
}

fn default_length() -> u32 {
    DEFAULT_PASSWORD_LENGTH
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_word_count() -> u32 {
    DEFAULT_PASSPHRASE_WORDS
}

fn default_separator() -> String {
    DEFAULT_PASSPHRASE_SEPARATOR.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasswordGeneratorOptions {
    #[serde(default = "default_mode")]
    pub mode: PasswordGeneratorMode,

    #[serde(default = "default_length")]
    pub length: u32,
    #[serde(default = "default_true")]
    pub include_lowercase: bool,
    #[serde(default = "default_true")]
    pub include_uppercase: bool,
    #[serde(default = "default_true")]
    pub include_digits: bool,
    #[serde(default = "default_true")]
    pub include_symbols: bool,
    #[serde(default = "default_false")]
    pub avoid_ambiguous: bool,

    #[serde(default = "default_word_count")]
    pub word_count: u32,
    #[serde(default = "default_separator")]
    pub separator: String,
    #[serde(default = "default_true")]
    pub capitalize: bool,
    #[serde(default = "default_true")]
    pub include_number: bool,
}

impl Default for PasswordGeneratorOptions {
    fn default() -> Self {
        Self {
            mode: default_mode(),
            length: default_length(),
            include_lowercase: true,
            include_uppercase: true,
            include_digits: true,
            include_symbols: true,
            avoid_ambiguous: false,
            word_count: default_word_count(),
            separator: default_separator(),
            capitalize: true,
            include_number: true,
        }
    }
}

impl PasswordGeneratorOptions {
    pub fn new_password(length: u32) -> Self {
        Self {
            mode: PasswordGeneratorMode::Password,
            length,
            ..Default::default()
        }
    }

    pub fn new_passphrase(word_count: u32) -> Self {
        Self {
            mode: PasswordGeneratorMode::Passphrase,
            word_count,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PasswordStrengthScore {
    VeryWeak = 0,
    Weak = 1,
    Fair = 2,
    Good = 3,
    Strong = 4,
}

impl PasswordStrengthScore {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::VeryWeak,
            1 => Self::Weak,
            2 => Self::Fair,
            3 => Self::Good,
            _ => Self::Strong,
        }
    }

    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    pub fn is_weak(&self) -> bool {
        self.as_u8() <= 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordStrength {
    pub score: u8,
    pub entropy_bits: f64,
}

impl PasswordStrength {
    pub fn new(score: u8, entropy_bits: f64) -> Self {
        Self {
            score: score.min(4),
            entropy_bits,
        }
    }

    pub fn score_enum(&self) -> PasswordStrengthScore {
        PasswordStrengthScore::from_u8(self.score)
    }

    pub fn is_weak(&self) -> bool {
        self.score <= 1
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPasswordResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub strength_score: u8,
    pub entropy_bits: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl GeneratedPasswordResult {
    pub fn success(password: String, strength: PasswordStrength) -> Self {
        Self {
            success: true,
            password: Some(password),
            strength_score: strength.score,
            entropy_bits: strength.entropy_bits,
            error: None,
        }
    }

    pub fn failure(err: impl fmt::Display) -> Self {
        Self {
            success: false,
            password: None,
            strength_score: 0,
            entropy_bits: 0.0,
            error: Some(err.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VaultGeneratorError {
    InvalidLength { length: u32, min: u32, max: u32 },
    NoCharacterClassesEnabled,
    InvalidWordCount { count: u32, min: u32, max: u32 },
    RngFailure(String),
}

impl fmt::Display for VaultGeneratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { length, min, max } => {
                write!(
                    f,
                    "password length {length} out of bounds: must be between {min} and {max}"
                )
            }
            Self::NoCharacterClassesEnabled => {
                write!(f, "no character classes enabled for password generation")
            }
            Self::InvalidWordCount { count, min, max } => {
                write!(
                    f,
                    "passphrase word count {count} out of bounds: must be between {min} and {max}"
                )
            }
            Self::RngFailure(reason) => {
                write!(f, "random number generation failed: {reason}")
            }
        }
    }
}

impl std::error::Error for VaultGeneratorError {}

// ---- generator ----



const WORDLIST_RAW: &str = include_str!("wordlist.txt");

static WORDLIST: OnceLock<Vec<&'static str>> = OnceLock::new();

pub fn get_wordlist() -> &'static [&'static str] {
    WORDLIST.get_or_init(|| {
        WORDLIST_RAW
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

const LOWERCASE: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
const SYMBOLS: &[u8] = b"!@#$%^&*()_+-=[]{}|;:,.<>?";

fn is_ambiguous_char(c: u8) -> bool {
    matches!(
        c,
        b'l' | b'I' | b'1' | b'O' | b'0' | b'o' | b'|' | b';' | b':'
    )
}

fn filter_class(class: &[u8], avoid_ambiguous: bool) -> Vec<u8> {
    if avoid_ambiguous {
        class
            .iter()
            .copied()
            .filter(|&b| !is_ambiguous_char(b))
            .collect()
    } else {
        class.to_vec()
    }
}

pub fn generate_password(
    opts: &PasswordGeneratorOptions,
) -> Result<GeneratedPasswordResult, VaultGeneratorError> {
    generate_password_with_rng(opts, &mut OsRng)
}

pub fn generate_password_with_rng<R: Rng + ?Sized>(
    opts: &PasswordGeneratorOptions,
    rng: &mut R,
) -> Result<GeneratedPasswordResult, VaultGeneratorError> {
    match opts.mode {
        PasswordGeneratorMode::Password => generate_standard_password(opts, rng),
        PasswordGeneratorMode::Passphrase => generate_passphrase(opts, rng),
    }
}

fn generate_standard_password<R: Rng + ?Sized>(
    opts: &PasswordGeneratorOptions,
    rng: &mut R,
) -> Result<GeneratedPasswordResult, VaultGeneratorError> {
    if opts.length < MIN_PASSWORD_LENGTH || opts.length > MAX_PASSWORD_LENGTH {
        return Err(VaultGeneratorError::InvalidLength {
            length: opts.length,
            min: MIN_PASSWORD_LENGTH,
            max: MAX_PASSWORD_LENGTH,
        });
    }

    let mut guaranteed: Vec<u8> = Vec::with_capacity(4);
    let mut combined_pool: Vec<u8> = Vec::with_capacity(90);

    if opts.include_lowercase {
        let pool = filter_class(LOWERCASE, opts.avoid_ambiguous);
        if !pool.is_empty() {
            let idx = rng.gen_range(0..pool.len());
            guaranteed.push(pool[idx]);
            combined_pool.extend_from_slice(&pool);
        }
    }

    if opts.include_uppercase {
        let pool = filter_class(UPPERCASE, opts.avoid_ambiguous);
        if !pool.is_empty() {
            let idx = rng.gen_range(0..pool.len());
            guaranteed.push(pool[idx]);
            combined_pool.extend_from_slice(&pool);
        }
    }

    if opts.include_digits {
        let pool = filter_class(DIGITS, opts.avoid_ambiguous);
        if !pool.is_empty() {
            let idx = rng.gen_range(0..pool.len());
            guaranteed.push(pool[idx]);
            combined_pool.extend_from_slice(&pool);
        }
    }

    if opts.include_symbols {
        let pool = filter_class(SYMBOLS, opts.avoid_ambiguous);
        if !pool.is_empty() {
            let idx = rng.gen_range(0..pool.len());
            guaranteed.push(pool[idx]);
            combined_pool.extend_from_slice(&pool);
        }
    }

    if combined_pool.is_empty() {
        return Err(VaultGeneratorError::NoCharacterClassesEnabled);
    }

    let mut password_bytes = guaranteed;
    let target_len = opts.length as usize;
    while password_bytes.len() < target_len {
        let idx = rng.gen_range(0..combined_pool.len());
        password_bytes.push(combined_pool[idx]);
    }

    password_bytes.shuffle(rng);

    let password = String::from_utf8(password_bytes)
        .map_err(|e| VaultGeneratorError::RngFailure(e.to_string()))?;

    let strength = estimate_strength(&password);
    Ok(GeneratedPasswordResult::success(password, strength))
}

fn generate_passphrase<R: Rng + ?Sized>(
    opts: &PasswordGeneratorOptions,
    rng: &mut R,
) -> Result<GeneratedPasswordResult, VaultGeneratorError> {
    if opts.word_count < MIN_PASSPHRASE_WORDS || opts.word_count > MAX_PASSPHRASE_WORDS {
        return Err(VaultGeneratorError::InvalidWordCount {
            count: opts.word_count,
            min: MIN_PASSPHRASE_WORDS,
            max: MAX_PASSPHRASE_WORDS,
        });
    }

    let wordlist = get_wordlist();
    if wordlist.is_empty() {
        return Err(VaultGeneratorError::RngFailure(
            "bundled wordlist is empty".to_string(),
        ));
    }

    let count = opts.word_count as usize;
    let mut words: Vec<String> = Vec::with_capacity(count);

    for _ in 0..count {
        let idx = rng.gen_range(0..wordlist.len());
        let raw = wordlist[idx];
        if opts.capitalize {
            let mut chars = raw.chars();
            let capitalized = match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            };
            words.push(capitalized);
        } else {
            words.push(raw.to_string());
        }
    }

    if opts.include_number && !words.is_empty() {
        let target_idx = rng.gen_range(0..words.len());
        let digit = rng.gen_range(0..10);
        words[target_idx].push_str(&digit.to_string());
    }

    let password = words.join(&opts.separator);
    let strength = estimate_strength(&password);
    Ok(GeneratedPasswordResult::success(password, strength))
}

pub fn estimate_strength(password: &str) -> PasswordStrength {
    if password.is_empty() {
        return PasswordStrength::new(0, 0.0);
    }

    let len = password.chars().count();
    let wordlist = get_wordlist();

    let is_passphrase = is_likely_passphrase(password, wordlist);

    let raw_entropy = if is_passphrase {
        compute_passphrase_entropy(password)
    } else {
        compute_standard_entropy(password, len)
    };

    let penalty = compute_penalties(password, len, is_passphrase);
    let final_entropy = (raw_entropy - penalty).max(0.0);
    let rounded_entropy = (final_entropy * 10.0).round() / 10.0;

    let score = if is_severely_weak(password, len) {
        if len < 6 || rounded_entropy < 15.0 {
            0
        } else {
            1
        }
    } else if rounded_entropy < 28.0 {
        0
    } else if rounded_entropy < 36.0 {
        1
    } else if rounded_entropy < 60.0 {
        2
    } else if rounded_entropy < 80.0 {
        3
    } else {
        4
    };

    PasswordStrength::new(score, rounded_entropy)
}

fn is_severely_weak(password: &str, len: usize) -> bool {
    if len < 8 {
        return true;
    }

    let lower = password.to_ascii_lowercase();
    const COMMON_PASSWORDS: &[&str] = &[
        "password", "123456", "12345678", "qwerty", "admin", "welcome", "secret", "letmein",
        "master", "login", "iloveyou", "abc123",
    ];

    for common in COMMON_PASSWORDS {
        if lower == *common || lower.contains(common) {
            return true;
        }
    }

    let first_char = password.chars().next();
    if password.chars().all(|c| Some(c) == first_char) {
        return true;
    }

    false
}

fn compute_standard_entropy(password: &str, len: usize) -> f64 {
    let mut pool: u32 = 0;
    let mut has_lower = false;
    let mut has_upper = false;
    let mut has_digit = false;
    let mut has_symbol = false;
    let mut has_other = false;

    for c in password.chars() {
        if c.is_ascii_lowercase() {
            has_lower = true;
        } else if c.is_ascii_uppercase() {
            has_upper = true;
        } else if c.is_ascii_digit() {
            has_digit = true;
        } else if c.is_ascii_punctuation() {
            has_symbol = true;
        } else {
            has_other = true;
        }
    }

    if has_lower {
        pool += 26;
    }
    if has_upper {
        pool += 26;
    }
    if has_digit {
        pool += 10;
    }
    if has_symbol {
        pool += 33;
    }
    if has_other {
        pool += 30;
    }

    if pool <= 1 {
        0.0
    } else {
        (len as f64) * (pool as f64).log2()
    }
}

fn is_likely_passphrase(password: &str, wordlist: &[&str]) -> bool {
    let separators = ['-', ' ', '_', '.'];
    for sep in separators {
        if password.contains(sep) {
            let segments: Vec<&str> = password.split(sep).filter(|s| !s.is_empty()).collect();
            if segments.len() >= 3 {
                let matches_count = segments
                    .iter()
                    .filter(|seg| {
                        let cleaned: String = seg
                            .chars()
                            .filter(|c| c.is_alphabetic())
                            .flat_map(|c| c.to_lowercase())
                            .collect();
                        wordlist.binary_search(&cleaned.as_str()).is_ok()
                            || wordlist.contains(&cleaned.as_str())
                    })
                    .count();
                if matches_count >= 2 {
                    return true;
                }
            }
        }
    }
    false
}

fn compute_passphrase_entropy(password: &str) -> f64 {
    let separators = ['-', ' ', '_', '.'];
    let sep = separators
        .iter()
        .copied()
        .find(|&s| password.contains(s))
        .unwrap_or('-');
    let segments: Vec<&str> = password.split(sep).filter(|s| !s.is_empty()).collect();
    let word_count = segments.len();

    let bits_per_word = 1296.0_f64.log2();
    let mut total = (word_count as f64) * bits_per_word;

    if password.chars().any(|c| c.is_ascii_digit()) {
        total += 3.32;
    }
    if password.chars().any(|c| c.is_ascii_uppercase()) {
        total += word_count as f64;
    }
    total += 3.0;

    total
}

fn compute_penalties(password: &str, len: usize, is_passphrase: bool) -> f64 {
    let mut penalty = 0.0;

    if is_passphrase {
        return penalty;
    }

    if len < 8 {
        penalty += (8 - len) as f64 * 8.0;
    }
    if len < 6 {
        penalty += 30.0;
    }

    let mut unique_chars = std::collections::HashSet::new();
    let chars: Vec<char> = password.chars().collect();
    for &c in &chars {
        unique_chars.insert(c);
    }

    if unique_chars.len() < len {
        penalty += (len - unique_chars.len()) as f64 * 2.0;
    }

    for window in chars.windows(2) {
        if window[0] == window[1] {
            penalty += 4.0;
        }
    }

    for window in chars.windows(3) {
        let b0 = window[0] as i32;
        let b1 = window[1] as i32;
        let b2 = window[2] as i32;
        if (b1 == b0 + 1 && b2 == b1 + 1) || (b1 == b0 - 1 && b2 == b1 - 1) {
            penalty += 8.0;
        }
    }

    let lower = password.to_ascii_lowercase();
    const KEYBOARD_PATTERNS: &[&str] = &["qwerty", "asdf", "zxcv", "1234"];
    for pat in KEYBOARD_PATTERNS {
        if lower.contains(pat) {
            penalty += 12.0;
        }
    }

    penalty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_bounds_validation() {
        let short_opts = PasswordGeneratorOptions {
            length: 7,
            ..Default::default()
        };
        let res_short = generate_password(&short_opts);
        assert_eq!(
            res_short,
            Err(VaultGeneratorError::InvalidLength {
                length: 7,
                min: 8,
                max: 128
            })
        );

        let long_opts = PasswordGeneratorOptions {
            length: 129,
            ..Default::default()
        };
        let res_long = generate_password(&long_opts);
        assert_eq!(
            res_long,
            Err(VaultGeneratorError::InvalidLength {
                length: 129,
                min: 8,
                max: 128
            })
        );

        let min_opts = PasswordGeneratorOptions {
            length: 8,
            ..Default::default()
        };
        let min_res = generate_password(&min_opts).expect("length 8 must succeed");
        assert_eq!(min_res.password.as_deref().unwrap_or("").len(), 8);

        let max_opts = PasswordGeneratorOptions {
            length: 128,
            ..Default::default()
        };
        let max_res = generate_password(&max_opts).expect("length 128 must succeed");
        assert_eq!(max_res.password.as_deref().unwrap_or("").len(), 128);
    }

    #[test]
    fn test_no_character_classes_enabled() {
        let opts = PasswordGeneratorOptions {
            length: 16,
            include_lowercase: false,
            include_uppercase: false,
            include_digits: false,
            include_symbols: false,
            ..Default::default()
        };
        let res = generate_password(&opts);
        assert_eq!(res, Err(VaultGeneratorError::NoCharacterClassesEnabled));
    }

    #[test]
    fn test_class_guarantees_satisfied() {
        let opts = PasswordGeneratorOptions {
            length: 8,
            include_lowercase: true,
            include_uppercase: true,
            include_digits: true,
            include_symbols: true,
            avoid_ambiguous: false,
            ..Default::default()
        };

        for _ in 0..50 {
            let res = generate_password(&opts).expect("generation must succeed");
            let pass = res.password.expect("password must be present");
            assert_eq!(pass.len(), 8);

            let has_lower = pass.chars().any(|c| c.is_ascii_lowercase());
            let has_upper = pass.chars().any(|c| c.is_ascii_uppercase());
            let has_digit = pass.chars().any(|c| c.is_ascii_digit());
            let has_symbol = pass.chars().any(|c| SYMBOLS.contains(&(c as u8)));

            assert!(has_lower);
            assert!(has_upper);
            assert!(has_digit);
            assert!(has_symbol);
        }
    }

    #[test]
    fn test_class_guarantees_subset() {
        let opts = PasswordGeneratorOptions {
            length: 10,
            include_lowercase: false,
            include_uppercase: true,
            include_digits: true,
            include_symbols: false,
            avoid_ambiguous: false,
            ..Default::default()
        };

        for _ in 0..20 {
            let res = generate_password(&opts).expect("generation must succeed");
            let pass = res.password.expect("password must be present");
            assert_eq!(pass.len(), 10);

            assert!(pass.chars().any(|c| c.is_ascii_uppercase()));
            assert!(pass.chars().any(|c| c.is_ascii_digit()));
            assert!(!pass.chars().any(|c| c.is_ascii_lowercase()));
            assert!(!pass.chars().any(|c| SYMBOLS.contains(&(c as u8))));
        }
    }

    #[test]
    fn test_avoid_ambiguous_characters() {
        let opts = PasswordGeneratorOptions {
            length: 64,
            include_lowercase: true,
            include_uppercase: true,
            include_digits: true,
            include_symbols: true,
            avoid_ambiguous: true,
            ..Default::default()
        };

        for _ in 0..20 {
            let res = generate_password(&opts).expect("generation must succeed");
            let pass = res.password.expect("password must be present");

            for c in pass.chars() {
                assert!(
                    !is_ambiguous_char(c as u8),
                    "character {c} is ambiguous but was generated"
                );
            }
        }
    }

    #[test]
    fn test_passphrase_bounds_validation() {
        let short_opts = PasswordGeneratorOptions {
            mode: PasswordGeneratorMode::Passphrase,
            word_count: 2,
            ..Default::default()
        };
        let res_short = generate_password(&short_opts);
        assert_eq!(
            res_short,
            Err(VaultGeneratorError::InvalidWordCount {
                count: 2,
                min: 3,
                max: 12
            })
        );

        let long_opts = PasswordGeneratorOptions {
            mode: PasswordGeneratorMode::Passphrase,
            word_count: 13,
            ..Default::default()
        };
        let res_long = generate_password(&long_opts);
        assert_eq!(
            res_long,
            Err(VaultGeneratorError::InvalidWordCount {
                count: 13,
                min: 3,
                max: 12
            })
        );
    }

    #[test]
    fn test_passphrase_generation_rules() {
        let opts = PasswordGeneratorOptions {
            mode: PasswordGeneratorMode::Passphrase,
            word_count: 4,
            separator: "-".to_string(),
            capitalize: true,
            include_number: true,
            ..Default::default()
        };

        for _ in 0..20 {
            let res = generate_password(&opts).expect("passphrase generation must succeed");
            let pass = res.password.expect("passphrase must be present");

            let parts: Vec<&str> = pass.split('-').collect();
            assert_eq!(parts.len(), 4);

            let mut has_digit = false;
            for part in parts {
                assert!(!part.is_empty());
                let first_char = part.chars().next().expect("word part must have first char");
                assert!(first_char.is_ascii_uppercase());
                if part.chars().any(|c| c.is_ascii_digit()) {
                    has_digit = true;
                }
            }
            assert!(has_digit);
        }
    }

    #[test]
    fn test_passphrase_without_number_and_no_capitalize() {
        let opts = PasswordGeneratorOptions {
            mode: PasswordGeneratorMode::Passphrase,
            word_count: 3,
            separator: ".".to_string(),
            capitalize: false,
            include_number: false,
            ..Default::default()
        };

        let res = generate_password(&opts).expect("passphrase generation must succeed");
        let pass = res.password.expect("passphrase must be present");

        let parts: Vec<&str> = pass.split('.').collect();
        assert_eq!(parts.len(), 3);

        for part in parts {
            assert!(part.chars().all(|c| c.is_ascii_lowercase()));
            assert!(!part.chars().any(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn test_deterministic_with_seeded_rng() {
        struct SeededRng {
            state: u64,
        }

        impl SeededRng {
            fn new(seed: u64) -> Self {
                Self { state: seed }
            }
        }

        impl rand::RngCore for SeededRng {
            fn next_u32(&mut self) -> u32 {
                self.next_u64() as u32
            }

            fn next_u64(&mut self) -> u64 {
                self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
                self.state
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                for b in dest {
                    *b = self.next_u32() as u8;
                }
            }

            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
                self.fill_bytes(dest);
                Ok(())
            }
        }

        let opts = PasswordGeneratorOptions::new_password(24);

        let mut rng1 = SeededRng::new(123456789);
        let res1 = generate_password_with_rng(&opts, &mut rng1).expect("generation must succeed");

        let mut rng2 = SeededRng::new(123456789);
        let res2 = generate_password_with_rng(&opts, &mut rng2).expect("generation must succeed");

        assert_eq!(res1.password, res2.password);
        assert_eq!(res1.strength_score, res2.strength_score);
        assert_eq!(res1.entropy_bits, res2.entropy_bits);
    }

    #[test]
    fn test_strength_estimation_weak_passwords() {
        let empty_strength = estimate_strength("");
        assert_eq!(empty_strength.score, 0);
        assert_eq!(empty_strength.entropy_bits, 0.0);
        assert!(empty_strength.is_weak());

        let common1 = estimate_strength("password");
        assert!(common1.is_weak());
        assert!(common1.score <= 1);

        let common2 = estimate_strength("123456");
        assert!(common2.is_weak());
        assert_eq!(common2.score, 0);

        let short = estimate_strength("aB1!");
        assert!(short.is_weak());
        assert_eq!(short.score, 0);

        let repeat = estimate_strength("aaaaaaaa");
        assert!(repeat.is_weak());
        assert!(repeat.score <= 1);

        let seq = estimate_strength("abcdefgh");
        assert!(seq.is_weak());
        assert!(seq.score <= 1);
    }

    #[test]
    fn test_strength_estimation_strong_passwords() {
        let strong_pw = "k7#mP9$vQ2!wR8&zT4@y";
        let str_strong = estimate_strength(strong_pw);
        assert!(!str_strong.is_weak());
        assert!(str_strong.score >= 3);
        assert!(str_strong.entropy_bits >= 70.0);

        let strong_passphrase = "Acorn-Blade-Cider-Draft-Eagle-Fence3";
        let str_phrase = estimate_strength(strong_passphrase);
        assert!(!str_phrase.is_weak());
        assert!(str_phrase.score >= 3);
        assert!(str_phrase.entropy_bits >= 60.0);
    }

    #[test]
    fn test_bundled_wordlist_is_available_and_populated() {
        let words = get_wordlist();
        assert_eq!(words.len(), 1296);
        assert_eq!(words[0], "acid");
        assert_eq!(words[words.len() - 1], "zoom");
    }

    #[test]
    fn test_score_enum_conversion_and_weak_boundary() {
        assert_eq!(
            PasswordStrengthScore::from_u8(0),
            PasswordStrengthScore::VeryWeak
        );
        assert_eq!(
            PasswordStrengthScore::from_u8(1),
            PasswordStrengthScore::Weak
        );
        assert_eq!(
            PasswordStrengthScore::from_u8(2),
            PasswordStrengthScore::Fair
        );
        assert_eq!(
            PasswordStrengthScore::from_u8(3),
            PasswordStrengthScore::Good
        );
        assert_eq!(
            PasswordStrengthScore::from_u8(4),
            PasswordStrengthScore::Strong
        );
        assert_eq!(
            PasswordStrengthScore::from_u8(99),
            PasswordStrengthScore::Strong
        );

        assert!(PasswordStrengthScore::VeryWeak.is_weak());
        assert!(PasswordStrengthScore::Weak.is_weak());
        assert!(!PasswordStrengthScore::Fair.is_weak());
        assert!(!PasswordStrengthScore::Good.is_weak());
        assert!(!PasswordStrengthScore::Strong.is_weak());
    }

    #[test]
    fn test_generated_result_constructors() {
        let success_res = GeneratedPasswordResult::success(
            "Secret123!".to_string(),
            PasswordStrength::new(3, 65.5),
        );
        assert!(success_res.success);
        assert_eq!(success_res.password.as_deref(), Some("Secret123!"));
        assert_eq!(success_res.strength_score, 3);
        assert_eq!(success_res.entropy_bits, 65.5);
        assert_eq!(success_res.error, None);

        let fail_res = GeneratedPasswordResult::failure("something failed");
        assert!(!fail_res.success);
        assert_eq!(fail_res.password, None);
        assert_eq!(fail_res.strength_score, 0);
        assert_eq!(fail_res.entropy_bits, 0.0);
        assert_eq!(fail_res.error.as_deref(), Some("something failed"));
    }

    #[test]
    fn test_property_all_lengths_8_to_128() {
        for len in (8..=128).step_by(7) {
            let opts = PasswordGeneratorOptions::new_password(len);
            let res = generate_password(&opts).expect("generation must succeed for valid length");
            let pass = res.password.expect("password must be present");
            assert_eq!(pass.len(), len as usize);
        }
    }

    #[test]
    fn test_property_all_word_counts_3_to_12() {
        for count in 3..=12 {
            let opts = PasswordGeneratorOptions {
                mode: PasswordGeneratorMode::Passphrase,
                word_count: count,
                separator: "-".to_string(),
                ..Default::default()
            };
            let res =
                generate_password(&opts).expect("generation must succeed for valid word count");
            let pass = res.password.expect("passphrase must be present");
            let parts: Vec<&str> = pass.split('-').collect();
            assert_eq!(parts.len(), count as usize);
        }
    }

    #[test]
    fn test_all_15_character_class_combinations() {
        for mask in 1..16 {
            let inc_lower = (mask & 1) != 0;
            let inc_upper = (mask & 2) != 0;
            let inc_digit = (mask & 4) != 0;
            let inc_sym = (mask & 8) != 0;

            let opts = PasswordGeneratorOptions {
                length: 12,
                include_lowercase: inc_lower,
                include_uppercase: inc_upper,
                include_digits: inc_digit,
                include_symbols: inc_sym,
                avoid_ambiguous: false,
                ..Default::default()
            };

            let res =
                generate_password(&opts).expect("generation must succeed for non-empty classes");
            let pass = res.password.expect("password must be present");

            if inc_lower {
                assert!(pass.chars().any(|c| c.is_ascii_lowercase()));
            } else {
                assert!(!pass.chars().any(|c| c.is_ascii_lowercase()));
            }

            if inc_upper {
                assert!(pass.chars().any(|c| c.is_ascii_uppercase()));
            } else {
                assert!(!pass.chars().any(|c| c.is_ascii_uppercase()));
            }

            if inc_digit {
                assert!(pass.chars().any(|c| c.is_ascii_digit()));
            } else {
                assert!(!pass.chars().any(|c| c.is_ascii_digit()));
            }

            if inc_sym {
                assert!(pass.chars().any(|c| SYMBOLS.contains(&(c as u8))));
            } else {
                assert!(!pass.chars().any(|c| SYMBOLS.contains(&(c as u8))));
            }
        }
    }

    #[test]
    fn test_passphrase_words_belong_to_wordlist() {
        let opts = PasswordGeneratorOptions {
            mode: PasswordGeneratorMode::Passphrase,
            word_count: 6,
            separator: "-".to_string(),
            capitalize: false,
            include_number: false,
            ..Default::default()
        };

        let wordlist = get_wordlist();
        let res = generate_password(&opts).expect("passphrase generation must succeed");
        let pass = res.password.expect("passphrase must be present");
        let parts: Vec<&str> = pass.split('-').collect();

        for part in parts {
            assert!(
                wordlist.contains(&part),
                "word {part} not found in wordlist"
            );
        }
    }
}
