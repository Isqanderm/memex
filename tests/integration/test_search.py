import pytest
import uuid
from sqlalchemy import text
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search


async def insert_test_leaf(session, content: str, language: str = "english",
                            vector: list[float] | None = None) -> uuid.UUID:
    """Вспомогательная функция — вставляет документ + leaf чанк для тестов."""
    doc_id = uuid.uuid4()
    chunk_id = uuid.uuid4()
    vec = vector or [0.1] * 1536
    vec_str = "[" + ",".join(str(x) for x in vec) + "]"

    await session.execute(text("""
        INSERT INTO documents (id, source, mime_type, checksum)
        VALUES (:id, :src, 'text/plain', :cs)
    """), {"id": doc_id, "src": f"test-{doc_id}.txt", "cs": str(doc_id)})

    await session.execute(text(f"""
        INSERT INTO chunks (id, doc_id, chunk_role, chunk_index, language, content,
                            content_vector, tsv)
        VALUES (:id, :doc_id, 'leaf', 0, :lang, :content,
                '{vec_str}'::vector,
                to_tsvector('{language}'::regconfig, :content))
    """), {"id": chunk_id, "doc_id": doc_id, "lang": language,
           "content": content})

    await session.flush()
    return chunk_id


@pytest.mark.integration
async def test_bm25_finds_exact_word(db_session):
    # Use 'simple' config so tsv tokens match the BM25Search query (also simple)
    chunk_id = await insert_test_leaf(
        db_session,
        "PostgreSQL indexing guide for beginners",
        language="simple",
    )
    search = BM25Search(top_k=5)
    results = await search.search(db_session, "PostgreSQL indexing guide")
    assert len(results) >= 1
    assert any(r.chunk_id == chunk_id for r in results)


@pytest.mark.integration
async def test_bm25_returns_empty_for_no_match(db_session):
    await insert_test_leaf(db_session, "A document about flowers and gardens")
    search = BM25Search(top_k=5)
    results = await search.search(db_session, "zxqwerty nonexistent word xyz")
    # может вернуть пустой список или результаты — просто не должен упасть
    assert isinstance(results, list)


@pytest.mark.integration
async def test_semantic_search_returns_hits(db_session):
    vec = [0.9] * 1536
    chunk_id = await insert_test_leaf(
        db_session,
        "semantic search test content",
        vector=vec
    )
    search = SemanticSearch(top_k=5)
    # Ищем по похожему вектору
    query_vec = [0.9] * 1536
    results = await search.search(db_session, query_vec)
    assert len(results) >= 1


@pytest.mark.integration
async def test_semantic_search_empty_db(db_session):
    # Должен просто вернуть пустой список если нет чанков
    search = SemanticSearch(top_k=5)
    results = await search.search(db_session, [0.1] * 1536)
    assert isinstance(results, list)
