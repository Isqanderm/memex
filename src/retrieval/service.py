import asyncio
from dataclasses import dataclass, field
from typing import AsyncIterator
from sqlalchemy.ext.asyncio import AsyncSession
from src.retrieval.semantic import SemanticSearch
from src.retrieval.bm25 import BM25Search
from src.retrieval.rrf import rrf_merge
from src.retrieval.expand import expand_to_l2
from src.retrieval.reranker import Reranker
from src.retrieval.context import ContextBuilder
from src.llm.protocol import LLMProvider
from src.retrieval.memory_search import MemorySearch, MemoryHit
from src.profiling import StepTimer


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
        memory_search: "MemorySearch | None" = None,
    ):
        self.semantic_search = semantic_search
        self.bm25_search = bm25_search
        self.reranker = reranker
        self.context_builder = context_builder
        self.llm_provider = llm_provider
        self.rrf_k = rrf_k
        self.reranker_top_n = reranker_top_n
        self.memory_search = memory_search

    async def query(
        self,
        session: AsyncSession,
        query: str,
        embed_fn,
        memory_search: "MemorySearch | None" = None,
    ) -> QueryResult:
        t = StepTimer("query")

        with t.step("embed"):
            query_vector = await embed_fn(query)

        with t.step("semantic"):
            semantic_hits = await self.semantic_search.search(session, query_vector)
        with t.step("bm25"):
            bm25_hits = await self.bm25_search.search(session, query)
        with t.step("expand"):
            merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
            l2_chunks = await expand_to_l2(session, merged)
        with t.step("rerank"):
            reranked = await self.reranker.rerank(query, l2_chunks, top_n=self.reranker_top_n)

        effective_memory_search = memory_search or self.memory_search
        mem_hits = []
        if effective_memory_search:
            with t.step("memory"):
                mem_hits = await effective_memory_search.search(session, query_vector)

        ctx = self.context_builder.build(query, reranked, memory_hits=mem_hits)

        with t.step("llm"):
            llm_response = await self.llm_provider.complete(ctx.prompt)

        t.log()
        return QueryResult(
            answer=llm_response.answer,
            sources=ctx.sources,
            input_tokens=llm_response.input_tokens,
            output_tokens=llm_response.output_tokens,
        )

    async def search_chunks(
        self,
        session: AsyncSession,
        query: str,
        embed_fn,
        top_k: int = 5,
    ) -> list[dict]:
        from pathlib import Path
        query_vector = await embed_fn(query)
        semantic_hits = await self.semantic_search.search(session, query_vector)
        bm25_hits = await self.bm25_search.search(session, query)
        merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
        l2_chunks = await expand_to_l2(session, merged)
        reranked = await self.reranker.rerank(query, l2_chunks, top_n=top_k)
        return [
            {
                "text": c.content,
                "doc_id": str(c.doc_id),
                "title": c.doc_title,
                "filename": Path(c.doc_source).name.split("-", 5)[-1] if c.doc_source else None,
                "section": c.section_heading,
                "page": c.page_number,
            }
            for c in reranked
        ]

    async def query_stream(
        self,
        session: AsyncSession,
        query: str,
        embed_fn,
        memory_search: "MemorySearch | None" = None,
    ) -> AsyncIterator[dict]:
        t = StepTimer("stream")

        with t.step("embed"):
            query_vector = await embed_fn(query)
        with t.step("semantic"):
            semantic_hits = await self.semantic_search.search(session, query_vector)
        with t.step("bm25"):
            bm25_hits = await self.bm25_search.search(session, query)
        with t.step("expand"):
            merged = rrf_merge(semantic_hits, bm25_hits, k=self.rrf_k)
            l2_chunks = await expand_to_l2(session, merged)
        with t.step("rerank"):
            reranked = await self.reranker.rerank(query, l2_chunks, top_n=self.reranker_top_n)

        effective_memory_search = memory_search or self.memory_search
        mem_hits = []
        if effective_memory_search:
            with t.step("memory"):
                mem_hits = await effective_memory_search.search(session, query_vector)

        ctx = self.context_builder.build(query, reranked, memory_hits=mem_hits)
        t.log()  # log before streaming starts

        async for token in self.llm_provider.complete_stream(ctx.prompt):
            yield {"type": "token", "data": token}

        yield {"type": "sources", "data": ctx.sources}
        yield {"type": "done"}
