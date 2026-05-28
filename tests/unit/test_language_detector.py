from src.ingestion.language import LanguageDetector


def test_detects_english():
    detector = LanguageDetector()
    result = detector.detect("The quick brown fox jumps over the lazy dog and runs away")
    assert result == "en"


def test_detects_russian():
    detector = LanguageDetector()
    result = detector.detect("Быстрая коричневая лиса прыгает через ленивую собаку в лесу")
    assert result == "ru"


def test_fallback_on_short_text():
    detector = LanguageDetector()
    result = detector.detect("Hi")
    assert result == "simple"


def test_fallback_on_empty():
    detector = LanguageDetector()
    result = detector.detect("")
    assert result == "simple"


def test_to_pg_config_russian():
    detector = LanguageDetector()
    assert detector.to_pg_config("ru") == "russian"


def test_to_pg_config_english():
    detector = LanguageDetector()
    assert detector.to_pg_config("en") == "english"


def test_to_pg_config_unknown():
    detector = LanguageDetector()
    assert detector.to_pg_config("xx") == "simple"
    assert detector.to_pg_config("zh") == "simple"
