use crate::paths::Path;
use std::collections::HashSet;

pub struct PathScorer;

impl PathScorer {
    pub fn score(path: &Path) -> f32 {
        let mut score = 0.0;

        // 1. Length penalty/reward
        // Prefer paths of length 2-4.
        let len = path.edges.len();
        if len >= 1 && len <= 4 {
            score += 1.0;
        } else {
            score += 0.5;
        }

        // 2. Diversity (number of unique files)
        let unique_files = path
            .nodes
            .iter()
            .map(|n| &n.file_path)
            .collect::<HashSet<_>>()
            .len();

        if unique_files > 1 {
            score += 0.5 * (unique_files as f32);
        }

        // 3. Node types pattern (heuristic)
        for edge in &path.edges {
            score += Self::edge_weight(&edge.kind);
        }

        score
    }

    pub fn edge_weight(kind: &crate::structure::graph::EdgeType) -> f32 {
        match kind {
            crate::structure::graph::EdgeType::Calls => 1.0,
            crate::structure::graph::EdgeType::Imports => 0.6,
            crate::structure::graph::EdgeType::MemberOf => 0.3,
            crate::structure::graph::EdgeType::DefinedIn => 0.2,
            crate::structure::graph::EdgeType::DataFlow => 0.7,
        }
    }

    /// Adjusts the score based on semantic coherence (cosine similarity between adjacent nodes).
    /// `embeddings` is a map from NodeId to its vector embedding.
    pub fn score_semantic(
        path: &mut Path,
        embeddings: &std::collections::HashMap<String, Vec<f32>>,
    ) {
        let mut coherence_score = 0.0;
        let mut pairs = 0;

        for window in path.nodes.windows(2) {
            let node_a = &window[0];
            let node_b = &window[1];

            if let (Some(vec_a), Some(vec_b)) = (
                embeddings.get(&node_a.node_id),
                embeddings.get(&node_b.node_id),
            ) {
                let sim = cosine_similarity(vec_a, vec_b);
                coherence_score += sim;
                pairs += 1;
            }
        }

        if pairs > 0 {
            // Normalize and add to total score
            let avg_coherence = coherence_score / (pairs as f32);
            // Weight semantic coherence heavily (e.g., +2.0 for perfect coherence)
            path.score += avg_coherence * 2.0;
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}
