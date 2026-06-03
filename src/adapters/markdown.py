import re

from src.adapters.protocol import Source
from src.models.parsed import ParsedDocument, Section


class MarkdownAdapter:
    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return (source.mime_type in ("text/markdown", "text/x-markdown")
                or path.endswith((".md", ".markdown")))

    def parse(self, source: Source) -> ParsedDocument:
        with open(source.path, encoding="utf-8", errors="replace") as f:
            content = f.read()
        sections = self._split_by_headings(content)
        return ParsedDocument(
            source=source.path,
            mime_type="text/markdown",
            sections=sections,
            metadata={"filename": source.filename or source.path},
        )

    def _split_by_headings(self, content: str) -> list[Section]:
        heading_re = re.compile(r'^(#{1,6})\s+(.+)$', re.MULTILINE)
        sections = []
        last_end = 0
        current_heading = None
        current_level = 0

        for match in heading_re.finditer(content):
            if last_end < match.start():
                text = content[last_end:match.start()].strip()
                if text:
                    sections.append(Section(content=text, heading=current_heading, level=current_level))
            current_heading = match.group(2).strip()
            current_level = len(match.group(1))
            last_end = match.end()

        remaining = content[last_end:].strip()
        if remaining:
            sections.append(Section(content=remaining, heading=current_heading, level=current_level))

        return sections if sections else [Section(content=content)]
