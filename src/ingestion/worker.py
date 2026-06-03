import asyncio
import logging

from sqlalchemy.ext.asyncio import async_sessionmaker

from src.db.repositories.job_repo import JobRepository
from src.ingestion.pipeline import IngestionPipeline

logger = logging.getLogger(__name__)


class IngestionWorker:
    def __init__(self, session_factory: async_sessionmaker, pipeline: IngestionPipeline):
        self.session_factory = session_factory
        self.pipeline = pipeline
        self._running = False

    async def start(self):
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

    def stop(self):
        self._running = False

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
                    logger.info(f"Job {job.id} done → doc {doc_id}")
                    return True
                except Exception as e:
                    await repo.mark_error(job.id, str(e))
                    logger.exception(f"Job {job.id} failed: {e}")
                    return True
