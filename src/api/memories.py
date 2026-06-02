import uuid
from typing import Literal

from fastapi import APIRouter, Depends, HTTPException, Query
from pydantic import BaseModel
from sqlalchemy.ext.asyncio import AsyncSession
from src.api.documents import get_db_session
from src.dependencies import get_memory_service, get_profile_service_instance

router = APIRouter(prefix="/api/memory", tags=["memory"])


class RememberRequest(BaseModel):
    content: str
    source: str = "explicit"


class ObserveRequest(BaseModel):
    conversation: str


@router.post("/remember")
async def remember(
    body: RememberRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_memory_service(session)
    result = await service.remember(session, body.content, source=body.source)
    await session.commit()
    return {"facts_extracted": result.facts_extracted, "memories_updated": result.memories_updated}


@router.post("/observe")
async def observe(
    body: ObserveRequest,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_memory_service(session)
    result = await service.observe(session, body.conversation)
    await session.commit()
    return {"facts_extracted": result.facts_extracted, "memories_updated": result.memories_updated}


@router.get("/list")
async def list_memories(
    session: AsyncSession = Depends(get_db_session),
    category: Literal["research", "reminder", "thought", "decision", "preference"] | None = Query(default=None),
):
    service = get_memory_service(session)
    memories = await service.list_active(session, category=category)
    return [
        {
            "id": str(m.id),
            "content": m.content,
            "source": m.source,
            "category": m.category,
            "project": m.project,
            "relation": m.relation,
            "created_at": m.created_at.isoformat() if m.created_at else None,
        }
        for m in memories
    ]


@router.get("/context")
async def context(
    session: AsyncSession = Depends(get_db_session),
):
    service = get_memory_service(session)
    profile_service = get_profile_service_instance()
    memories = await service.list_active(session)
    profile = await profile_service.build_profile(memories)
    return {"static": profile.static, "dynamic": profile.dynamic, "raw_count": profile.raw_count}


@router.delete("/{memory_id}")
async def forget_memory(
    memory_id: uuid.UUID,
    session: AsyncSession = Depends(get_db_session),
):
    service = get_memory_service(session)
    ok = await service.forget_memory(session, memory_id)
    await session.commit()
    if not ok:
        raise HTTPException(status_code=404, detail="Memory not found")
    return {"status": "deleted"}
