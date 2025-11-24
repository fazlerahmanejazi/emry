use coderet_config::SummaryLevel;

pub mod generator;
use generator::SummaryGenerator;

pub async fn generate_summary(text: &str, context: &str, model: &str, max_tokens: usize) -> String {
    let generator =
        match SummaryGenerator::new(Some(model.to_string()), max_tokens, "v1".to_string(), 2) {
            Ok(g) => g,
            Err(_) => return format!("{} | (LLM Init Failed)", context),
        };

    match generator.generate(text, context).await {
        Ok(sum) => sum,
        Err(_) => format!("{} | (LLM Gen Failed)", context),
    }
}

pub fn summary_level_for_symbol_kind(kind: &str) -> Option<SummaryLevel> {
    match kind {
        "function" | "method" => Some(SummaryLevel::Function),
        "class" | "interface" | "struct" => Some(SummaryLevel::Class),
        _ => None,
    }
}
