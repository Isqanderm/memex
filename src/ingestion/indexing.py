import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.repositories.document_repo import DocumentRepository
from src.db.repositories.chunk_repo import ChunkRepository
from src.models.chunk import ChunkData
from src.models.parsed import ParsedDocument


class IndexingStage:
    async def index(
        self,
        session: AsyncSession,
        parsed_doc: ParsedDocument,
        chunks: list[ChunkData],
        checksum: str,
    ) -> uuid.UUID:
        doc_repo = DocumentRepository(session)
        chunk_repo = ChunkRepository(session)

        doc = await doc_repo.create(
            source=parsed_doc.source,
            mime_type=parsed_doc.mime_type,
            checksum=checksum,
            title=parsed_doc.metadata.get("title"),
            metadata=parsed_doc.metadata,
        )

        parents = [c for c in chunks if c.chunk_role == "parent"]
        leaves = [c for c in chunks if c.chunk_role == "leaf"]

        parent_ids = await chunk_repo.bulk_insert_parents(doc.id, parents)
        await chunk_repo.bulk_insert_leaves(doc.id, leaves, parent_ids)

        return doc.id
