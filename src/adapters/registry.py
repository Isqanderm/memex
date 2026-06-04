from src.adapters.protocol import DocumentAdapter, Source
from src.models.parsed import ParsedDocument


class AdapterRegistry:
    def __init__(self) -> None:
        self._adapters: list[DocumentAdapter] = []

    def register(self, adapter: DocumentAdapter) -> None:
        self._adapters.append(adapter)

    def get(self, source: Source) -> DocumentAdapter | None:
        for adapter in self._adapters:
            if adapter.can_handle(source):
                return adapter
        return None

    def parse(self, source: Source) -> ParsedDocument:
        adapter = self.get(source)
        if adapter is None:
            raise ValueError(f"No adapter found for {source.mime_type} ({source.path})")
        return adapter.parse(source)
