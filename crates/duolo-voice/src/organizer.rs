//! 把口述原话整理成一条可以直接发给编程 Agent 的指令。

use std::time::Duration;

use anyhow::Result;
use duolo_core::VoiceConfig;

use crate::process::run;

/// 整理时提供给模型的上下文，帮助它理解指代。
#[derive(Debug, Clone, Default)]
pub struct OrganizeContext {
    pub project: Option<String>,
    pub task: Option<String>,
}

pub fn organize_prompt(raw: &str, ctx: &OrganizeContext) -> String {
    let mut context = String::new();
    if let Some(project) = &ctx.project {
        context.push_str(&format!("项目：{project}\n"));
    }
    if let Some(task) = &ctx.task {
        context.push_str(&format!("当前任务：{task}\n"));
    }
    format!(
        "你是语音转写整理助手。下面是用户口述给编程 Agent 的一段话，来自语音识别，可能有口头语、重复、改口或识别错误。\n\
请把它整理成一条可以直接发给编程 Agent 的指令：\n\
- 去掉口头语和重复；\n\
- 出现改口时只保留最后的说法；\n\
- 修正明显的识别错误，文件名、函数名、表名等专有名词保持原样；\n\
- 不要添加用户没有说过的要求，不要回答、不要解释；\n\
- 只输出整理后的指令本身，不加引号和前缀。\n\n\
{context}口述原文：\n{raw}"
    )
}

/// 整理口述内容。失败或超时由调用方决定是否退回原话。
pub fn organize(cfg: &VoiceConfig, raw: &str, ctx: &OrganizeContext) -> Result<String> {
    let prompt = organize_prompt(raw, ctx);
    let out = run(&cfg.organizer, Some(&prompt), Duration::from_secs(cfg.organize_timeout_secs))?;
    let cleaned = out.trim().trim_matches(['"', '“', '”']).trim().to_string();
    Ok(if cleaned.is_empty() { raw.to_string() } else { cleaned })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_context_and_raw() {
        let ctx = OrganizeContext { project: Some("shop".into()), task: None };
        let p = organize_prompt("嗯 导出 CSV", &ctx);
        assert!(p.contains("项目：shop"));
        assert!(p.ends_with("嗯 导出 CSV"));
    }

    #[test]
    fn uses_organizer_output() {
        let cfg = VoiceConfig {
            organizer: vec!["sh".into(), "-c".into(), "echo '\"导出 CSV\"'".into(), "sh".into()],
            ..VoiceConfig::default()
        };
        assert_eq!(organize(&cfg, "嗯 导出 CSV", &OrganizeContext::default()).unwrap(), "导出 CSV");
    }
}
