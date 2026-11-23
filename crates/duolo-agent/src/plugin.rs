//! 一个可以用的 Agent：声明加上用户在 config.toml 里的修改。

use duolo_core::process::{self, OneShot};
use duolo_core::{AgentKind, AgentOverride, AgentRole};

use crate::manifest::{HeadlessSpec, Keys, Manifest, PromptVia, Screen};
use crate::transcript::{self, Home, Watch};

#[derive(Debug, Clone)]
pub struct Plugin {
    manifest: Manifest,
    icon: Option<String>,
    /// config.toml 里整条换掉的审阅命令。
    review_command: Option<Vec<String>>,
}

/// 在终端里启动它要知道的事。
#[derive(Debug, Clone, Copy)]
pub struct Start<'a> {
    pub role: AgentRole,
    /// 恢复上一次会话；这时不带 `prompt`。
    pub resume: bool,
    pub prompt: Option<&'a str>,
}

/// 启动它的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    /// 要等它准备好之后再粘贴过去的第一句话。
    pub paste: Option<String>,
}

impl Plugin {
    pub(crate) fn new(manifest: Manifest, icon: Option<String>) -> Self {
        Self { manifest, icon, review_command: None }
    }

    /// 套用 config.toml 里 `[agents.<id>]` 的修改，没写的沿用原来的。
    pub(crate) fn with(mut self, changes: &AgentOverride) -> Self {
        fn set<T: Clone>(target: &mut T, value: &Option<T>) {
            if let Some(value) = value {
                target.clone_from(value);
            }
        }
        set(&mut self.manifest.command, &changes.command);
        set(&mut self.manifest.start.args, &changes.args);
        set(&mut self.manifest.start.resume_args, &changes.resume_args);
        set(&mut self.manifest.start.review_args, &changes.review_args);
        if let Some(install) = &changes.install {
            self.manifest.install = crate::manifest::Install { default: install.clone(), ..Default::default() };
        }
        self.review_command = changes.review_command.clone().filter(|argv| !argv.is_empty());
        self
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn kind(&self) -> AgentKind {
        AgentKind(self.manifest.id.clone())
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn command(&self) -> &str {
        &self.manifest.command
    }

    /// 没安装时提示用户运行的命令。
    pub fn install(&self) -> &str {
        self.manifest.install.command()
    }

    /// 单色的 SVG 图标。
    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    pub fn icon_color(&self) -> Option<&str> {
        self.manifest.icon_color.as_deref()
    }

    pub fn screen(&self) -> &Screen {
        &self.manifest.screen
    }

    pub fn keys(&self) -> &Keys {
        &self.manifest.keys
    }

    pub(crate) fn env_remove(&self) -> &[String] {
        &self.manifest.env.remove
    }

    /// 本机找得到它的命令（不保证能运行）。
    pub fn is_installed(&self) -> bool {
        process::find(self.command()).is_some()
    }

    /// 在终端里启动它的命令。
    pub fn launch(&self, start: Start<'_>) -> Launch {
        // Windows 上 npm 装的命令是批处理文件，要经过 cmd.exe，带不了换行，也容易被特殊字符打乱。
        self.launch_as(start, process::is_batch_file(process::resolve(self.command())))
    }

    fn launch_as(&self, start: Start<'_>, batch: bool) -> Launch {
        let spec = &self.manifest.start;
        let mut args = spec.args.clone();
        let mut paste = None;
        if start.role == AgentRole::Reviewer {
            args.extend(spec.review_args.iter().cloned());
        }
        if start.resume {
            args.extend(spec.resume_args.iter().cloned());
        } else if let Some(prompt) = start.prompt {
            match spec.prompt {
                _ if batch => paste = Some(prompt.to_string()),
                PromptVia::Paste => paste = Some(prompt.to_string()),
                PromptVia::Flag => args.push(format!("{}{prompt}", spec.prompt_flag)),
                PromptVia::Argument => args.extend(process::prompt_args(spec.separator.as_deref(), prompt)),
            }
        }
        Launch { program: self.command().to_string(), args, paste }
    }

    /// 后台审阅的命令；不支持审阅时为 None。
    pub fn review(&self) -> Option<OneShot> {
        match &self.review_command {
            Some(argv) => Some(OneShot::from(argv.clone())),
            None => self.manifest.review.as_ref().map(|spec| self.headless(spec)),
        }
    }

    /// 做小事（起分支名、整理口述）的命令。
    pub fn quick(&self) -> Option<OneShot> {
        self.manifest.quick.as_ref().map(|spec| self.headless(spec))
    }

    fn headless(&self, spec: &HeadlessSpec) -> OneShot {
        let mut argv = vec![self.command().to_string()];
        argv.extend(spec.args.iter().cloned());
        OneShot { argv, separator: spec.separator.clone(), stdin_args: spec.stdin_args.clone(), env_remove: Vec::new() }
    }

    /// 开始跟着它在 `worktree` 里的会话记录；读不了它的记录时为 None。
    pub fn watch(&self, worktree: &std::path::Path, home: &Home) -> Option<Watch> {
        let spec = self.manifest.transcript.as_ref()?;
        let format = transcript::format(&spec.format)?;
        Some(Watch::start(format, worktree, home.data_dir(&spec.home, spec.home_env.as_deref())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(start: &str) -> Plugin {
        let text = format!("schema = 1\nid = \"x\"\nname = \"X\"\ncommand = \"x\"\n[start]\n{start}");
        Plugin::new(Manifest::parse(&text).unwrap(), None)
    }

    fn write(prompt: &str) -> Start<'_> {
        Start { role: AgentRole::Writer, resume: false, prompt: Some(prompt) }
    }

    #[test]
    fn prompts_are_never_read_as_options() {
        let args = |start: &str, prompt: &str| plugin(start).launch_as(write(prompt), false).args;
        assert_eq!(args("separator = \"--\"", "- fix it"), ["--", "- fix it"]);
        // 不支持 `--` 的：前面加个空格，意思不变，也不会被当成选项。
        assert_eq!(args("", "- fix it"), [" - fix it"]);
        assert_eq!(args("", "fix it"), ["fix it"]);
        assert_eq!(args("prompt = \"flag\"\nprompt_flag = \"--prompt=\"", "- fix it"), ["--prompt=- fix it"]);
    }

    #[test]
    fn prompt_can_be_pasted_instead() {
        let pasted = plugin("prompt = \"paste\"\nargs = [\"-v\"]").launch_as(write("a\nb"), false);
        assert_eq!(pasted, Launch { program: "x".into(), args: vec!["-v".into()], paste: Some("a\nb".into()) });
        // 批处理文件带不了换行，不管声明怎么写。
        for start in ["separator = \"--\"", "prompt = \"flag\"\nprompt_flag = \"--prompt=\""] {
            let batch = plugin(start).launch_as(write("a\nb"), true);
            assert_eq!((batch.args.len(), batch.paste.as_deref()), (0, Some("a\nb")));
        }
    }

    #[test]
    fn resuming_does_not_repeat_the_prompt() {
        let p = plugin("resume_args = [\"-c\"]\nreview_args = [\"--ro\"]");
        let start = Start { role: AgentRole::Reviewer, resume: true, prompt: Some("again") };
        assert_eq!(p.launch_as(start, false).args, ["--ro", "-c"]);
    }

    #[test]
    fn changing_one_setting_keeps_the_rest() {
        let text = "schema = 1\nid = \"x\"\nname = \"X\"\ncommand = \"x\"\n\
                    [start]\nresume_args = [\"-c\"]\nseparator = \"--\"\n[review]\nargs = [\"-p\"]\n[install]\ndefault = \"i\"\n";
        let base = Plugin::new(Manifest::parse(text).unwrap(), None);
        let moved = base.clone().with(&AgentOverride { command: Some("/opt/x".into()), ..Default::default() });
        assert_eq!(moved.manifest.start.resume_args, ["-c"]);
        assert_eq!(moved.review().unwrap().argv, ["/opt/x", "-p"], "审阅也用换过的命令");
        assert_eq!(moved.install(), "i");

        let custom = base.with(&AgentOverride {
            resume_args: Some(Vec::new()),
            review_command: Some(vec!["my-review".into()]),
            ..Default::default()
        });
        assert!(custom.manifest.start.resume_args.is_empty(), "写成空的就是空的");
        assert_eq!(custom.review().unwrap(), OneShot::from(vec!["my-review".to_string()]));
    }
}
