<h1 align="center">Duolo</h1>

<p align="center">Claude writes, Codex reviews. Run a team of coding agents in parallel on your own machine.</p>

<p align="center">
  <a href="https://github.com/OtterMind/duolo/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/OtterMind/duolo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center">English · <a href="README.zh-CN.md">简体中文</a></p>

Duolo is a local-first desktop app for macOS, Windows and Linux. Every task gets its own git worktree with the real agent CLI running inside it: Claude Code, Codex, OpenCode, pi or Grok. When the work is done, a second model reviews it read-only and you hand the findings back with one click. The sidebar shows the state of every task, and you can speak a request instead of typing it. Your code and sessions stay on your computer.

> Duolo is early. There are no released installers yet, so for now you build it from source. macOS gets the most use; Windows and Linux are compiled and tested in CI but haven't been used end to end on real machines.

```
┌────────────────────────┬─────────────────────────────────────────────────┐
│ 🔍 Search tasks ⌘K  ⊞  │ ✳ Export orders ✓  feat/order-csv +48 −6 Review ⋯│
│                        ├─────────────────────────────────────────────────┤
│ ▾ shop               + │                                                 │
│ ✳ Export orders     ✓  │   The native Claude Code interface              │
│   order-csv +48 −6  2m │   (your keystrokes go straight to it)           │
│ ✳ Stripe billing    ◐  │                                                 │
│ ❀ Fix login styles  ●  │ ┌─────────────────────────────────────────────┐ │
│                        │ │ ❯ Store all amounts in integer cents     🎙 │ │
│ ⚙                      │ └─────────────────────────────────────────────┘ │
└────────────────────────┴─────────────────────────────────────────────────┘
```

## Features

- **Parallel tasks.** Each task has its own branch and working directory (a git worktree under `~/.duolo/wt/`). Tasks don't affect each other or the checkout you are working in.
- **The real CLIs.** The main pane is Claude Code, Codex, OpenCode, pi or Grok itself, and you can switch between them per task. Duolo doesn't change how they behave.
- **Terminal or chat.** Claude Code, Codex and agents that speak ACP (OpenCode and Grok for now) can also run in chat mode: Duolo draws the conversation, tool calls and approval buttons itself, and streams the reply as it is written. Switch with the icons in the task's top bar; it continues the same session, and new tasks use your last choice.
- **Status at a glance.** One row per task: which model, and whether it is running, waiting for you, done or failed, plus the review result. Search, collapse projects, double-click to rename, right-click for more.
- **One writes, one reviews.** Pick Review under Hand off and Codex reads everything the task changed since it branched off, including uncommitted work and new files. It reviews in a temporary copy with only read-only tools: Claude can only read files, and Codex runs in its read-only sandbox with connectors and plugins turned off. Nothing it does can touch your work. One click sends the findings to Claude.
- **Hand off to another agent.** Click Hand off in the task's top bar (or right-click the task) and pick who and what: review the changes, take over in the same folder, or continue in a new task. A new task copies the current code, uncommitted changes included, to its own branch, and the original agent keeps working, so both run side by side. Either way the new agent gets the request and the recent conversation, plus an optional note from you, and looks at the current changes first. Switch back within a day and the earlier agent picks up its own conversation, told what happened while it was away. From the command line: `duolo handoff <task> --to codex [--fork | --review] [note]`; in the terminal UI, press `t`.
- **Side by side.** Drag tasks from the sidebar to the right (or `⌘`-click them) to watch up to six at once; drop on a pane's edge to put it beside, in the middle to replace it. The grid icon next to a project shows all its tasks side by side, waiting ones first. Drag the lines between panes to resize them (double-click one to even it out), drag a pane by its handle to move it, enlarge one with `⌘⇧↵`, and close one with its × (the task keeps running). `⌘\` goes back to one.
- **Built-in terminal.** Press `⌃`` (or the terminal icon in a task's top bar) to open terminals below the agent, in the task's own folder with your own shell; open several as tabs, drag the line above them to resize. If an agent exits (say after `Ctrl+C`), the bar at the bottom resumes its conversation or opens a terminal right there. Pasting a copied image or file (from Finder, a screenshot, or a web page) gives the agent its path, so Claude Code and Codex attach it. Links in any terminal open in your browser with `⌘`-click (`Ctrl`-click on Windows and Linux). Nothing opens outside Duolo ("Open in system terminal" is still in the ⋯ menu). A task doesn't need an AI either: pick Terminal when creating it and type a command such as `pnpm dev` or `cargo test`, or leave it empty for a plain shell. It runs in a shell in the task's own branch, and the terminal stays open afterwards so you can keep typing. From the command line: `duolo new --writer shell "cargo test"`.
- **Projects.** A project is a folder: one git repository, or a folder holding several. Add one (`⌘O`) by opening a folder, creating one, cloning from GitHub, or picking a repository found on your computer. You can also drop a folder on the window. New folders and clones go to `~/DuoloProjects`.
- **New tasks.** Click + next to a project (or press `⌘N`), type a request and press Enter. Everything else has a default you can change: repository, base branch, branch name (AI suggests one such as `feat/order-csv-export`), and who writes and who reviews.
- **Voice.** Hold `Fn` to talk (`Ctrl+Shift+Space` on Windows and Linux) and release to insert. macOS and Windows use the built-in recognizer with no setup. Duolo can tidy up what you said before inserting it, and you can switch back to your exact words.
- **Nothing lost on close.** Closing a task commits uncommitted changes to its branch before deleting the working directory. If something can't be saved, such as a nested repository, the task stays open and Duolo tells you why. Files ignored by git, such as `.env`, are deleted.
- **No startup questions.** Before starting an agent in a task, Duolo marks the folder as trusted in Claude Code (`~/.claude.json`) and Codex (`~/.codex/config.toml`, for the repository) and turns off their update prompts, so they don't stop to ask. Claude's entry is removed when the task closes.
- **Sessions resume.** Reopening a task continues that agent's own conversation, by session ID (`claude --resume <id>`, `codex resume <id>`).
- **Keeps working in the background** (macOS). `⌘W` hides the window and agents keep going; `⌘Q` stops them. On Windows and Linux closing the window quits, after asking if agents are running.
- **14 languages**, in the app, the command line and the terminal UI.

## Install

You need git and at least one agent CLI: [Claude Code](https://docs.claude.com/en/docs/claude-code) (`curl -fsSL https://claude.ai/install.sh | bash`), [Codex](https://github.com/openai/codex) (`curl -fsSL https://chatgpt.com/codex/install.sh | sh`), [OpenCode](https://opencode.ai) (`npm install -g opencode-ai`), [pi](https://pi.dev) (`npm install -g @earendil-works/pi-coding-agent`) or [Grok](https://x.ai/cli) (`curl -fsSL https://x.ai/cli/install.sh | bash`). Duolo can also install Claude Code and Codex for you and sign you in with your existing Claude or ChatGPT account. If it sees the Claude or ChatGPT desktop app, it offers to. You can sign in to several accounts (or use the ones already signed in to Orca); when one runs out of usage, Duolo switches to the next and continues the conversation. Building from source also needs Rust 1.89+, Node 20+ and pnpm.

