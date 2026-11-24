//! 提前信任任务的目录：Agent 第一次在一个文件夹里启动时会先问"信任这个文件夹吗"，
//! 用户添加项目就是信任它，所以 Duolo 在启动前替用户记下，Agent 就不会停在这个对话框上。
//!
//! 只改 Agent 自己配置文件里的这一项，别的内容原样保留；文件不在、读不懂时什么也不做（到时候照样会问）。
//! 每种 Agent 的写法实现 [`Store`]，插件的声明按名字引用。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// 一种记录信任的写法。`dir` 是解析过链接的真实路径。
pub(crate) trait Store: std::fmt::Debug + Send + Sync {
    /// 记下信任 `dir`。已经有关于它的记录（信任或不信任）时不改：那是用户自己的决定。
    fn grant(&self, file: &Path, dir: &Path) -> Result<()>;
    /// 删掉 Duolo 为 `dir` 记下的信任（任务关掉以后）。
    fn revoke(&self, file: &Path, dir: &Path) -> Result<()>;
}

/// 按名字找写法。
pub(crate) fn store(name: &str) -> Option<&'static dyn Store> {
    match name {
        "claude" => Some(&ClaudeJson),
        "codex" => Some(&CodexToml),
        "antigravity" => Some(&AntigravityJson),
        _ => None,
    }
}

/// 解析过链接的真实路径；目录已经删了时，用它上一级的真实路径拼上名字。Agent 记的是真实路径。
pub(crate) fn real_path(dir: &Path) -> PathBuf {
    if let Ok(real) = dir.canonicalize() {
        return real;
    }
    match (dir.parent().and_then(|p| p.canonicalize().ok()), dir.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => dir.to_path_buf(),
    }
}

/// Claude Code：JSON 文件里 `projects."<路径>".hasTrustDialogAccepted`。
#[derive(Debug)]
struct ClaudeJson;

impl Store for ClaudeJson {
    fn grant(&self, file: &Path, dir: &Path) -> Result<()> {
        let Some(mut config) = read_json(file)? else { return Ok(()) };
        let key = dir.display().to_string();
        let Some(projects) =
            config.as_object_mut().map(|c| c.entry("projects").or_insert_with(|| Value::Object(Default::default())))
        else {
            return Ok(());
        };
        let Some(projects) = projects.as_object_mut() else { return Ok(()) };
        let entry = projects.entry(key).or_insert_with(|| Value::Object(Default::default()));
        let Some(entry) = entry.as_object_mut() else { return Ok(()) };
        if entry.contains_key("hasTrustDialogAccepted") {
            return Ok(());
        }
        entry.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
        replace(file, &serde_json::to_string_pretty(&config)?)
    }

    fn revoke(&self, file: &Path, dir: &Path) -> Result<()> {
        let Some(mut config) = read_json(file)? else { return Ok(()) };
        let key = dir.display().to_string();
        let removed = config
            .get_mut("projects")
            .and_then(Value::as_object_mut)
            .and_then(|projects| projects.shift_remove(&key))
            .is_some();
        if removed {
            replace(file, &serde_json::to_string_pretty(&config)?)?;
        }
        Ok(())
    }
}

/// Antigravity：JSON 文件里的 `trustedWorkspaces` 数组，按确切路径记（上一级的不算数，参考 Orca 的做法）。
#[derive(Debug)]
struct AntigravityJson;

const TRUSTED_WORKSPACES: &str = "trustedWorkspaces";

impl Store for AntigravityJson {
    fn grant(&self, file: &Path, dir: &Path) -> Result<()> {
        let Some(mut config) = read_json(file)? else { return Ok(()) };
        let key = dir.display().to_string();
        let Some(list) = config
            .as_object_mut()
            .map(|c| c.entry(TRUSTED_WORKSPACES).or_insert_with(|| Value::Array(Vec::new())))
            .and_then(Value::as_array_mut)
        else {
            return Ok(());
        };
        if list.iter().any(|e| e.as_str() == Some(key.as_str())) {
            return Ok(());
        }
        list.push(Value::String(key));
        replace(file, &serde_json::to_string_pretty(&config)?)
    }

    fn revoke(&self, file: &Path, dir: &Path) -> Result<()> {
        let Some(mut config) = read_json(file)? else { return Ok(()) };
        let key = dir.display().to_string();
        let Some(list) = config.get_mut(TRUSTED_WORKSPACES).and_then(Value::as_array_mut) else { return Ok(()) };
        let before = list.len();
        list.retain(|e| e.as_str() != Some(key.as_str()));
        if list.len() != before {
            replace(file, &serde_json::to_string_pretty(&config)?)?;
        }
        Ok(())
    }
}

/// 文件不在时为 None；读不懂时也是 None（不动用户的文件）。
fn read_json(file: &Path) -> Result<Option<Value>> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(serde_json::from_str::<Value>(&text).ok().filter(Value::is_object)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", file.display())),
    }
}

