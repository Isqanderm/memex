from dataclasses import dataclass, field
from pathlib import Path

from src.retrieval.expand import L2Chunk


@dataclass
class QueryContext:
    prompt: str
    sources: list[dict] = field(default_factory=list)


class ContextBuilder:
    SYSTEM = (
        "Отвечай только на основе предоставленных источников. "
        "Если ответа нет в источниках — скажи об этом явно. "
        "Цитируй источники как [1], [2] и т.д."
    )

    def build(self, query: str, chunks: list[L2Chunk]) -> QueryContext:
        sources_text = ""
        sources_meta = []

        for i, chunk in enumerate(chunks, start=1):
            parts = [f"[{i}]"]
            if chunk.doc_title:
                parts.append(chunk.doc_title)
            if chunk.section_heading:
                parts.append(f"— {chunk.section_heading}")
            if chunk.page_number:
                parts.append(f"(стр. {chunk.page_number})")

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

        prompt = f"{self.SYSTEM}\n\nИсточники:\n{sources_text}\nВопрос: {query}"
        return QueryContext(prompt=prompt, sources=sources_meta)
