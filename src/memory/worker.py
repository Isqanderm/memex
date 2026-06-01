import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.models import MemoryExtractionJob


async def queue_document_extraction(session: AsyncSession, doc_id: str) -> MemoryExtractionJob:
    job = MemoryExtractionJob(
        id=uuid.uuid4(),
        source_ref=doc_id,
        source="document",
        status="pending",
        facts_extracted=0,
    )
    session.add(job)
    await session.flush()
    return job


async def run_document_extraction(
    session: AsyncSession,
    job: MemoryExtractionJob,
    doc_text: str,
    memory_service,
) -> None:
    job.status = "processing"
    await session.flush()
    try:
        result = await memory_service.remember(session, doc_text, source="document")
        job.status = "done"
        job.facts_extracted = result.facts_extracted
    except Exception as e:
        job.status = "error"
        job.error = str(e)
    await session.flush()