```sh
git clone https://github.com/OtterMind/duolo.git && cd duolo
cd app && pnpm install && pnpm tauri build
cargo install --path ../crates/duolo-cli        # optional: the duolo command
```

Bundles are written to `target/release/bundle/`:

| System | Bundle | Notes |
|---|---|---|
| macOS 12+ | `macos/Duolo.app` | Copy it to Applications |
| Windows 10 / 11 | `nsis/Duolo_*_x64-setup.exe`, `msi/*.msi` | The setup program installs for the current user, no administrator rights needed |
| Linux | `deb/*.deb`, `appimage/*.AppImage` | Needs WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+). To build: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev libssl-dev` |

For development, `cd app && pnpm tauri dev` reloads the interface as you edit.

## Usage

Open Duolo and you land in Claude's prompt: your latest task, or a new one. Type your request there as usual. Your first message becomes the task title, and AI names the branch from it. Press `⌘N` for another task.

```sh
cd ~/code/shop
duolo                                              # open Duolo with this repository
duolo new "Export orders as CSV, filter by date"   # create a task without opening the app
duolo ls                                           # list tasks
duolo handoff 3 --to codex                         # let Codex take over task 3
duolo handoff 3 --to pi --fork "Use cursor paging" # Pi continues in a new task; task 3 keeps going
duolo tui                                          # terminal UI, handy over SSH
```

### Shortcuts

These are the macOS keys. On Windows and Linux `⌘` is `Ctrl` and `⌥` is `Alt`, and `⌘` plus a letter also takes `Shift` (`⌘N` is `Ctrl+Shift+N`), because `Ctrl` plus a letter belongs to the agent in the terminal. `Ctrl+C` copies when text is selected, and `Ctrl+V` pastes.

| Key | Action |
|---|---|
| `⌘N` | New task (press Enter with nothing typed for a blank one) |
| `⌘K` | Search tasks |
| `⌥↓` | Next task waiting for you |
| `⌘O` | Add a project |
| `⌘⇧R` | Review the current task |
| `⌃Tab` | Switch between the task's agents |
| `⌘1`–`⌘9`, `⌘⌥↑↓` | Switch tasks |
| `⌘\` | Put another task beside this one / back to one pane |
| `⌃`` | Show or hide the task's terminals |
| `⌘⌥←→` / `⌘⇧↵` | Side by side: go to the previous or next pane / enlarge the current one |
| `⌘⇧F` | Show the working directory in your file manager |
| `⌘⇧W` | Close the task |
| Hold `Fn` | Talk, release to insert. Tap Space while holding to lock, press Fn again to finish. You can also click the microphone. On Windows and Linux hold `Ctrl+Shift+Space`, or tap it to start and again to stop |
| `↵` / `⌘↵` / `Esc` while recording | Insert / insert and send / discard |
| `⇧↵` | New line in the agent's input |
| `⌘W` / `⌘Q` | Hide the window (macOS only) / quit |

