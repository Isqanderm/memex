import uuid
from src.retrieval.semantic import SearchHit


def rrf_merge(
    semantic_hits: list[SearchHit],
    bm25_hits: list[SearchHit],
    k: int = 60,
    top_n: int = 20,
) -> list[SearchHit]:
    """Reciprocal Rank Fusion — объединяет два ranked списка."""
    scores: dict[uuid.UUID, float] = {}
    hit_map: dict[uuid.UUID, SearchHit] = {}

    for rank, hit in enumerate(semantic_hits, start=1):
        scores[hit.chunk_id] = scores.get(hit.chunk_id, 0) + 1.0 / (rank + k)
        hit_map[hit.chunk_id] = hit

    for rank, hit in enumerate(bm25_hits, start=1):
        scores[hit.chunk_id] = scores.get(hit.chunk_id, 0) + 1.0 / (rank + k)
        if hit.chunk_id not in hit_map:
            hit_map[hit.chunk_id] = hit

    ranked = sorted(scores.items(), key=lambda x: x[1], reverse=True)
    return [hit_map[chunk_id] for chunk_id, _ in ranked[:top_n]]
