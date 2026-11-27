//! 定时任务（自动运行）：按时间表在项目里新建一个任务，交给 Agent 去做。
//!
//! 时间表是 5 段的 cron，按这台电脑的时区。桌面应用每 [`CHECK_EVERY`] 调一次 [`run_due`]：
//! 到点的先在 [`Engine::update`] 的锁里"领取"（记下这次执行、把下一次往后推），领到的再在锁外建任务、
//! 启动 Agent。桌面应用、终端界面、命令行可能同时开着，领取在锁里做，同一次只会执行一次。
//!
//! 只有桌面应用执行：Agent 进程属于启动它的进程。命令行的"立即运行"只是记下请求（`run_requested`），
//! 由桌面应用下一次检查时执行。应用没开的时候不执行；重新打开后，错过不超过宽限期
//! （`missed_grace_minutes`，默认 12 小时）的补跑一次，更早的记为错过。同一个定时任务上一次建的任务
//! 还在干活时，这次跳过，不排队。

use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Local, TimeZone, Utc};
use croner::Cron;
use duolo_core::text::{derive_title, now_secs};
use duolo_core::{
    AgentKind, Automation, AutomationId, AutomationRun, ProjectId, RunOutcome, RunTrigger, State, TaskId, UserError,
};

use crate::engine::{Engine, TaskOptions};
use crate::status::AgentStatus;

/// 桌面应用多久检查一次。时间表精确到分钟，半分钟查一次不会错过。
pub const CHECK_EVERY: Duration = Duration::from_secs(30);

/// 一直停在"正在建任务"超过这么久（应用在建任务时退出了），记为中断。
const STALE_STARTING_SECS: u64 = 10 * 60;

/// 跳过的原因，对应语言包里的 `automation.skip.<reason>`。
pub mod skip {
    /// 错过了宽限期（应用没开、电脑睡着了）。
    pub const MISSED: &str = "missed";
    /// 上一次建的任务还在干活。
    pub const BUSY: &str = "busy";
    /// 建任务时应用退出了。
    pub const INTERRUPTED: &str = "interrupted";
}

/// 新建、修改定时任务时填的内容。
#[derive(Debug, Clone)]
pub struct AutomationDraft {
    /// 空的时候按需求起名。
    pub name: String,
    pub project: ProjectId,
    pub prompt: String,
    pub agent: Option<AgentKind>,
    pub base: Option<String>,
    pub cron: String,
}

/// 解析时间表：5 段的 cron（分 时 日 月 星期），或者 `@daily` 这样的别名。不接受精确到秒的 6 段写法。
pub fn parse_cron(expr: &str) -> Result<Cron, UserError> {
    let expr = expr.trim();
    let invalid = || UserError::InvalidSchedule { expr: expr.to_string() };
    if !expr.starts_with('@') && expr.split_whitespace().count() != 5 {
        return Err(invalid());
    }
    Cron::from_str(expr).map_err(|_| invalid())
}

/// `after` 之后（不含）下一次的时间，按 `tz` 时区。算不出来（例如 2 月 30 日）时为 None。
fn next_in<Tz: TimeZone>(cron: &Cron, after: u64, tz: &Tz) -> Option<u64> {
    let from = DateTime::<Utc>::from_timestamp(i64::try_from(after).ok()?, 0)?.with_timezone(tz);
    let next = cron.find_next_occurrence(&from, false).ok()?;
    u64::try_from(next.timestamp()).ok()
}

/// `now` 之前（含）最近一次的时间。
fn latest_in<Tz: TimeZone>(cron: &Cron, now: u64, tz: &Tz) -> Option<u64> {
    let at = DateTime::<Utc>::from_timestamp(i64::try_from(now).ok()?, 0)?.with_timezone(tz);
    let latest = cron.find_previous_occurrence(&at, true).ok()?;
    u64::try_from(latest.timestamp()).ok()
}

/// 按这台电脑的时区，`after` 之后下一次的时间。
pub fn next_run(expr: &str, after: u64) -> Option<u64> {
    next_in(&parse_cron(expr).ok()?, after, &Local)
}

/// 这台电脑时区的"年-月-日 时:分"，命令行显示用。
pub fn local_time(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| Local.timestamp_opt(s, 0).single())
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// 一个定时任务这次检查要做的事。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    scheduled_for: u64,
    trigger: RunTrigger,
    /// 跳过时的原因；None 时去建任务。
    skip: Option<&'static str>,
}

