//! 对 git 命令行的薄封装：仓库定位、worktree 管理、改动统计。
//!
//! 写操作统一走 `git` 命令行而不是自己实现，行为和用户手动操作完全一致。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use duolo_core::UserError;

/// 这些变量会让 git 不管 `-C` 指的是哪里，去操作别的仓库。从 git 钩子或别名里启动 Duolo 时会继承到。
const REPO_VARS: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
    "GIT_NAMESPACE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    git_with(dir, args, &[])
}

/// 带上环境变量运行 git（在 [`REPO_VARS`] 去掉之后设置）。
fn git_with(dir: &Path, args: &[&str], env: &[(&str, &std::ffi::OsStr)]) -> Result<String> {
    let mut cmd = duolo_core::process::command("git");
    cmd.arg("-C").arg(dir).args(args);
    for var in REPO_VARS {
        cmd.env_remove(var);
    }
    cmd.envs(env.iter().copied());
    let output = cmd.output().map_err(|_| UserError::GitMissing)?;
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

/// 本地分支列表，当前分支排在最前面。
pub fn branches(root: &Path) -> Result<Vec<String>> {
    let current = current_branch(root).ok();
    let out = git(root, &["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"])?;
    let mut list: Vec<String> = out.lines().map(str::trim).filter(|b| !b.is_empty()).map(String::from).collect();
    if let Some(current) = current {
        list.retain(|b| *b != current);
        list.insert(0, current);
    }
    Ok(list)
}

/// `name` 能不能当作新分支名（`git check-ref-format --branch`）。
pub fn is_valid_branch_name(dir: &Path, name: &str) -> bool {
    !name.is_empty() && !name.starts_with('-') && git(dir, &["check-ref-format", "--branch", name]).is_ok()
}

/// 仓库里是否已经有这个本地分支。
pub fn branch_exists(root: &Path, name: &str) -> bool {
    git(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{name}")]).is_ok()
}

/// `path` 本身是不是一个 git 仓库的根目录（主仓库，不是别的仓库的 worktree）。
pub fn is_repo_root(path: &Path) -> bool {
    path.join(".git").is_dir()
}

/// 把普通文件夹初始化为 git 仓库，并把现有文件做成第一次提交（没有文件时提交一个空提交），
/// 这样才能基于它创建 worktree。
pub fn init_repo(path: &Path) -> Result<()> {
    git(path, &["init", "-q"])?;
    git(path, &["add", "-A"])?;
    let mut args = vec!["commit", "-q", "--allow-empty", "-m", "Initial commit"];
    args.splice(0..0, identity(path));
    git(path, &args)?;
    Ok(())
}

/// 克隆 `url` 到 `dest`。
pub fn clone(url: &str, dest: &Path) -> Result<()> {
    let parent = dest.parent().context("clone destination has no parent")?;
    std::fs::create_dir_all(parent)?;
    let dest_str = dest.to_str().context("clone destination is not valid UTF-8")?;
    git(parent, &["clone", "-q", url, dest_str])?;
    Ok(())
}

/// 从克隆地址推出目录名：`git@github.com:a/shop.git` → `shop`。
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let last = url.trim().trim_end_matches('/').rsplit(['/', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty()).then(|| name.to_string())
}

/// 文件的身份：同一个目录不管路径怎么写（大小写、符号链接）都一样。
/// macOS / Linux 用设备号和 inode。
#[cfg(unix)]
pub fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Windows：解析成真实路径（展开链接），再忽略大小写比较。
#[cfg(not(unix))]
pub fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::hash::{Hash, Hasher};
    let real = std::fs::canonicalize(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    real.to_string_lossy().to_lowercase().hash(&mut hasher);
    Some((0, hasher.finish()))
}

/// 在 `roots` 下找 git 仓库：每个目录本身和往下两层（例如 `~/code/org/repo`），不进入仓库内部和隐藏目录。
/// 只算主仓库（`.git` 是目录）；其他仓库的 worktree（`.git` 是文件）跳过，它们不是独立的项目。
/// 按最近修改时间从新到旧排列，最多 `limit` 个。
pub fn find_repos(roots: &[PathBuf], limit: usize) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u32, found: &mut Vec<(std::time::SystemTime, PathBuf)>) {
        let git = dir.join(".git");
        if git.is_dir() {
            let modified = std::fs::metadata(&git).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            found.push((modified, dir.to_path_buf()));
            return;
        }
        if git.exists() {
            return;
        }
        if depth == 0 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten().take(500) {
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if !hidden && entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), depth - 1, found);
            }
        }
    }
    let mut found = Vec::new();
    // macOS 的文件系统默认不区分大小写，`~/projects` 和 `~/Projects` 是同一个目录：按文件标识去重。
    let mut seen = std::collections::HashSet::new();
    for root in roots.iter().filter(|r| file_id(r).is_some_and(|id| seen.insert(id))) {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        for entry in entries.flatten().take(500) {
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if !hidden && entry.file_type().is_ok_and(|t| t.is_dir()) {
                walk(&entry.path(), 1, &mut found);
            }
        }
    }
    found.sort_by_key(|repo| std::cmp::Reverse(repo.0));
    found.into_iter().take(limit).map(|(_, path)| path).collect()
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

