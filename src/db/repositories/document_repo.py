import uuid

from sqlalchemy import delete, select
from sqlalchemy.ext.asyncio import AsyncSession

from src.db.models import Chunk, Document


class DocumentRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def get_by_checksum(self, checksum: str) -> Document | None:
        result = await self.session.execute(
            select(Document).where(Document.checksum == checksum)
        )
        return result.scalar_one_or_none()

    async def create(self, source: str, mime_type: str, checksum: str,
                     title: str | None = None, metadata: dict | None = None) -> Document:
        doc = Document(
            id=uuid.uuid4(),
            source=source,
            mime_type=mime_type,
            checksum=checksum,
            title=title,
            metadata_=metadata or {},
        )
        self.session.add(doc)
        await self.session.flush()
        return doc

    async def delete_chunks(self, doc_id: uuid.UUID) -> None:
        await self.session.execute(delete(Chunk).where(Chunk.doc_id == doc_id))