/// 决定一个定时任务这次要不要执行，以及下一次的时间。`busy`：上一次建的任务还在干活。
/// 返回 None 表示这次什么都不做。
fn plan<Tz: TimeZone>(automation: &Automation, now: u64, tz: &Tz, busy: bool) -> Option<(Plan, Option<u64>)> {
    let skip_if_busy = |plan: Plan| Plan { skip: plan.skip.or(busy.then_some(skip::BUSY)), ..plan };
    if automation.run_requested {
        let plan = Plan { scheduled_for: now, trigger: RunTrigger::Manual, skip: None };
        return Some((skip_if_busy(plan), automation.next_run_at));
    }
    let due = automation.next_run_at.filter(|&at| at <= now)?;
    if !automation.enabled {
        return None;
    }
    let cron = parse_cron(&automation.cron).ok()?;
    // 错过了好几次（应用关了几天）只算最近一次。
    let scheduled_for = latest_in(&cron, now, tz).unwrap_or(due).max(due);
    let next = next_in(&cron, now, tz);
    let grace = u64::from(automation.missed_grace_minutes) * 60 + 2 * CHECK_EVERY.as_secs();
    let plan = Plan { scheduled_for, trigger: RunTrigger::Scheduled, skip: None };
    if now.saturating_sub(scheduled_for) > grace {
        return Some((Plan { skip: Some(skip::MISSED), ..plan }, next));
    }
    Some((skip_if_busy(plan), next))
}

/// 领到、要去建任务的一次执行。
#[derive(Debug, Clone)]
struct Launch {
    automation: Automation,
    scheduled_for: u64,
}

/// 在锁里领取到点的执行：记下这次执行（跳过的直接记结果），把下一次往后推。
fn claim<Tz: TimeZone>(state: &mut State, now: u64, tz: &Tz, busy: &[AutomationId]) -> Vec<Launch> {
    let mut launches = Vec::new();
    for automation in &mut state.automations {
        for run in &mut automation.runs {
            if run.outcome == RunOutcome::Starting && now.saturating_sub(run.at) > STALE_STARTING_SECS {
                run.outcome = RunOutcome::Skipped { reason: skip::INTERRUPTED.into() };
            }
        }
        let starting = automation.runs.iter().any(|r| r.outcome == RunOutcome::Starting);
        let Some((plan, next)) = plan(automation, now, tz, starting || busy.contains(&automation.id)) else {
            continue;
        };
        automation.next_run_at = next;
        automation.run_requested = false;
        let outcome = match plan.skip {
            Some(reason) => RunOutcome::Skipped { reason: reason.into() },
            None => RunOutcome::Starting,
        };
        if outcome == RunOutcome::Starting {
            launches.push(Launch { automation: automation.clone(), scheduled_for: plan.scheduled_for });
        }
        automation.push_run(AutomationRun {
            scheduled_for: plan.scheduled_for,
            at: now,
            trigger: plan.trigger,
            outcome,
        });
    }
    launches
}

fn lock(engine: &Mutex<Engine>) -> MutexGuard<'_, Engine> {
    engine.lock().unwrap_or_else(|p| p.into_inner())
}

/// 执行到点的定时任务，返回新建的任务。桌面应用定期调用；建任务要几秒，不拿着锁做。
pub fn run_due(engine: &Mutex<Engine>) -> Vec<TaskId> {
    let launches = {
        let mut engine = lock(engine);
        let busy = engine.busy_automations();
        let now = now_secs();
        match engine.update(|state| Ok(claim(state, now, &Local, &busy))) {
            Ok(launches) => launches,
            Err(err) => {
                eprintln!("automations: {err:#}");
                return Vec::new();
            }
        }
    };
    let mut started = Vec::new();
    for launch in launches {
        let result = launch_one(engine, &launch);
        let outcome = match &result {
            Ok(task) => RunOutcome::Started { task: *task },
            Err(err) => {
                eprintln!("automation {} failed: {err:#}", launch.automation.id);
                let user = err.chain().find_map(|e| e.downcast_ref::<UserError>());
                RunOutcome::Failed {
                    error: user.map_or("internal", UserError::code).into(),
                    params: user
                        .map(|e| e.params().into_iter().map(|(k, v)| (k.to_string(), v)).collect())
                        .unwrap_or_default(),
                }
            }
        };
        let id = launch.automation.id;
        let recorded = lock(engine).update(|state| {
            if let Some(run) = state
                .automation_mut(id)
                .and_then(|a| a.runs.iter_mut().rev().find(|r| r.scheduled_for == launch.scheduled_for))
            {
                run.outcome = outcome;
            }
            Ok(())
        });
        if let Err(err) = recorded {
            eprintln!("automation {id}: {err:#}");
        }
        started.extend(result.ok());
    }
    started
}

