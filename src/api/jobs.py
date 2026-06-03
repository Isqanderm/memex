import uuid

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession

from src.db.session import get_db_session
from src.db.models import IngestionJob

router = APIRouter(tags=["jobs"])


class JobResponse(BaseModel):
    job_id: str
    status: str
    doc_id: str | None = None
    error: str | None = None


@router.get("/jobs/{job_id}", response_model=JobResponse)
async def get_job(
    job_id: uuid.UUID,
    session: AsyncSession = Depends(get_db_session),
):
    result = await session.execute(
        select(IngestionJob).where(IngestionJob.id == job_id)
    )
    job = result.scalar_one_or_none()
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return JobResponse(
        job_id=str(job.id),
        status=job.status,
        doc_id=str(job.doc_id) if job.doc_id else None,
        error=job.error,
    )
