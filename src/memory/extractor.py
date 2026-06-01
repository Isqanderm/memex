import json
import uuid
from dataclasses import dataclass
from datetime import datetime
from src.llm.protocol import LLMProvider

EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Ignore facts with no lasting relevance (weather, third-party chitchat).
- If a fact is time-bound (e.g. "meeting tomorrow"), add "forget_after" as an ISO datetime.
- For permanent facts, omit "forget_after".

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit"}}]}}"""

RESOLVE_PROMPT = """\
New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation of the new fact to it:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is logically inferred from the old one
- new: not meaningfully related

Return JSON only:
{{"relations": [{{"id": "...", "type": "updates|extends|derives|new"}}]}}"""


@dataclass
class ExtractedFact:
    content: str
    forget_after: datetime | None = None


@dataclass
class RelationResult:
    memory_id: uuid.UUID
    relation: str  # updates | extends | derives | new


def _parse_json(text: str) -> dict:
    start = text.find("{")
    end = text.rfind("}") + 1
    if start == -1 or end == 0:
        raise ValueError(f"No JSON found in: {text[:100]}")
    return json.loads(text[start:end])


class FactExtractor:
    def __init__(self, llm_provider: LLMProvider):
        self.llm = llm_provider

    async def extract_facts(self, text: str) -> list[ExtractedFact]:
        prompt = EXTRACT_PROMPT.format(text=text)
        response = await self.llm.complete(prompt)
        try:
            data = _parse_json(response.answer)
            results = []
            for f in data.get("facts", []):
                forget_after = None
                if fa := f.get("forget_after"):
                    try:
                        forget_after = datetime.fromisoformat(fa)
                    except ValueError:
                        pass
                results.append(ExtractedFact(content=f["content"], forget_after=forget_after))
            return results
        except Exception:
            return []

    async def resolve_relations(
        self,
        new_fact: str,
        existing: list[tuple[uuid.UUID, str]],
    ) -> list[RelationResult]:
        if not existing:
            return []
        existing_str = "\n".join(f'  id={mid}: "{content}"' for mid, content in existing)
        prompt = RESOLVE_PROMPT.format(new_fact=new_fact, existing=existing_str)
        response = await self.llm.complete(prompt)
        try:
            data = _parse_json(response.answer)
            return [
                RelationResult(memory_id=uuid.UUID(r["id"]), relation=r["type"])
                for r in data.get("relations", [])
            ]
        except Exception:
            return []
