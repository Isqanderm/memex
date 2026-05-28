import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text


@dataclass
class SearchHit:
    chunk_id: uuid.UUID
    content: str
    parent_chunk_id: uuid.UUID | None
    doc_id: uuid.UUID
    score: float
    section_heading: str | None = None
    page_number: int | None = None


class SemanticSearch:
    def __init__(self, top_k: int = 20):
        self.top_k = top_k

    async def search(
        self,
        session: AsyncSession,
        query_vector: list[float],
        top_k: int | None = None,
    ) -> list[SearchHit]:
        k = top_k or self.top_k
        vec_str = "[" + ",".join(str(x) for x in query_vector) + "]"

        result = await session.execute(text(f"""
            SELECT id, content, parent_chunk_id, doc_id,
                   section_heading, page_number,
                   1 - (content_vector <=> '{vec_str}'::vector) AS score
            FROM chunks
            WHERE chunk_role = 'leaf'
              AND content_vector IS NOT NULL
            ORDER BY content_vector <=> '{vec_str}'::vector
            LIMIT :k
        """), {"k": k})

        return [
            SearchHit(
                chunk_id=row.id,
                content=row.content,
                parent_chunk_id=row.parent_chunk_id,
                doc_id=row.doc_id,
                score=float(row.score),
                section_heading=row.section_heading,
                page_number=row.page_number,
            )
            for row in result
        ]
