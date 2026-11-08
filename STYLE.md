# Style

These rules apply to every file in every RevolaOS repository.

## Documentation

- Write for a person who knows the domain but has not read the code. Plain sentences, one idea each.
- Use a table when it is shorter than the prose would be.
- Describe current behaviour, interfaces, constraints and the reasoning needed to maintain the code. Nothing else.
- Say what is missing with a one-line TODO that names the missing piece.
- No filler, no restating what the code already says, no marketing.

Never committed, in docs or code:

| Not allowed | Where it goes instead |
|---|---|
| Development history ("changed to", "previously", "now uses") | commit message |
| Change logs | release page |
| Session logs, handoff notes, dated progress, status reports | conversation or local ignored notes |
| Notes addressed to LLMs, echoed prompts | nowhere |
| Measurements and benchmark results | pull request description |

## Code comments

Comment only what the code cannot say: hardware facts, Apple struct layouts, why this hook site and not another, invariants, units. No comments that narrate the code.

## Code

- Complexity matches need. No abstraction for one caller, no layer that only forwards, no registry with one user.
- One concern per directory. Extensions never reach into core internals.
- Apple addresses come only from the generated symbols header, in core and extensions alike. No literal addresses in C.
- Every change reports its resident and peak RAM in the pull request. An increase needs a stated reason.
- C: C11, `-Os`, Thumb for C code, ARM only where a hook entry or a measured hot path needs it. Python: Ruff, line length 88. Rust: `cargo fmt`, `cargo clippy` clean.

## Reversing documentation

One file per Apple subsystem per firmware version. Addresses live in the symbols file; prose uses symbol names.

## Commits

Subject: `type(scope): summary`, lower case, imperative, no period, at most 72 characters. Body says why. Ported code cites the RepriseOS path and commit. Commit messages are the only place for history. No generated-by trailers.

| Type | Use |
|---|---|
| feat | new behaviour |
| fix | corrected behaviour |
| perf | less RAM or time, same behaviour |
| refactor | same behaviour, different code |
| docs | documentation only |
| test | tests only |
| build | build scripts, toolchain, dependencies |
| ci | workflows |
| chore | everything else, such as repository setup |

Scope is the directory or component touched, for example `core`, `loader`, `symbols`, `installer`, `workflows`, `repo`.

Every repository ships `.githooks/commit-msg` and a workflow that rejects other subjects. Enable the hook once per clone:

```sh
git config core.hooksPath .githooks
```
