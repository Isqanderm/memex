import uuid
from dataclasses import dataclass
from sqlalchemy.ext.asyncio import AsyncSession
from src.memory.extractor import FactExtractor
from src.db.repositories.memory_repo import MemoryRepository
from src.db.models import Memory


@dataclass
class RememberResult:
    facts_extracted: int
    memories_updated: int


class MemoryService:
    def __init__(
        self,
        repo: MemoryRepository,
        extractor: FactExtractor,
        embed_fn,  # async callable: str -> list[float]
    ):
        self.repo = repo
        self.extractor = extractor
        self.embed_fn = embed_fn

    async def remember(
        self,
        session: AsyncSession,
        text: str,
        source: str = "explicit",
    ) -> RememberResult:
        facts = await self.extractor.extract_facts(text)
        facts_extracted = len(facts)
        memories_updated = 0

        for fact in facts:
            vector = await self.embed_fn(fact.content)
            # 0.60 threshold for conflict candidates — calibrated for text-embedding-3-small
            similar = await self.repo.get_active_by_vector(vector, threshold=0.60)
            existing = [(m.id, m.content) for m, _ in similar]
            relations = await self.extractor.resolve_relations(fact.content, existing)

            parent_id: uuid.UUID | None = None
            relation_type: str | None = None

            for rel in relations:
                if rel.relation == "updates":
                    await self.repo.deactivate(rel.memory_id)
                    parent_id = rel.memory_id
                    relation_type = "updates"
                    memories_updated += 1
                elif rel.relation in ("extends", "derives") and parent_id is None:
                    parent_id = rel.memory_id
                    relation_type = rel.relation

            await self.repo.create(
                content=fact.content,
                raw_input=text,
                source=source,
                vector=vector,
                parent_id=parent_id,
                relation=relation_type,
                forget_after=fact.forget_after,
                category=fact.category,
                project=fact.project,
            )

        return RememberResult(facts_extracted=facts_extracted, memories_updated=memories_updated)

    async def observe(self, session: AsyncSession, conversation: str) -> RememberResult:
        observe_prompt = (
            "What new personal facts about the user did you learn in this conversation?\n"
            "Return only new information, not a recap. Ignore facts already discussed before.\n\n"
            f"Conversation:\n{conversation}"
        )
        return await self.remember(session, observe_prompt, source="conversation")

    async def forget_memory(self, session: AsyncSession, memory_id: uuid.UUID) -> bool:
        mem = await self.repo.get_by_id(memory_id)
        if mem is None:
            return False
        await self.repo.deactivate(memory_id)
        return True

    async def list_active(self, session: AsyncSession) -> list[Memory]:
        return await self.repo.get_all_active()
