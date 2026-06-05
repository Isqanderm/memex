import os
from typing import Generator

import httpx
import pytest


@pytest.fixture(scope="session")
def base_url() -> str:
    return os.environ.get("MEMEX_BASE_URL", "http://localhost:8000")


@pytest.fixture(scope="session")
def client(base_url: str) -> Generator[httpx.Client, None, None]:
    with httpx.Client(base_url=base_url, timeout=30.0) as c:
        yield c


@pytest.fixture(scope="session", autouse=True)
def require_server(base_url: str) -> None:
    """Skip the entire golden suite if the server is not reachable."""
    try:
        httpx.get(f"{base_url}/health", timeout=5.0)
    except httpx.ConnectError:
        pytest.skip(f"Memex server not reachable at {base_url}")
