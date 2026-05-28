# ADR-0008: LLM Provider Abstraction — Custom Protocol

**Статус:** accepted
**Дата:** 2026-05-29
**Автор:** Александр Мельник

---

## Контекст

`ContextBuilder` передаёт собранный контекст в LLM для генерации ответа. Нужно решить как вызывать LLM — напрямую через конкретный SDK или через абстракцию. Система должна работать с Claude (Anthropic) и GPT-4o (OpenAI), без переписывания кода при смене провайдера.

## Рассматриваемые варианты

### Вариант 1: Прямые SDK вызовы

```python
from anthropic import Anthropic
client = Anthropic()
client.messages.create(model="claude-opus-4-7", messages=[...])
```

**Как работает:** конкретный SDK вызывается напрямую в `ContextBuilder`.
**Плюсы:**
- Нет абстракций — читаемый код
- Полный доступ к специфичным параметрам провайдера
**Минусы:**
- Смена провайдера = правки по всему коду
- Нельзя тестировать `ContextBuilder` без реального API
- Нарушает принцип зависимости от абстракций
**Риски:**
- При смене модели или провайдера — хирургический рефакторинг

### Вариант 2: Свой Protocol (LLMProvider)

Лёгкая абстракция через Python `Protocol`. Каждый провайдер — отдельный класс. `ContextBuilder` зависит только от протокола.

```python
class LLMResponse:
    answer: str
    input_tokens: int
    output_tokens: int

class LLMProvider(Protocol):
    def complete(self, prompt: str) -> LLMResponse: ...

class ClaudeProvider:
    def complete(self, prompt: str) -> LLMResponse: ...

class OpenAIProvider:
    def complete(self, prompt: str) -> LLMResponse: ...

class MockProvider:          # для тестов
    def complete(self, prompt: str) -> LLMResponse: ...
```

**Как работает:** `ContextBuilder` получает `LLMProvider` через DI. Провайдер выбирается по конфигу при старте.
**Плюсы:**
- Смена провайдера — только в конфиге (env `LLM_PROVIDER=claude|openai`)
- `ContextBuilder` тестируется с `MockProvider` без API
- Интерфейс минимален: один метод `complete`
- Нет внешних зависимостей кроме SDK провайдеров
**Минусы:**
- Нужно написать адаптер под каждый новый провайдер (~30 строк)
- Специфичные параметры (temperature, top_p) — через конфиг провайдера, не напрямую
**Риски:**
- При значительно разных API провайдеров абстракция может "протечь" — решается через конфиг провайдера

### Вариант 3: liteLLM

Сторонняя библиотека с единым API для 100+ LLM провайдеров.

```python
from litellm import completion
response = completion(model="claude-opus-4-7", messages=[...])
```

**Как работает:** liteLLM нормализует вызовы к разным провайдерам.
**Плюсы:**
- Нет написания адаптеров — всё из коробки
- Поддержка 100+ провайдеров
**Минусы:**
- Дополнительная зависимость (крупная библиотека)
- Чужой интерфейс — если liteLLM изменится или сломается, ломается приложение
- Для 2-3 провайдеров — избыточно
**Риски:**
- Внешняя зависимость в критическом пути каждого запроса

### Вариант 4: LangChain / LlamaIndex

Абстракции LLM внутри больших AI-фреймворков.

**Плюсы:**
- Экосистема готовых интеграций
**Минусы:**
- Тянут весь фреймворк как зависимость
- Избыточно, когда нужна только LLM-абстракция
**Риски:**
- Сильная привязка к фреймворку

## Решение

Выбрал **Вариант 2: Свой Protocol**.

Интерфейс LLM для Memex минимален: передать prompt, получить ответ. Этого достаточно для одного метода `complete`. Писать адаптер под провайдер — ~30 строк кода, это не overhead. Взамен получаем: тестируемость без API, смену провайдера через env, нулевые дополнительные зависимости. liteLLM разумен при 5+ провайдерах — сейчас их два.

**Конфигурация провайдера:**
```
LLM_PROVIDER=claude          # или openai
LLM_MODEL=claude-opus-4-7    # конкретная модель
LLM_MAX_TOKENS=2048
LLM_TEMPERATURE=0.1
```

**Расположение:** `src/llm/` — отдельный модуль.
```
src/llm/
├── protocol.py       ← LLMProvider Protocol + LLMResponse dataclass
├── claude.py         ← ClaudeProvider
├── openai.py         ← OpenAIProvider
└── factory.py        ← create_provider(config) → LLMProvider
```

## Последствия

**Придётся:**
- Создать `src/llm/` модуль с Protocol и двумя провайдерами
- `ContextBuilder` получает `LLMProvider` через DI (не создаёт сам)
- `factory.py` читает env и возвращает нужный провайдер

**Стало проще:**
- Тесты `ContextBuilder` — с `MockProvider`, без API-ключей
- Смена модели или провайдера — только env переменные
- Добавить новый провайдер — один файл, один класс

**Стало невозможным:**
- Использовать специфичные параметры провайдера напрямую в `ContextBuilder`
  (решается через конфиг провайдера при инициализации)

**Направление зависимостей:**
```
retrieval/context_builder.py → llm/protocol.py   (зависит от абстракции)
llm/claude.py                → anthropic SDK      (деталь реализации)
llm/openai.py                → openai SDK         (деталь реализации)
llm/factory.py               → llm/claude.py, llm/openai.py
api/dependencies.py          → llm/factory.py     (DI точка входа)
```
