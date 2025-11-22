use crate::ranking::features::Features;

pub trait Ranker {
    fn score(&self, features: &Features) -> f32;
}

pub struct LinearRanker {
    weights: Vec<f32>,
}

impl LinearRanker {
    pub fn new(weights: Vec<f32>) -> Self {
        Self { weights }
    }

    pub fn default() -> Self {
        // Default weights: BM25=0.4, Vector=0.4, Exact=0.1, Symbol=0.1
        Self {
            weights: vec![0.4, 0.4, 0.1, 0.1],
        }
    }
}

impl Ranker for LinearRanker {
    fn score(&self, features: &Features) -> f32 {
        let feats = features.to_vec();
        let mut score = 0.0;
        for (i, w) in self.weights.iter().enumerate() {
            if i < feats.len() {
                score += w * feats[i];
            }
        }
        score
    }
}
