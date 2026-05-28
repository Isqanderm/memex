import pytest
from pathlib import Path
from src.adapters.protocol import Source
from src.adapters.text import TextAdapter
from src.adapters.markdown import MarkdownAdapter


def test_text_can_handle_txt():
    assert TextAdapter().can_handle(Source(path="doc.txt", mime_type="text/plain"))

def test_text_cannot_handle_pdf():
    assert not TextAdapter().can_handle(Source(path="doc.pdf", mime_type="application/pdf"))

def test_text_parses_content(tmp_path):
    f = tmp_path / "test.txt"
    f.write_text("Hello world.\nSecond line.")
    doc = TextAdapter().parse(Source(path=str(f), mime_type="text/plain"))
    assert len(doc.sections) == 1
    assert "Hello world" in doc.sections[0].content

def test_markdown_can_handle_md():
    assert MarkdownAdapter().can_handle(Source(path="doc.md", mime_type="text/markdown"))
    assert MarkdownAdapter().can_handle(Source(path="doc.md", mime_type="text/plain", filename="README.md"))

def test_markdown_extracts_headings(tmp_path):
    f = tmp_path / "test.md"
    f.write_text("# Introduction\n\nFirst section.\n\n## Details\n\nMore content.")
    doc = MarkdownAdapter().parse(Source(path=str(f), mime_type="text/markdown"))
    headings = [s.heading for s in doc.sections if s.heading]
    assert "Introduction" in headings
    assert "Details" in headings

def test_markdown_no_headings(tmp_path):
    f = tmp_path / "plain.md"
    f.write_text("Just plain text without any headings.")
    doc = MarkdownAdapter().parse(Source(path=str(f), mime_type="text/markdown"))
    assert len(doc.sections) >= 1
    assert "plain text" in doc.sections[0].content
