import pytest
from src.adapters.registry import AdapterRegistry
from src.adapters.protocol import Source
from src.models.parsed import ParsedDocument, Section


class FakePdfAdapter:
    def can_handle(self, source: Source) -> bool:
        return source.mime_type == "application/pdf"

    def parse(self, source: Source) -> ParsedDocument:
        return ParsedDocument(source=source.path, mime_type="application/pdf",
                              sections=[Section(content="PDF content")])


class AlwaysAdapter:
    def can_handle(self, source: Source) -> bool:
        return True

    def parse(self, source: Source) -> ParsedDocument:
        return ParsedDocument(source=source.path, mime_type="", sections=[])


def test_registry_finds_correct_adapter():
    registry = AdapterRegistry()
    registry.register(FakePdfAdapter())
    source = Source(path="doc.pdf", mime_type="application/pdf")
    adapter = registry.get(source)
    assert adapter is not None

def test_registry_returns_none_for_unknown():
    registry = AdapterRegistry()
    source = Source(path="doc.xyz", mime_type="application/octet-stream")
    assert registry.get(source) is None

def test_registry_first_match_wins():
    registry = AdapterRegistry()
    registry.register(FakePdfAdapter())
    registry.register(AlwaysAdapter())
    source = Source(path="doc.pdf", mime_type="application/pdf")
    adapter = registry.get(source)
    assert isinstance(adapter, FakePdfAdapter)

def test_registry_parse_raises_for_unknown():
    registry = AdapterRegistry()
    source = Source(path="doc.xyz", mime_type="unknown/type")
    with pytest.raises(ValueError):
        registry.parse(source)

def test_registry_parse_calls_adapter():
    registry = AdapterRegistry()
    registry.register(FakePdfAdapter())
    source = Source(path="test.pdf", mime_type="application/pdf")
    doc = registry.parse(source)
    assert doc.mime_type == "application/pdf"
