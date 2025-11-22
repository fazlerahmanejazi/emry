use crate::structure::index::SymbolIndex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Features {
    pub bm25_score: f32,
    pub vector_score: f32,
    pub exact_match: f32,
    pub symbol_match: f32,
    // Add more features as needed
}

impl Features {
    pub fn to_vec(&self) -> Vec<f32> {
        vec![
            self.bm25_score,
            self.vector_score,
            self.exact_match,
            self.symbol_match,
        ]
    }
}

pub struct FeatureExtractor<'a> {
    symbol_index: Option<&'a SymbolIndex>,
}

impl<'a> FeatureExtractor<'a> {
    pub fn new(symbol_index: Option<&'a SymbolIndex>) -> Self {
        Self { symbol_index }
    }

    pub fn extract(
        &self,
        query: &str,
        content: &str,
        bm25_score: f32,
        vector_score: f32,
    ) -> Features {
        let exact_match = if content.contains(query) { 1.0 } else { 0.0 };
        
        let mut symbol_match = 0.0;
        if let Some(index) = self.symbol_index {
            // Check if query matches any symbol name exactly
            if !index.search(query).is_empty() {
                // If query is a symbol, check if this content *contains* that symbol definition
                // This is a bit weak. Ideally we check if this chunk *is* the definition.
                // For now, simple check: if query is a known symbol, boost relevant chunks.
                // But wait, we want to know if *this chunk* is relevant to the symbol.
                // If the chunk contains the symbol name, it's likely relevant.
                if content.contains(query) {
                    symbol_match = 1.0;
                }
            }
        }

        Features {
            bm25_score,
            vector_score,
            exact_match,
            symbol_match,
        }
    }
}