/// 删除 worktree 和它的登记（目录已经不在时只删登记）。分支保留，方便之后查看或合并。
pub fn remove_worktree(root: &Path, path: &Path, force: bool) -> Result<()> {
    let path_str = path.to_str().context("worktree path is not valid UTF-8")?;
    let mut args = vec!["worktree", "remove", path_str];
    if force {
        args.push("--force");
    }
    git(root, &args)?;
    Ok(())
}

/// 记下 worktree 现在的样子（包括没提交的改动和新文件，不包括被 git 忽略的）：一个以 `HEAD` 为父提交的提交，
/// 只写进 git 的对象库，不改 worktree、分支和暂存区。没有分支指着它，git 至少两周后才会清理。
pub fn snapshot_commit(worktree: &Path, message: &str) -> Result<String> {
    let temp = tempfile::Builder::new().prefix("duolo-index").tempdir().context("cannot create a temporary index")?;
    let index = temp.path().join("index");
    // 从现在的暂存区开始，git 已经算好的文件信息不用重算。
    let current = worktree.join(git(worktree, &["rev-parse", "--git-path", "index"])?);
    let _ = std::fs::copy(current, &index);
    let env = [("GIT_INDEX_FILE", index.as_os_str())];
    git_with(worktree, &["add", "-A"], &env)?;
    let tree = git_with(worktree, &["write-tree"], &env)?;
    let mut args = identity(worktree);
    args.extend(["commit-tree", &tree, "-p", "HEAD", "-m", message]);
    git(worktree, &args)
}

/// 把 worktree 现在的样子检出到 `dest`，给审阅这样只读的事用，返回检出的提交（见 [`snapshot_commit`]）。
/// `dest` 是一个独立的克隆：对象借用原仓库的（不复制），但分支、配置都是自己的，也没有远程，
/// 在里面删分支、推送都碰不到原仓库。用完调用 [`remove_snapshot`]。
pub fn snapshot(worktree: &Path, dest: &Path) -> Result<String> {
    let parent = dest.parent().context("snapshot path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let dest_str = dest.to_str().context("snapshot path is not valid UTF-8")?;
    let commit = snapshot_commit(worktree, "Duolo snapshot")?;
    let common = worktree.join(git(worktree, &["rev-parse", "--git-common-dir"])?);
    let common = common.to_str().context("repository path is not valid UTF-8")?;
    // 不带模板：用户的模板里可能有钩子。
    git(parent, &["clone", "--quiet", "--shared", "--no-checkout", "--template=", common, dest_str])?;
    git(dest, &["remote", "remove", "origin"])?;
    // 不跑钩子；大文件（LFS）不下载，审阅看的是改动。
    let env = [("GIT_LFS_SKIP_SMUDGE", std::ffi::OsStr::new("1"))];
    git_with(dest, &["-c", "core.hooksPath=/dev/null", "checkout", "--quiet", "--detach", &commit], &env)?;
    Ok(commit)
}

/// 删掉 [`snapshot`] 建的副本。里面被改过、多了文件也照样删。
pub fn remove_snapshot(dest: &Path) -> Result<()> {
    // Windows 上刚结束的进程可能还占着文件，等一会儿再试。
    let mut removed = std::fs::remove_dir_all(dest);
    for _ in 0..10 {
        match &removed {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                std::thread::sleep(std::time::Duration::from_millis(300));
                removed = std::fs::remove_dir_all(dest);
            }
            _ => break,
        }
    }
    match removed {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
        _ => Ok(()),
    }
}

/// 给审阅者看的改动：`from` 之后的提交、改了哪些文件、完整的 diff（到 `to`），写成一份 Markdown。
/// 审阅者不用自己运行 git（有的审阅者手里没有运行命令的工具）。
pub fn changes_report(worktree: &Path, from: &str, to: &str) -> Result<String> {
    let range = format!("{from}..{to}~1");
    // 文件名原样写，不转义成 `\346\226…`。
    let plain = ["-c", "core.quotePath=false"];
    let commits = git(worktree, &["log", "--oneline", "--no-decorate", &range])?;
    let files = git(worktree, &[&plain[..], &["diff", "--stat=200", "--find-renames", from, to]].concat())?;
    let diff = git(worktree, &[&plain[..], &["diff", "--find-renames", from, to]].concat())?;
    let or_none = |text: String| if text.is_empty() { "(none)".to_string() } else { text };
    Ok(format!(
        "# Changes to review\n\nEverything changed since commit {from}, including work that was not committed yet.\n\n\
## Commits\n\n{}\n\n## Files\n\n{}\n\n## Diff\n\n{}\n",
        or_none(commits),
        or_none(files),
        or_none(diff)
    ))
}

