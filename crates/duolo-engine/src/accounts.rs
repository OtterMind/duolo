//! 多个账号（见 `duolo_agent::Account`）：每种 Agent 用哪个账号；一个账号的额度用完时换下一个，接着原来的会话继续做。
//!
//! 额度用完由 Agent 自己报（钩子、聊天协议里的错误类型，插件的 `[account].limit_errors`），不认画面上的字。
//! 换账号就是换启动时的环境变量：结束现在的进程，用新账号接着同一次会话启动，再让它继续。
//! 对话记录在各个账号之间共用（账号文件夹里链接到默认文件夹），所以接得回来。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use duolo_agent::{Account, Home, Registry};
use duolo_core::process::{output_within, OneShot, Prompted};
use duolo_core::{t, AgentKey, AgentKind, Settings, UserError};

use crate::engine::Engine;

/// 额度用完的账号多久之内不再换回去（之后再用时还没恢复，会再报一次）。
const EXHAUSTED_FOR: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Default)]
pub(crate) struct Accounts {
    /// 每种 Agent 选的账号（`settings.json` 的 `accounts`）。
    chosen: BTreeMap<String, String>,
    /// 额度用完时自动换账号（`settings.json` 的 `auto_switch_account`）。
    auto_switch: bool,
    /// 每个正在运行的 Agent 用的是哪个账号（启动时定下）。
    used: HashMap<AgentKey, Account>,
    /// 额度用完的账号，什么时候发现的。
    exhausted: HashMap<(AgentKind, String), Instant>,
}

impl Accounts {
    pub(crate) fn new(settings: &Settings) -> Self {
        Self {
            chosen: settings.accounts.clone(),
            auto_switch: settings.auto_switch_account.unwrap_or(true),
            ..Default::default()
        }
    }
}

impl Engine {
    /// 在 Duolo 里添加的账号放在哪里。
    fn own_accounts(&self, kind: &AgentKind) -> PathBuf {
        self.paths.root().join("accounts").join(&kind.0)
    }

    /// 这种 Agent 能用的账号，第一个是默认的。只看文件夹，很快。
    pub fn accounts_of(&self, kind: &AgentKind) -> Result<Vec<Account>> {
        Ok(self.agents.get(kind)?.accounts(&self.own_accounts(kind), &self.live.home))
    }

    /// 现在选的账号；选过的文件夹没了时用默认的。
    pub fn chosen_account(&self, kind: &AgentKind) -> Account {
        let id = self.accounts.chosen.get(&kind.0).cloned().unwrap_or_default();
        self.accounts_of(kind)
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.id() == id)
            .unwrap_or_else(Account::default_account)
    }

    /// 选账号（`id` 见 [`Account::id`]）：之后启动的用它，正在运行的不受影响。
    pub fn choose_account(&mut self, kind: &AgentKind, id: &str) -> Result<()> {
        if !self.accounts_of(kind)?.iter().any(|a| a.id() == id) {
            return Err(UserError::AccountNotFound.into());
        }
        self.accounts.chosen.insert(kind.0.clone(), id.to_string());
        self.accounts.exhausted.remove(&(kind.clone(), id.to_string()));
        self.save_account_settings();
        Ok(())
    }

    pub fn auto_switch_account(&self) -> bool {
        self.accounts.auto_switch
    }

    pub fn set_auto_switch_account(&mut self, on: bool) {
        self.accounts.auto_switch = on;
        self.save_account_settings();
    }

    fn save_account_settings(&self) {
        let file = self.paths.settings_file();
        if let Ok(mut settings) = Settings::load(&file) {
            settings.accounts.clone_from(&self.accounts.chosen);
            settings.auto_switch_account = Some(self.accounts.auto_switch);
            let _ = settings.save(&file);
        }
    }

    /// 新账号的文件夹（建好、对话记录链接好），在里面登录。登录没成功时用 [`Engine::remove_account`] 删掉。
    pub fn new_account_dir(&self, kind: &AgentKind) -> Result<PathBuf> {
        let plugin = self.agents.get(kind)?;
        if plugin.account_env().is_none() {
            return Err(UserError::AccountNotFound.into());
        }
        // 按添加的先后排序。
        let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let dir = self.own_accounts(kind).join(format!("{millis:015}"));
        plugin.prepare_account(&dir)?;
        Ok(dir)
    }

    /// 删掉在 Duolo 里添加的账号（文件夹）。别的软件里的、默认的不能删。
    pub fn remove_account(&mut self, kind: &AgentKind, id: &str) -> Result<()> {
        let root = self.own_accounts(kind);
        let dir = PathBuf::from(id);
        if id.is_empty() || dir.parent() != Some(root.as_path()) {
            return Err(UserError::AccountNotFound.into());
        }
        // 链接只删链接本身，不会删到默认文件夹里的对话记录。
        std::fs::remove_dir_all(&dir)?;
        if self.accounts.chosen.get(&kind.0).map(String::as_str) == Some(id) {
            self.accounts.chosen.remove(&kind.0);
            self.save_account_settings();
        }
        Ok(())
    }

    /// 启动前定下这个 Agent 用哪个账号。
    pub(crate) fn use_account(&mut self, key: AgentKey) {
        let Ok(kind) = self.plugin_of(key).map(|p| p.kind()) else { return };
        let account = self.chosen_account(&kind);
        self.accounts.used.insert(key, account);
    }

    /// 这个 Agent 用的账号要设的环境变量。
    pub(crate) fn account_vars(&self, key: AgentKey) -> Vec<(String, String)> {
        match (self.accounts.used.get(&key), self.plugin_of(key)) {
            (Some(account), Ok(plugin)) => plugin.account_vars(account),
            _ => Vec::new(),
        }
    }

    /// 这个 Agent 的数据目录（读会话记录、写信任），跟着它用的账号。
    pub(crate) fn home_of(&self, key: AgentKey) -> Home {
        match (self.accounts.used.get(&key), self.plugin_of(key)) {
            (Some(account), Ok(plugin)) => plugin.home_for(&self.live.home, account),
            _ => self.live.home.clone(),
        }
    }

    /// Agent 报了错。额度用完时：记下这个账号用完了，换下一个没用完的账号，接着原来的会话重新启动，让它继续。
    /// 都用完了、只有一个账号、关掉了自动换时什么也不做（它自己会显示额度什么时候恢复）。
    pub(crate) fn agent_failed(&mut self, key: AgentKey, error: &str) {
        let Ok(plugin) = self.plugin_of(key) else { return };
        if !plugin.is_limit_error(error) || !self.accounts.auto_switch {
            return;
        }
        let kind = plugin.kind();
        let used = self.accounts.used.get(&key).map(Account::id).unwrap_or_default();
        self.accounts.exhausted.insert((kind.clone(), used.clone()), Instant::now());
        let Some(next) = self.next_account(&kind, &used) else { return };
        // 别的任务已经换过了（选的已经不是用完的那个）就跟着它，不再往后换。
        if self.chosen_account(&kind).id() == used {
            self.accounts.chosen.insert(kind.0.clone(), next.id());
            self.save_account_settings();
        }
        self.stop_agent(key);
        if self.spawn(key, false).is_ok() {
            self.queue_message(key, t("account.continue"));
        }
    }

    /// `used` 之后第一个没用完的账号。
    fn next_account(&self, kind: &AgentKind, used: &str) -> Option<Account> {
        let accounts = self.accounts_of(kind).ok()?;
        let start = accounts.iter().position(|a| a.id() == used).unwrap_or(0);
        let fresh = |a: &Account| {
            self.accounts.exhausted.get(&(kind.clone(), a.id())).is_none_or(|at| at.elapsed() >= EXHAUSTED_FOR)
        };
        (1..accounts.len()).map(|i| &accounts[(start + i) % accounts.len()]).find(|a| fresh(a)).cloned()
    }
}

/// 一个账号登录了没有、登录的是谁。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountStatus {
    /// 不知道（插件没有查登录的命令、查不出来）时为 None。
    pub signed_in: Option<bool>,
    /// 查登录的命令输出 JSON、里面有 `email` 时。
    pub email: Option<String>,
}

/// 查一个账号的登录状态。要一小会儿，不要拿着引擎的锁调用。
pub fn account_status(agents: &Registry, kind: &AgentKind, account: &Account) -> AccountStatus {
    let Some(status) = agents.get(kind).ok().and_then(|p| p.account_status_of(account)) else {
        return AccountStatus::default();
    };
    let status = OneShot { env_remove: agents.env_remove(), ..status };
    let Some(run) = Prompted::new(&status, None) else { return AccountStatus::default() };
    let Ok((exit, out)) = output_within(run, std::time::Duration::from_secs(10)) else {
        return AccountStatus::default();
    };
    let email = serde_json::from_str::<serde_json::Value>(out.trim())
        .ok()
        .and_then(|v| v["email"].as_str().map(str::to_string))
        .filter(|e| !e.is_empty());
    AccountStatus { signed_in: Some(exit.success()), email }
}
