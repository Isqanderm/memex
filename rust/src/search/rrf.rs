/// A ranked search result from semantic or BM25 retrieval.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub chunk_id: String,
    pub content: String,
    pub parent_chunk_id: Option<String>,
    pub doc_id: String,
    pub score: f32,
    pub section_heading: Option<String>,
    pub page_number: Option<u32>,
}

/// Reciprocal Rank Fusion — merges two ranked lists into a single ranked list.
///
/// Formula: score(d) = Σ_list 1 / (rank + k)
/// where `rank` is 1-based position in each list.
///
/// `k` is typically 60 (smoothing constant).
/// Returns at most `top_n` results sorted by descending RRF score.
pub fn rrf_merge(
    semantic_hits: &[SearchHit],
    bm25_hits: &[SearchHit],
    k: usize,
    top_n: usize,
) -> Vec<SearchHit> {
    use std::collections::HashMap;

    if semantic_hits.is_empty() && bm25_hits.is_empty() {
        return vec![];
    }

    // Map chunk_id → accumulated RRF score
    let mut scores: HashMap<&str, f32> = HashMap::new();

    for (rank, hit) in semantic_hits.iter().enumerate() {
        let rrf = 1.0 / ((rank + 1 + k) as f32);
        *scores.entry(hit.chunk_id.as_str()).or_insert(0.0) += rrf;
    }

    for (rank, hit) in bm25_hits.iter().enumerate() {
        let rrf = 1.0 / ((rank + 1 + k) as f32);
        *scores.entry(hit.chunk_id.as_str()).or_insert(0.0) += rrf;
    }

    // Collect unique hits (prefer the copy from semantic_hits first, then bm25)
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut merged: Vec<SearchHit> = Vec::new();

    for hit in semantic_hits.iter().chain(bm25_hits.iter()) {
        if seen.insert(hit.chunk_id.as_str()) {
            let mut h = hit.clone();
            h.score = *scores.get(hit.chunk_id.as_str()).unwrap_or(&0.0);
            merged.push(h);
        }
    }

    // Sort by descending RRF score
    merged.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    if top_n > 0 && merged.len() > top_n {
        merged.truncate(top_n);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hit(chunk_id: &str) -> SearchHit {
        SearchHit {
            chunk_id: chunk_id.to_string(),
            content: format!("content of {chunk_id}"),
            parent_chunk_id: None,
            doc_id: "doc-1".to_string(),
            score: 0.0,
            section_heading: None,
            page_number: None,
        }
    }

    #[test]
    fn rrf_merges_two_lists() {
        // "b" appears in both lists — should rank first due to double contribution
        let semantic = vec![make_hit("a"), make_hit("b")];
        let bm25 = vec![make_hit("b"), make_hit("c")];

        let result = rrf_merge(&semantic, &bm25, 60, 10);

        // "b" should be first (appears in both lists)
        assert_eq!(result[0].chunk_id, "b", "b should rank first as it appears in both lists");

        // All 3 unique chunk IDs should appear in result
        let ids: Vec<&str> = result.iter().map(|h| h.chunk_id.as_str()).collect();
        assert!(ids.contains(&"a"), "a should appear in merged result");
        assert!(ids.contains(&"b"), "b should appear in merged result");
        assert!(ids.contains(&"c"), "c should appear in merged result");
        assert_eq!(result.len(), 3, "exactly 3 unique chunks");
    }

    #[test]
    fn rrf_empty_inputs() {
        assert!(rrf_merge(&[], &[], 60, 10).is_empty());
    }

    #[test]
    fn rrf_top_n_limits_output() {
        let semantic: Vec<SearchHit> = (0..5).map(|i| make_hit(&format!("chunk-{i}"))).collect();
        let result = rrf_merge(&semantic, &[], 60, 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn rrf_one_empty_list() {
        let semantic = vec![make_hit("x"), make_hit("y")];
        let result = rrf_merge(&semantic, &[], 60, 10);
        assert_eq!(result.len(), 2);
    }
}
