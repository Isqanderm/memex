"""
Memory quality eval against a running memex (Rust) server.

For each row of fixtures/memory_eval.jsonl: remember the text (and the optional
update_text), diff the scope listing to see which facts were extracted, match them to
expected facts by token-F1 >= 0.6, then run the row's search queries.

Usage:
    uv run python tests/research/memory_eval.py --base-url http://localhost:8000 --scope eval
    uv run python tests/research/memory_eval.py --model-label qwen2.5-7b --keep
    uv run python tests/research/memory_eval.py --search-only   # threshold sweeps without the LLM

Prints facts precision / recall, search hit@1 / hit@3 and updates detected.
Always exits 0 (research tool). Results go to results/<date>-<model-label>.json.
"""
import argparse
import json
import re
import string
from datetime import date
from pathlib import Path

import httpx

HERE = Path(__file__).parent
DATASET = HERE / "fixtures" / "memory_eval.jsonl"
RESULTS_DIR = HERE / "results"
F1_THRESHOLD = 0.6
_PUNCT = re.compile(f"[{re.escape(string.punctuation)}«»“”—–…]")


def normalize(text: str) -> list[str]:
    return _PUNCT.sub(" ", text.lower()).split()


def token_f1(a: str, b: str) -> float:
    ta, tb = normalize(a), normalize(b)
    if not ta or not tb:
        return 0.0
    common = 0
    pool = list(tb)
    for t in ta:
        if t in pool:
            pool.remove(t)
            common += 1
    if common == 0:
        return 0.0
    p, r = common / len(ta), common / len(tb)
    return 2 * p * r / (p + r)


def match_facts(expected: list[str], extracted: list[dict]) -> dict[int, dict]:
    """Greedy one-to-one matching: expected index -> extracted memory, F1 >= threshold."""
    pairs = sorted(
        ((token_f1(e, m["content"]), i, j) for i, e in enumerate(expected) for j, m in enumerate(extracted)),
        reverse=True,
    )
    matched: dict[int, dict] = {}
    used: set[int] = set()
    for score, i, j in pairs:
        if score < F1_THRESHOLD:
            break
        if i in matched or j in used:
            continue
        matched[i] = extracted[j]
        used.add(j)
    return matched


def list_scope(client: httpx.Client, scope: str) -> list[dict]:
    r = client.get("/api/memory/list", params={"scope": scope})
    r.raise_for_status()
    return r.json()


def remember(client: httpx.Client, text: str, scope: str) -> dict:
    r = client.post("/api/memory/remember", json={"content": text, "source": "explicit", "scope": scope})
    r.raise_for_status()
    return r.json()


def run_row(client: httpx.Client, row: dict, scope: str, search_only: bool = False) -> dict:
    if search_only:
        # facts already in the scope (seeded by a previous --keep run): match against all of them
        matched = match_facts(row["expected_facts"], list_scope(client, scope))
        return {
            "id": row["id"], "extracted": [], "n_extracted": 0, "n_matched_extracted": 0,
            "n_expected": len(row["expected_facts"]), "n_matched_expected": len(matched),
            "first_facts_extracted": None, "expected_updates": None, "updates": None,
            "matched_ids": {str(i): m["id"] for i, m in matched.items()},
        }
    before = {m["id"] for m in list_scope(client, scope)}
    res = remember(client, row["text"], scope)
    updates = None
    if row.get("update_text"):
        updates = remember(client, row["update_text"], scope).get("memories_updated", 0)
    extracted = [m for m in list_scope(client, scope) if m["id"] not in before]
    matched = match_facts(row["expected_facts"], extracted)
    return {
        "id": row["id"],
        "extracted": [m["content"] for m in extracted],
        "n_extracted": len(extracted),
        "n_matched_extracted": len(matched),
        "n_expected": len(row["expected_facts"]),
        "n_matched_expected": len(matched),
        "first_facts_extracted": res.get("facts_extracted"),
        "expected_updates": row.get("expected_updates"),
        "updates": updates,
        "matched_ids": {str(i): m["id"] for i, m in matched.items()},
    }


