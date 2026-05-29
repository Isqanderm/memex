import hashlib
import uuid
from pathlib import Path
from fastapi import APIRouter, UploadFile, File, Depends, HTTPException
from fastapi.responses import FileResponse
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.session import get_db_session
from src.db.repositories.document_repo import DocumentRepository
from src.db.repositories.job_repo import JobRepository
from src.config import get_settings

router = APIRouter(tags=["documents"])


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


@router.get("/documents/{doc_id}/file")
async def serve_document_file(
    doc_id: str,
    session: AsyncSession = Depends(get_db_session),
):
    from sqlalchemy import select
    from src.db.models import Document
    try:
        doc_uuid = uuid.UUID(doc_id)
    except ValueError:
        raise HTTPException(status_code=400, detail="Invalid document ID")
    result = await session.execute(select(Document).where(Document.id == doc_uuid))
    doc = result.scalar_one_or_none()
    if not doc:
        raise HTTPException(status_code=404, detail="Document not found")
    path = Path(doc.source)
    if not path.exists():
        raise HTTPException(status_code=404, detail="File not found on disk")
    display_name = path.name.split('-', 5)[-1] if '-' in path.name else path.name
    return FileResponse(path=str(path), media_type=doc.mime_type, filename=display_name)


@router.patch("/documents/{doc_id}")
async def update_document(
    doc_id: str,
    body: dict,
    session: AsyncSession = Depends(get_db_session),
):
    from sqlalchemy import select
    from src.db.models import Document
    try:
        doc_uuid = uuid.UUID(doc_id)
    except ValueError:
        raise HTTPException(status_code=400, detail="Invalid document ID")
    result = await session.execute(select(Document).where(Document.id == doc_uuid))
    doc = result.scalar_one_or_none()
    if not doc:
        raise HTTPException(status_code=404, detail="Document not found")
    if "title" in body and body["title"] is not None:
        doc.title = body["title"]
    if "tags" in body:
        doc.metadata_ = {**(doc.metadata_ or {}), "tags": body["tags"]}
    await session.commit()
    return {"id": str(doc.id), "title": doc.title, "metadata": doc.metadata_}


@router.delete("/documents/{doc_id}", status_code=204)
async def delete_document(
    doc_id: str,
    session: AsyncSession = Depends(get_db_session),
):
    from sqlalchemy import select
    from src.db.models import Document
    try:
        doc_uuid = uuid.UUID(doc_id)
    except ValueError:
        raise HTTPException(status_code=400, detail="Invalid document ID")
    result = await session.execute(select(Document).where(Document.id == doc_uuid))
    doc = result.scalar_one_or_none()
    if not doc:
        raise HTTPException(status_code=404, detail="Document not found")
    # Delete file from disk
    path = Path(doc.source)
    if path.exists():
        path.unlink()
    await session.delete(doc)
    await session.commit()


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
            "title": d.title,
            "mime_type": d.mime_type,
            "indexed_at": d.indexed_at.isoformat() if d.indexed_at else None,
            "tags": (d.metadata_ or {}).get("tags", []),
        }
        for d in docs
    ]