/// `path` 现在还是不是一个能用的 worktree（它的 `.git` 文件还在，主仓库也还在）。
pub fn is_worktree(path: &Path) -> bool {
    // 先看 `.git`：没有它时 git 会往上找到别的仓库（例如家目录本身就是仓库），那就认错了。
    path.join(".git").is_file() && git(path, &["rev-parse", "--git-dir"]).is_ok()
}

/// 把 worktree 里还没有保存到任何分支的内容保存下来，之后就可以放心删除它：
/// 没提交的改动（包括新文件）提交到当前分支；不在任何分支上时（Agent 切到了某个提交，或者 rebase 做到一半），
/// 给它建一个分支 `fallback`（已经有了就加序号）。rebase、merge 自动暂存的改动放进 `git stash`。被 git 忽略的文件不算。
/// `base` 是任务的基准分支，它里面原本就有的子模块提交不算没保存。
///
/// 返回保存到了哪个分支；本来就没有要保存的返回 None。没法完整保存时报错，什么都不删。
pub fn save_worktree(worktree: &Path, base: &str, fallback: &str, message: &str) -> Result<Option<String>> {
    reveal_hidden_changes(worktree)?;
    if let Some(nested) = unpushed_nested(worktree, Some(base))? {
        return Err(UserError::NestedRepo { path: nested.display().to_string() }.into());
    }
    let saved = commit_until_clean(worktree, message)?;
    keep_autostash(worktree)?;
    Ok(current_or_new_branch(worktree, fallback)?.filter(|b| saved || b.is_new).map(|b| b.name))
}

/// [`save_worktree`] 之后、已经挪走等着删除的 worktree：把挪动时刚好写进来的改动也保存下来。
/// 有子模块时跳过：子模块的配置里记着原来的位置，挪走后 git 打不开它（前一次已经保存过了）。
pub fn save_moved_worktree(moved: &Path, fallback: &str, message: &str) -> Result<Option<String>> {
    if !nested_links(moved)?.is_empty() {
        return Ok(None);
    }
    let saved = commit_until_clean(moved, message)?;
    Ok(current_or_new_branch(moved, fallback)?.filter(|b| saved || b.is_new).map(|b| b.name))
}

/// 提交全部改动，返回有没有提交过。Agent 的子进程可能还在写文件：再确认一遍，直到没有新的改动。
fn commit_until_clean(worktree: &Path, message: &str) -> Result<bool> {
    let mut saved = false;
    for _ in 0..3 {
        if let Some(path) = nested_changes(worktree)? {
            return Err(UserError::NestedRepo { path: path.display().to_string() }.into());
        }
        match commit_all(worktree, message) {
            Ok(false) => return Ok(saved),
            Ok(true) => saved = true,
            // 文件正写到一半，git 读到的比它说的长度短（Windows 上常见）：等一下再来。
            Err(err) if is_being_written(&err) => std::thread::sleep(std::time::Duration::from_millis(300)),
            Err(err) => return Err(err),
        }
    }
    Err(UserError::StillChanging { path: worktree.display().to_string() }.into())
}

/// git 读文件时它还在被写。
fn is_being_written(err: &anyhow::Error) -> bool {
    match err.downcast_ref::<UserError>() {
        Some(UserError::GitFailed { detail, .. }) => {
            detail.contains("short read") || detail.contains("changed as we read")
        }
        _ => false,
    }
}

struct Branch {
    name: String,
    is_new: bool,
}

/// 当前分支。不在分支上、当前提交也不属于任何分支时新建 `fallback`；已经属于别的分支时返回 None。
fn current_or_new_branch(worktree: &Path, fallback: &str) -> Result<Option<Branch>> {
    match git(worktree, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(name) => Ok(Some(Branch { name, is_new: false })),
        Err(_) => {
            let kept = git(
                worktree,
                &["for-each-ref", "--count=1", "--contains", "HEAD", "refs/heads", "refs/remotes", "refs/tags"],
            )?;
            if !kept.is_empty() {
                return Ok(None);
            }
            let name = unique_branch(worktree, fallback);
            git(worktree, &["branch", &name, "HEAD"])?;
            Ok(Some(Branch { name, is_new: true }))
        }
    }
}

/// 用 `update-index --assume-unchanged` / `--skip-worktree` 藏起来的文件，git 看不到它们的改动：取消标记。
fn reveal_hidden_changes(worktree: &Path) -> Result<()> {
    let listed = git(worktree, &["ls-files", "-v", "-z"])?;
    let entries = || listed.split('\0').filter(|e| e.len() > 2).map(|e| (e.as_bytes()[0], &e[2..]));
    let assumed: Vec<&str> = entries().filter(|(tag, _)| tag.is_ascii_lowercase()).map(|(_, path)| path).collect();
    // 稀疏检出的文件也有 skip-worktree 标记，但它们不在磁盘上，不用管。
    let skipped: Vec<&str> = entries()
        .filter(|(tag, path)| tag.eq_ignore_ascii_case(&b'S') && worktree.join(path).symlink_metadata().is_ok())
        .map(|(_, path)| path)
        .collect();
    for (flag, paths) in [("--no-assume-unchanged", assumed), ("--no-skip-worktree", skipped)] {
        for chunk in paths.chunks(100) {
            git(worktree, &[&["update-index", flag, "--"], chunk].concat())?;
        }
    }
    Ok(())
}

