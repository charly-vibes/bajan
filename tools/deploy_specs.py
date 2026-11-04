#!/usr/bin/env python3
"""Deploy specodelic corpus specs into the openspec layout.

Transform (per corpus file specs/<name>.md -> openspec/specs/<name>/spec.md),
under specodelic format Revision 18 (naming law):

  1. the frontmatter id is kept verbatim — a spec.md file derives its
     expected id from its PARENT DIRECTORY (`-` <-> `.` mapping), and the
     corpus naming law already matches (specs/ge-ascii_render.md declares
     id: ge.ascii_render; deployed dir ge-ascii_render/spec.md expects the
     same). A mismatch is a loud failure, never a silent rewrite.
  2. no link rewriting or cross-file downgrades — Revision 18 retired the
     `id: spec` self-containment; wiki-refs resolve corpus-wide, so
     [[cli.c1]] and [[ge.text_parser]] stay literal.
  3. a ## Requirements mirror is generated from the Properties table:
     one Scenario per property, each carrying a
     - **VERIFIES** [[<own-id>.<property-id>]] bullet (ah sync coverage link)
"""
import re
import sys
import pathlib

LINK = re.compile(r"\[\[([a-zA-Z0-9_.\-]+)\]\]")


def parse_row(line):
    return [c.strip() for c in line.strip().strip("|").split("|")]


def is_sep(cells):
    return all(set(c) <= set(":- ") and c for c in cells)


def properties_rows(lines):
    """Parse the ## Properties table into a list of cell dicts."""
    rows = []
    cols = None
    in_props = False
    for line in lines:
        stripped = line.strip()
        in_props = in_props or stripped == "## Properties"
        if not in_props:
            continue
        if stripped.startswith("## ") and stripped != "## Properties":
            break
        if not stripped.startswith("|"):
            continue
        cells = parse_row(stripped)
        if is_sep(cells):
            continue
        if cols is None:
            cols = [c.lower() for c in cells]
            continue
        rows.append(dict(zip(cols, cells)))
    return rows


def requirement_block(prop, own):
    pid = prop["id"]
    gen = prop.get("generator") or "the documented inputs"
    pred = prop.get("predicate") or ""
    return [
        f"#### Scenario: {pid}",
        "",
        f"- **WHEN** {gen}",
        f"- **THEN** {pred}",
        f"- **VERIFIES** [[{own}.{pid}]]",
        "",
    ]


def purpose_section(text):
    """Insert a ## Purpose section from the prose block after the H1 title.

    openspec strict validation requires "## Purpose" before "## Requirements".
    The corpus keeps a prose paragraph directly under the H1; promote it.
    Idempotent: skipped when a ## Purpose heading already exists.
    """
    if re.search(r"^## Purpose\s*$", text, flags=re.M):
        return text
    lines = text.splitlines()
    try:
        h1 = next(i for i, l in enumerate(lines) if l.startswith("# "))
    except StopIteration:
        return text
    first_h2 = next((i for i, l in enumerate(lines[h1:], h1) if l.startswith("## ")), len(lines))
    prose = [l for l in lines[h1 + 1:first_h2] if l.strip()]
    if not prose:
        prose = ["TBD — see the frontmatter statement."]
    block = ["## Purpose", ""] + prose + [""]
    return "\n".join(lines[:first_h2] + block + lines[first_h2:]) + "\n"


def requirements_mirror(text, own):
    mirror = [
        "## Requirements",
        "",
        "### Requirement: Property coverage mirror",
        "",
        "Every property row SHALL be verified by exactly one dedicated scenario;",
        "`ah sync` SHALL derive one contract per VERIFIES link.",
        "",
    ]
    for prop in properties_rows(text.splitlines()):
        mirror += requirement_block(prop, own)
    return text.rstrip("\n") + "\n\n" + "\n".join(mirror) + "\n"


def expected_id(stem):
    """Naming law: filename stem with `-` mapped to `.` (`_` is literal)."""
    return stem.replace("-", ".")


def deploy_one(path, out_root):
    text = path.read_text()
    own = re.search(r"^id:\s*(\S+)", text, re.M).group(1)
    expected = expected_id(path.stem)
    if own != expected:
        sys.exit(f"deploy: {path}: frontmatter id {own!r} != naming-law id {expected!r} — fix the corpus, do not rewrite ids on deploy")
    dest = out_root / path.stem / "spec.md"
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(requirements_mirror(purpose_section(text), own))
    print(f"deployed {path.name} -> {dest} (own id {own})")


def main():
    corpus = pathlib.Path("specs")
    out_root = pathlib.Path("openspec/specs")
    for path in sorted(corpus.glob("*.md")):
        deploy_one(path, out_root)


if __name__ == "__main__":
    sys.exit(main())
