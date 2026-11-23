//! 把口述原话整理成一条可以直接发给编程 Agent 的指令。

use std::time::Duration;

use anyhow::Result;
use duolo_core::process::OneShot;

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
        context.push_str(&format!("Project: {project}\n"));
    }
    if let Some(task) = &ctx.task {
        context.push_str(&format!("Current task: {task}\n"));
    }
    format!(
        "You clean up voice transcripts. Below is what a user said to a coding agent, from speech recognition; \
it may contain filler words, repetitions, self-corrections or recognition errors.\n\
Rewrite it as one instruction that can be sent to the coding agent directly:\n\
- remove filler words and repetitions;\n\
- when the user corrects themselves, keep only the final version;\n\
- fix obvious recognition errors, but keep file names, function names, table names and other identifiers as spoken;\n\
- do not add requirements the user did not state; do not answer or explain;\n\
- write in the same language the user spoke;\n\
- output only the instruction itself, without quotes or prefixes.\n\n\
{context}Transcript:\n{raw}"
    )
}

/// 用 `command` 整理口述内容。失败或超时由调用方决定是否退回原话。
pub fn organize(command: &OneShot, timeout: Duration, raw: &str, ctx: &OrganizeContext) -> Result<String> {
    let prompt = organize_prompt(raw, ctx);
    let out = run(command, Some(&prompt), timeout)?;
    let cleaned = out.trim().trim_matches(['"', '“', '”']).trim().to_string();
    Ok(if cleaned.is_empty() { raw.to_string() } else { cleaned })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_context_and_raw() {
        let ctx = OrganizeContext { project: Some("shop".into()), task: None };
        let p = organize_prompt("um export CSV", &ctx);
        assert!(p.contains("Project: shop"));
        assert!(p.ends_with("um export CSV"));
    }

    #[test]
    fn uses_organizer_output() {
        let argv = ["sh", "-c", "echo '\"Export CSV\"'", "sh"].map(String::from).to_vec();
        let timeout = Duration::from_secs(5);
        let out = organize(&argv.into(), timeout, "um export CSV", &OrganizeContext::default()).unwrap();
        assert_eq!(out, "Export CSV");
    }
}
