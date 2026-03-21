# AGENTS.md

## Project Overview

`remux` is an in-progress terminal multiplexer intended to be a modern, opinionated replacement for `tmux`.
The repository is a Rust workspace focused on a clean architecture rather than compatibility with existing `tmux` behavior or configuration.

The current codebase is split into a small set of crates with clear responsibilities:

- `daemon/`: long-running backend process that owns sessions, panes, PTYs, rendering, layout, and actor-style coordination.
- `cli/`: user-facing client binary that parses commands, connects to the daemon over a Unix socket, and runs the terminal UI flow.
- `core/`: shared protocol, message, state, constants, and utility code used by both the daemon and CLI.
- `handle-macro/`: proc-macro crate that generates actor handle helpers.

## Architecture Notes

At a high level, the daemon is the system of record and the CLI is a thin client.
Communication between them happens through the shared types in `core/`.
Most meaningful behavioral changes should preserve that separation:

- daemon logic stays in `daemon/`
- transport/message contracts stay in `core/`
- terminal interaction and command entry stay in `cli/`
- repetitive actor handle boilerplate belongs in `handle-macro/`

This project is still in active development and not yet stable.
Favor code clarity and strong structure over temporary compatibility shims.

## Refactoring Guidance

It is acceptable to make large, cohesive refactors in one pass when the result is materially better.
Do not assume every change needs to be split into incremental or progressive refactors.
If the right solution is a substantial restructuring, take it.

Backwards compatibility is not a priority here.
Do not preserve old APIs, behaviors, configuration formats, or internal abstractions unless there is a concrete reason in the current codebase to keep them.
Prefer deleting obsolete paths over carrying them forward.

## Working Norms

- Keep module boundaries intentional and avoid leaking daemon internals into shared crates.
- Prefer simplifying architecture over layering on adapters, compatibility wrappers, or transitional glue.
- When changing protocol or message shapes, update both sides of the daemon/CLI boundary together.
- Add or update tests when practical, especially around rendering, protocol handling, and parsing.
- Use conventional commits if creating commits for this repository.

## Useful Commands

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo fmt --all`
