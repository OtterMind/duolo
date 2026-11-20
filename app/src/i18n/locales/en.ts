// 英文是基准语言：所有键都在这里定义，其他语言必须提供同样的键（类型检查保证）。
// 占位符写成 {name}，由 t() 填入。新增文案时先加在这里，再补齐所有语言。

export const en = {
  // 通用
  "common.cancel": "Cancel",
  "common.pleaseWait": "Please wait…",
  "common.more": "More",
  "common.collapse": "Close",
  "common.ago": "{time} ago",

  // Agent 状态
  "status.not_started": "Not started",
  "status.running": "Running",
  "status.needs_input": "Needs you",
  "status.idle": "Waiting",
  "status.exited": "Ended",
  "status.failed": "Crashed",

  // 侧边栏
  "sidebar.newTask": "New Task",
  "sidebar.newTaskIn": "New task in {project}",
  "sidebar.noTasks": "No tasks yet",
  "sidebar.noProjects": "No projects yet",
  "sidebar.addProject": "Add Project",
  "sidebar.closeTask": "Close task",
  "sidebar.noneRunning": "No agents running",
  "sidebar.running": "{count} running",
  "sidebar.needsYou": "{count} need you",
  "sidebar.review": "Codex review",
  "sidebar.reviewRunning": "In progress",
  "sidebar.reviewFailed": "Failed",
  "sidebar.language": "Language",
  "sidebar.languageAuto": "System default",
  "task.untitled": "New Task",

  // 空状态和环境检查
  "empty.preparing": "Preparing a workspace…",
  "empty.addProjectTitle": "Add a project to start",
  "empty.addProjectBody":
    "Choose a git repository. Each task runs on its own branch and working directory, so your checkout stays untouched.",
  "empty.chooseFolder": "Choose Folder…",
  "empty.noOpenTask": "No open task",
  "empty.newTask": "New Task",
  "env.voice": "Voice",
  "env.missing": "{command} not found",
  "env.installHint": "Install it, then reopen Duolo.",
  "dialog.chooseRepo": "Choose a git repository",

  // 任务页
  "task.basedOn": "Based on {base}",
  "task.reveal": "Show in Finder",
  "task.openTerminal": "Open in Terminal",
  "task.copyPath": "Copy Working Directory Path",
  "task.close": "Close Task…",
  "task.starting": "Starting {agent}…",
  "task.agentExited": "{agent} ended",
  "task.agentFailed": "{agent} crashed",
  "task.resume": "Resume Session",

  // 审阅
  "review.button": "Review",
  "review.buttonTitle": "Let Codex review this task's changes in the background, read-only (⌘⇧R)",
  "review.running": "Reviewing {time}",
  "review.runningTitle": "Codex is reviewing, read-only",
  "review.resultTitle": "Show review (⌘⇧R)",
  "review.failed": "Review failed",
  "review.panelTitle": "Codex Review",
  "review.inProgress": "in progress {time}",
  "review.runningNote":
    "Codex is reading every change on {branch} relative to {base}, read-only. This usually takes a minute or two; keep working with Claude meanwhile.",
  "review.none": "No review yet.",
  "review.stop": "Stop Review",
  "review.sendToClaude": "Send to Claude",
  "review.rerun": "Review Again",
  "review.start": "Start Review",
  "review.handoff": "Please address the following code review (a read-only review by Codex):",

  // 语音
  "voice.title": "Voice input (⌘⇧Space)",
  "voice.notConfiguredTitle": "Voice input is not configured",
  "voice.stop": "stop",
  "voice.cancel": "cancel",
  "voice.processing": "Cleaning up…",
  "voice.needTask": "Open a task first, then use voice input.",

  // 确认框
  "confirm.closeTitle": "Close “{title}”?",
  "confirm.closeBody":
    "This stops the task's agents and deletes its working directory. The branch {branch} is kept, so no changes are lost.",
  "confirm.closeAction": "Close Task",
  "confirm.quitTitle": "Quit Duolo?",
  "confirm.quitBody":
    "{count} agents are still running and will be stopped. You can resume their sessions next time.\nTo just hide the window, use ⌘W.",
  "confirm.quitAction": "Quit",

  // 错误（键名后缀与 crates/duolo-core/src/error.rs 的 code 一致）
  "error.notGitRepo": "{path} is not inside a git repository. Choose a folder in a git repo.",
  "error.gitMissing": "git isn't installed or isn't on your PATH.",
  "error.gitFailed": "git {command} failed: {detail}",
  "error.commandNotFound": "Couldn't start {program}. Make sure it's installed and on your PATH.",
  "error.voiceNotConfigured":
    "Voice input isn't set up yet: set voice.command in ~/.duolo/config.toml (see README). Meanwhile, press Fn twice to use macOS Dictation.",
  "error.voicePermission": "Duolo needs microphone and speech recognition access. Allow it in System Settings → Privacy & Security, then try again.",
  "error.nothingRecognized": "No speech was recognized.",
  "error.notRecording": "Not recording.",
  "error.reviewNotConfigured": "No review command is configured (review_command in ~/.duolo/config.toml).",
  "error.agentNotRunning": "The agent isn't running.",
  "error.taskNotFound": "This task no longer exists.",
  "error.projectNotFound": "This project no longer exists.",
  "error.unknownAgent": "There's no agent named “{kind}” in ~/.duolo/config.toml.",
  "error.internal": "Something went wrong: {message}",
};

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, string>;
