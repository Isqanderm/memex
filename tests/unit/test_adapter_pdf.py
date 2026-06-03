from src.adapters.pdf import PdfAdapter
from src.adapters.protocol import Source


def test_pdf_can_handle():
    assert PdfAdapter().can_handle(Source(path="doc.pdf", mime_type="application/pdf"))
    assert PdfAdapter().can_handle(Source(path="doc.pdf", mime_type="application/octet-stream", filename="doc.pdf"))

def test_pdf_cannot_handle_txt():
    assert not PdfAdapter().can_handle(Source(path="doc.txt", mime_type="text/plain"))

def test_pdf_parses_blank(tmp_path):
    from pypdf import PdfWriter
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    path = tmp_path / "blank.pdf"
    with open(path, "wb") as f:
        writer.write(f)

    doc = PdfAdapter().parse(Source(path=str(path), mime_type="application/pdf"))
    assert doc.mime_type == "application/pdf"
    assert isinstance(doc.sections, list)

def test_pdf_returns_parsed_document(tmp_path):
    from pypdf import PdfWriter
    writer = PdfWriter()
    writer.add_blank_page(width=200, height=200)
    path = tmp_path / "test.pdf"
    with open(path, "wb") as f:
        writer.write(f)

    from src.models.parsed import ParsedDocument
    doc = PdfAdapter().parse(Source(path=str(path), mime_type="application/pdf"))
    assert isinstance(doc, ParsedDocument)
    assert doc.source == str(path)
