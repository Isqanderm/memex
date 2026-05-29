from fastapi import APIRouter, Request, Form, Depends
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select

router = APIRouter(tags=["ui"])
templates = Jinja2Templates(directory="templates")


async def get_db_session() -> AsyncSession:
    from src.db.session import get_session_factory
    factory = get_session_factory()
    async with factory() as session:
        async with session.begin():
            yield session


@router.get("/", response_class=HTMLResponse)
async def index(request: Request):
    return templates.TemplateResponse(request, "index.html")


@router.get("/documents", response_class=HTMLResponse)
async def documents_page(
    request: Request,
    session: AsyncSession = Depends(get_db_session),
):
    from src.db.models import Document
    result = await session.execute(
        select(Document).order_by(Document.indexed_at.desc())
    )
    docs = result.scalars().all()
    return templates.TemplateResponse(request, "documents.html", {"docs": docs})


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
