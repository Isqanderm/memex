import uuid
import pytest
from unittest.mock import AsyncMock, MagicMock
from src.memory.service import MemoryService, RememberResult
from src.memory.extractor import ExtractedFact, RelationResult, FactExtractor
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


def make_memory(content="User works at Acme"):
    m = Memory()
    m.id = uuid.uuid4()
    m.content = content
    m.raw_input = content
    m.source = "explicit"
    m.is_active = True
    m.content_vector = [0.1] * 1536
    m.forget_after = None
    m.relation = None
    m.parent_id = None
    return m


@pytest.mark.asyncio
async def test_remember_creates_new_fact_when_no_similar():
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[])
    repo.create = AsyncMock(return_value=make_memory())

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(return_value=[ExtractedFact(content="User works at Acme")])
    extractor.resolve_relations = AsyncMock(return_value=[])

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    result = await service.remember(AsyncMock(), "I work at Acme")

    assert isinstance(result, RememberResult)
    assert result.facts_extracted == 1
    assert result.memories_updated == 0
    repo.create.assert_called_once()


@pytest.mark.asyncio
async def test_remember_deactivates_old_on_updates():
    old_mem = make_memory("User works at Acme")
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[(old_mem, 0.92)])
    repo.deactivate = AsyncMock()
    repo.create = AsyncMock(return_value=make_memory("User works at Beta"))

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(return_value=[ExtractedFact(content="User works at Beta")])
    extractor.resolve_relations = AsyncMock(
        return_value=[RelationResult(memory_id=old_mem.id, relation="updates")]
    )

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    result = await service.remember(AsyncMock(), "I now work at Beta")

    repo.deactivate.assert_called_once_with(old_mem.id)
    assert result.memories_updated == 1


@pytest.mark.asyncio
async def test_remember_extends_does_not_deactivate():
    old_mem = make_memory("User works at Acme")
    repo = MagicMock(spec=MemoryRepository)
    repo.get_active_by_vector = AsyncMock(return_value=[(old_mem, 0.90)])
    repo.deactivate = AsyncMock()
    repo.create = AsyncMock(return_value=make_memory())

    extractor = MagicMock(spec=FactExtractor)
    extractor.extract_facts = AsyncMock(
        return_value=[ExtractedFact(content="User is a senior engineer at Acme")]
    )
    extractor.resolve_relations = AsyncMock(
        return_value=[RelationResult(memory_id=old_mem.id, relation="extends")]
    )

    embed_fn = AsyncMock(return_value=[0.1] * 1536)

    service = MemoryService(repo=repo, extractor=extractor, embed_fn=embed_fn)
    await service.remember(AsyncMock(), "I'm a senior engineer at Acme")

    repo.deactivate.assert_not_called()


@pytest.mark.asyncio
async def test_memory_worker_queues_job_after_doc_indexing():
    from src.memory.worker import queue_document_extraction
    session = AsyncMock()
    session.add = MagicMock()
    session.flush = AsyncMock()
    doc_id = uuid.uuid4()
    await queue_document_extraction(session, str(doc_id))
    session.add.assert_called_once()
    session.flush.assert_called_once()


@pytest.mark.asyncio
async def test_ingestion_worker_calls_memory_factory_on_success():
    """IngestionWorker calls memory_service_factory after successful indexing."""
    from src.ingestion.worker import IngestionWorker
    from sqlalchemy.ext.asyncio import async_sessionmaker

    doc_id = uuid.uuid4()
    pipeline = MagicMock()
    pipeline.process = AsyncMock(return_value=doc_id)

    memory_service = MagicMock()
    memory_service_factory = MagicMock(return_value=memory_service)

    session = AsyncMock()
    # Simulate execute() returning chunks with content
    chunk_row = MagicMock()
    chunk_row.__iter__ = MagicMock(return_value=iter([("Some document text",)]))
    result_mock = MagicMock()
    result_mock.all.return_value = [("Some document text",)]
    session.execute = AsyncMock(return_value=result_mock)
    session.add = MagicMock()
    session.flush = AsyncMock()

    job_repo = MagicMock()
    job = MagicMock()
    job.id = uuid.uuid4()
    job.source = "/tmp/test.txt"
    job.checksum = "abc123"
    job_repo.claim_next = AsyncMock(return_value=job)
    job_repo.mark_done = AsyncMock()

    session_cm = MagicMock()
    session_cm.__aenter__ = AsyncMock(return_value=session)
    session_cm.__aexit__ = AsyncMock(return_value=False)
    begin_cm = MagicMock()
    begin_cm.__aenter__ = AsyncMock(return_value=None)
    begin_cm.__aexit__ = AsyncMock(return_value=False)
    session.begin = MagicMock(return_value=begin_cm)

    import src.ingestion.worker as worker_mod
    original_repo = worker_mod.JobRepository
    worker_mod.JobRepository = MagicMock(return_value=job_repo)

    session_factory = MagicMock(return_value=session_cm)

    try:
        worker = IngestionWorker(
            session_factory=session_factory,
            pipeline=pipeline,
            memory_service_factory=memory_service_factory,
        )
        await worker._process_one()
        memory_service_factory.assert_called_once_with(session)
    finally:
        worker_mod.JobRepository = original_repo


@pytest.mark.asyncio
async def test_ingestion_worker_skips_memory_when_no_factory():
    """IngestionWorker with no memory_service_factory doesn't fail."""
    from src.ingestion.worker import IngestionWorker

    doc_id = uuid.uuid4()
    pipeline = MagicMock()
    pipeline.process = AsyncMock(return_value=doc_id)

    session = AsyncMock()
    job_repo = MagicMock()
    job = MagicMock()
    job.id = uuid.uuid4()
    job.source = "/tmp/test.txt"
    job.checksum = "abc123"
    job_repo.claim_next = AsyncMock(return_value=job)
    job_repo.mark_done = AsyncMock()

    session_cm = MagicMock()
    session_cm.__aenter__ = AsyncMock(return_value=session)
    session_cm.__aexit__ = AsyncMock(return_value=False)
    begin_cm = MagicMock()
    begin_cm.__aenter__ = AsyncMock(return_value=None)
    begin_cm.__aexit__ = AsyncMock(return_value=False)
    session.begin = MagicMock(return_value=begin_cm)
    session_factory = MagicMock(return_value=session_cm)

    import src.ingestion.worker as worker_mod
    original_repo = worker_mod.JobRepository
    worker_mod.JobRepository = MagicMock(return_value=job_repo)

    try:
        worker = IngestionWorker(
            session_factory=session_factory,
            pipeline=pipeline,
            memory_service_factory=None,
        )
        result = await worker._process_one()
        assert result is True
    finally:
        worker_mod.JobRepository = original_repo
