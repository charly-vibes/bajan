<!-- SPECODELIC:START -->
## Specodelic — spec format rules (managed block)

This repo's `specs/`-style markdown spec files (YAML frontmatter +
fixed-schema tables) are linted by `spk` (crates.io: specodelic).
Write specs so `spk lint` passes; embedded format revision: specodelic.md Revision 8

### Lint rules (every violation names its `rule_id`)

- `linter.frontmatter_valid` — frontmatter `kind` must be `intent` — the only top-level intent kind
- `linter.id_matches_file` — frontmatter `id` must equal the filename stem with `-` mapped to `.` (`_` is literal)
- `linter.unique_id` — every row id in a file must be unique across all of the file's tables
- `linter.guard_required` — every transition must carry a non-null guard that cites an invariant Constraint
- `linter.model_present` — the Model section must contain both a States list and a Transitions table (empty-but-present beats absent)
- `linter.ears_syntax` — the intent statement must contain an imperative `SHALL` and match one of the five EARS patterns
- `linter.no_conjoined_id` — an id must not encode two capabilities joined by `and`/`or`
- `linter.no_universal_in_id` — an id must not contain a universal token (all/every/any/always/never)
- `linter.total_refs` — every structured-field [[link]] must resolve to a definition somewhere in the corpus
- `linter.coverage` — every constraint must have a deriving property (`∃ property.derives_from == <constraint>`)
- `linter.no_orphan_property` — every property must derive from at least one constraint

### Commands

- `spk lint <dir>` — check the invariants (fails with a hint on zero files)
- `spk graph <dir>` — typed reference graph + blast-radius
- `spk compile <files>` — emit TOML / proptest / TLA+ artifacts
- `spk explain [topic]` — the embedded format primer (works offline)
- `spk doctor` — diagnose workspace + block currency
- `spk feedback bug --dry-run` — file an issue against upstream

Refresh this block after upgrading: `spk init --force`.
<!-- SPECODELIC:END -->

<!-- ah:managed:start -->
## espectacular

Run `ah check` to verify spec-test correspondence before committing.

- `ah check` — validate all deployed specs
- `ah check --changes <name>` — validate with a change overlay
- `ah init` — set up or refresh espectacular project files
- `ah doctor` — diagnose setup issues
- `ah explain <topic>` — playbook guidance for finding kinds and suggested actions
- `ah doctor --enable <adapter>` — write adapter config into .espectacular/config.toml
- `ah signals` — emit dont drift signals
<!-- ah:managed:end -->

<!-- WAI:REFLECT:REF:START -->
## Accumulated Project Patterns

Project-specific conventions, gotchas, and architecture notes live in
`.wai/resources/reflections/`. Run `wai search "<topic>"` to retrieve relevant
context before starting research or creating tickets.

> **Before research or ticket creation**: always run `wai search "<topic>"` to
> check for known patterns. Do not rediscover what is already documented.

<!-- WAI:REFLECT:REF:END -->

<!-- BEADS:START -->
<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY.
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session
<!-- END BEADS INTEGRATION -->
<!-- BEADS:END -->

<!-- DONT:START -->
# DONT MANAGED BLOCK — DO NOT EDIT

This project uses `dont` for grounded-claim workflow.

At session start run `dont prime --json`.

Canonical agent instructions: `.dont/AGENTS.md`.

Edits inside this managed block will be overwritten by `dont doctor --fix`.
<!-- DONT:END -->

<!-- OPENSPEC:START -->
<!-- openspec: managed block -->
See openspec/ for spec-driven development.
<!-- OPENSPEC:END -->

<!-- WAI:START --># Workflow Tools

This project uses **wai** to track the *why* behind decisions — research,
reasoning, and design choices that shaped the code. Run `wai status` first
to orient yourself.

Detected workflow tools:
- **wai** — research, reasoning, and design decisions
- **beads** — issue tracking (tasks, bugs, dependencies). CLI command: **`bd`** (not `beads`)
- **openspec** — specifications and change proposals (see `openspec/AGENTS.md`)

> **CRITICAL**: Apply TDD and Tidy First throughout — not just when writing code:
> - **Planning/task creation**: each ticket should map to a red→green→refactor cycle; refactoring tasks must be separate tickets from feature tasks.
> - **Design**: define the test shape (inputs/outputs) before designing the implementation.
> - **Implementation**: write the failing test first, then make it pass, then tidy in a separate commit.

> **When beginning research or creating a ticket**: run `wai search "<topic>"` to check for existing patterns before writing new content.

## Quick Start

1. `wai sync` — ensure agent tools are projected
2. `wai status` — see active projects, phase, and suggestions
3. `bd ready` — find available work items

When context reaches ~40%: stop and tell the user — responses degrade past
this point. Recommend `wai close` then `/clear` to resume cleanly.
Do NOT skip `wai close` — it enables resume detection.

## Autonomous Work Policy

Proceed without routine confirmation when the next step is clear.
Do not ask to continue, fix, or commit — just do it.

**Stop and ask** only when:
- Conflicting requirements or ambiguous intent
- Destructive actions (data loss, force-push, drop table)
- Credentials, secrets, or external services not yet authorized
- Unresolved test failures after two attempts
- Push, deploy, or release — always get explicit authorization
- Context approaching 40% — recommend `wai close` then `/clear`

## Detailed Instructions

Full workflow reference — session lifecycle, capturing work, command cheat
sheets, cross-tool sync, and PARA structure — lives in **`.wai/AGENTS.md`**.
Read it at the start of your first session or when you need detailed guidance.

Keep this managed block so `wai init` can refresh the instructions.

<!-- WAI:END -->