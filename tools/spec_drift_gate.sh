#!/usr/bin/env bash
# spec-drift-gate — pre-commit check that the deployed openspec/ mirror is
# current with a fresh redeploy AND staged into this commit.
#
# After `tools/deploy_specs.py` runs, the WORKTREE mirror is always fresh by
# construction, so the gate's real question is: does the INDEX (what is being
# committed) carry that fresh mirror? The failure output names the remedy
# instead of diagnosing the wrong cause (beads gently-cgw):
#
#   - fresh mirror staged (worktree == index)          -> pass
#   - fresh mirror == HEAD, index differs              -> "stale copy is staged"
#   - fresh mirror != HEAD, index == HEAD (untouched)  -> "current but unstaged"
#   - fresh mirror has untracked new files             -> "unstaged new files"
#   - index holds an older mirror (differs from both)  -> "staged mirror is stale"
#
# Invoked from lefthook.yml (pre-commit: spec-drift-gate).
set -u
cd "$(git rev-parse --show-toplevel)" || exit 1
P=openspec/specs/

python3 tools/deploy_specs.py >/dev/null || exit 1

fail() {
  git --no-pager diff --stat HEAD -- "$P" >&2
  exit 1
}

wt_eq_head()  { git diff --quiet HEAD -- "$P"; }
wt_eq_index() { git diff --quiet -- "$P"; }
idx_eq_head() { git diff --quiet --cached HEAD -- "$P"; }

# New mirror files are invisible to git diff but must still be staged.
if [ -n "$(git ls-files --others --exclude-standard -- "$P")" ]; then
  echo "drift: deployed mirror has unstaged new files — git add openspec/ + specs/ (+ specodelic/) into this commit" >&2
  fail
fi

# Worktree mirror was just regenerated; if the index matches it, the fresh
# mirror is staged and the protocol is satisfied.
if wt_eq_index; then
  exit 0
fi

if wt_eq_head; then
  # Mirror is current, but the index holds a different (older) copy.
  echo "drift: deployed mirror is current, but a stale copy is staged — git add $P into this commit" >&2
  fail
fi

if idx_eq_head; then
  echo "drift: deployed mirror is current but unstaged — git add openspec/ + specs/ (+ specodelic/) into this commit" >&2
else
  echo "drift: staged mirror is stale — redeploy (python3 tools/deploy_specs.py) and git add openspec/ + specs/ (+ specodelic/)" >&2
fi
fail
