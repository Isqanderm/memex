from fastapi import APIRouter, Depends
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession

from src.api.documents import get_db_session
from src.dependencies import get_embedding_client, get_retrieval_service

router = APIRouter(tags=["query"])


class QueryRequest(BaseModel):
    query: str
    top_k: int = 5


class QueryResponse(BaseModel):
    answer: str
    sources: list[dict]


@router.post("/query", response_model=QueryResponse)
async def query_documents(
    request: QueryRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text])
        return results[0]

    result = await service.query(session, request.query, embed_fn=embed)
    return QueryResponse(answer=result.answer, sources=result.sources)


class ChunksRequest(BaseModel):
    query: str
    top_k: int = 5


@router.post("/search/chunks")
async def search_chunks(
    request: ChunksRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_retrieval_service()
    embedding_client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        results = await embedding_client.embed_batch([text])
        return results[0]

    chunks = await service.search_chunks(session, request.query, embed_fn=embed, top_k=request.top_k)
    return {"chunks": chunks}
