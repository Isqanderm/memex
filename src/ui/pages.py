import json
from fastapi import APIRouter, Request, Form, Depends
from fastapi.responses import HTMLResponse, StreamingResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select, func

from src.db.session import get_db_session

router = APIRouter(tags=["ui"])
templates = Jinja2Templates(directory="templates")


async def _doc_count(session: AsyncSession) -> int:
    from src.db.models import Document
    result = await session.execute(select(func.count()).select_from(Document))
    return result.scalar() or 0


@router.get("/", response_class=HTMLResponse)
async def index(request: Request, session: AsyncSession = Depends(get_db_session)):
    count = await _doc_count(session)
    return templates.TemplateResponse(
        request, "index.html", {"active_page": "search", "doc_count": count}
    )


@router.get("/documents", response_class=HTMLResponse)
async def documents_page(
    request: Request,
    session: AsyncSession = Depends(get_db_session),
):
    from src.db.models import Document, IngestionJob
    docs_result = await session.execute(
        select(Document).order_by(Document.indexed_at.desc())
    )
    docs = docs_result.scalars().all()

    jobs_result = await session.execute(
        select(IngestionJob)
        .where(IngestionJob.status.in_(["pending", "processing", "error"]))
        .order_by(IngestionJob.created_at.desc())
    )
    active_jobs = jobs_result.scalars().all()

    return templates.TemplateResponse(
        request,
        "documents.html",
        {
            "docs": docs,
            "active_jobs": active_jobs,
            "active_page": "documents",
            "doc_count": len(docs),
        },
    )


@router.get("/upload", response_class=HTMLResponse)
async def upload_page(
    request: Request,
    session: AsyncSession = Depends(get_db_session),
):
    count = await _doc_count(session)
    return templates.TemplateResponse(
        request, "upload.html", {"active_page": "upload", "doc_count": count}
    )


@router.get("/jobs-fragment", response_class=HTMLResponse)
async def jobs_fragment(
    request: Request,
    session: AsyncSession = Depends(get_db_session),
):
    from src.db.models import IngestionJob
    jobs_result = await session.execute(
        select(IngestionJob)
        .where(IngestionJob.status.in_(["pending", "processing", "error"]))
        .order_by(IngestionJob.created_at.desc())
    )
    active_jobs = jobs_result.scalars().all()
    return templates.TemplateResponse(
        request, "_jobs_fragment.html", {"active_jobs": active_jobs}
    )


@router.post("/search", response_class=HTMLResponse)
async def search(
    request: Request,
    query: str = Form(...),
    session: AsyncSession = Depends(get_db_session),
):
    from src.dependencies import get_retrieval_service, get_embedding_client
    service = get_retrieval_service()
    client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        return (await client.embed_batch([text]))[0]

    try:
        result = await service.query(session, query, embed_fn=embed)
        return templates.TemplateResponse(
            request,
            "_results.html",
            {"query": query, "answer": result.answer, "sources": result.sources},
        )
    except Exception as e:
        safe_query = query.replace("<", "&lt;").replace(">", "&gt;")
        safe_error = str(e).replace("<", "&lt;").replace(">", "&gt;")
        return HTMLResponse(
            f'<div class="exchange">'
            f'<div class="user-bubble-wrap"><div class="user-bubble">{safe_query}</div></div>'
            f'<div class="bot-bubble-wrap"><div class="bot-avatar">⬡</div>'
            f'<div class="bot-bubble" style="color:#f87171">Error: {safe_error}</div></div>'
            f'</div>',
            status_code=200,
        )


@router.post("/search/stream")
async def search_stream(
    query: str = Form(...),
    session: AsyncSession = Depends(get_db_session),
):
    from src.dependencies import get_retrieval_service, get_embedding_client
    service = get_retrieval_service()
    client = get_embedding_client()

    async def embed(text: str) -> list[float]:
        return (await client.embed_batch([text]))[0]

    async def generate():
        try:
            async for event in service.query_stream(session, query, embed_fn=embed):
                if event["type"] == "token":
                    yield f"event: token\ndata: {json.dumps(event['data'])}\n\n"
                elif event["type"] == "sources":
                    yield f"event: sources\ndata: {json.dumps(event['data'])}\n\n"
                elif event["type"] == "done":
                    yield "event: done\ndata: {}\n\n"
        except Exception as e:
            safe = str(e).replace('"', '\\"')
            yield f'event: error\ndata: {{"message": "{safe}"}}\n\n'

    return StreamingResponse(
        generate(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )
