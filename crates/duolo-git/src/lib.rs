//! 对 git 命令行的薄封装：仓库定位、worktree 管理、改动统计。
//!
//! 写操作统一走 `git` 命令行而不是自己实现，行为和用户手动操作完全一致。

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use duolo_core::UserError;

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|_| UserError::GitMissing)?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(UserError::GitFailed { command: args.join(" "), detail }.into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 返回包含 `path` 的仓库根目录。
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    git(path, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map_err(|_| UserError::NotGitRepo { path: path.display().to_string() }.into())
}

/// 当前分支名；处于分离头指针时返回提交号。
pub fn current_branch(root: &Path) -> Result<String> {
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return git(root, &["rev-parse", "--short", "HEAD"]);
    }
    Ok(branch)
}

/// 基于 `base` 新建分支 `branch`，并在 `path` 创建 worktree。
pub fn add_worktree(root: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let path_str = path.to_str().context("worktree path is not valid UTF-8")?;
    git(root, &["worktree", "add", "-b", branch, path_str, base])?;
    Ok(())
}

/// 重命名 worktree 当前所在的分支。
pub fn rename_branch(worktree: &Path, new_name: &str) -> Result<()> {
    git(worktree, &["branch", "-m", new_name])?;
    Ok(())
}

/// 删除 worktree。分支保留，方便之后查看或合并。
pub fn remove_worktree(root: &Path, path: &Path, force: bool) -> Result<()> {
    let path_str = path.to_str().context("worktree path is not valid UTF-8")?;
    let mut args = vec!["worktree", "remove", path_str];
    if force {
        args.push("--force");
    }
    git(root, &args)?;
    Ok(())
}

/// 工作区相对 `base` 的改动统计（包含未提交的修改）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStat {
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
}

impl DiffStat {
    pub fn is_empty(&self) -> bool {
        self.files == 0
    }
}

pub fn diff_stat(worktree: &Path, base: &str) -> Result<DiffStat> {
    let out = git(worktree, &["diff", "--shortstat", base])?;
    Ok(parse_shortstat(&out))
}

/// 解析 `git diff --shortstat` 的输出，例如
/// ` 3 files changed, 12 insertions(+), 4 deletions(-)`。
fn parse_shortstat(text: &str) -> DiffStat {
    let mut stat = DiffStat::default();
    for part in text.split(',') {
        let mut words = part.split_whitespace();
        let (Some(num), Some(label)) = (words.next(), words.next()) else { continue };
        let Ok(num) = num.parse::<u32>() else { continue };
        if label.starts_with("file") {
            stat.files = num;
        } else if label.starts_with("insertion") {
            stat.insertions = num;
        } else if label.starts_with("deletion") {
            stat.deletions = num;
        }
    }
    stat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortstat() {
        let s = parse_shortstat(" 3 files changed, 12 insertions(+), 4 deletions(-)");
        assert_eq!(s, DiffStat { files: 3, insertions: 12, deletions: 4 });
        let s = parse_shortstat(" 1 file changed, 1 deletion(-)");
        assert_eq!(s, DiffStat { files: 1, insertions: 0, deletions: 1 });
        assert!(parse_shortstat("").is_empty());
    }

    #[test]
    fn worktree_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        let run = |args: &[&str]| git(&repo, args).unwrap();
        run(&["init", "-q", "-b", "main"]);
        run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);

        let root = repo_root(&repo).unwrap();
        assert_eq!(current_branch(&root).unwrap(), "main");

        let wt = dir.path().join("wt/1-demo");
        add_worktree(&root, &wt, "duolo/1-demo", "main").unwrap();
        assert!(wt.join(".git").exists());
        std::fs::write(wt.join("a.txt"), "hello\n").unwrap();
        git(&wt, &["add", "a.txt"]).unwrap();
        let stat = diff_stat(&wt, "main").unwrap();
        assert_eq!(stat.files, 1);
        assert_eq!(stat.insertions, 1);

        remove_worktree(&root, &wt, true).unwrap();
        assert!(!wt.exists());
    }
}
