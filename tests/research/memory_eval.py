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
Always exits 0 (research tool). Results go to results/<date>T<HHMMSS>-<model-label>.json.

Comparable search A/B: one --keep run (seed), then per server configuration
    --search-only --seed-results <seed results json>
which reuses the memory ids matched in the seed run instead of re-matching against the whole scope.
"""
import argparse
import json
import re
import string
from datetime import datetime
from pathlib import Path

import httpx

HERE = Path(__file__).parent
DATASET = HERE / "fixtures" / "memory_eval.jsonl"
RESULTS_DIR = HERE / "results"
F1_THRESHOLD = 0.6
TOP_K = 3
_EDGE_PUNCT = string.punctuation + "«»“”„—–…"


def normalize(text: str) -> list[str]:
    """Lowercase, split on whitespace, strip only edge punctuation: MEMEX_URL, 5.2.1, a@b.c stay one token."""
    return [t for t in (w.strip(_EDGE_PUNCT) for w in text.lower().split()) if t]


def has_exact_tokens(fact: str, exact_tokens: list) -> bool:
    """Every entry must appear verbatim (case-insensitive); an entry may be a list of alternatives."""
    low = fact.lower()
    return all(any(a.lower() in low for a in ([t] if isinstance(t, str) else t)) for t in exact_tokens)


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


def match_facts(expected: list[str], extracted: list[dict], exact_tokens: list[list] | None = None) -> dict[int, dict]:
    """Greedy one-to-one matching: expected index -> extracted memory, F1 >= threshold.

    exact_tokens is aligned with expected: exact_tokens[i] (may be empty) must appear verbatim
    in the fact matched to expected[i]."""
    pairs = sorted(
        ((token_f1(e, m["content"]), i, j) for i, e in enumerate(expected) for j, m in enumerate(extracted)
         if not exact_tokens or has_exact_tokens(m["content"], exact_tokens[i])),
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


def run_row(client: httpx.Client, row: dict, scope: str, search_only: bool = False,
            seed_ids: dict[str, dict[str, str]] | None = None) -> dict:
    if search_only:
        # facts already in the scope (seeded by a previous --keep run)
        if seed_ids is not None:
            ids = seed_ids.get(row["id"], {})
            return {
                "id": row["id"], "extracted": [], "n_extracted": 0, "n_matched_extracted": 0,
                "n_expected": len(row["expected_facts"]), "n_matched_expected": len(ids),
                "first_facts_extracted": None, "expected_updates": None, "updates": None,
                "matched_ids": ids,
            }
        matched = match_facts(row["expected_facts"], list_scope(client, scope), row.get("exact_tokens"))
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
    matched = match_facts(row["expected_facts"], extracted, row.get("exact_tokens"))
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


def run_queries(client: httpx.Client, row: dict, result: dict, scope: str, top_k: int = TOP_K) -> list[dict]:
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
    matched_q = [q for q in queries if q["matched"]]
    return {
        "queries_matched": len(matched_q),
        "hit_at_1_matched": ratio(sum(q["hit1"] for q in matched_q), len(matched_q)),
        "hit_at_3_matched": ratio(sum(q["hit3"] for q in matched_q), len(matched_q)),
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


def print_report(s: dict, search_only: bool = False) -> None:
    print(f"{'metric':<22}{'value':>10}")
    print("-" * 32)
    if search_only:
        print("facts precision/recall: n/a (search-only)")
    else:
        print(f"{'facts precision':<22}{s['facts_precision']:>10.2f}")
        print(f"{'facts recall':<22}{s['facts_recall']:>10.2f}")
    print(f"{'search hit@1':<22}{s['hit_at_1']:>10.2f}")
    print(f"{'search hit@3':<22}{s['hit_at_3']:>10.2f}")
    print(f"{'hit@1 (of matched)':<22}{s['hit_at_1_matched']:>10.2f}")
    print(f"{'hit@3 (of matched)':<22}{s['hit_at_3_matched']:>10.2f}")
    print(f"{'search precision@3':<22}{s['precision_at_3']:>10.2f}")
    print(f"{'updates detected':<22}{s['updates_detected']:>7}/{s['updates_total']}")
    print(f"({s['rows']} rows, {s['queries']} queries, {s['queries_matched']} with a matched fact)")


def cleanup(client: httpx.Client, scope: str) -> int:
    items = list_scope(client, scope)
    for m in items:
        client.delete(f"/api/memory/{m['id']}")
    return len(items)


def self_test() -> None:
    assert normalize("The MEMEX_URL is http://x:8000, v5.2.1.") == ["the", "memex_url", "is", "http://x:8000", "v5.2.1"]
    assert token_f1("User's MEMEX_URL is set", "user's memex_url is set") == 1.0
    assert token_f1("fastembed version 5.2.1", "fastembed version 5.2.0") < 1.0
    assert has_exact_tokens("User pins fastembed 5.2.1", ["5.2.1"])
    assert not has_exact_tokens("User pins fastembed 5.2.0", ["5.2.1"])
    assert has_exact_tokens("Surname Shevchenko-Zadneprovsky", [["Шевченко-Заднепровский", "shevchenko-zadneprovsky"]])
    exp = ["User's project uses fastembed version 5.2.1"]
    bad = [{"content": "User's project uses fastembed version 5.2.0"}]
    good = [{"content": "User's project uses fastembed version 5.2.1"}]
    assert match_facts(exp, bad, [["5.2.1"]]) == {}
    assert 0 in match_facts(exp, good, [["5.2.1"]])
    # en-11 shape: two expected facts, each with its own exact tokens, both extracted correctly
    exp2 = ["User's project uses fastembed version 5.2.1", "User's project uses tantivy 0.22"]
    got2 = [{"content": "User's project uses tantivy 0.22"}, {"content": "User's project uses fastembed version 5.2.1"}]
    assert sorted(match_facts(exp2, got2, [["5.2.1"], ["0.22"]])) == [0, 1]
    assert match_facts(exp2, [{"content": "User's project uses tantivy 0.21"}], [["5.2.1"], ["0.22"]]) == {}
    print("self-test ok")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base-url", default="http://localhost:8000")
    ap.add_argument("--scope", default="eval")
    ap.add_argument("--model-label", default="local", help="used only for the results filename")
    ap.add_argument("--dataset", default=str(DATASET))
    ap.add_argument("--search-only", action="store_true",
                    help="skip remember and cleanup; use facts already in the scope (seed with a previous --keep run)")
    ap.add_argument("--seed-results", help="with --search-only: results JSON of the --keep run whose matched_ids to reuse")
    ap.add_argument("--keep", action="store_true", help="do not delete the scope's facts at the end")
    ap.add_argument("--conflict-sim", help="label only: MEMORY_CONFLICT_SIMILARITY the server runs with")
    ap.add_argument("--retrieval-sim", help="label only: MEMORY_RETRIEVAL_SIMILARITY the server runs with")
    ap.add_argument("--self-test", action="store_true", help="run scoring self-checks and exit")
    args = ap.parse_args()

    if args.self_test:
        self_test()
        return

    seed_ids = None
    if args.seed_results:
        seed_ids = {r["id"]: r["matched_ids"] for r in json.loads(Path(args.seed_results).read_text())["rows"]}

    rows = [json.loads(line) for line in Path(args.dataset).read_text().splitlines() if line.strip()]
    results, queries = [], []
    error = None
    with httpx.Client(base_url=args.base_url, timeout=300) as client:
        try:
            for row in rows:
                res = run_row(client, row, args.scope, args.search_only, seed_ids)
                results.append(res)
                queries.extend(run_queries(client, row, res, args.scope))
                print(f"{row['id']}: extracted {res['n_extracted']}, matched {res['n_matched_expected']}/{res['n_expected']}", flush=True)
        except Exception as e:  # research tool: report and keep partial results
            error = str(e)
            print(f"aborted: {e}")

        summary = summarize(results, queries)
        print()
        print_report(summary, args.search_only)
        RESULTS_DIR.mkdir(exist_ok=True)
        label = re.sub(r"[^\w.-]", "-", args.model_label + ("-search-only" if args.search_only else ""))
        out = RESULTS_DIR / f"{datetime.now().strftime('%Y-%m-%dT%H%M%S')}-{label}.json"
        doc = {
            "config": {
                "base_url": args.base_url, "scope": args.scope, "model_label": args.model_label,
                "f1_threshold": F1_THRESHOLD, "top_k": TOP_K, "search_only": args.search_only,
                "seed_results": args.seed_results,
                "conflict_sim": args.conflict_sim, "retrieval_sim": args.retrieval_sim,
            },
            "summary": summary, "rows": results, "queries": queries,
        }
        if error:
            doc.update(error=error, aborted=True)
        out.write_text(json.dumps(doc, ensure_ascii=False, indent=2))
        print(f"results: {out}")

        if not (args.keep or args.search_only):
            try:
                print(f"cleaned up {cleanup(client, args.scope)} facts in scope {args.scope!r}")
            except Exception as e:
                print(f"cleanup failed: {e}")
                doc.update(error=f"{error + '; ' if error else ''}cleanup: {e}", aborted=True)
                out.write_text(json.dumps(doc, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
