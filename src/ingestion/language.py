LANG_TO_PG = {
    "ru": "russian",
    "en": "english",
    "de": "german",
    "fr": "french",
    "es": "spanish",
    "it": "italian",
}


class LanguageDetector:
    def detect(self, text: str) -> str:
        if len(text.strip()) < 20:
            return "simple"
        try:
            from langdetect import detect
            return detect(text)
        except Exception:
            return "simple"

    def to_pg_config(self, lang: str) -> str:
        return LANG_TO_PG.get(lang, "simple")
