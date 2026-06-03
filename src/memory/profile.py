from dataclasses import dataclass
from datetime import datetime, timedelta, timezone

from src.db.models import Memory
from src.llm.protocol import LLMProvider

STATIC_THRESHOLD_DAYS = 30

PROFILE_PROMPT = """\
Summarize the following facts about a user into a concise profile (2-4 sentences max, ≤150 tokens).
Write in third person. Include only factual information from the list.

Facts:
{facts}

Profile summary:"""


@dataclass
class UserProfile:
    static: str
    dynamic: str
    raw_count: int


def _make_aware(dt: datetime) -> datetime:
    if dt.tzinfo is None:
        return dt.replace(tzinfo=timezone.utc)
    return dt


class ProfileService:
    def __init__(self, llm_provider: LLMProvider):
        self.llm = llm_provider

    async def build_profile(self, memories: list[Memory]) -> UserProfile:
        if not memories:
            return UserProfile(static="", dynamic="", raw_count=0)

        cutoff = datetime.now(timezone.utc) - timedelta(days=STATIC_THRESHOLD_DAYS)
        static_mems = [m for m in memories if m.created_at and _make_aware(m.created_at) < cutoff]
        dynamic_mems = [m for m in memories if not m.created_at or _make_aware(m.created_at) >= cutoff]

        static_text = await self._summarize(static_mems) if static_mems else ""
        dynamic_text = await self._summarize(dynamic_mems) if dynamic_mems else ""

        return UserProfile(
            static=static_text,
            dynamic=dynamic_text,
            raw_count=len(memories),
        )

    async def _summarize(self, memories: list[Memory]) -> str:
        if not memories:
            return ""
        facts = "\n".join(f"- {m.content}" for m in memories)
        prompt = PROFILE_PROMPT.format(facts=facts)
        response = await self.llm.complete(prompt)
        return response.answer.strip()
