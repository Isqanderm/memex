from functools import lru_cache

from src.adapters.docx import DocxAdapter
from src.adapters.markdown import MarkdownAdapter
from src.adapters.markitdown_adapter import MarkItDownAdapter
from src.adapters.pdf import PdfAdapter
from src.adapters.registry import AdapterRegistry
from src.adapters.text import TextAdapter
from src.config import get_settings
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.embedding import EmbeddingStage, OpenAIEmbeddingClient
from src.ingestion.indexing import IndexingStage
from src.ingestion.language import LanguageDetector
from src.ingestion.pipeline import IngestionPipeline
from src.llm.factory import create_llm_provider
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
    return OpenAIEmbeddingClient(
        api_key=settings.openai_api_key,
        model=settings.embedding_model,
    )


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