/// rebase、merge 开始前自动暂存的改动（`rebase.autoStash`）只记在 worktree 的登记里，删掉就没了：放进 `git stash`。
fn keep_autostash(worktree: &Path) -> Result<()> {
    for name in ["rebase-merge/autostash", "rebase-apply/autostash", "MERGE_AUTOSTASH"] {
        let path = worktree.join(git(worktree, &["rev-parse", "--git-path", name])?);
        let Ok(sha) = std::fs::read_to_string(path) else { continue };
        let sha = sha.trim();
        let stashed = git(worktree, &["stash", "list", "--format=%H"])?;
        if !sha.is_empty() && !stashed.lines().any(|s| s == sha) {
            git(worktree, &["stash", "store", "-m", "autostash", sha])?;
        }
    }
    Ok(())
}

/// 提交全部改动（包括新文件），没有改动时返回 false。
fn commit_all(dir: &Path, message: &str) -> Result<bool> {
    // 有文件加不进来（例如没有读权限）就报错，不能漏掉。
    git(dir, &["-c", "add.ignoreErrors=false", "add", "-A"])?;
    // 有改动时 git 以 1 退出，没有时是 0。
    if git(dir, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    // 只是为了不丢东西：不跑钩子，不签名（签名可能弹窗要密码）。
    let mut args = vec![
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "commit.gpgSign=false",
        "commit",
        "-q",
        "--no-verify",
        "-m",
        message,
    ];
    args.splice(0..0, identity(dir));
    git(dir, &args)?;
    Ok(true)
}

/// 没有配置完整的 git 身份时，提交用的占位身份（配置好了就是空的）。
fn identity(dir: &Path) -> Vec<&'static str> {
    let known = ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"].iter().all(|var| git(dir, &["var", var]).is_ok());
    if known {
        Vec::new()
    } else {
        vec!["-c", "user.name=Duolo", "-c", "user.email=duolo@localhost"]
    }
}

/// worktree 里有改动的另一个仓库：没加进来的嵌套仓库，或者有改动的子模块。
/// `git add` 只会记下它的提交号，不会保存里面的内容，删掉 worktree 就没了。
fn nested_changes(worktree: &Path) -> Result<Option<PathBuf>> {
    let out = git(
        worktree,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all", "--ignore-submodules=none", "--no-renames"],
    )?;
    // <sub> 是 `S<提交变了><有改动><有新文件>`。只是提交变了不算：有没有推送由 unpushed_nested 看。
    let dirty = |sub: &str| sub.starts_with('S') && (sub.contains('M') || sub.contains('U'));
    Ok(out
        .split('\0')
        .find_map(|entry| match entry.as_bytes().first()? {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            b'1' => {
                let mut fields = entry.splitn(9, ' ');
                dirty(fields.nth(2)?).then(|| fields.nth(5)).flatten()
            }
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            b'u' => {
                let mut fields = entry.splitn(11, ' ');
                dirty(fields.nth(2)?).then(|| fields.nth(7)).flatten()
            }
            // 列出全部新文件时，只有嵌套仓库会以目录的形式出现。
            b'?' => entry[2..].strip_suffix('/'),
            _ => None,
        })
        .map(|path| worktree.join(path)))
}

/// 已经记进 git、也检出了的另一个仓库：子模块，或者提交成了引用的嵌套仓库。返回相对路径。
fn nested_links(dir: &Path) -> Result<Vec<String>> {
    // <模式> <提交> <阶段>\t<路径>；160000 是对另一个仓库的引用。
    Ok(git(dir, &["ls-files", "--stage", "-z"])?
        .split('\0')
        .filter(|e| e.starts_with("160000 "))
        .filter_map(|e| e.split_once('\t'))
        .map(|(_, path)| path.to_string())
        .filter(|path| dir.join(path).join(".git").exists())
        .collect())
}

