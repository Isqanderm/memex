from functools import lru_cache
from typing import cast

from sqlalchemy.ext.asyncio import AsyncSession

from src.adapters.docx import DocxAdapter
from src.adapters.markdown import MarkdownAdapter
from src.adapters.markitdown_adapter import MarkItDownAdapter
from src.adapters.pdf import PdfAdapter
from src.adapters.registry import AdapterRegistry
from src.adapters.text import TextAdapter
from src.config import get_settings
from src.db.repositories.memory_repo import MemoryRepository
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.embedding import EmbeddingStage, LocalEmbeddingClient
from src.ingestion.indexing import IndexingStage
from src.ingestion.language import LanguageDetector
from src.ingestion.pipeline import IngestionPipeline
from src.llm.factory import create_llm_provider
from src.memory.extractor import FactExtractor
from src.memory.profile import ProfileService
from src.memory.service import MemoryService
from src.retrieval.bm25 import BM25Search
from src.retrieval.context import ContextBuilder
from src.retrieval.reranker import Reranker
from src.retrieval.semantic import SemanticSearch
from src.retrieval.service import RetrievalService


@lru_cache
def get_adapter_registry() -> AdapterRegistry:
    registry = AdapterRegistry()
    registry.register(PdfAdapter())
    registry.register(DocxAdapter())
    registry.register(MarkdownAdapter())
    registry.register(TextAdapter())
    registry.register(MarkItDownAdapter())
    return registry


@lru_cache
def get_embedding_client():
    settings = get_settings()
    return LocalEmbeddingClient(model=settings.local_embedding_model)


@lru_cache
def get_ingestion_pipeline() -> IngestionPipeline:
    settings = get_settings()
    return IngestionPipeline(
        adapter_registry=get_adapter_registry(),
        chunker=SmallToBigChunker(
            l2_size=settings.l2_chunk_size,
            l1_size=settings.l1_chunk_size,
            l2_overlap=settings.l2_chunk_overlap,
        ),
        embedding_stage=EmbeddingStage(client=get_embedding_client()),
        indexing_stage=IndexingStage(),
        language_detector=LanguageDetector(),
    )


@lru_cache
def get_retrieval_service() -> RetrievalService:
    settings = get_settings()
    return RetrievalService(
        semantic_search=SemanticSearch(top_k=settings.semantic_top_k),
        bm25_search=BM25Search(top_k=settings.bm25_top_k),
        reranker=Reranker(),
        context_builder=ContextBuilder(),
        llm_provider=create_llm_provider(settings),
        rrf_k=settings.rrf_k,
        reranker_top_n=settings.reranker_top_n,
    )


@lru_cache
def get_memory_extractor() -> FactExtractor:
    settings = get_settings()
    return FactExtractor(create_llm_provider(settings))


@lru_cache
def get_profile_service_instance() -> ProfileService:
    settings = get_settings()
    return ProfileService(llm_provider=create_llm_provider(settings))


def get_memory_service(session: AsyncSession) -> MemoryService:
    embedding_client = get_embedding_client()

    async def embed_fn(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text])
        return cast(list[float], results[0])

    return MemoryService(
        repo=MemoryRepository(session),
        extractor=get_memory_extractor(),
        embed_fn=embed_fn,
    )
