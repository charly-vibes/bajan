# wai Workflow Reference

> This file is managed by `wai init`. Do not edit manually.
> Changes will be overwritten on the next init.


## Starting a Session

1. Run `wai sync` to ensure all agent tools and skills are correctly projected.
2. Run `wai status` to see active projects, current phase, and suggestions.
3. Check the phase — it tells you what kind of work is expected:
   - **research** → gather information, explore options
   - **design** → make architectural decisions
   - **plan** → break work into tasks
   - **implement** → write code, guided by research/plans
   - **review** → validate against plans
   - **archive** → wrap up
4. Read existing artifacts with `wai search "<topic>"` before starting new work.

## Capturing Work

Record the reasoning behind your work, not just the output:

```bash
wai add research "findings"         # What you learned, trade-offs
wai add plan "approach"             # How you'll implement, why
wai add design "decisions"          # Architecture choices, rationale
wai add research --file notes.md    # Import longer content
```

Use `--project <name>` if multiple projects exist. Otherwise wai picks the first one.

Phases are a guide, not a gate. Use `wai phase show` / `wai phase next`.

## Ending a Session

Before saying "done", run this checklist:

```
[ ] wai handoff create <project>   # capture context for next session
[ ] wai reflect                    # update CLAUDE.md with project patterns (every ~5 sessions)
[ ] git add <files> && git commit  # commit code + handoff
```

### Autonomous Loop

One task per session. The resume loop:

1. `wai prime` — orient (shows ⚡ RESUMING if mid-task)
2. Work on the single task
3. `wai close` — capture state (run this before every `/clear`)
4. `git add <files> && git commit`
5. `/clear` — fresh context

→ Next session: `wai prime` shows RESUMING with exact next steps.

When context reaches ~40%: stop and tell the user — responses degrade past
this point. Recommend `wai close` then `/clear` to resume cleanly.
Do NOT skip `wai close` — it enables resume detection.

## Quality Gate

Before your final commit or response, produce a quality ledger:

```
Changed  — files/modules touched and why
Verified — commands run to confirm correctness (test, build, lint)
Review   — what was reviewed, by whom/what (self, ro5, pair)
Risks    — known risks, edge cases, or deferred concerns
Next     — follow-up work, if any
```

The ledger lives in the commit message or session handoff, not in code.

## Quick Reference

### wai
```bash
wai status                    # Project status and next steps
wai add research "notes"      # Add research artifact
wai add plan "plan"           # Add plan artifact
wai add design "design"       # Add design artifact
wai add skill <name>          # Scaffold a new agent skill
wai search "query"            # Search across artifacts
wai search --tag <tag>        # Filter by tag (repeatable)
wai search --latest           # Most recent match only
wai why "why use TOML?"       # Ask why (LLM-powered oracle)
wai why src/config.rs         # Explain a file's history
wai reflect                   # Synthesize project patterns into CLAUDE.md
wai close                     # Session handoff + pending-resume signal
wai phase show                # Current phase
wai doctor                    # Workspace health
wai pipeline list             # List pipelines
wai pipeline start <n> --topic=<t>  # Start a run; set WAI_PIPELINE_RUN=<id>
wai pipeline next             # Advance to next step
```

## Structure

The `.wai/` directory organizes artifacts using the PARA method:
- **projects/** — active work with phase tracking and dated artifacts
- **areas/** — ongoing responsibilities (no end date)
- **resources/** — reference material, agent configs, templates
- **archives/** — completed or inactive items

Do not edit `.wai/config.toml` directly. Use `wai` commands instead.
