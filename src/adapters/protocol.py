from dataclasses import dataclass
from typing import Protocol, runtime_checkable
from src.models.parsed import ParsedDocument


@dataclass
class Source:
    path: str
    mime_type: str
    filename: str = ""


@runtime_checkable
class DocumentAdapter(Protocol):
    def can_handle(self, source: Source) -> bool: ...
    def parse(self, source: Source) -> ParsedDocument: ...