/// 建任务、启动写代码的 Agent（第一次启动带着需求）。
fn launch_one(engine: &Mutex<Engine>, launch: &Launch) -> Result<TaskId> {
    let automation = &launch.automation;
    let opts =
        TaskOptions { writer: automation.agent.clone(), base: automation.base.clone(), ..TaskOptions::default() };
    let task = crate::ops::create_task(engine, &automation.project, &automation.prompt, &opts)?;
    let mut engine = lock(engine);
    engine.update(|state| {
        if let Some(task) = state.task_mut(task) {
            task.automation = Some(automation.id);
        }
        Ok(())
    })?;
    let key = engine.writer_key(task)?;
    engine.start_agent(key)?;
    Ok(task)
}

impl Engine {
    /// 上一次建的任务还在干活（在跑、或在等用户回答）的定时任务。
    fn busy_automations(&mut self) -> Vec<AutomationId> {
        let last: Vec<(AutomationId, TaskId)> =
            self.state.automations.iter().filter_map(|a| Some((a.id, a.last_task()?))).collect();
        last.into_iter()
            .filter(|&(_, task)| {
                self.writer_key(task)
                    .is_ok_and(|key| matches!(self.agent_status(key), AgentStatus::Running | AgentStatus::NeedsInput))
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// 检查填的内容，补上默认的名字。
    fn check_draft(&self, draft: &AutomationDraft) -> Result<(String, Cron)> {
        self.state.project(&draft.project).ok_or(UserError::ProjectNotFound)?;
        if let Some(agent) = &draft.agent {
            self.agents.get(agent)?;
        }
        let cron = parse_cron(&draft.cron)?;
        let name = match draft.name.trim() {
            "" => derive_title(&draft.prompt),
            name => name.to_string(),
        };
        Ok((name, cron))
    }

    /// 新建定时任务。
    pub fn add_automation(&mut self, draft: AutomationDraft) -> Result<AutomationId> {
        let (name, cron) = self.check_draft(&draft)?;
        let now = now_secs();
        self.update(|state| {
            let id = state.allocate_automation_id();
            state.automations.push(Automation {
                id,
                name,
                project: draft.project,
                prompt: draft.prompt.trim().to_string(),
                agent: draft.agent,
                base: draft.base.filter(|b| !b.trim().is_empty()),
                cron: draft.cron.trim().to_string(),
                enabled: true,
                next_run_at: next_in(&cron, now, &Local),
                missed_grace_minutes: Automation::default_grace(),
                run_requested: false,
                created_at: now,
                runs: Vec::new(),
            });
            Ok(id)
        })
    }

    /// 修改定时任务。执行记录保留，下一次的时间按新的时间表重新算。
    pub fn update_automation(&mut self, id: AutomationId, draft: AutomationDraft) -> Result<()> {
        let (name, cron) = self.check_draft(&draft)?;
        let next = next_in(&cron, now_secs(), &Local);
        self.update(|state| {
            let automation = state.automation_mut(id).ok_or(UserError::AutomationNotFound)?;
            automation.name = name;
            automation.project = draft.project;
            automation.prompt = draft.prompt.trim().to_string();
            automation.agent = draft.agent;
            automation.base = draft.base.filter(|b| !b.trim().is_empty());
            automation.cron = draft.cron.trim().to_string();
            automation.next_run_at = next;
            Ok(())
        })
    }

    /// 删除定时任务。它建过的任务留着。
    pub fn remove_automation(&mut self, id: AutomationId) -> Result<()> {
        self.update(|state| {
            state.automation(id).ok_or(UserError::AutomationNotFound)?;
            state.automations.retain(|a| a.id != id);
            Ok(())
        })
    }

    /// 暂停或恢复。恢复时从现在起算下一次，暂停期间的不补跑。
    pub fn set_automation_enabled(&mut self, id: AutomationId, enabled: bool) -> Result<()> {
        let now = now_secs();
        self.update(|state| {
            let automation = state.automation_mut(id).ok_or(UserError::AutomationNotFound)?;
            if enabled && !automation.enabled {
                automation.next_run_at = parse_cron(&automation.cron).ok().and_then(|c| next_in(&c, now, &Local));
            }
            automation.enabled = enabled;
            Ok(())
        })
    }

    /// 要求立即运行一次，由桌面应用下一次检查时执行（桌面应用里点的会马上检查）。
    pub fn request_automation_run(&mut self, id: AutomationId) -> Result<()> {
        self.update(|state| {
            state.automation_mut(id).ok_or(UserError::AutomationNotFound)?.run_requested = true;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::FixedOffset;

    use super::*;

    /// 东八区。
    fn tz() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    /// 东八区的某个时间对应的 Unix 秒。
    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> u64 {
        tz().with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp() as u64
    }

    fn automation(cron: &str, next: u64) -> Automation {
        Automation {
            id: AutomationId(1),
            name: "Nightly".into(),
            project: ProjectId("shop".into()),
            prompt: "Fix flaky tests".into(),
            agent: None,
            base: None,
            cron: cron.into(),
            enabled: true,
            next_run_at: Some(next),
            missed_grace_minutes: Automation::default_grace(),
            run_requested: false,
            created_at: 0,
            runs: Vec::new(),
        }
    }

    #[test]
    fn parses_five_fields_and_aliases_only() {
        assert!(parse_cron("0 9 * * 1-5").is_ok());
        assert!(parse_cron(" @daily ").is_ok());
        for bad in ["", "0 9 * *", "0 0 9 * * *", "61 9 * * *", "every day"] {
            assert!(matches!(parse_cron(bad), Err(UserError::InvalidSchedule { .. })), "{bad}");
        }
    }

    #[test]
    fn next_time_follows_the_time_zone() {
        let cron = parse_cron("0 9 * * 1-5").unwrap();
        // 2026-11-27 是星期五。
        assert_eq!(next_in(&cron, at(2026, 11, 27, 8, 0), &tz()), Some(at(2026, 11, 27, 9, 0)));
        assert_eq!(next_in(&cron, at(2026, 11, 27, 9, 0), &tz()), Some(at(2026, 11, 30, 9, 0)), "跳过周末，不含当前");
        assert_eq!(latest_in(&cron, at(2026, 11, 29, 12, 0), &tz()), Some(at(2026, 11, 27, 9, 0)));
    }

    #[test]
    fn runs_when_due_and_moves_on() {
        let a = automation("0 9 * * *", at(2026, 11, 27, 9, 0));
        assert_eq!(plan(&a, at(2026, 11, 27, 8, 59), &tz(), false), None, "还没到");
        let (plan_, next) = plan(&a, at(2026, 11, 27, 9, 0) + 20, &tz(), false).unwrap();
        assert_eq!(plan_, Plan { scheduled_for: at(2026, 11, 27, 9, 0), trigger: RunTrigger::Scheduled, skip: None });
        assert_eq!(next, Some(at(2026, 11, 28, 9, 0)));
    }

    #[test]
    fn catches_up_once_within_grace_and_skips_older() {
        // 应用关了三天，12 点打开：只补最近一次（当天 9 点）。
        let a = automation("0 9 * * *", at(2026, 11, 24, 9, 0));
        let (p, next) = plan(&a, at(2026, 11, 27, 12, 0), &tz(), false).unwrap();
        assert_eq!((p.scheduled_for, p.skip), (at(2026, 11, 27, 9, 0), None));
        assert_eq!(next, Some(at(2026, 11, 28, 9, 0)));
        // 每周一次，错过了 3 天：超过 12 小时宽限，记为错过。
        let weekly = automation("0 9 * * 1", at(2026, 11, 23, 9, 0));
        let (p, next) = plan(&weekly, at(2026, 11, 26, 9, 0), &tz(), false).unwrap();
        assert_eq!(p.skip, Some(skip::MISSED));
        assert_eq!(next, Some(at(2026, 11, 30, 9, 0)));
    }

    #[test]
    fn busy_and_paused() {
        let a = automation("0 9 * * *", at(2026, 11, 27, 9, 0));
        let (p, _) = plan(&a, at(2026, 11, 27, 9, 1), &tz(), true).unwrap();
        assert_eq!(p.skip, Some(skip::BUSY));
        let paused = Automation { enabled: false, ..a.clone() };
        assert_eq!(plan(&paused, at(2026, 11, 27, 9, 1), &tz(), false), None);
        // 暂停了也能手动运行，下一次的时间不变。
        let manual = Automation { run_requested: true, ..paused };
        let (p, next) = plan(&manual, at(2026, 11, 27, 9, 1), &tz(), false).unwrap();
        assert_eq!((p.trigger, p.skip, next), (RunTrigger::Manual, None, Some(at(2026, 11, 27, 9, 0))));
    }

    #[test]
    fn claims_once_and_records_runs() {
        let mut state = State::default();
        state.automations.push(automation("0 9 * * *", at(2026, 11, 27, 9, 0)));
        let now = at(2026, 11, 27, 9, 0) + 5;
        let launches = claim(&mut state, now, &tz(), &[]);
        assert_eq!(launches.len(), 1);
        let a = &state.automations[0];
        assert_eq!(a.runs.last().map(|r| &r.outcome), Some(&RunOutcome::Starting));
        assert_eq!(a.next_run_at, Some(at(2026, 11, 28, 9, 0)));
        assert!(claim(&mut state, now + 30, &tz(), &[]).is_empty(), "同一次不会领两遍");
        // 一直停在"正在建任务"（应用退出了）：过一阵记为中断，之后照常执行。
        let later = at(2026, 11, 28, 9, 0) + 5;
        assert_eq!(claim(&mut state, later, &tz(), &[]).len(), 1);
        let runs = &state.automations[0].runs;
        assert_eq!(runs[0].outcome, RunOutcome::Skipped { reason: skip::INTERRUPTED.into() });
        assert_eq!(runs[1].outcome, RunOutcome::Starting);
    }

    #[test]
    fn keeps_recent_runs_only() {
        let mut a = automation("@hourly", 0);
        for i in 0..(Automation::KEEP_RUNS as u64 + 5) {
            let outcome = RunOutcome::Skipped { reason: skip::BUSY.into() };
            a.push_run(AutomationRun { scheduled_for: i, at: i, trigger: RunTrigger::Scheduled, outcome });
        }
        assert_eq!(a.runs.len(), Automation::KEEP_RUNS);
        assert_eq!(a.runs[0].scheduled_for, 5);
    }

    /// 用真的引擎走一遍：立即运行建出任务、记在执行记录里；没到点的不动。
    #[cfg(unix)]
    #[test]
    fn run_now_creates_a_task() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("shop");
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        duolo_git::init_repo(&repo).unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            "[agents.claude]\ncommand = \"sh\"\nargs = [\"-c\", \"cat\", \"agent\"]\n",
        )
        .unwrap();
        let mut opened = Engine::open(duolo_core::Paths::at(&home)).unwrap();
        let project = opened.add_project(&repo).unwrap();
        let draft = AutomationDraft {
            name: String::new(),
            project: project.clone(),
            prompt: "Fix flaky tests".into(),
            agent: None,
            base: None,
            cron: "0 9 1 1 *".into(),
        };
        let id = opened.add_automation(draft).unwrap();
        assert_eq!(opened.state().automation(id).unwrap().name, "Fix flaky tests", "没填名字时按需求起名");
        let engine = Mutex::new(opened);
        assert!(run_due(&engine).is_empty(), "没到点");

        lock(&engine).request_automation_run(id).unwrap();
        let started = run_due(&engine);
        assert_eq!(started.len(), 1);
        let mut engine = lock(&engine);
        let task = engine.state().task(started[0]).unwrap().clone();
        assert_eq!((task.automation, task.prompt.as_str()), (Some(id), "Fix flaky tests"));
        let automation = engine.state().automation(id).unwrap();
        assert!(!automation.run_requested);
        let run = automation.runs.last().unwrap();
        assert_eq!((run.trigger, &run.outcome), (RunTrigger::Manual, &RunOutcome::Started { task: task.id }));
        engine.shutdown();
    }
}
