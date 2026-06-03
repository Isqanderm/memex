import asyncio
import logging
import uuid
from collections.abc import Callable
from typing import TYPE_CHECKING

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from src.db.repositories.job_repo import JobRepository
from src.ingestion.pipeline import IngestionPipeline

if TYPE_CHECKING:
    from src.memory.service import MemoryService

logger = logging.getLogger(__name__)


class IngestionWorker:
    def __init__(
        self,
        session_factory: async_sessionmaker[AsyncSession],
        pipeline: IngestionPipeline,
        memory_service_factory: Callable[[AsyncSession], "MemoryService"] | None = None,
    ):
        self.session_factory = session_factory
        self.pipeline = pipeline
        self.memory_service_factory = memory_service_factory
        self._running = False

    async def start(self) -> None:
        self._running = True
        logger.info("IngestionWorker started")
        while self._running:
            try:
                processed = await self._process_one()
                if not processed:
                    await asyncio.sleep(1)
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.exception(f"Worker loop error: {e}")
                await asyncio.sleep(1)

    def stop(self) -> None:
        self._running = False

    async def _extract_memory(self, session: AsyncSession, doc_id: uuid.UUID) -> None:
        from sqlalchemy import select

        from src.db.models import Chunk
        from src.memory.worker import queue_document_extraction, run_document_extraction

        result = await session.execute(
            select(Chunk.content)
            .where(Chunk.doc_id == doc_id, Chunk.chunk_role == "leaf")
            .order_by(Chunk.chunk_index)
        )
        doc_text = "\n\n".join(row[0] for row in result.all())
        if not doc_text.strip():
            return

        if self.memory_service_factory is None:
            return

        mem_job = await queue_document_extraction(session, str(doc_id))
        memory_service = self.memory_service_factory(session)
        await run_document_extraction(session, mem_job, doc_text, memory_service)
        logger.info(f"Memory extraction done for doc {doc_id}: {mem_job.facts_extracted} facts")

    async def _process_one(self) -> bool:
        async with self.session_factory() as session:
            async with session.begin():
                repo = JobRepository(session)
                job = await repo.claim_next()
                if not job:
                    return False

                try:
                    doc_id = await self.pipeline.process(session, job.source, job.checksum)
                    await repo.mark_done(job.id, doc_id)
                    if self.memory_service_factory:
                        await self._extract_memory(session, doc_id)
                    logger.info(f"Job {job.id} done → doc {doc_id}")
                    return True
                except Exception as e:
                    await repo.mark_error(job.id, str(e))
                    logger.exception(f"Job {job.id} failed: {e}")
                    return True
