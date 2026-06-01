import uuid
from dataclasses import dataclass
from datetime import datetime
from sqlalchemy.ext.asyncio import AsyncSession
from src.db.repositories.memory_repo import MemoryRepository


@dataclass
class MemoryHit:
    memory_id: uuid.UUID
    content: str
    score: float
    source: str
    created_at: datetime


class MemorySearch:
    # Lower threshold than conflict detection (0.60) — retrieval needs broad recall
    # text-embedding-3-small gives ~0.3-0.4 for loosely related query/fact pairs
    RETRIEVAL_THRESHOLD = 0.30

    def __init__(self, repo: MemoryRepository, top_k: int = 10):
        self.repo = repo
        self.top_k = top_k

    async def search(
        self,
        session: AsyncSession,
        query_vector: list[float],
    ) -> list[MemoryHit]:
        results = await self.repo.get_active_by_vector(
            query_vector, limit=self.top_k, threshold=self.RETRIEVAL_THRESHOLD
        )
        return [
            MemoryHit(
                memory_id=mem.id,
                content=mem.content,
                score=score,
                source=mem.source,
                created_at=mem.created_at,
            )
            for mem, score in results
        ]