/// Codex：TOML 文件里 `[projects."<路径>"] trust_level = "trusted"`。只在文件末尾追加，别的内容一个字不改。
#[derive(Debug)]
struct CodexToml;

impl Store for CodexToml {
    fn grant(&self, file: &Path, dir: &Path) -> Result<()> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err).with_context(|| format!("cannot read {}", file.display())),
        };
        let Ok(config) = text.parse::<toml::Table>() else { return Ok(()) };
        let key = dir.display().to_string();
        let known = config.get("projects").and_then(toml::Value::as_table).is_some_and(|p| p.contains_key(&key));
        if known {
            return Ok(());
        }
        let quoted = toml::Value::String(key).to_string();
        let mut updated = text.trim_end_matches('\n').to_string();
        if !updated.is_empty() {
            updated.push_str("\n\n");
        }
        updated.push_str(&format!("[projects.{quoted}]\ntrust_level = \"trusted\"\n"));
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        replace(file, &updated)
    }

    /// 信任的是主仓库（用户的项目），不删。
    fn revoke(&self, _file: &Path, _dir: &Path) -> Result<()> {
        Ok(())
    }
}

/// 先写到旁边的临时文件再换过去，写到一半出错也不会留下半个文件；保留原来的权限（里面可能有登录信息）。
fn replace(file: &Path, content: &str) -> Result<()> {
    let dir = file.parent().context("config file has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(content.as_bytes())?;
    if let Ok(meta) = fs::metadata(file) {
        fs::set_permissions(temp.path(), meta.permissions())?;
    }
    temp.persist(file).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_trust_is_added_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        let before = r#"{"numStartups": 3, "projects": {"/a": {"hasTrustDialogAccepted": false}}, "z": 1}"#;
        fs::write(&file, before).unwrap();
        let wt = Path::new("/w/shop/1-x");
        ClaudeJson.grant(&file, wt).unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(config["projects"]["/w/shop/1-x"]["hasTrustDialogAccepted"], true);
        assert_eq!((config["numStartups"].as_u64(), config["z"].as_u64()), (Some(3), Some(1)), "别的不动");
        let keys: Vec<&String> = config.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["numStartups", "projects", "z"], "顺序也不变");
        // 用户说过不信任的，不替他改。
        ClaudeJson.grant(&file, Path::new("/a")).unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(config["projects"]["/a"]["hasTrustDialogAccepted"], false);

        ClaudeJson.revoke(&file, wt).unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert!(config["projects"].get("/w/shop/1-x").is_none());
        // 没装过、文件不在：什么也不做。
        ClaudeJson.grant(&dir.path().join("none.json"), wt).unwrap();
        assert!(!dir.path().join("none.json").exists());
    }

    #[test]
    fn antigravity_trust_is_added_once_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        fs::write(&file, r#"{"trustedWorkspaces": ["/home/me/app"], "theme": "dark"}"#).unwrap();
        let wt = Path::new("/w/shop/1-x");
        AntigravityJson.grant(&file, wt).unwrap();
        AntigravityJson.grant(&file, wt).unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(config["trustedWorkspaces"], serde_json::json!(["/home/me/app", "/w/shop/1-x"]));
        assert_eq!(config["theme"], "dark", "别的不动");
        AntigravityJson.revoke(&file, wt).unwrap();
        let config: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(config["trustedWorkspaces"], serde_json::json!(["/home/me/app"]), "用户自己的留着");
        // 没装过、文件不在：什么也不做。
        AntigravityJson.grant(&dir.path().join("none.json"), wt).unwrap();
        assert!(!dir.path().join("none.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_file_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude.json");
        fs::write(&file, "{}").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        ClaudeJson.grant(&file, Path::new("/w")).unwrap();
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn codex_trust_is_appended_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        let before = "model = \"x\" # mine\n\n[projects.\"/a\"]\ntrust_level = \"untrusted\"\n";
        fs::write(&file, before).unwrap();
        let repo = Path::new("/w/shop \"q\"");
        CodexToml.grant(&file, repo).unwrap();
        CodexToml.grant(&file, repo).unwrap();
        CodexToml.grant(&file, Path::new("/a")).unwrap();
        let after = fs::read_to_string(&file).unwrap();
        assert!(after.starts_with(before.trim_end()), "原来的内容一字不改：{after}");
        let config: toml::Table = after.parse().unwrap();
        assert_eq!(config["projects"]["/w/shop \"q\""]["trust_level"].as_str(), Some("trusted"));
        assert_eq!(config["projects"]["/a"]["trust_level"].as_str(), Some("untrusted"), "用户的决定不改");
        assert_eq!(after.matches("[projects.").count(), 2, "只加一次");
        // 还没有配置文件：建一个。
        let fresh = dir.path().join("sub/config.toml");
        CodexToml.grant(&fresh, repo).unwrap();
        assert!(fs::read_to_string(&fresh).unwrap().contains("trust_level = \"trusted\""));
    }
}
