"""
A/B benchmark: ContextBuilder prompt v1 vs v2.

Calls LLM directly with both prompts — no running server needed.

Usage:
    OPENAI_API_KEY=sk-... uv run python tests/research/rq_prompt_ab_test.py

Success criteria (v2 should be better or equal on all categories):
    memory_priority:   v2 >= v1
    document_priority: v2 >= v1
    temporal:          v2 > v1   (new date injection)
    knowledge_update:  v2 >= v1
    abstaining:        v2 >= v1  (explicit "I don't know" instruction)
    hybrid:            v2 >= v1
"""
import json
import os
from dataclasses import dataclass
from pathlib import Path

import openai

DATASETS = Path(__file__).parent / "datasets" / "rq_prompt_ab_cases.json"
MODEL = os.getenv("OPENAI_MODEL", "gpt-4o-mini")

# ── Prompt templates ──────────────────────────────────────────────────────────

SYSTEM_V1 = (
    "Answer based on the provided sources and personal memory facts (if any). "
    "If the answer is not in the sources or memory — say so explicitly. "
    "Cite document sources as [1], [2], etc. Cite memory facts as [memory]."
)

SYSTEM_V2 = """\
You are a question-answering assistant with access to two types of context:

1. PERSONAL MEMORY FACTS — atomic facts about the user (high signal, always current).
   Use these for questions about the user's life, preferences, location, work, etc.

2. DOCUMENT SOURCES — detailed content from indexed documents.
   Use these for specifics, evidence, quotes, and facts from documents.
   This is your primary source for detailed information.

Today's date: {date}

Instructions:
- For questions about the user, prioritize memory facts over documents.
- For questions about topics/documents, use document sources for details.
- Memory facts are summaries — if a document source contains more detail, use it.
- If neither memory nor documents contain the answer, say "I don't know" explicitly.
- Cite document sources as [1], [2], etc. Cite memory facts as [memory].\
"""


# ── Context builders ──────────────────────────────────────────────────────────

def build_context_v1(memories: list[str], chunks: list[dict]) -> str:
    sources = ""
    if memories:
        sources += "\nPersonal memory facts:\n---\n"
        for m in memories:
            sources += f"[memory] {m}\n"
    for i, chunk in enumerate(chunks, 1):
        sources += f"\n[{i}] {chunk.get('title', '')}\n---\n{chunk['content']}\n"
    return sources


def build_context_v2(memories: list[str], chunks: list[dict]) -> str:
    sources = ""
    if memories:
        sources += "\nPersonal memory facts:\n"
        for m in memories:
            sources += f"  [memory] {m}\n"
    if chunks:
        sources += "\nDocument sources:\n"
        for i, chunk in enumerate(chunks, 1):
            sources += f"\n[{i}] {chunk.get('title', '')}\n---\n{chunk['content']}\n"
    return sources


def build_prompt_v1(question: str, memories: list[str], chunks: list[dict], date: str) -> str:
    context = build_context_v1(memories, chunks)
    return f"{SYSTEM_V1}\n\nSources:\n{context}\nQuestion: {question}"


def build_prompt_v2(question: str, memories: list[str], chunks: list[dict], date: str) -> str:
    system = SYSTEM_V2.format(date=date)
    context = build_context_v2(memories, chunks)
    return f"{system}\n{context}\nQuestion: {question}"


# ── LLM call ─────────────────────────────────────────────────────────────────

def call_llm(client: openai.OpenAI, prompt: str) -> str:
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": prompt}],
        max_tokens=300,
        temperature=0,
    )
    return resp.choices[0].message.content.strip()


def judge_answer(client: openai.OpenAI, question: str, answer: str, expected: str) -> int:
    """LLM-as-judge: scores answer 1-5 on relevance and correctness."""
    prompt = f"""Rate this answer on a scale 1-5 (5=perfect, 1=wrong/hallucinated).

Question: {question}
Expected to contain: {expected}
Answer: {answer}

Scoring:
5 = Correct, contains expected info, no hallucination
4 = Mostly correct, minor issues
3 = Partially correct
2 = Mostly wrong but not harmful
1 = Wrong, hallucinates, or refuses when it shouldn't

Return ONLY a single digit 1-5."""
    resp = client.chat.completions.create(
        model=MODEL,
        messages=[{"role": "user", "content": prompt}],
        max_tokens=5,
        temperature=0,
    )
    text = resp.choices[0].message.content.strip()
    try:
        return int(text[0])
    except (ValueError, IndexError):
        return 3


# ── Scoring ───────────────────────────────────────────────────────────────────

@dataclass
class CaseResult:
    case_id: str
    category: str
    question: str
    answer_v1: str
    answer_v2: str
    keyword_v1: bool
    keyword_v2: bool
    anti_v1: bool   # True = anti_keyword present (bad)
    anti_v2: bool
    judge_v1: int
    judge_v2: int

    @property
    def v1_score(self) -> float:
        kw = 1.0 if self.keyword_v1 else 0.0
        anti = -0.5 if self.anti_v1 else 0.0
        return max(0.0, kw + anti)

    @property
    def v2_score(self) -> float:
        kw = 1.0 if self.keyword_v2 else 0.0
        anti = -0.5 if self.anti_v2 else 0.0
        return max(0.0, kw + anti)


