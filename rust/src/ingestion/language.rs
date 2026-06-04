use whichlang::Lang;

pub struct LanguageDetector;

/// Map whichlang::Lang enum to ISO 639-1 two-letter code.
fn whichlang_to_iso(lang: Lang) -> &'static str {
    match lang {
        Lang::Ara => "ar",
        Lang::Cmn => "zh",
        Lang::Deu => "de",
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Hin => "hi",
        Lang::Ita => "it",
        Lang::Jpn => "ja",
        Lang::Kor => "ko",
        Lang::Nld => "nl",
        Lang::Por => "pt",
        Lang::Rus => "ru",
        Lang::Spa => "es",
        Lang::Swe => "sv",
        Lang::Tur => "tr",
        Lang::Vie => "vi",
    }
}

impl LanguageDetector {
    /// Detect language from text. Returns ISO 639-1 code ("en", "ru", "de", etc.)
    pub fn detect(&self, text: &str) -> String {
        let trimmed = text.trim();
        if trimmed.len() < 20 {
            return "en".to_string();
        }
        let lang = whichlang::detect_language(trimmed);
        whichlang_to_iso(lang).to_string()
    }

    /// Returns tantivy-compatible language code (same as ISO 639-1 for supported langs)
    pub fn to_tantivy_code(&self, lang: &str) -> &'static str {
        match lang {
            "ru" => "ru",
            "en" => "en",
            "de" => "de",
            "fr" => "fr",
            "es" => "es",
            "it" => "it",
            "pt" => "pt",
            "nl" => "nl",
            "sv" => "sv",
            "fi" => "fi",
            "da" => "da",
            "no" => "no",
            "ar" => "ar",
            "hu" => "hu",
            "el" => "el",
            "ro" => "ro",
            "tr" => "tr",
            _ => "en",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(text: &str) -> String {
        LanguageDetector.detect(text)
    }

    #[test]
    fn detects_english() {
        assert_eq!(detect("The quick brown fox jumps over the lazy dog"), "en");
    }

    #[test]
    fn detects_russian() {
        // whichlang should detect Cyrillic text as Russian
        assert!(["ru"].contains(&detect("Быстрый коричневый лис прыгает через ленивого пса").as_str()));
    }

    #[test]
    fn short_text_returns_en() {
        assert_eq!(detect("hi"), "en");
    }

    #[test]
    fn to_tantivy_code_known_lang() {
        let d = LanguageDetector;
        assert_eq!(d.to_tantivy_code("ru"), "ru");
        assert_eq!(d.to_tantivy_code("de"), "de");
        assert_eq!(d.to_tantivy_code("fi"), "fi");
    }

    #[test]
    fn to_tantivy_code_unknown_falls_back_to_en() {
        let d = LanguageDetector;
        assert_eq!(d.to_tantivy_code("zh"), "en");
        assert_eq!(d.to_tantivy_code("ja"), "en");
        assert_eq!(d.to_tantivy_code("xx"), "en");
    }
}