def run_queries(client: httpx.Client, row: dict, result: dict, scope: str, top_k: int = 3) -> list[dict]:
    out = []
    for q in row.get("queries", []):
        mid = result["matched_ids"].get(str(q["expect_fact"]))
        hits, p3 = [], None
        if mid:
            r = client.post("/api/memory/search", json={"query": q["q"], "scope": scope, "top_k": top_k})
            r.raise_for_status()
            found = r.json()["hits"][:3]
            hits = [h["id"] for h in found]
            if found:
                p3 = sum(any(token_f1(e, h["content"]) >= F1_THRESHOLD for e in row["expected_facts"]) for h in found) / len(found)
        out.append({"p3": p3, 
            "row": row["id"], "q": q["q"], "matched": bool(mid),
            "hit1": bool(mid) and hits[:1] == [mid], "hit3": bool(mid) and mid in hits[:3],
        })
    return out


def ratio(a: int, b: int) -> float:
    return a / b if b else 0.0


def summarize(results: list[dict], queries: list[dict]) -> dict:
    p3 = [q["p3"] for q in queries if q.get("p3") is not None]
    upd = [r for r in results if r["expected_updates"] is not None]
    return {
        "facts_precision": ratio(sum(r["n_matched_extracted"] for r in results), sum(r["n_extracted"] for r in results)),
        "facts_recall": ratio(sum(r["n_matched_expected"] for r in results), sum(r["n_expected"] for r in results)),
        "hit_at_1": ratio(sum(q["hit1"] for q in queries), len(queries)),
        "hit_at_3": ratio(sum(q["hit3"] for q in queries), len(queries)),
        "precision_at_3": sum(p3) / len(p3) if p3 else 0.0,
        "updates_detected": sum(1 for r in upd if (r["updates"] or 0) >= r["expected_updates"]),
        "updates_total": len(upd),
        "rows": len(results),
        "queries": len(queries),
    }


def print_report(s: dict) -> None:
    print(f"{'metric':<22}{'value':>10}")
    print("-" * 32)
    print(f"{'facts precision':<22}{s['facts_precision']:>10.2f}")
    print(f"{'facts recall':<22}{s['facts_recall']:>10.2f}")
    print(f"{'search hit@1':<22}{s['hit_at_1']:>10.2f}")
    print(f"{'search hit@3':<22}{s['hit_at_3']:>10.2f}")
    print(f"{'search precision@3':<22}{s['precision_at_3']:>10.2f}")
    print(f"{'updates detected':<22}{s['updates_detected']:>7}/{s['updates_total']}")
    print(f"({s['rows']} rows, {s['queries']} queries)")


def cleanup(client: httpx.Client, scope: str) -> int:
    items = list_scope(client, scope)
    for m in items:
        client.delete(f"/api/memory/{m['id']}")
    return len(items)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base-url", default="http://localhost:8000")
    ap.add_argument("--scope", default="eval")
    ap.add_argument("--model-label", default="local", help="used only for the results filename")
    ap.add_argument("--dataset", default=str(DATASET))
    ap.add_argument("--search-only", action="store_true",
                    help="skip remember and cleanup; use facts already in the scope (seed with a previous --keep run)")
    ap.add_argument("--keep", action="store_true", help="do not delete the scope's facts at the end")
    args = ap.parse_args()

    rows = [json.loads(line) for line in Path(args.dataset).read_text().splitlines() if line.strip()]
    results, queries = [], []
    with httpx.Client(base_url=args.base_url, timeout=300) as client:
        try:
            for row in rows:
                res = run_row(client, row, args.scope, args.search_only)
                results.append(res)
                queries.extend(run_queries(client, row, res, args.scope))
                print(f"{row['id']}: extracted {res['n_extracted']}, matched {res['n_matched_expected']}/{res['n_expected']}", flush=True)
        except httpx.HTTPError as e:
            print(f"request failed: {e}")
        finally:
            if not (args.keep or args.search_only):
                print(f"cleaned up {cleanup(client, args.scope)} facts in scope {args.scope!r}")

    summary = summarize(results, queries)
    print()
    print_report(summary)
    RESULTS_DIR.mkdir(exist_ok=True)
    if args.search_only:
        args.model_label += "-search-only"
    label = re.sub(r"[^\w.-]", "-", args.model_label)
    out = RESULTS_DIR / f"{date.today().isoformat()}-{label}.json"
    out.write_text(json.dumps({"summary": summary, "rows": results, "queries": queries}, ensure_ascii=False, indent=2))
    print(f"results: {out}")


if __name__ == "__main__":
    main()
