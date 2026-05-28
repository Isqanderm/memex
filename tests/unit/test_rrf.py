import uuid
from src.retrieval.rrf import rrf_merge
from src.retrieval.semantic import SearchHit


def make_hit(uid: str, score: float = 0.5, parent_id: str | None = None) -> SearchHit:
    return SearchHit(
        chunk_id=uuid.UUID(uid),
        content="text",
        parent_chunk_id=uuid.UUID(parent_id) if parent_id else None,
        doc_id=uuid.uuid4(),
        score=score,
    )

ID1 = "00000000-0000-0000-0000-000000000001"
ID2 = "00000000-0000-0000-0000-000000000002"
ID3 = "00000000-0000-0000-0000-000000000003"


def test_shared_chunk_ranks_first():
    semantic = [make_hit(ID1), make_hit(ID2)]
    bm25 = [make_hit(ID1), make_hit(ID3)]
    result = rrf_merge(semantic, bm25, k=60)
    assert result[0].chunk_id == uuid.UUID(ID1)


def test_deduplicates_chunks():
    hits = [make_hit(ID1), make_hit(ID1)]
    result = rrf_merge(hits, hits, k=60)
    ids = [r.chunk_id for r in result]
    assert len(ids) == len(set(ids))


def test_empty_lists():
    result = rrf_merge([], [], k=60)
    assert result == []


def test_one_empty_list():
    semantic = [make_hit(ID1), make_hit(ID2)]
    result = rrf_merge(semantic, [], k=60)
    assert len(result) == 2


def test_top_n_limit():
    hits = [make_hit(f"0000000{i}-0000-0000-0000-000000000001"[:36]) for i in range(1, 6)]
    result = rrf_merge(hits, hits, k=60, top_n=3)
    assert len(result) <= 3
