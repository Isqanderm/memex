import json
import uuid
from dataclasses import dataclass
from datetime import datetime
from src.llm.protocol import LLMProvider

EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Include: identity, skills, location, work, relationships, projects, preferences, events the user participated in.
- Exclude: opinions and emotional reactions ("talks were great", "enjoyed it"), third-party info, weather. Always extract the underlying event/fact even if accompanied by an opinion.
- Normalize state: prefer "User uses X" over "User switched from Y to X".
- Time-bound facts (meetings, trips, deadlines) ARE included — add "forget_after" as an ISO datetime for them.
- For permanent facts, omit "forget_after".
- Set "category" to one of: research, reminder, thought, decision, preference. Omit if none fits.
- Set "project" to the project/context name if the fact belongs to one (e.g. "Memex", "work", "personal"). Omit if unclear.

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit", "category": "...or omit", "project": "...or omit"}}]}}"""

RESOLVE_PROMPT = """\
New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation of the new fact to it:
- updates: new fact contradicts and supersedes the old one (e.g. "User works at Beta" updates "User works at Acme")
- extends: new fact adds detail without contradiction (e.g. "User is senior engineer at Acme" extends "User works at Acme")
- derives: new fact is a logical conclusion from the old one (e.g. "User has 10+ years experience" derives from "User started working in 2015")
- new: not meaningfully related to the existing fact

Return JSON only:
{{"relations": [{{"id": "...", "type": "updates|extends|derives|new"}}]}}"""


@dataclass
class ExtractedFact:
    content: str
    forget_after: datetime | None = None
    category: str | None = None   # research|reminder|thought|decision|preference
    project: str | None = None


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


_VALID_CATEGORIES = frozenset({"research", "reminder", "thought", "decision", "preference"})


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
                raw_category = f.get("category") or None
                category = raw_category if raw_category in _VALID_CATEGORIES else None
                results.append(ExtractedFact(
                    content=f["content"],
                    forget_after=forget_after,
                    category=category,
                    project=f.get("project") or None,
                ))
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
