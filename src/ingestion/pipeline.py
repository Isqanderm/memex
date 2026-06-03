import mimetypes
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession

from src.adapters.protocol import Source
from src.adapters.registry import AdapterRegistry
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.embedding import EmbeddingStage
from src.ingestion.indexing import IndexingStage
from src.ingestion.language import LanguageDetector


class IngestionPipeline:
    def __init__(
        self,
        adapter_registry: AdapterRegistry,
        chunker: SmallToBigChunker,
        embedding_stage: EmbeddingStage,
        indexing_stage: IndexingStage,
        language_detector: LanguageDetector,
    ):
        self.adapter_registry = adapter_registry
        self.chunker = chunker
        self.embedding_stage = embedding_stage
        self.indexing_stage = indexing_stage
        self.language_detector = language_detector

    async def process(self, session: AsyncSession, source_path: str, checksum: str):
        mime_type, _ = mimetypes.guess_type(source_path)
        mime_type = mime_type or "application/octet-stream"

        source = Source(
            path=source_path,
            mime_type=mime_type,
            filename=Path(source_path).name,
        )

        parsed = self.adapter_registry.parse(source)
        chunks = self.chunker.chunk(parsed)

        for chunk in chunks:
            chunk.language = self.language_detector.detect(chunk.content[:200])

        chunks = await self.embedding_stage.process(chunks)
        doc_id = await self.indexing_stage.index(session, parsed, chunks, checksum)
        return doc_id