/// [`nested_links`] 里有还没推送到任何地方的提交或暂存的那一个。
fn unpushed_nested(dir: &Path, base: Option<&str>) -> Result<Option<PathBuf>> {
    for path in nested_links(dir)? {
        let nested = dir.join(&path);
        let mut args = vec!["rev-list", "-n1", "HEAD", "--branches"];
        if git(&nested, &["rev-parse", "--verify", "--quiet", "refs/stash"]).is_ok() {
            args.push("refs/stash");
        }
        args.extend(["--not", "--remotes", "--tags"]);
        // 任务开始前就记在基准分支里的提交，别处一定有。
        let known =
            base.and_then(|base| git(dir, &["rev-parse", "--verify", "--quiet", &format!("{base}:{path}")]).ok());
        if let Some(known) = known.as_deref().filter(|sha| git(&nested, &["cat-file", "-e", sha]).is_ok()) {
            args.push(known);
        }
        // 读不出来（例如还没有提交）也当作没保存。
        if git(&nested, &args).map_or(true, |unpushed| !unpushed.is_empty()) {
            return Ok(Some(nested));
        }
        if let Some(deeper) = unpushed_nested(&nested, None)? {
            return Ok(Some(deeper));
        }
    }
    Ok(None)
}

/// `wanted` 已经存在时依次加 `-2`、`-3`…，返回仓库里还没有的分支名。
pub fn unique_branch(repo: &Path, wanted: &str) -> String {
    if !branch_exists(repo, wanted) {
        return wanted.to_string();
    }
    (2..).map(|n| format!("{wanted}-{n}")).find(|b| !branch_exists(repo, b)).expect("a free suffix always exists")
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

/// 任务从 `base` 分出来的那个提交：任务的改动都相对它算。`base` 之后的新提交是别人的，不算进来；
/// 任务合并过 `base`（或它的远程分支）的话，是最近一次合并的位置。
pub fn fork_point(worktree: &Path, base: &str) -> Result<String> {
    // 任务合并的往往是远程的 main，本地的 main 可能还是旧的，两个都要看。
    let upstream = git(worktree, &["rev-parse", "--verify", "--quiet", &format!("{base}@{{upstream}}")]).ok();
    let mut args = vec!["merge-base", "HEAD", base];
    args.extend(upstream.as_deref());
    git(worktree, &args)
}

/// 任务相对 `base` 的改动统计，包括没提交的修改和还没加进 git 的新文件。
pub fn diff_stat(worktree: &Path, base: &str) -> Result<DiffStat> {
    // 没有共同的提交（例如任务新建了孤立分支）时直接和 base 比。
    let from = fork_point(worktree, base).unwrap_or_else(|_| base.to_string());
    let mut stat = parse_shortstat(&git(worktree, &["diff", "--shortstat", &from])?);
    let untracked = git(worktree, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    for (i, path) in untracked.split('\0').filter(|p| !p.is_empty()).enumerate() {
        stat.files += 1;
        if i < MAX_COUNTED_FILES {
            stat.insertions += count_lines(&worktree.join(path));
        }
    }
    Ok(stat)
}

/// 新文件太多（通常是忘了忽略的生成物）时，只数前这么多个的行数。
const MAX_COUNTED_FILES: usize = 500;
/// 比这大的新文件不数行数。
const MAX_COUNTED_BYTES: u64 = 1024 * 1024;

/// 文本文件的行数；二进制、太大、读不出来的是 0（和 git 一样，二进制不算行数）。
fn count_lines(path: &Path) -> u32 {
    let small_file = path.symlink_metadata().is_ok_and(|m| m.is_file() && m.len() <= MAX_COUNTED_BYTES);
    let Some(bytes) = small_file.then(|| std::fs::read(path).ok()).flatten() else { return 0 };
    if bytes.contains(&0) {
        return 0;
    }
    let newlines = bytes.iter().filter(|b| **b == b'\n').count();
    let unfinished = bytes.last().is_some_and(|b| *b != b'\n');
    u32::try_from(newlines + usize::from(unfinished)).unwrap_or(u32::MAX)
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
    fn a_file_being_written_means_still_changing() {
        let busy = |detail: &str| -> anyhow::Error {
            UserError::GitFailed { command: "add -A".into(), detail: detail.into() }.into()
        };
        assert!(is_being_written(&busy("error: short read while indexing busy.txt\nfatal: updating files failed")));
        assert!(!is_being_written(&busy("error: open(\"a\"): Permission denied\nerror: unable to index file 'a'")));
        assert!(!is_being_written(&anyhow::anyhow!("short read")));
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
        assert_eq!(branches(&root).unwrap()[0], "main");
    }

    /// 一个仓库和它的 worktree（在 `feat` 分支上）。
    fn repo_with_worktree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        init_repo(&repo).unwrap();
        let wt = dir.path().join("wt");
        add_worktree(&repo, &wt, "feat", "HEAD").unwrap();
        (dir, repo, wt)
    }

    #[test]
    fn snapshot_has_every_change_and_leaves_the_worktree_alone() {
        let (dir, repo, wt) = repo_with_worktree();
        git(&wt, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "step one"])
            .unwrap();
        std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
        std::fs::write(wt.join("staged.txt"), "staged\n").unwrap();
        git(&wt, &["add", "staged.txt"]).unwrap();
        std::fs::write(wt.join("新文件.txt"), "new\n").unwrap();
        std::fs::write(wt.join("debug.log"), "noise\n").unwrap();
        // 检出时的钩子不跑。
        let hook = repo.join(".git/hooks/post-checkout");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\ntouch hook-ran\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let status = || git(&wt, &["status", "--porcelain"]).unwrap();
        let base = git(&repo, &["rev-parse", "HEAD"]).unwrap();
        let before = (status(), git(&wt, &["rev-parse", "HEAD"]).unwrap());

        let copy = dir.path().join("snap/1-review");
        let commit = snapshot(&wt, &copy).unwrap();
        // Windows 上 git 检出时可能把换行换成 CRLF（core.autocrlf）。
        let read = |path: &Path| std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        assert_eq!(read(&copy.join("a.txt")), "changed\n");
        assert_eq!(read(&copy.join("新文件.txt")), "new\n");
        assert!(copy.join("staged.txt").exists() && !copy.join("debug.log").exists());
        assert!(!copy.join("hook-ran").exists());
        let diff = git(&copy, &["diff", "--name-only", &before.1]).unwrap();
        assert_eq!(diff.lines().count(), 3, "{diff}");
        assert_eq!((status(), git(&wt, &["rev-parse", "HEAD"]).unwrap()), before, "原来的没动");

        // 副本自己的分支、没有远程：在里面删分支、推送碰不到原仓库。
        assert!(copy.join(".git").is_dir());
        assert_eq!(git(&copy, &["remote"]).unwrap(), "");
        git(&copy, &["branch", "-f", "feat", "HEAD"]).unwrap();
        git(&copy, &["branch", "-D", "feat"]).unwrap();
        assert_eq!(git(&repo, &["rev-parse", "feat"]).unwrap(), before.1);

        let report = changes_report(&wt, &base, &commit).unwrap();
        assert!(report.contains("step one") && !report.contains("Duolo snapshot"), "{report}");
        assert!(report.contains("新文件.txt") && report.contains("+changed"), "{report}");

        // 审阅时被写了东西也照样删掉。
        std::fs::write(copy.join("junk.txt"), "x\n").unwrap();
        remove_snapshot(&copy).unwrap();
        assert!(!copy.exists());
        assert!(!git(&repo, &["worktree", "list"]).unwrap().contains("1-review"));
        assert_eq!(read(&wt.join("a.txt")), "changed\n");
        remove_snapshot(&copy).unwrap();
    }

    #[test]
    fn saves_uncommitted_work_before_removal() {
        let (_dir, repo, wt) = repo_with_worktree();
        assert!(is_worktree(&wt));
        assert!(!is_worktree(&repo), "主仓库不是 worktree");
        assert_eq!(save_worktree(&wt, "HEAD", "saved", "save").unwrap(), None, "没有改动就不提交");

        std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
        std::fs::create_dir(wt.join("新目录")).unwrap();
        std::fs::write(wt.join("新目录/b.txt"), "new\n").unwrap();
        std::fs::write(wt.join("debug.log"), "noise\n").unwrap();
        // 提交钩子失败也不能拦住保存。
        let hook = repo.join(".git/hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        assert_eq!(save_worktree(&wt, "HEAD", "saved", "save").unwrap().as_deref(), Some("feat"));
        remove_worktree(&repo, &wt, true).unwrap();
        assert_eq!(git(&repo, &["show", "feat:a.txt"]).unwrap(), "changed");
        assert_eq!(git(&repo, &["show", "feat:新目录/b.txt"]).unwrap(), "new");
        assert!(git(&repo, &["show", "feat:debug.log"]).is_err(), "被忽略的文件不提交");
    }

    #[test]
    fn saves_commits_that_are_on_no_branch() {
        let (_dir, repo, wt) = repo_with_worktree();
        git(&wt, &["checkout", "-q", "--detach"]).unwrap();
        assert_eq!(save_worktree(&wt, "HEAD", "saved", "save").unwrap(), None, "这个提交已经在分支上");

        std::fs::write(wt.join("a.txt"), "detached\n").unwrap();
        git(&repo, &["branch", "saved"]).unwrap();
        assert_eq!(save_worktree(&wt, "HEAD", "saved", "save").unwrap().as_deref(), Some("saved-2"));
        assert_eq!(git(&repo, &["show", "saved-2:a.txt"]).unwrap(), "detached");
    }

    #[test]
    fn refuses_to_drop_nested_repositories() {
        let (_dir, _repo, wt) = repo_with_worktree();
        let nested = wt.join("vendor/lib");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("x.txt"), "x\n").unwrap();
        init_repo(&nested).unwrap();
        let err = save_worktree(&wt, "HEAD", "saved", "save").unwrap_err();
        match err.downcast_ref() {
            Some(UserError::NestedRepo { path }) => assert!(Path::new(path) == nested, "{path}"),
            _ => panic!("{err:#}"),
        }
        assert!(nested.join("x.txt").exists());
    }

    /// worktree 里的另一个仓库（有一次提交），返回它的路径。
    fn nested(wt: &Path) -> PathBuf {
        let dir = wt.join("lib");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("x.txt"), "x\n").unwrap();
        init_repo(&dir).unwrap();
        dir
    }

    fn is_nested_error(result: Result<Option<String>>) -> bool {
        matches!(result.unwrap_err().downcast_ref(), Some(UserError::NestedRepo { .. }))
    }

    #[test]
    fn refuses_nested_repository_that_is_already_committed() {
        let (_dir, _repo, wt) = repo_with_worktree();
        let lib = nested(&wt);
        commit_all(&wt, "embed").unwrap();
        assert!(is_nested_error(save_worktree(&wt, "HEAD~1", "saved", "save")), "只记了引用，内容没保存");

        // 提交已经在别处（远程分支）时可以放心删，但暂存不行。
        git(&lib, &["update-ref", "refs/remotes/origin/main", "HEAD"]).unwrap();
        assert_eq!(save_worktree(&wt, "HEAD~1", "saved", "save").unwrap(), None);
        std::fs::write(lib.join("x.txt"), "wip\n").unwrap();
        git(&lib, &["-c", "user.name=t", "-c", "user.email=t@t", "stash", "-q"]).unwrap();
        assert!(is_nested_error(save_worktree(&wt, "HEAD~1", "saved", "save")));
    }

    #[test]
    fn saves_changes_hidden_from_git_status() {
        let (_dir, repo, wt) = repo_with_worktree();
        std::fs::write(wt.join("a.txt"), "assumed\n").unwrap();
        git(&wt, &["update-index", "--assume-unchanged", "a.txt"]).unwrap();
        std::fs::write(wt.join(".gitignore"), "skipped\n").unwrap();
        git(&wt, &["update-index", "--skip-worktree", ".gitignore"]).unwrap();
        assert_eq!(save_worktree(&wt, "HEAD", "saved", "save").unwrap().as_deref(), Some("feat"));
        assert_eq!(git(&repo, &["show", "feat:a.txt"]).unwrap(), "assumed");
        assert_eq!(git(&repo, &["show", "feat:.gitignore"]).unwrap(), "skipped");
    }

    #[test]
    fn keeps_autostash_of_an_unfinished_rebase() {
        let (_dir, repo, wt) = repo_with_worktree();
        let identity = ["-c", "user.name=t", "-c", "user.email=t@t"];
        let run = |dir: &Path, args: &[&str]| git(dir, &[&identity, args].concat());
        let main = current_branch(&repo).unwrap();
        for (dir, text) in [(&repo, "theirs\n"), (&wt, "ours\n")] {
            std::fs::write(dir.join("a.txt"), text).unwrap();
            run(dir, &["commit", "-qam", "change"]).unwrap();
        }
        std::fs::write(wt.join(".gitignore"), "precious\n").unwrap();
        run(&wt, &["-c", "rebase.autoStash=true", "rebase", &main]).unwrap_err();
        for _ in 0..2 {
            save_worktree(&wt, "HEAD", "saved", "save").unwrap();
        }
        assert_eq!(git(&repo, &["stash", "list"]).unwrap().lines().count(), 1, "重试时不重复暂存");
        assert_eq!(git(&repo, &["show", "stash@{0}:.gitignore"]).unwrap(), "precious");
    }

    #[test]
    fn refuses_while_files_keep_changing() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (_dir, _repo, wt) = repo_with_worktree();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let (flag, log) = (stop.clone(), wt.join("busy.txt"));
        let writer = std::thread::spawn(move || {
            let mut n = 0u64;
            while !flag.load(Ordering::Relaxed) {
                n += 1;
                std::fs::write(&log, n.to_string()).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let result = save_worktree(&wt, "HEAD", "saved", "save");
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();
        let err = result.unwrap_err();
        assert!(matches!(err.downcast_ref(), Some(UserError::StillChanging { .. })), "{err:#}");
        assert!(save_worktree(&wt, "HEAD", "saved", "save").unwrap().is_some(), "停下来之后能保存");
    }

    #[test]
    fn saves_late_changes_after_moving() {
        let (_dir, repo, wt) = repo_with_worktree();
        let moved = wt.with_file_name("moved");
        std::fs::rename(&wt, &moved).unwrap();
        std::fs::write(moved.join("late.txt"), "late\n").unwrap();
        assert_eq!(save_moved_worktree(&moved, "saved", "save").unwrap().as_deref(), Some("feat"));
        remove_worktree(&repo, &wt, true).unwrap();
        assert_eq!(git(&repo, &["show", "feat:late.txt"]).unwrap(), "late");
    }

    #[test]
    fn broken_worktree_is_recognized() {
        let (_dir, repo, wt) = repo_with_worktree();
        std::fs::rename(&repo, repo.with_file_name("moved")).unwrap();
        assert!(!is_worktree(&wt));
    }

    #[test]
    fn diff_counts_only_the_tasks_own_changes() {
        let (_dir, repo, wt) = repo_with_worktree();
        let main = current_branch(&repo).unwrap();
        assert!(diff_stat(&wt, &main).unwrap().is_empty());

        // 别人往基准分支上提交了三个文件：和这个任务无关。
        for name in ["x", "y", "z"] {
            std::fs::write(repo.join(name), "1\n").unwrap();
        }
        commit_all(&repo, "others").unwrap();
        assert!(diff_stat(&wt, &main).unwrap().is_empty(), "{:?}", diff_stat(&wt, &main));

        // 已提交、没提交、还没加进 git 的新文件都算；被忽略的、二进制的行数不算。
        std::fs::write(wt.join("committed.txt"), "1\n2\n").unwrap();
        commit_all(&wt, "mine").unwrap();
        std::fs::write(wt.join("a.txt"), "a\nb\n").unwrap();
        std::fs::create_dir(wt.join("new")).unwrap();
        std::fs::write(wt.join("new/文件.txt"), "1\n2\nno newline").unwrap();
        std::fs::write(wt.join("new/bin"), b"\0\n\n\n").unwrap();
        std::fs::write(wt.join("x.log"), "ignored\n").unwrap();
        let stat = diff_stat(&wt, &main).unwrap();
        assert_eq!(stat, DiffStat { files: 4, insertions: 6, deletions: 0 });

        // 合并基准分支之后，别人的改动也不算。
        git(&wt, &["add", "-A"]).unwrap();
        commit_all(&wt, "more").unwrap();
        git(&wt, &["-c", "user.name=t", "-c", "user.email=t@t", "merge", "-q", &main]).unwrap();
        assert_eq!(diff_stat(&wt, &main).unwrap().files, 4);
    }

    #[test]
    fn diff_ignores_changes_merged_from_the_remote() {
        let (dir, repo, wt) = repo_with_worktree();
        let main = current_branch(&repo).unwrap();
        // 远程的 main 比本地的新，任务合并了远程的。
        let remote = dir.path().join("remote");
        clone(repo.to_str().unwrap(), &remote).unwrap();
        std::fs::write(remote.join("theirs"), "1\n").unwrap();
        commit_all(&remote, "theirs").unwrap();
        git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]).unwrap();
        git(&repo, &["fetch", "-q", "origin"]).unwrap();
        git(&repo, &["branch", "-q", "--set-upstream-to", &format!("origin/{main}"), &main]).unwrap();
        git(&wt, &["merge", "-q", &format!("origin/{main}")]).unwrap();
        assert!(diff_stat(&wt, &main).unwrap().is_empty());
    }

    #[test]
    fn branch_names() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path()).unwrap();
        assert!(is_repo_root(dir.path()));
        assert!(is_valid_branch_name(dir.path(), "feat/order-csv"));
        assert!(!is_valid_branch_name(dir.path(), "bad name"));
        assert!(!is_valid_branch_name(dir.path(), "-x"));
        let current = current_branch(dir.path()).unwrap();
        assert!(branch_exists(dir.path(), &current));
        assert!(!branch_exists(dir.path(), "feat/nope"));
    }

    #[test]
    fn init_plain_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "hi\n").unwrap();
        init_repo(dir.path()).unwrap();
        assert!(repo_root(dir.path()).is_ok());
        assert_eq!(git(dir.path(), &["ls-files"]).unwrap(), "notes.md");
    }

    #[test]
    fn finds_repos_two_levels_down() {
        let dir = tempfile::tempdir().unwrap();
        for repo in ["shop", "org/api", "org/deep/too-deep"] {
            std::fs::create_dir_all(dir.path().join(repo).join(".git")).unwrap();
        }
        std::fs::create_dir_all(dir.path().join(".hidden/.git")).unwrap();
        // 别的仓库的 worktree：`.git` 是一个文件。
        std::fs::create_dir_all(dir.path().join("shop-feature")).unwrap();
        std::fs::write(dir.path().join("shop-feature/.git"), "gitdir: ../shop/.git/worktrees/x\n").unwrap();
        // 同一个目录用两个路径传进来（别名），只算一次。
        let alias = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), alias.path().join("same")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(dir.path(), alias.path().join("same")).unwrap();
        let roots = [dir.path().to_path_buf(), alias.path().join("same")];
        let mut found: Vec<_> = find_repos(&roots, 10)
            .into_iter()
            .map(|p| p.strip_prefix(dir.path()).unwrap().display().to_string().replace('\\', "/"))
            .collect();
        found.sort();
        assert_eq!(found, ["org/api", "shop"]);
    }

    #[test]
    fn name_from_url() {
        assert_eq!(repo_name_from_url("git@github.com:a/shop.git").as_deref(), Some("shop"));
        assert_eq!(repo_name_from_url("https://github.com/a/shop/").as_deref(), Some("shop"));
        assert_eq!(repo_name_from_url("").as_deref(), None);
    }
}
