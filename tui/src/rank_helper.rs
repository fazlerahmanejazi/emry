fn rank_cfg(config: &coderet_config::Config) -> RankConfig {
    RankConfig {
        lexical_weight: config.ranking.lexical,
        vector_weight: config.ranking.vector,
        graph_weight: config.ranking.graph,
        symbol_weight: config.ranking.symbol,
        graph_max_depth: config.graph.max_depth,
        graph_decay: config.graph.decay,
        graph_path_weight: config.graph.path_weight,
        bm25_k1: config.bm25.k1,
        bm25_b: config.bm25.b,
        bm25_avg_len: config.bm25.avg_len,
        edge_weights: config.graph.edge_weights.clone(),
        summary_similarity_threshold: 0.25,
        summary_boost_weight: config.ranking.summary,
    }
}
