import hashlib
import uuid
from pathlib import Path
from fastapi import APIRouter, UploadFile, File, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.session import get_session_factory
from src.db.repositories.document_repo import DocumentRepository
from src.db.repositories.job_repo import JobRepository
from src.config import get_settings

router = APIRouter(tags=["documents"])


async def get_db_session() -> AsyncSession:
    factory = get_session_factory()
    async with factory() as session:
        async with session.begin():
            yield session


class UploadResponse(BaseModel):
    doc_id: str | None = None
    job_id: str | None = None
    status: str


@router.post("/documents", response_model=UploadResponse)
async def upload_document(
    file: UploadFile = File(...),
    session: AsyncSession = Depends(get_db_session),
):
    settings = get_settings()
    content = await file.read()
    checksum = hashlib.sha256(content).hexdigest()

    doc_repo = DocumentRepository(session)
    existing_doc = await doc_repo.get_by_checksum(checksum)
    if existing_doc:
        return UploadResponse(doc_id=str(existing_doc.id), status="already_indexed")

    job_repo = JobRepository(session)
    existing_job = await job_repo.get_by_checksum_active(checksum)
    if existing_job:
        return UploadResponse(job_id=str(existing_job.id), status="already_queued")

    filename = file.filename or "upload"
    dest = settings.upload_dir / f"{uuid.uuid4()}-{filename}"
    dest.write_bytes(content)

    job = await job_repo.create(source=str(dest), checksum=checksum)
    return UploadResponse(job_id=str(job.id), status="pending")


@router.get("/documents")
async def list_documents(session: AsyncSession = Depends(get_db_session)):
    from sqlalchemy import select
    from src.db.models import Document
    result = await session.execute(
        select(Document).order_by(Document.indexed_at.desc())
    )
    docs = result.scalars().all()
    return [
        {
            "id": str(d.id),
            "source": d.source,
            "title": d.title,
            "mime_type": d.mime_type,
            "indexed_at": d.indexed_at.isoformat() if d.indexed_at else None,
        }
        for d in docs
    ]
