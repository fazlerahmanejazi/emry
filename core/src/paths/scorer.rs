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
        let unique_files = path.nodes.iter()
            .map(|n| &n.file_path)
            .collect::<HashSet<_>>()
            .len();
        
        if unique_files > 1 {
            score += 0.5 * (unique_files as f32);
        }

        // 3. Node types pattern (heuristic)
        for edge in &path.edges {
            if edge.kind == "DefinedIn" {
                score += 0.2;
            } else if edge.kind == "Calls" {
                score += 0.5; // Calls are more interesting for flow
            }
        }

        score
    }
}
