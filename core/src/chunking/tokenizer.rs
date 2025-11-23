use once_cell::sync::Lazy;
use tiktoken_rs::{cl100k_base, CoreBPE};

static TOKENIZER: Lazy<CoreBPE> = Lazy::new(|| cl100k_base().expect("Failed to load tokenizer"));

/// Count exact tokens using tiktoken (GPT-compatible)
pub fn count_tokens(text: &str) -> usize {
    TOKENIZER.encode_with_special_tokens(text).len()
}

/// Fast token estimation using character count
/// Useful for quick checks before expensive tokenization
pub fn estimate_tokens(text: &str) -> usize {
    // Average for code: ~4 characters per token
    (text.len() + 3) / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens() {
        let text = "fn hello() { println!(\"world\"); }";
        let count = count_tokens(text);
        assert!(count > 0);
        assert!(count < 20); // Should be around 10-15 tokens
    }

    #[test]
    fn test_estimate_tokens() {
        let text = "a".repeat(400);
        let estimate = estimate_tokens(&text);
        assert_eq!(estimate, 100); // 400 / 4 = 100
    }
}