def run_case(client: openai.OpenAI, case: dict) -> CaseResult:
    q = case["question"]
    memories = case.get("memories", [])
    chunks = case.get("chunks", [])
    date = case.get("question_date", "2026-06-02")
    expected = case["expected_keyword"].lower()
    anti = (case.get("anti_keyword") or "").lower()

    prompt_v1 = build_prompt_v1(q, memories, chunks, date)
    prompt_v2 = build_prompt_v2(q, memories, chunks, date)

    ans_v1 = call_llm(client, prompt_v1)
    ans_v2 = call_llm(client, prompt_v2)

    kw_v1 = expected in ans_v1.lower()
    kw_v2 = expected in ans_v2.lower()
    anti_v1 = bool(anti and anti in ans_v1.lower())
    anti_v2 = bool(anti and anti in ans_v2.lower())

    judge_v1 = judge_answer(client, q, ans_v1, expected)
    judge_v2 = judge_answer(client, q, ans_v2, expected)

    return CaseResult(
        case_id=case["id"],
        category=case["category"],
        question=q,
        answer_v1=ans_v1,
        answer_v2=ans_v2,
        keyword_v1=kw_v1,
        keyword_v2=kw_v2,
        anti_v1=anti_v1,
        anti_v2=anti_v2,
        judge_v1=judge_v1,
        judge_v2=judge_v2,
    )


# ── Report ────────────────────────────────────────────────────────────────────

def print_report(results: list[CaseResult]) -> bool:
    categories = sorted({r.category for r in results})

    print("\n" + "=" * 70)
    print(f"{'CASE':<35} {'V1':>4} {'V2':>4} {'J1':>3} {'J2':>3}  {'Δ':>4}")
    print("-" * 70)

    by_cat: dict[str, list[CaseResult]] = {}
    for r in results:
        by_cat.setdefault(r.category, []).append(r)

    all_pass = True
    for cat in categories:
        cat_results = by_cat[cat]
        print(f"\n[{cat}]")
        for r in cat_results:
            delta = r.v2_score - r.v1_score
            delta_str = f"+{delta:.1f}" if delta > 0 else f"{delta:.1f}"
            mark = "✓" if r.v2_score >= r.v1_score else "✗"
            print(f"  {mark} {r.case_id:<32} {r.v1_score:>4.1f} {r.v2_score:>4.1f} {r.judge_v1:>3} {r.judge_v2:>3}  {delta_str:>4}")

        v1_avg = sum(r.v1_score for r in cat_results) / len(cat_results)
        v2_avg = sum(r.v2_score for r in cat_results) / len(cat_results)
        j1_avg = sum(r.judge_v1 for r in cat_results) / len(cat_results)
        j2_avg = sum(r.judge_v2 for r in cat_results) / len(cat_results)
        cat_pass = v2_avg >= v1_avg
        if not cat_pass:
            all_pass = False
        mark = "✓" if cat_pass else "✗"
        print(f"  {mark} CATEGORY AVG: v1={v1_avg:.2f} v2={v2_avg:.2f} judge v1={j1_avg:.1f} v2={j2_avg:.1f}")

    print("\n" + "=" * 70)
    total_v1 = sum(r.v1_score for r in results) / len(results)
    total_v2 = sum(r.v2_score for r in results) / len(results)
    total_j1 = sum(r.judge_v1 for r in results) / len(results)
    total_j2 = sum(r.judge_v2 for r in results) / len(results)
    print(f"OVERALL  keyword: v1={total_v1:.2f}  v2={total_v2:.2f}  Δ={total_v2-total_v1:+.2f}")
    print(f"OVERALL  judge:   v1={total_j1:.1f}   v2={total_j2:.1f}   Δ={total_j2-total_j1:+.1f}")
    print()

    if all_pass and total_v2 >= total_v1:
        print("✓ V2 PASSED — improved or equal on all categories")
    else:
        print("✗ V2 FAILED — regression in one or more categories")

    return all_pass and total_v2 >= total_v1


def print_answers(results: list[CaseResult]):
    print("\n" + "=" * 70)
    print("DETAILED ANSWERS")
    print("=" * 70)
    for r in results:
        print(f"\n[{r.case_id}]")
        print(f"Q: {r.question}")
        print(f"V1: {r.answer_v1[:200]}")
        print(f"V2: {r.answer_v2[:200]}")


def main():
    api_key = os.getenv("OPENAI_API_KEY")
    if not api_key:
        print("OPENAI_API_KEY not set")
        return

    data = json.loads(DATASETS.read_text())
    cases = data["cases"]
    client = openai.OpenAI(api_key=api_key)

    print(f"Running {len(cases)} cases with model={MODEL}...")
    results = []
    for i, case in enumerate(cases, 1):
        print(f"  {i}/{len(cases)} {case['id']}...", end=" ", flush=True)
        r = run_case(client, case)
        results.append(r)
        delta = r.v2_score - r.v1_score
        print(f"v1={r.v1_score:.1f} v2={r.v2_score:.1f} Δ={delta:+.1f}")

    verbose = os.getenv("VERBOSE", "0") == "1"
    if verbose:
        print_answers(results)

    print_report(results)


if __name__ == "__main__":
    main()
