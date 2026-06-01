"""
RQ1: Answer quality with memory layer vs baseline RAG.

Prerequisites: Memex running at MEMEX_URL with memory layer enabled.

Usage:
    MEMEX_URL=http://localhost:8000 uv run python tests/research/rq1_eval.py

Success criterion: overall accuracy >= 0.80
"""
import asyncio
import json
import os
from pathlib import Path

import httpx

MEMEX_URL = os.getenv("MEMEX_URL", "http://localhost:8000")
DATASETS = Path(__file__).parent / "datasets" / "rq1_eval_conversations.json"


async def ingest_memories(client: httpx.AsyncClient, texts: list[str]) -> None:
    for text in texts:
        resp = await client.post(f"{MEMEX_URL}/api/memory/remember", json={"content": text})
        resp.raise_for_status()


async def ask(client: httpx.AsyncClient, question: str) -> str:
    resp = await client.post(f"{MEMEX_URL}/api/query", json={"query": question})
    resp.raise_for_status()
    return resp.json().get("answer", "")


async def clear_memories(client: httpx.AsyncClient) -> None:
    resp = await client.get(f"{MEMEX_URL}/api/memory/list")
    if resp.status_code == 200:
        for mem in resp.json():
            await client.delete(f"{MEMEX_URL}/api/memory/{mem['id']}")


async def run_eval():
    data = json.loads(DATASETS.read_text())
    correct, total = 0, 0

    async with httpx.AsyncClient(timeout=120.0) as client:
        for session in data["sessions"]:
            print(f"\n--- Session {session['id']} ---")
            await clear_memories(client)
            await ingest_memories(client, session["ingestion"])
            await asyncio.sleep(2)

            for qa in session["questions"]:
                answer = await ask(client, qa["q"])
                expected = qa["expected_keyword"].lower()
                anti = qa.get("anti_keyword", "").lower()
                hit = expected in answer.lower() and (not anti or anti not in answer.lower())
                correct += int(hit)
                total += 1
                mark = "✓" if hit else "✗"
                print(f"  {mark} Q: {qa['q']}")
                print(f"      A: {answer[:120]}")

    accuracy = correct / total if total > 0 else 0
    print(f"\nAccuracy: {accuracy:.2f}  ({correct}/{total})")
    print(f"{'✓ RQ1 PASSED' if accuracy >= 0.80 else '✗ RQ1 FAILED — check memory layer integration'}")


if __name__ == "__main__":
    asyncio.run(run_eval())
