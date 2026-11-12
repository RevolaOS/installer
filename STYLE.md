# Style

These rules apply to every file in every RevolaOS repository.

## Principles

1. **RAM first.** 64 MiB in total, about 20 MiB of heap shared with Apple's music database. Every pull request reports resident and peak bytes from the emulator heap profile. An increase needs a stated reason. A disabled extension costs zero bytes. Reuse what exists before adding anything.
2. **Performance second.** No work in audio callbacks beyond the DSP itself. No redraw of unchanged pixels. No disk reads for things that can be computed. No per-draw checks when a one-time hook can decide. Hot paths bind directly; the dispatcher runs only when two handlers share a site.
3. **Optimise before merge.** Measure, remove what measurement does not justify, and stop when the next step costs more clarity than it saves. "Works" is not the bar.
4. **Complexity matches need.** No abstraction for one caller, no layer that only forwards, no registry with one user. Defer anything with one user, delete anything with none. A person must be able to reason about the code.
5. **Clean structure.** One concern per directory. Extensions never reach into core internals. Apple addresses come only from the generated symbols header; no literal addresses in C.
6. **Measure, do not guess.** Emulator heap profile for every RAM claim; hardware for audio, cache and storage behaviour.

### Core or extension

Something is core only if the loader, SDK or settings store needs it, if it must act before the loader can run, or if the boot cannot recover without it. Everything else is an extension, however small. One extension per concern. If one feature cannot work without another, it is a module of that extension, not a dependency. Extensions bind core hook sites only; when no site covers the code, add one to the core.

## Documentation

- Write for a person who knows the domain but has not read the code. Plain sentences, one idea each.
- Use a table when it is shorter than the prose would be.
- Describe current behaviour, interfaces, constraints and the reasoning needed to maintain the code. Nothing else.
- Say what is missing with a one-line TODO that names the missing piece.
- No filler, no restating what the code already says, no marketing.

A repository's README says what the repository is, how it is laid out, how to build it and where the documentation is. Documentation lives in `docs/`, one page per topic, with an index in `docs/README.md`.

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

C: C11, `-Os`, Thumb for C code, ARM only where a hook entry or a measured hot path needs it. Python: Ruff, line length 88. Rust: `cargo fmt`, `cargo clippy` clean.

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
