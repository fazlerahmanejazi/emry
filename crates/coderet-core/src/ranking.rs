use crate::models::Chunk;

#[derive(Debug, Clone)]
pub struct RankedChunk {
    pub score: f32,
    pub lexical_score: Option<f32>,
    pub vector_score: Option<f32>,
    pub graph_boost: Option<f32>,
    pub chunk: Chunk,
}

#[derive(Debug, Clone)]
pub struct RankConfig {
    pub lexical_weight: f32,
    pub vector_weight: f32,
    pub graph_weight: f32,
    pub graph_max_depth: usize,
    pub graph_decay: f32,
    pub graph_path_weight: f32,
    pub symbol_weight: f32,
    pub bm25_k1: f32,
    pub bm25_b: f32,
    pub edge_weights: std::collections::HashMap<String, f32>,
    pub bm25_avg_len: usize,
}

impl Default for RankConfig {
    fn default() -> Self {
        Self {
            lexical_weight: 0.6,
            vector_weight: 0.4,
            graph_weight: 0.1,
            graph_max_depth: 4,
            graph_decay: 0.35,
            graph_path_weight: 1.0,
            symbol_weight: 0.15,
            bm25_k1: 1.2,
            bm25_b: 0.75,
            edge_weights: std::collections::HashMap::new(),
            bm25_avg_len: 50,
        }
    }
}

pub fn rank(hits: Vec<(f32, Chunk)>, lexical_weight: f32) -> Vec<RankedChunk> {
    hits.into_iter()
        .map(|(score, chunk)| RankedChunk {
            score: score * lexical_weight,
            lexical_score: Some(score),
            vector_score: None,
            graph_boost: None,
            chunk,
        })
        .collect()
}
