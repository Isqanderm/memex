import uuid
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import text
from src.retrieval.semantic import SearchHit
from src.ingestion.language import LanguageDetector


class BM25Search:
    def __init__(self, top_k: int = 20):
        self.top_k = top_k
        self._lang_detector = LanguageDetector()

    async def search(
        self,
        session: AsyncSession,
        query: str,
        top_k: int | None = None,
    ) -> list[SearchHit]:
        k = top_k or self.top_k
        lang = self._lang_detector.detect(query)
        pg_config = self._lang_detector.to_pg_config(lang)

        result = await session.execute(text(f"""
            SELECT c.id, c.content, c.parent_chunk_id, c.doc_id,
                   c.section_heading, c.page_number,
                   ts_rank(c.tsv, query) AS score
            FROM chunks c,
                 plainto_tsquery('{pg_config}'::regconfig, :query) query
            WHERE c.chunk_role = 'leaf'
              AND c.tsv @@ query
            ORDER BY score DESC
            LIMIT :k
        """), {"query": query, "k": k})

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
