import uuid

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession

from src.models.chunk import ChunkData


class ChunkRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def bulk_insert_parents(
        self,
        doc_id: uuid.UUID,
        parents: list[ChunkData],
    ) -> dict[int, uuid.UUID]:
        """Вставляет L2 (parent) чанки. Возвращает parent_temp_index → UUID."""
        parent_ids: dict[int, uuid.UUID] = {}

        for parent in parents:
            chunk_id = uuid.uuid4()
            parent_ids[parent.chunk_index] = chunk_id
            pg_lang = _safe_pg_lang(parent.language)

            await self.session.execute(text("""
                INSERT INTO chunks
                    (id, doc_id, chunk_role, chunk_index, section_heading,
                     section_level, page_number, language, content, tsv)
                VALUES
                    (:id, :doc_id, 'parent', :idx, :heading,
                     :level, :page, :lang, :content,
                     to_tsvector(cast(:pg_lang as regconfig), :content))
            """), {
                "id": chunk_id,
                "doc_id": doc_id,
                "idx": parent.chunk_index,
                "heading": parent.section_heading,
                "level": parent.section_level,
                "page": parent.page_number,
                "lang": parent.language,
                "content": parent.content,
                "pg_lang": pg_lang,
            })

        return parent_ids

    async def bulk_insert_leaves(
        self,
        doc_id: uuid.UUID,
        leaves: list[ChunkData],
        parent_ids: dict[int, uuid.UUID],
    ) -> None:
        """Вставляет L1 (leaf) чанки с векторами и ссылками на родителей."""
        for chunk in leaves:
            chunk_id = uuid.uuid4()
            parent_id = parent_ids.get(chunk.parent_temp_index) if chunk.parent_temp_index is not None else None
            pg_lang = _safe_pg_lang(chunk.language)
            vector_str = str(chunk.embedding) if chunk.embedding else None

            await self.session.execute(text("""
                INSERT INTO chunks
                    (id, doc_id, parent_chunk_id, chunk_role, chunk_index,
                     section_heading, section_level, page_number,
                     language, content, content_vector, tsv)
                VALUES
                    (:id, :doc_id, :parent_id, 'leaf', :idx,
                     :heading, :level, :page,
                     :lang, :content,
                     cast(:vector as vector),
                     to_tsvector(cast(:pg_lang as regconfig), :content))
            """), {
                "id": chunk_id,
                "doc_id": doc_id,
                "parent_id": parent_id,
                "idx": chunk.chunk_index,
                "heading": chunk.section_heading,
                "level": chunk.section_level,
                "page": chunk.page_number,
                "lang": chunk.language,
                "content": chunk.content,
                "vector": vector_str,
                "pg_lang": pg_lang,
            })


def _safe_pg_lang(lang: str) -> str:
    valid = {"russian", "english", "german", "french", "spanish", "italian", "simple"}
    from src.ingestion.language import LanguageDetector
    pg = LanguageDetector().to_pg_config(lang)
    return pg if pg in valid else "simple"