Holding Fn needs the Accessibility permission once; Settings in the bottom left corner takes you there. If the 🌐 key opens the emoji picker or switches input sources, set "Press 🌐 key to" to "Do Nothing" in System Settings → Keyboard.

## Configuration

`~/.duolo/config.toml`. Every field is optional:

```toml
writer = "claude"          # default agent for writing code
reviewer = "codex"         # default agent for reviews

[agents.claude]            # changes the built-in settings; anything not listed stays as it is
command = "/opt/homebrew/bin/claude"
args = ["--model", "opus"]

server = "https://duolo.example.com"  # the Duolo server (speech recognition); empty means none

[voice]
organize = false   # have the server tidy up filler words and self-corrections (off by default)
```

### Adding another agent

Every agent is a plugin: an `agent.toml` that says how to start, resume and review with it, in `~/.duolo/agents/<id>/`. See [docs/agents.md](docs/agents.md) (in Chinese). How Duolo keeps working when an agent updates (status reports, no startup dialogs, version checks, hooks) is in [docs/agent-compat.md](docs/agent-compat.md).

### Voice

Speech is recognized by the Duolo server (which passes it to Doubao streaming speech recognition), the same on macOS, Windows and Linux:

- Duolo records from the current input device, so a headset's microphone works when you wear one. The system asks for microphone access the first time.
- Each recording carries hints from the current task (recently touched files, code names the agent mentioned, the branch and project names) and the latest conversation, so mixed-language speech and code names come out right more often.
- No sign-up: the first time, Duolo gets an anonymous identity from the server and keeps it in `~/.duolo/account.json`. The server limits use per identity per day.
- Your voice and these hints are sent to the Duolo server and the recognition service. If the server can't be reached, voice input is unavailable and Duolo says why.

## Architecture

```
app/ (React + xterm.js)
   │ Tauri commands / events / channels
duolo-app ─┐
duolo-tui ─┴▶ duolo-engine ─▶ duolo-pty   (PTY, output, state detection)
     │              ├───────▶ duolo-git   (worktrees, diffs)
     └▶ duolo-voice └───────▶ duolo-core  (model, config, storage)
```

Details are in [docs/architecture.md](docs/architecture.md), and the localization rules in [docs/i18n.md](docs/i18n.md) (both in Chinese).

## Limitations

- There is no separate daemon: quitting stops all agents, and their sessions resume when you reopen a task.
- Agent state is detected from the terminal screen, not from hooks.
- Windows and Linux haven't been tested end to end on real machines.
- There is no diff viewer, merge or pull request support, system notifications or automatic updates yet. Native iOS and Android apps are in progress ([docs/mobile.md](docs/mobile.md)), with a relay in [server/](server/README.md); the desktop app does not connect to it yet.
- On Linux the push-to-talk shortcut works on X11 only. On Wayland, click the microphone.

## Contributing

Issues and pull requests are welcome. Before submitting, run:

```sh
(cd app && pnpm build)          # type check and language files
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
(cd server && pnpm check && pnpm test)   # if you changed the server; run docker compose up -d first
```

Conventions are in [AGENTS.md](AGENTS.md): all user-facing text must exist in all 14 languages, and the desktop app must work on all three systems.

## License

[MIT](LICENSE). Claude and Claude Code are trademarks of Anthropic, and Codex is a trademark of OpenAI. Duolo is an independent project and is not affiliated with either company.
