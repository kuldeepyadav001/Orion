#!/usr/bin/env python3
"""Measure the character -> token density of Orion's own documentation
against the real bge-small-en-v1.5 server.

Why this exists
---------------
`MAX_EMBED_CHARS` is a *character* budget, but the embedding model has a
*token* window (512). The two are not interchangeable: WordPiece spends one
token per character on digit runs and punctuation, and about five tokens per
hundred characters on ordinary prose. A chunk that fits the character
budget can therefore overflow the token window, and the server rejects the
whole request with HTTP 500 -- which today fails the entire document.

This script measures the real distribution so the budget can be set from
data instead of from a guess about average English.
"""
import json
import os
import sys
import urllib.request

URL = "http://127.0.0.1:8082/v1/embeddings"
TOKEN = "orion-eval-token"
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
DOCS = [
    "docs/PROJECT-STATE.md",
    "docs/ORION-PLAN.md",
    "docs/ARCHITECTURE.md",
    "docs/SECURITY.md",
    "docs/RUNNING-IT.md",
    "README.md",
]


def embed_one(text):
    body = json.dumps({"input": [text], "model": "bge"}).encode()
    req = urllib.request.Request(
        URL,
        data=body,
        headers={
            "Authorization": "Bearer " + TOKEN,
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            d = json.loads(r.read())
            return r.status, d.get("usage", {}).get("prompt_tokens", -1)
    except urllib.error.HTTPError as e:
        return e.code, -1


def main():
    size = int(sys.argv[1]) if len(sys.argv) > 1 else 1200
    rows = []
    for rel in DOCS:
        path = os.path.join(ROOT, rel)
        with open(path, encoding="utf-8") as fh:
            text = fh.read()
        for i in range(0, len(text), size):
            piece = text[i : i + size]
            status, tokens = embed_one(piece)
            rows.append((rel, len(piece), tokens, status))

    ok = [r for r in rows if r[3] == 200]
    bad = [r for r in rows if r[3] != 200]
    ratios = sorted(r[1] / r[2] for r in ok if r[2] > 0)
    print(f"piece size      : {size} chars")
    print(f"pieces          : {len(rows)}  ({len(bad)} rejected)")
    if ok:
        print(f"tokens  min/med/max : {min(r[2] for r in ok)} / "
              f"{sorted(r[2] for r in ok)[len(ok)//2]} / {max(r[2] for r in ok)}")
        print(f"chars per token: min {ratios[0]:.2f}  "
              f"p10 {ratios[len(ratios)//10]:.2f}  "
              f"median {ratios[len(ratios)//2]:.2f}  "
              f"max {ratios[-1]:.2f}")
    for r in bad[:10]:
        print(f"  REJECTED {r[0]} chars={r[1]} status={r[3]}")
    if ok:
        print(f"worst accepted piece : {max(r[1] for r in ok)} chars / "
              f"{max(r[2] for r in ok)} tokens")
        worst = max(ok, key=lambda r: r[2])
        print(f"  from {worst[0]}")
    if bad:
        worst = max(bad, key=lambda r: r[1])
        print(f"smallest rejected    : {min(r[1] for r in bad)} chars")


if __name__ == "__main__":
    main()
