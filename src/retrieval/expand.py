import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text
from src.retrieval.semantic import SearchHit


@dataclass
class L2Chunk:
    chunk_id: uuid.UUID
    content: str
    doc_id: uuid.UUID
    section_heading: str | None
    page_number: int | None
    doc_title: str | None
    doc_source: str | None = None


async def expand_to_l2(
    session: AsyncSession,
    hits: list[SearchHit],
) -> list[L2Chunk]:
    parent_ids = list({h.parent_chunk_id for h in hits if h.parent_chunk_id})
    if not parent_ids:
        return []

    result = await session.execute(text("""
        SELECT c.id, c.content, c.doc_id, c.section_heading, c.page_number,
               d.title AS doc_title, d.source AS doc_source
        FROM chunks c
        JOIN documents d ON d.id = c.doc_id
        WHERE c.id = ANY(:ids)
    """), {"ids": parent_ids})

    return [
        L2Chunk(
            chunk_id=row.id,
            content=row.content,
            doc_id=row.doc_id,
            section_heading=row.section_heading,
            page_number=row.page_number,
            doc_title=row.doc_title,
            doc_source=row.doc_source,
        )
        for row in result
    ]
