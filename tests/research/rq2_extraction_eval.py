"""
RQ2: Fact extraction and relation resolution accuracy.

Usage:
    ANTHROPIC_API_KEY=sk-... uv run python tests/research/rq2_extraction_eval.py

Success criteria:
    Extraction precision >= 0.90
    Extraction recall    >= 0.80
    Relation accuracy    >= 0.85
    updates recall       >= 0.90
"""
import json
import os
from pathlib import Path

import anthropic

DATASETS = Path(__file__).parent / "datasets" / "rq2_extraction_cases.json"

EXTRACT_PROMPT = """\
Extract atomic facts about the user from the following text.
Rules:
- Each fact is one statement, no pronouns — use "User" as subject.
- Ignore facts with no lasting relevance (e.g., weather, third-party chitchat).
- If a fact is time-bound (e.g. "meeting tomorrow"), add "forget_after": "<ISO datetime close to the event>".
- For permanent facts, omit "forget_after".

Text: {text}

Return JSON only:
{{"facts": [{{"content": "...", "forget_after": "...or omit"}}]}}"""

RESOLVE_PROMPT = """\
New fact: "{new_fact}"

Existing similar facts:
{existing}

For each existing fact determine the relation of the new fact to it:
- updates: new fact contradicts and supersedes the old one
- extends: new fact adds detail without contradiction
- derives: new fact is logically inferred from the old one
- new: not meaningfully related

Return JSON only:
{{"relations": [{{"id": "...", "type": "updates|extends|derives|new"}}]}}"""


def call_llm(client: anthropic.Anthropic, prompt: str) -> str:
    msg = client.messages.create(
        model="claude-haiku-4-5-20251001",
        max_tokens=512,
        messages=[{"role": "user", "content": prompt}],
    )
    return msg.content[0].text


def extract_json(text: str) -> dict:
    start = text.find("{")
    end = text.rfind("}") + 1
    if start == -1 or end == 0:
        raise ValueError(f"No JSON object found in: {text[:100]}")
    return json.loads(text[start:end])


def run_extraction_eval(client, cases):
    tp, fp, fn = 0, 0, 0
    for case in cases:
        prompt = EXTRACT_PROMPT.format(text=case["input"])
        raw = call_llm(client, prompt)
        try:
            result = extract_json(raw)
            extracted = [f["content"].lower() for f in result.get("facts", [])]
        except Exception:
            extracted = []

        expected = [e.lower() for e in case["expected_facts"]]
        matched = sum(1 for e in expected if any(e[:30] in x for x in extracted))
        tp += matched
        fn += len(expected) - matched
        fp += max(0, len(extracted) - matched)
        print(f"  Input: {case['input'][:60]}")
        print(f"  Expected: {expected}")
        print(f"  Got: {extracted}")
        print()

    precision = tp / (tp + fp) if (tp + fp) > 0 else 0
    recall = tp / (tp + fn) if (tp + fn) > 0 else 0
    return precision, recall


def run_relation_eval(client, cases):
    correct = 0
    updates_tp, updates_total = 0, 0
    for case in cases:
        existing_str = "\n".join(
            f'  id={e["id"]}: "{e["content"]}"' for e in case["existing"]
        )
        prompt = RESOLVE_PROMPT.format(new_fact=case["new_fact"], existing=existing_str)
        raw = call_llm(client, prompt)
        try:
            result = extract_json(raw)
            relations = {r["id"]: r["type"] for r in result.get("relations", [])}
        except Exception:
            relations = {}

        for exp in case["expected"]:
            got = relations.get(exp["id"], "new")
            if got == exp["type"]:
                correct += 1
            if exp["type"] == "updates":
                updates_total += 1
                if got == "updates":
                    updates_tp += 1
            print(f"  new='{case['new_fact'][:40]}' vs id={exp['id']}")
            print(f"  expected={exp['type']}  got={got}")

    total = sum(len(c["expected"]) for c in cases)
    accuracy = correct / total if total > 0 else 0
    updates_recall = updates_tp / updates_total if updates_total > 0 else None
    return accuracy, updates_recall


def main():
    api_key = os.getenv("ANTHROPIC_API_KEY")
    if not api_key:
        print("ANTHROPIC_API_KEY not set — skipping live eval")
        return

    data = json.loads(DATASETS.read_text())
    client = anthropic.Anthropic(api_key=api_key)

    print("=== RQ2: Extraction accuracy ===")
    precision, recall = run_extraction_eval(client, data["extraction_cases"])
    print(f"Precision: {precision:.2f}  (target >= 0.90)")
    print(f"Recall:    {recall:.2f}  (target >= 0.80)")

    print("\n=== RQ2: Relation accuracy ===")
    accuracy, updates_recall = run_relation_eval(client, data["relation_cases"])
    print(f"Accuracy:       {accuracy:.2f}  (target >= 0.85)")
    if updates_recall is not None:
        print(f"updates recall: {updates_recall:.2f}  (target >= 0.90)")
    else:
        print("updates recall: N/A (no updates cases in dataset)")

    ok = precision >= 0.90 and recall >= 0.80 and accuracy >= 0.85
    if updates_recall is not None:
        ok = ok and updates_recall >= 0.90
    print(f"\n{'✓ RQ2 PASSED' if ok else '✗ RQ2 FAILED — iterate prompts before proceeding'}")


if __name__ == "__main__":
    main()
