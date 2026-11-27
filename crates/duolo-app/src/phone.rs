//! 手机端：把引擎接到 `duolo_link` 上。总览由这里按快照算出来，命令交给 `duolo_engine::ops::execute`。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use duolo_core::wire::{
    AgentOption, DiffOverview, Overview, ProjectOverview, PromptOverview, ReviewOverview, TaskOverview, TaskState,
    VoiceOverview,
};
use duolo_core::{AgentKind, AgentRole, Command, CommandResult, TaskEvent, TaskId};
use duolo_engine::Engine;
use tauri::{AppHandle, Manager};

use crate::view::Snapshot;
use crate::AppState;

/// 项目的分支列表多久重新读一次（要跑 git，不放进 250 毫秒一次的快照里）。
const BRANCHES_TTL: Duration = Duration::from_secs(60);

/// 读到分支列表的时间、当前分支、所有分支。
type Branches = (Instant, String, Vec<String>);

/// 给 `duolo_link` 用的电脑端。
pub struct AppHost {
    app: AppHandle,
    branches: Mutex<HashMap<String, Branches>>,
}

impl std::fmt::Debug for AppHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppHost")
    }
}

impl AppHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app, branches: Mutex::new(HashMap::new()) }
    }

    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// 项目的当前分支（新任务默认从它开始）和所有分支，一分钟读一次。
    fn branches_of(&self, root: &str) -> (String, Vec<String>) {
        let mut cache = self.branches.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, branch, list)) = cache.get(root) {
            if at.elapsed() < BRANCHES_TTL {
                return (branch.clone(), list.clone());
            }
        }
        let path = Path::new(root);
        let branch = duolo_git::current_branch(path).unwrap_or_default();
        let list = duolo_git::branches(path).unwrap_or_default();
        cache.insert(root.to_string(), (Instant::now(), branch.clone(), list.clone()));
        (branch, list)
    }

    /// 总览：快照里已经有的直接用，其余的（最后一句话、在等的提示、改动统计）从引擎的事件摘要拿。
    pub fn overview(&self, engine: &mut Engine, snapshot: &Snapshot) -> Overview {
        let option = |engine: &Engine, kind: &AgentKind| AgentOption {
            kind: kind.0.clone(),
            name: engine.agents().name(kind),
            color: engine.agents().get(kind).ok().and_then(|p| p.icon_color()).map(str::to_string),
        };
        let automations = snapshot.automations_project.clone();
        let mut projects = Vec::new();
        let mut tasks = Vec::new();
        for project in &snapshot.projects {
            // 定时任务专用的项目不给手机看（里面的任务照样显示在各自的列表里）。
            if automations.as_deref() == Some(project.id.as_str()) && project.tasks.is_empty() {
                continue;
            }
            let (branch, branches) = self.branches_of(&project.root);
            projects.push(ProjectOverview { id: project.id.clone(), name: project.name.clone(), branch, branches });
            for task in &project.tasks {
                let id = TaskId(task.id);
                let digest = engine.digest(id);
                let Some(model) = engine.state().task(id).cloned() else { continue };
                let writer = model.agent_with_role(AgentRole::Writer);
                let writer_kind = writer.map_or_else(|| engine.config().writer.clone(), |a| a.kind.clone());
                let status = writer.map_or("not_started", |a| engine.agent_status(model.key(a.id)).code());
                let since = writer.and_then(|a| engine.status_since(model.key(a.id))).unwrap_or(0);
                let reviewer = AgentKind(task.reviewer.kind.clone());
                tasks.push(TaskOverview {
                    id,
                    project: project.id.clone(),
                    title: task.title.clone(),
                    untitled: task.untitled,
                    branch: task.branch.clone(),
                    status: status.to_string(),
                    state: task.state,
                    repo: task.repo.clone(),
                    writer: option(engine, &writer_kind),
                    reviewer: option(engine, &reviewer),
                    updated_at: digest.at.max(task.created_at * 1000),
                    since,
                    last: digest.last.clone(),
                    prompt: digest
                        .prompt
                        .clone()
                        .filter(|_| task.state == TaskState::NeedsYou)
                        .map(|(seq, question, detail, options)| PromptOverview { seq, question, detail, options }),
                    reviewing_since: task.reviewing_since.map(|s| s * 1000),
                    review: task.review.as_ref().map(|r| ReviewOverview {
                        finished_at: r.finished_at * 1000,
                        ok: r.ok,
                        verdict: r.verdict.to_string(),
                        issues: r.issues,
                        summary: r.summary.clone(),
                    }),
                    diff: digest.diff.map(|(files, insertions, deletions)| DiffOverview {
                        files,
                        insertions,
                        deletions,
                    }),
                    seq: digest.seq,
                });
            }
        }
        let agents = engine
            .agents()
            .iter()
            .filter(|p| p.is_installed())
            .map(|p| option(engine, &AgentKind(p.id().to_string())))
            .collect();
        Overview {
            rev: 0,
            projects,
            tasks,
            agents,
            writer: engine.config().writer.0.clone(),
            reviewer: engine.config().reviewer.0.clone(),
            voice: voice_overview(engine),
        }
    }
}

/// 设置 → 语音：没设置过整理时用 `config.toml` 的（和 `commands::voice_config` 一样）。
fn voice_overview(engine: &Engine) -> VoiceOverview {
    let settings = duolo_core::Settings::load(&engine.paths().settings_file()).unwrap_or_default();
    VoiceOverview {
        organize: settings.voice_organize.unwrap_or(engine.config().voice.organize),
        words: settings.voice_words,
    }
}

impl duolo_link::Host for AppHost {
    fn overview(&self) -> Overview {
        let state = self.state();
        let mut engine = state.engine();
        let snapshot = crate::view::snapshot(&mut engine);
        self.overview(&mut engine, &snapshot)
    }

    fn events_since(&self, task: TaskId, after: u64) -> Vec<TaskEvent> {
        self.state().engine().events_since(task, after).unwrap_or_default()
    }

    fn review(&self, task: TaskId) -> Option<(u64, String)> {
        let state = self.state();
        let engine = state.engine();
        let review = engine.state().task(task)?.review.clone()?;
        Some((review.finished_at * 1000, review.output))
    }

    fn execute(&self, command: Command) -> CommandResult {
        duolo_engine::ops::execute(&self.state().engine, command)
    }
}
