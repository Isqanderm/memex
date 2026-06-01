"""
Dev-mode profiling. Enable with MEMEX_PROFILE=1.

Usage in code:
    from src.profiling import StepTimer
    t = StepTimer("query")
    with t.step("semantic_search"):
        ...
    t.log()
"""
import logging
import os
import time
from contextlib import contextmanager

ENABLED = os.getenv("MEMEX_PROFILE", "0") == "1"
logger = logging.getLogger("memex.profile")


class StepTimer:
    def __init__(self, label: str):
        self.label = label
        self._steps: list[tuple[str, float]] = []
        self._start = time.perf_counter()

    @contextmanager
    def step(self, name: str):
        t0 = time.perf_counter()
        try:
            yield
        finally:
            self._steps.append((name, (time.perf_counter() - t0) * 1000))

    def total_ms(self) -> float:
        return (time.perf_counter() - self._start) * 1000

    def log(self):
        if not ENABLED:
            return
        total = self.total_ms()
        parts = "  ".join(f"{name}={ms:.0f}ms" for name, ms in self._steps)
        print(f"[profile] {self.label}  total={total:.0f}ms  {parts}", flush=True)
