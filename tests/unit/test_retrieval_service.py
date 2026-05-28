import pytest
import uuid
from unittest.mock import AsyncMock, MagicMock
from src.retrieval.service import RetrievalService, QueryResult
from src.retrieval.semantic import SearchHit, SemanticSearch
from src.retrieval.bm25 import BM25Search
from src.retrieval.expand import L2Chunk
from src.retrieval.reranker import Reranker
from src.retrieval.context import ContextBuilder
from tests.mocks.mock_llm import MockLLMProvider


def make_search_hit(parent_id: uuid.UUID | None = None) -> SearchHit:
    return SearchHit(
        chunk_id=uuid.uuid4(),
        content="test content",
        parent_chunk_id=parent_id,
        doc_id=uuid.uuid4(),
        score=0.9,
    )


def make_l2_chunk() -> L2Chunk:
    return L2Chunk(
        chunk_id=uuid.uuid4(),
        content="L2 content for context",
        doc_id=uuid.uuid4(),
        section_heading="Test Section",
        page_number=1,
        doc_title="Test Doc",
    )


@pytest.mark.asyncio
async def test_service_returns_query_result():
    parent_id = uuid.uuid4()
    hit = make_search_hit(parent_id=parent_id)
    l2 = make_l2_chunk()

    semantic = MagicMock(spec=SemanticSearch)
    semantic.search = AsyncMock(return_value=[hit])

    bm25 = MagicMock(spec=BM25Search)
    bm25.search = AsyncMock(return_value=[hit])

    reranker = MagicMock(spec=Reranker)
    reranker.rerank = AsyncMock(return_value=[l2])

    # Мокаем expand_to_l2 через patch
    import src.retrieval.service as svc_mod
    original_expand = svc_mod.expand_to_l2

    async def mock_expand(session, hits):
        return [l2]

    svc_mod.expand_to_l2 = mock_expand

    try:
        service = RetrievalService(
            semantic_search=semantic,
            bm25_search=bm25,
            reranker=reranker,
            context_builder=ContextBuilder(),
            llm_provider=MockLLMProvider(response="Answer here"),
        )

        session = AsyncMock()
        result = await service.query(session, "test query", embed_fn=AsyncMock(return_value=[0.1]*1536))

        assert isinstance(result, QueryResult)
        assert result.answer == "Answer here"
        assert isinstance(result.sources, list)
    finally:
        svc_mod.expand_to_l2 = original_expand


@pytest.mark.asyncio
async def test_service_calls_embed_fn():
    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    semantic = MagicMock(spec=SemanticSearch)
    semantic.search = AsyncMock(return_value=[])
    bm25 = MagicMock(spec=BM25Search)
    bm25.search = AsyncMock(return_value=[])
    reranker = MagicMock(spec=Reranker)
    reranker.rerank = AsyncMock(return_value=[])

    import src.retrieval.service as svc_mod
    original_expand = svc_mod.expand_to_l2
    svc_mod.expand_to_l2 = AsyncMock(return_value=[])

    try:
        service = RetrievalService(
            semantic_search=semantic,
            bm25_search=bm25,
            reranker=reranker,
            context_builder=ContextBuilder(),
            llm_provider=MockLLMProvider(),
        )
        await service.query(AsyncMock(), "query", embed_fn=embed_fn)
        embed_fn.assert_called_once_with("query")
    finally:
        svc_mod.expand_to_l2 = original_expand
