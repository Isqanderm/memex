# ADR-0013: MarkItDownAdapter — Microsoft MarkItDown для PPTX, XLSX, EPUB

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

После реализации адаптеров для PDF, DOCX, MD, TXT возникла необходимость поддержать ещё три формата: PowerPoint (PPTX), Excel (XLSX/XLS) и EPUB. Писать нативные адаптеры для каждого означает добавить `python-pptx`, `openpyxl`, и epub-парсер как прямые зависимости — плюс написать логику извлечения структуры для каждого.

Параллельно была изучена библиотека Microsoft MarkItDown — универсальный Python-конвертер файлов в Markdown, созданный специально для LLM/RAG пайплайнов.

## Рассматриваемые варианты

### Вариант 1: Нативные адаптеры для каждого формата

Отдельные адаптеры на `python-pptx`, `openpyxl`, `ebooklib`.

**Плюсы:**
- Полный контроль над извлечением структуры
- Нет промежуточного шага конвертации
- Прямой доступ к метаданным (номер слайда в PPTX и т.д.)
**Минусы:**
- Три новые зависимости + код для каждой
- Парсинг таблиц Excel в осмысленные секции — нетривиально
**Риски:**
- Разное качество структурирования для каждого формата

### Вариант 2: MarkItDown как backend для всех адаптеров (включая PDF и DOCX)

Заменить все адаптеры на один универсальный через MarkItDown.

**Плюсы:**
- Один адаптер вместо пяти
- Поддержка 10+ форматов из коробки
**Минусы:**
- **Критично:** теряются `page_number` из PDF — citations без номеров страниц
- Двойной парсинг для PDF: MarkItDown → Markdown string → MarkdownAdapter
- Markdown вывод из PDF-файлов часто плоский (без заголовков) — pdfminer не понимает визуальную структуру
**Риски:**
- MarkItDown v0.1.x — API нестабилен

### Вариант 3: MarkItDown как fallback — только для новых форматов (выбран)

PdfAdapter и DocxAdapter остаются на нативных библиотеках. MarkItDown используется только для PPTX, XLSX/XLS, EPUB — форматов где у нас нет специализированной реализации.

**Плюсы:**
- Сохраняем `page_number` из PDF (критично для citations)
- Сохраняем heading structure из DOCX (python-docx даёт уровни напрямую)
- PPTX, XLSX, EPUB — бесплатно, без написания нативных парсеров
- `MarkItDownAdapter` встаёт последним в registry — не конкурирует с нативными адаптерами
**Минусы:**
- MarkItDown — дополнительная зависимость (~5 транзитивных пакетов)
- Двухшаговая конвертация для PPTX/XLSX: файл → Markdown string → `_split_by_headings` → секции
- EPUB extra (`markitdown[epub]`) не существует в v0.1.6 — EPUB парсится через базовые зависимости (beautifulsoup4)
**Риски:**
- MarkItDown v0.1.x: API может измениться. Изоляция в одном файле (`markitdown_adapter.py`) снижает риск.

## Решение

Выбрал **Вариант 3: MarkItDown как fallback**.

Главный инвариант системы — citations показывают номер страницы PDF. Это реальная ценность для пользователя ("стр. 47") и теряется при переходе на MarkItDown для PDF. Поэтому PdfAdapter остаётся на pypdf.

Для PPTX, XLSX, EPUB написание нативных парсеров — непропорциональные усилия при наличии готового инструмента. MarkItDown создан Microsoft именно для LLM/RAG контекста, качество конвертации достаточное.

**Архитектура:**

```
AdapterRegistry (порядок важен — первый подходящий wins):
  1. PdfAdapter       ← pypdf, page_number
  2. DocxAdapter      ← python-docx, heading levels
  3. MarkdownAdapter  ← regex, heading structure
  4. TextAdapter      ← plain text
  5. MarkItDownAdapter ← PPTX, XLSX, XLS, EPUB (все остальное)
```

**Внутренняя реализация MarkItDownAdapter:**
```
файл → MarkItDown.convert() → Markdown string
     → MarkdownAdapter._split_by_headings() → List[Section]
     → ParsedDocument
```

Page numbers в ParsedDocument для этих форматов — `None` (эти форматы не имеют стабильной пагинации).

## Последствия

**Добавлено:**
- `src/adapters/markitdown_adapter.py` — `MarkItDownAdapter`
- `markitdown[pptx,xlsx]` в `pyproject.toml` (epub extra отсутствует в v0.1.6)
- `MarkItDownAdapter` зарегистрирован последним в `src/dependencies.py`
- 11 unit-тестов покрывают `can_handle` и парсинг XLSX/PPTX

**Форматы после изменения:**
| Формат | Адаптер | Page numbers | Heading structure |
|--------|---------|-------------|-------------------|
| PDF | PdfAdapter (pypdf) | ✅ | ❌ (визуальная) |
| DOCX | DocxAdapter (python-docx) | ❌ | ✅ |
| MD | MarkdownAdapter | ❌ | ✅ |
| TXT | TextAdapter | ❌ | ❌ |
| PPTX | MarkItDownAdapter | ❌ | ✅ (заголовки слайдов) |
| XLSX | MarkItDownAdapter | ❌ | ⚠️ (имена листов) |
| EPUB | MarkItDownAdapter | ❌ | ✅ (главы) |

**Путь к миграции:** если MarkItDown изменит API — правки только в `markitdown_adapter.py`. Остальной pipeline не затронут.
