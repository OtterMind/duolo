<h1 align="center">Duolo</h1>

<p align="center">Claude writes, Codex reviews. Run a team of coding agents in parallel on your own machine.</p>

<p align="center">
  <a href="https://github.com/OtterMind/duolo/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/OtterMind/duolo/actions/workflows/ci.yml/badge.svg"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center">English · <a href="README.zh-CN.md">简体中文</a></p>

Duolo is a local-first desktop app for macOS, Windows and Linux. Every task gets its own git worktree with the real Claude Code or Codex CLI running inside it. When the work is done, a second model reviews it read-only and you hand the findings back with one click. The sidebar shows the state of every task, and you can speak a request instead of typing it. Your code and sessions stay on your computer.

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
- **The real CLIs.** The main pane is Claude Code or Codex itself. Duolo doesn't change how they behave.
- **Status at a glance.** One row per task: which model, and whether it is running, waiting for you, done or failed, plus the review result. Search, collapse projects, double-click to rename, right-click for more.
- **One writes, one reviews.** Click Review and Codex reads everything the task changed since it branched off, including uncommitted work and new files, without touching anything. One click sends the findings to Claude.
- **Projects.** A project is a folder: one git repository, or a folder holding several. Add one (`⌘O`) by opening a folder, creating one, cloning from GitHub, or picking a repository found on your computer. You can also drop a folder on the window. New folders and clones go to `~/DuoloProjects`.
- **New tasks.** Click + next to a project (or press `⌘N`), type a request and press Enter. Everything else has a default you can change: repository, base branch, branch name (AI suggests one such as `feat/order-csv-export`), and who writes and who reviews.
- **Voice.** Hold `Fn` to talk (`Ctrl+Shift+Space` on Windows and Linux) and release to insert. macOS and Windows use the built-in recognizer with no setup. Duolo can tidy up what you said before inserting it, and you can switch back to your exact words.
- **Nothing lost on close.** Closing a task commits uncommitted changes to its branch before deleting the working directory. If something can't be saved, such as a nested repository, the task stays open and Duolo tells you why. Files ignored by git, such as `.env`, are deleted.
- **Sessions resume.** Reopening a task continues its last conversation (`claude --continue`, `codex resume --last`).
- **Keeps working in the background** (macOS). `⌘W` hides the window and agents keep going; `⌘Q` stops them. On Windows and Linux closing the window quits, after asking if agents are running.
- **14 languages**, in the app, the command line and the terminal UI.

## Install

You need git and at least one agent CLI: [Claude Code](https://docs.claude.com/en/docs/claude-code) (`npm install -g @anthropic-ai/claude-code`) or [Codex](https://github.com/openai/codex) (`npm install -g @openai/codex`). Building from source also needs Rust 1.89+, Node 20+ and pnpm.

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

[agents.claude]
command = "claude"
resume_args = ["--continue"]

[agents.codex]
command = "codex"
resume_args = ["resume", "--last"]
review_args = ["--sandbox", "read-only"]

[voice]
# A command that records and transcribes. It starts recording when launched and prints the text when told to stop
# (SIGINT on macOS and Linux, a line saying "stop" on standard input on Windows).
command = ["/path/to/duolo/scripts/voice-whisper.sh"]
organize = true                                   # tidy up filler words and self-corrections
organizer = ["claude", "-p", "--model", "haiku"]  # the command that does it
```

### Voice

- **macOS** uses the system's speech recognition (on device where supported). It asks for microphone and speech permission the first time.
- **Windows** uses System.Speech dictation on device. Install the speech pack for your language under Settings → Time & language → Speech.
- **Linux** has no built-in recognizer, so configure one as below.

Recognition follows the interface language and can be set separately in Settings. To use your own recognizer, point `voice.command` at it. `scripts/voice-whisper.sh` records with sox and transcribes locally with whisper.cpp:

```sh
brew install sox whisper-cpp          # Linux: install sox from your package manager and build whisper.cpp
mkdir -p ~/.duolo/models
curl -L -o ~/.duolo/models/ggml-large-v3-turbo.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin
```

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
