from src.adapters.protocol import Source
from src.models.parsed import ParsedDocument, Section


class PdfAdapter:
    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return source.mime_type == "application/pdf" or path.endswith(".pdf")

    def parse(self, source: Source) -> ParsedDocument:
        from pypdf import PdfReader
        reader = PdfReader(source.path)
        sections = []

        for page_num, page in enumerate(reader.pages, start=1):
            text = (page.extract_text() or "").strip()
            if text:
                sections.append(Section(content=text, page_number=page_num))

        metadata = {}
        if reader.metadata:
            metadata["title"] = reader.metadata.get("/Title", "") or ""
            metadata["author"] = reader.metadata.get("/Author", "") or ""

        return ParsedDocument(
            source=source.path,
            mime_type="application/pdf",
            sections=sections or [Section(content="")],
            metadata=metadata,
        )
