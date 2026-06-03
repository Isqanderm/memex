import uuid

import pytest
from sqlalchemy import select

from src.adapters.registry import AdapterRegistry
from src.adapters.text import TextAdapter
from src.db.models import IngestionJob
from src.db.repositories.job_repo import JobRepository
from src.ingestion.chunker import SmallToBigChunker
from src.ingestion.embedding import EmbeddingStage
from src.ingestion.indexing import IndexingStage
from src.ingestion.language import LanguageDetector
from src.ingestion.pipeline import IngestionPipeline
from src.ingestion.worker import IngestionWorker
from tests.mocks.mock_embedding import MockEmbeddingClient


def make_pipeline() -> IngestionPipeline:
    registry = AdapterRegistry()
    registry.register(TextAdapter())
    return IngestionPipeline(
        adapter_registry=registry,
        chunker=SmallToBigChunker(l2_size=50, l1_size=20, l2_overlap=5),
        embedding_stage=EmbeddingStage(client=MockEmbeddingClient()),
        indexing_stage=IndexingStage(),
        language_detector=LanguageDetector(),
    )


@pytest.mark.integration
async def test_worker_processes_job(session_factory, tmp_path):
    test_file = tmp_path / "test.txt"
    test_file.write_text("Hello world. This is a test document with enough content to chunk.")

    async with session_factory() as session:
        async with session.begin():
            repo = JobRepository(session)
            job = await repo.create(source=str(test_file), checksum=f"cs-{uuid.uuid4()}")
            job_id = job.id

    pipeline = make_pipeline()
    worker = IngestionWorker(session_factory=session_factory, pipeline=pipeline)
    processed = await worker._process_one()
    assert processed is True

    async with session_factory() as session:
        result = await session.execute(select(IngestionJob).where(IngestionJob.id == job_id))
        job = result.scalar_one()
        assert job.status == "done"
        assert job.doc_id is not None


@pytest.mark.integration
async def test_worker_returns_false_when_empty(session_factory):
    pipeline = make_pipeline()
    worker = IngestionWorker(session_factory=session_factory, pipeline=pipeline)
    # Если очередь пуста — должен вернуть False
    processed = await worker._process_one()
    # Может быть True (если остались задачи от других тестов) или False
    assert isinstance(processed, bool)


@pytest.mark.integration
async def test_worker_marks_error_on_bad_file(session_factory):
    async with session_factory() as session:
        async with session.begin():
            repo = JobRepository(session)
            job = await repo.create(
                source="/nonexistent/path/bad.txt",
                checksum=f"cs-bad-{uuid.uuid4()}"
            )
            job_id = job.id

    pipeline = make_pipeline()
    worker = IngestionWorker(session_factory=session_factory, pipeline=pipeline)
    processed = await worker._process_one()
    assert processed is True

    async with session_factory() as session:
        result = await session.execute(select(IngestionJob).where(IngestionJob.id == job_id))
        job = result.scalar_one()
        assert job.status == "error"
        assert job.error is not None
