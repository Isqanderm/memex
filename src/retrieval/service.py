import asyncio
from dataclasses import dataclass, field
from sqlalchemy.ext.asyncio import AsyncSession
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search
from src.retrieval.rrf import rrf_merge
from src.retrieval.expand import expand_to_l2
from src.retrieval.reranker import Reranker
from src.retrieval.context import ContextBuilder
from src.llm.protocol import LLMProvider


@dataclass
class QueryResult:
    answer: str
    sources: list[dict] = field(default_factory=list)
    input_tokens: int = 0
    output_tokens: int = 0


class RetrievalService:
    def __init__(
        self,
        semantic_search: SemanticSearch,
        bm25_search: BM25Search,
        reranker: Reranker,
        context_builder: ContextBuilder,
        llm_provider: LLMProvider,
        rrf_k: int = 60,
        reranker_top_n: int = 5,
    ):
        self.semantic_search = semantic_search
        self.bm25_search = bm25_search
        self.reranker = reranker
        self.context_builder = context_builder
        self.llm_provider = llm_provider
        self.rrf_k = rrf_k
        self.reranker_top_n = reranker_top_n

    async def query(
        self,
        session: AsyncSession,
        query: str,
        embed_fn,
    ) -> QueryResult:
        query_vector = await embed_fn(query)

        semantic_hits, bm25_hits = await asyncio.gather(
            self.semantic_search.search(session, query_vector),
            self.bm25_search.search(session, query),
        )

        merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
        l2_chunks = await expand_to_l2(session, merged)
        reranked = await self.reranker.rerank(query, l2_chunks, top_n=self.reranker_top_n)

        ctx = self.context_builder.build(query, reranked)
        llm_response = await self.llm_provider.complete(ctx.prompt)

        return QueryResult(
            answer=llm_response.answer,
            sources=ctx.sources,
            input_tokens=llm_response.input_tokens,
            output_tokens=llm_response.output_tokens,
        )
