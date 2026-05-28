import asyncio
from src.retrieval.expand import L2Chunk


class Reranker:
    _model = None
    MODEL_NAME = "cross-encoder/ms-marco-MiniLM-L-6-v2"

    def _get_model(self):
        if Reranker._model is None:
            from sentence_transformers import CrossEncoder
            Reranker._model = CrossEncoder(self.MODEL_NAME)
        return Reranker._model

    async def rerank(self, query: str, chunks: list[L2Chunk], top_n: int = 5) -> list[L2Chunk]:
        if not chunks:
            return []

        loop = asyncio.get_event_loop()
        pairs = [(query, c.content) for c in chunks]

        scores = await loop.run_in_executor(
            None,
            lambda: self._get_model().predict(pairs),
        )

        ranked = sorted(zip(chunks, scores), key=lambda x: x[1], reverse=True)
        return [chunk for chunk, _ in ranked[:top_n]]
