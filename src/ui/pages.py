from fastapi import APIRouter, Request, Form, Depends
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select

from src.db.session import get_db_session

router = APIRouter(tags=["ui"])
templates = Jinja2Templates(directory="templates")


@router.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse(request, "index.html")


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
        request, "documents.html", {"docs": docs, "active_jobs": active_jobs}
    )


@router.get("/upload", response_class=HTMLResponse)
async def upload_page(request: Request):
    return templates.TemplateResponse(request, "upload.html")


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
            {"answer": result.answer, "sources": result.sources},
        )
    except Exception as e:
        return HTMLResponse(f'<p class="error">Ошибка: {e}</p>', status_code=500)
