import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import select, text
from sqlalchemy.ext.asyncio import AsyncSession

from src.db.models import Memory


class MemoryRepository:
    def __init__(self, session: AsyncSession):
        self.session = session

    async def create(
        self,
        content: str,
        raw_input: str,
        source: str,
        vector: list[float],
        parent_id: uuid.UUID | None = None,
        relation: str | None = None,
        forget_after: datetime | None = None,
        category: str | None = None,
        project: str | None = None,
    ) -> Memory:
        m = Memory(
            id=uuid.uuid4(),
            content=content,
            raw_input=raw_input,
            source=source,
            is_active=True,
            forget_after=forget_after,
            relation=relation,
            parent_id=parent_id,
            content_vector=vector,
            category=category,
            project=project,
        )
        self.session.add(m)
        await self.session.flush()
        return m

    async def deactivate(self, memory_id: uuid.UUID) -> None:
        result = await self.session.execute(
            select(Memory).where(Memory.id == memory_id)
        )
        mem = result.scalar_one_or_none()
        if mem:
            mem.is_active = False
            await self.session.flush()

    async def get_all_active(self, category: str | None = None) -> list[Memory]:
        q = select(Memory).where(Memory.is_active)
        if category:
            q = q.where(Memory.category == category)
        result = await self.session.execute(q.order_by(Memory.created_at.desc()))
        return list(result.scalars().all())

    async def get_by_id(self, memory_id: uuid.UUID) -> Memory | None:
        result = await self.session.execute(
            select(Memory).where(Memory.id == memory_id)
        )
        return result.scalar_one_or_none()

    async def get_active_by_vector(
        self,
        vector: list[float],
        limit: int = 5,
        threshold: float = 0.75,
        category: str | None = None,
    ) -> list[tuple[Memory, float]]:
        # Inline vector like SemanticSearch does — SQLAlchemy text() mishandles :param::type cast
        vec_str = "[" + ",".join(str(x) for x in vector) + "]"
        category_filter = "AND category = :category" if category else ""
        params: dict[str, Any] = {"threshold": threshold, "limit": limit}
        if category:
            params["category"] = category
        rows = await self.session.execute(
            text(f"""
                SELECT id, 1 - (content_vector <=> '{vec_str}'::vector) AS score
                FROM memories
                WHERE is_active = TRUE
                  AND content_vector IS NOT NULL
                  AND 1 - (content_vector <=> '{vec_str}'::vector) >= :threshold
                  {category_filter}
                ORDER BY content_vector <=> '{vec_str}'::vector
                LIMIT :limit
            """),
            params,
        )
        ids_scores = [(row.id, row.score) for row in rows]
        if not ids_scores:
            return []
        id_list = [r[0] for r in ids_scores]
        mems_result = await self.session.execute(
            select(Memory).where(Memory.id.in_(id_list))
        )
        mems_by_id = {m.id: m for m in mems_result.scalars().all()}
        return [(mems_by_id[mid], score) for mid, score in ids_scores if mid in mems_by_id]

    async def expire_stale(self) -> int:
        result = await self.session.execute(
            text("""
                UPDATE memories SET is_active = FALSE
                WHERE forget_after < NOW() AND is_active = TRUE
                RETURNING id
            """)
        )
        await self.session.flush()
        return len(result.fetchall())
