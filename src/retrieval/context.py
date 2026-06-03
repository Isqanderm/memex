import datetime
from dataclasses import dataclass, field
from pathlib import Path

from src.retrieval.expand import L2Chunk


@dataclass
class QueryContext:
    prompt: str
    sources: list[dict] = field(default_factory=list)


# Kept for A/B benchmarking — do not delete
SYSTEM_V1 = (
    "Answer based on the provided sources and personal memory facts (if any). "
    "If the answer is not in the sources or memory — say so explicitly. "
    "Cite document sources as [1], [2], etc. Cite memory facts as [memory]."
)

SYSTEM_V2 = """\
You are a question-answering assistant with access to two types of context:

1. PERSONAL MEMORY FACTS — atomic facts about the user (high signal, always current).
   Use these for questions about the user's life, preferences, location, work, etc.

2. DOCUMENT SOURCES — detailed content from indexed documents.
   Use these for specifics, evidence, quotes, and facts from documents.
   This is your primary source for detailed information.

Today's date: {date}

Instructions:
- For questions about the user, prioritize memory facts over documents.
- For questions about topics/documents, use document sources for details.
- Memory facts are summaries — if a document source contains more detail, use it.
- If neither memory nor documents contain the answer, say "I don't know" explicitly.
- Cite document sources as [1], [2], etc. Cite memory facts as [memory].\
"""


class ContextBuilder:
    def build(
        self,
        query: str,
        chunks: list[L2Chunk],
        memory_hits: list = None,
        today: str | None = None,
    ) -> QueryContext:
        today = today or datetime.date.today().isoformat()
        system = SYSTEM_V2.format(date=today)

        sources_text = ""
        sources_meta = []

        if memory_hits:
            sources_text += "\nPersonal memory facts:\n"
            for hit in memory_hits[:5]:
                parts = ["memory"]
                if hit.category:
                    parts.append(hit.category)
                if hit.project:
                    parts.append(hit.project)
                if hit.created_at:
                    parts.append(hit.created_at.strftime("%Y-%m-%d"))
                tag = " | ".join(parts)
                sources_text += f"  [{tag}] {hit.content}\n"

        if chunks:
            sources_text += "\nDocument sources:\n"
            for i, chunk in enumerate(chunks, start=1):
                parts = [f"[{i}]"]
                if chunk.doc_title:
                    parts.append(chunk.doc_title)
                if chunk.section_heading:
                    parts.append(f"— {chunk.section_heading}")
                if chunk.page_number:
                    parts.append(f"(p. {chunk.page_number})")

                sources_text += "\n" + " ".join(parts) + "\n"
                sources_text += "---\n"
                sources_text += chunk.content + "\n"

                raw_name = Path(chunk.doc_source).name if chunk.doc_source else None
                filename = raw_name.split('-', 5)[-1] if raw_name else None
                sources_meta.append({
                    "index": i,
                    "doc_id": str(chunk.doc_id),
                    "title": chunk.doc_title,
                    "section": chunk.section_heading,
                    "page": chunk.page_number,
                    "preview": chunk.content[:200],
                    "filename": filename,
                })

        prompt = f"{system}\n{sources_text}\nQuestion: {query}"
        return QueryContext(prompt=prompt, sources=sources_meta)
