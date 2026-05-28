def pytest_configure(config):
    config.addinivalue_line("markers", "unit: unit tests, no external dependencies")
    config.addinivalue_line("markers", "integration: requires Docker")
    config.addinivalue_line("markers", "e2e: requires API keys")
