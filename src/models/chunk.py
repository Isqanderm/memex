from dataclasses import dataclass


@dataclass
class ChunkData:
    """Domain model чанка — используется в pipeline до записи в БД."""
    content: str
    chunk_role: str          # 'parent' | 'leaf'
    chunk_index: int
    language: str = "simple"
    section_heading: str | None = None
    section_level: int | None = None
    page_number: int | None = None
    embedding: list[float] | None = None
    parent_temp_index: int | None = None  # индекс L2-родителя в текущем batch
