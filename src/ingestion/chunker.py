from src.models.parsed import ParsedDocument, Section
from src.models.chunk import ChunkData


def _split_text(text: str, size: int, overlap: int) -> list[str]:
    """Разбивает текст на части по ~size слов с overlap."""
    words = text.split()
    if not words:
        return []
    if len(words) <= size:
        return [text]

    chunks = []
    start = 0
    while start < len(words):
        end = min(start + size, len(words))
        chunks.append(" ".join(words[start:end]))
        if end == len(words):
            break
        start += size - overlap

    return chunks if chunks else [text]


class SmallToBigChunker:
    def __init__(
        self,
        l2_size: int = 512,
        l1_size: int = 128,
        l2_overlap: int = 64,
    ):
        self.l2_size = l2_size
        self.l1_size = l1_size
        self.l2_overlap = l2_overlap

    def chunk(self, doc: ParsedDocument) -> list[ChunkData]:
        all_chunks: list[ChunkData] = []
        parent_index = 0

        for section in doc.sections:
            if not section.content.strip():
                continue

            l2_texts = _split_text(section.content, self.l2_size, self.l2_overlap)

            for l2_text in l2_texts:
                parent = ChunkData(
                    content=l2_text,
                    chunk_role="parent",
                    chunk_index=parent_index,
                    section_heading=section.heading,
                    section_level=section.level,
                    page_number=section.page_number,
                )
                all_chunks.append(parent)
                current_parent_index = parent_index
                parent_index += 1

                l1_texts = _split_text(l2_text, self.l1_size, 0)
                for leaf_idx, l1_text in enumerate(l1_texts):
                    leaf = ChunkData(
                        content=l1_text,
                        chunk_role="leaf",
                        chunk_index=leaf_idx,
                        section_heading=section.heading,
                        section_level=section.level,
                        page_number=section.page_number,
                        parent_temp_index=current_parent_index,
                    )
                    all_chunks.append(leaf)

        return all_chunks
