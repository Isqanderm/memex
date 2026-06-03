"""
RQ3: Cost and latency of the memory layer.

Prerequisites: Memex running at MEMEX_URL.

Usage:
    MEMEX_URL=http://localhost:8000 uv run python tests/research/rq3_benchmark.py

Success criteria:
    remember() p95 latency  < 5000ms
    context()  p95 latency  < 2000ms
"""
import asyncio
import os
import statistics
import time

import httpx

MEMEX_URL = os.getenv("MEMEX_URL", "http://localhost:8000")

SAMPLE_TEXTS = [
    "I work at Acme Corp as a backend engineer.",
    "I prefer Python for backend and TypeScript for frontend.",
    "I live in Saint Petersburg.",
    "I was born in 1990.",
    "Currently building a self-hosted RAG tool called Memex.",
    "I use vim as my editor.",
    "I prefer dark mode in all my applications.",
    "My manager is called Alexei.",
    "I have a standup meeting every Monday at 10am.",
    "I started this project in March 2026.",
]


async def benchmark_remember(client: httpx.AsyncClient) -> list[float]:
    latencies = []
    for text in SAMPLE_TEXTS:
        t0 = time.perf_counter()
        resp = await client.post(f"{MEMEX_URL}/api/memory/remember", json={"content": text})
        resp.raise_for_status()
        latencies.append((time.perf_counter() - t0) * 1000)
    return latencies


async def benchmark_recall(client: httpx.AsyncClient, n: int = 10) -> list[float]:
    queries = ["where do I work?", "what are my preferences?", "what am I building?"]
    latencies = []
    for q in queries * (n // len(queries) + 1):
        t0 = time.perf_counter()
        await client.post(f"{MEMEX_URL}/api/query", json={"query": q})
        latencies.append((time.perf_counter() - t0) * 1000)
        await asyncio.sleep(0.1)
    return latencies


async def benchmark_context(client: httpx.AsyncClient, n: int = 5) -> list[float]:
    latencies = []
    for _ in range(n):
        t0 = time.perf_counter()
        resp = await client.get(f"{MEMEX_URL}/api/memory/context")
        resp.raise_for_status()
        latencies.append((time.perf_counter() - t0) * 1000)
    return latencies


def p95(values: list[float]) -> float:
    if not values:
        return 0.0
    return sorted(values)[int(len(values) * 0.95)]


async def run():
    async with httpx.AsyncClient(timeout=30.0) as client:
        print("=== RQ3: remember() latency ===")
        rem = await benchmark_remember(client)
        print(f"  p50={statistics.median(rem):.0f}ms  p95={p95(rem):.0f}ms  (target p95 < 5000ms)")

        print("\n=== RQ3: recall() latency ===")
        rec = await benchmark_recall(client)
        print(f"  p50={statistics.median(rec):.0f}ms  p95={p95(rec):.0f}ms")

        print("\n=== RQ3: context() latency ===")
        ctx = await benchmark_context(client)
        print(f"  p50={statistics.median(ctx):.0f}ms  p95={p95(ctx):.0f}ms  (target p95 < 2000ms)")

        ok_rem = p95(rem) < 5000
        ok_ctx = p95(ctx) < 2000
        print(f"\n{'✓' if ok_rem else '✗'} remember() p95 < 5000ms")
        print(f"{'✓' if ok_ctx else '✗'} context()  p95 < 2000ms")
        print(f"\n{'✓ RQ3 PASSED' if ok_rem and ok_ctx else '✗ RQ3 FAILED'}")


if __name__ == "__main__":
    asyncio.run(run())
