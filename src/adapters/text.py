from src.adapters.protocol import Source
from src.models.parsed import ParsedDocument, Section


class TextAdapter:
    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return source.mime_type == "text/plain" or path.endswith(".txt")

    def parse(self, source: Source) -> ParsedDocument:
        with open(source.path, encoding="utf-8", errors="replace") as f:
            content = f.read()
        return ParsedDocument(
            source=source.path,
            mime_type="text/plain",
            sections=[Section(content=content)],
            metadata={"filename": source.filename or source.path},
        )
