from dataclasses import dataclass, field
from typing import Any


@dataclass
class Section:
    content: str
    heading: str | None = None
    level: int = 0          # 0=flat, 1=h1, 2=h2
    page_number: int | None = None
    metadata: dict[str, Any] = field(default_factory=dict)


@dataclass
class ParsedDocument:
    source: str
    mime_type: str
    sections: list[Section]
    metadata: dict[str, Any] = field(default_factory=dict)
