from src.adapters.markdown import MarkdownAdapter
from src.adapters.protocol import Source
from src.models.parsed import ParsedDocument, Section

PPTX_MIME = "application/vnd.openxmlformats-officedocument.presentationml.presentation"
XLSX_MIME = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
XLS_MIME = "application/vnd.ms-excel"
EPUB_MIME = "application/epub+zip"

_SUPPORTED_EXTENSIONS = (".pptx", ".xlsx", ".xls", ".epub")
_SUPPORTED_MIMES = {PPTX_MIME, XLSX_MIME, XLS_MIME, EPUB_MIME}

_md_adapter = MarkdownAdapter()


class MarkItDownAdapter:
    """Адаптер для PPTX, XLSX/XLS, EPUB через Microsoft MarkItDown.

    Конвертирует файл в Markdown строку, затем извлекает структуру секций
    через MarkdownAdapter._split_by_headings. Page numbers не поддерживаются
    (эти форматы не имеют стабильной пагинации).
    """

    def can_handle(self, source: Source) -> bool:
        path = source.filename or source.path
        return (
            source.mime_type in _SUPPORTED_MIMES
            or any(path.endswith(ext) for ext in _SUPPORTED_EXTENSIONS)
        )

    def parse(self, source: Source) -> ParsedDocument:
        from markitdown import MarkItDown
        md = MarkItDown()
        result = md.convert(source.path)
        markdown_text = result.text_content or ""

        sections = _md_adapter._split_by_headings(markdown_text)
        if not sections:
            sections = [Section(content=markdown_text)]

        mime = source.mime_type or _guess_mime(source.path)
        return ParsedDocument(
            source=source.path,
            mime_type=mime,
            sections=sections,
            metadata={"filename": source.filename or source.path},
        )


def _guess_mime(path: str) -> str:
    if path.endswith(".pptx"):
        return PPTX_MIME
    if path.endswith((".xlsx", ".xls")):
        return XLSX_MIME
    if path.endswith(".epub"):
        return EPUB_MIME
    return "application/octet-stream"
