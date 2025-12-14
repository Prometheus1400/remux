# remux

remux is a modern, opinionated terminal multiplexer aimed to be a **tmux replacement**.

While tmux is incredibly powerful, it is also notoriously arcane to configure, difficult to extend cleanly, and unfriendly to newcomers. remux focuses on **simplicity, clarity, and extensibility** without sacrificing performance.

> **Status:** 🚧 Active development. Don't expect it to work yet.

---

## Motivation

If you've used tmux long enough, you've likely run into one or more of the following:

- Configuration requires learning a custom DSL
- Keybindings are hard to reason about and share
- Plugins rely on shell hacks and fragile hooks

remux is an attempt to reimagine terminal multiplexing with:

- A **clean, explicit configuration format**
- A **real plugin system** (not shell scripts glued together)
- Predictable defaults that work out of the box

---

## Non-Goals

remux is **not** trying to:

- Be a drop-in tmux config replacement
- Support every obscure tmux feature
- Preserve decades of backwards compatibility

If you want tmux-as-it-is, tmux already does that very well.

---

## Core Concepts

remux keeps the core ideas that make tmux powerful.

### Layout Model

- **Workspace** – top-level container (similar to a tmux session)
- **View** – a collection of panes arranged in a layout
- **Pane** – a single PTY running a command

### Keybindings

Keybindings are:

- Declarative
- Context-aware
- Easy to discover
