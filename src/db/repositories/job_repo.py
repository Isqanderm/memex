import uuid

from sqlalchemy import select, text, update
from sqlalchemy.ext.asyncio import AsyncSession

from src.db.models import IngestionJob


class JobRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def create(self, source: str, checksum: str) -> IngestionJob:
        job = IngestionJob(id=uuid.uuid4(), source=source, checksum=checksum)
        self.session.add(job)
        await self.session.flush()
        return job

    async def get_by_id(self, job_id: uuid.UUID) -> IngestionJob | None:
        return await self.session.get(IngestionJob, job_id)

    async def get_by_checksum_active(self, checksum: str) -> IngestionJob | None:
        result = await self.session.execute(
            select(IngestionJob).where(
                IngestionJob.checksum == checksum,
                IngestionJob.status.in_(["pending", "processing"]),
            )
        )
        return result.scalar_one_or_none()

    async def claim_next(self) -> IngestionJob | None:
        result = await self.session.execute(text("""
            SELECT id FROM ingestion_jobs
            WHERE status = 'pending'
            ORDER BY created_at
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        """))
        row = result.fetchone()
        if not row:
            return None
        await self.session.execute(text("""
            UPDATE ingestion_jobs
            SET status = 'processing', updated_at = now()
            WHERE id = :id
        """), {"id": row.id})
        return await self.session.get(IngestionJob, row.id)

    async def mark_done(self, job_id: uuid.UUID, doc_id: uuid.UUID) -> None:
        await self.session.execute(
            update(IngestionJob)
            .where(IngestionJob.id == job_id)
            .values(status="done", doc_id=doc_id)
        )

    async def mark_error(self, job_id: uuid.UUID, error: str) -> None:
        await self.session.execute(
            update(IngestionJob)
            .where(IngestionJob.id == job_id)
            .values(status="error", error=error[:2000])
        )
