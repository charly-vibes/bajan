#!/usr/bin/env python3
"""Check every relative link in the generated kaya dossier.

Purpose: catch a committed dossier page that points at a target that does not
exist before it ships to the site.
Responsibilities: walk docs/research/ and docs/public/, resolve every relative
markdown link against its page, and fail naming each unresolved target.
Rationale: the check is internal-only — no network access, no HTML parser
dependency — and it judges exactly the committed bytes (vendored from
kaya/scripts/check-links.py, adapted to markdown instead of built HTML).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

LINK = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


def is_external(target: str) -> bool:
    if not target or target.startswith("#"):
        return True
    if target.startswith("//"):
        return True
    parts = urlsplit(target)
    return bool(parts.scheme)


def resolve(page: Path, target: str) -> Path | None:
    """The filesystem target a relative link must reach, or None if absent."""
    parts = urlsplit(target)
    relative = unquote(parts.path)
    if not relative:
        return None
    candidate = (page.parent / relative).resolve()
    return candidate if candidate.is_file() else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--docs",
        nargs="*",
        default=["docs/research", "docs/public"],
        help="generated dossier directories to walk",
    )
    args = parser.parse_args()

    failures = 0
    pages = 0
    for doc_dir in args.docs:
        root = Path(doc_dir)
        if not root.is_dir():
            print(f"check-links: {doc_dir} is missing; build the dossier first", file=sys.stderr)
            return 2
        for page in sorted(root.rglob("*.md")):
            pages += 1
            text = page.read_text(encoding="utf-8", errors="replace")
            for target in LINK.findall(text):
                if is_external(target):
                    continue
                if resolve(page, target) is None:
                    failures += 1
                    print(f"unresolved link: {page} -> {target}", file=sys.stderr)

    if failures:
        print(f"check-links: {failures} unresolved link(s) in {pages} page(s)", file=sys.stderr)
        return 1
    print(f"check-links: {pages} page(s), 0 unresolved links")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
