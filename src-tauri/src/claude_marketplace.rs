use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn resolve_plugin_source(input: &str) -> Result<(String, String), String> {
    let root = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/plugins/marketplaces");
    resolve_plugin_source_in(&root, input)
}

fn resolve_plugin_source_in(root: &Path, input: &str) -> Result<(String, String), String> {
    let (plugin, requested_marketplace) = split_plugin_id(input)?;
    let mut matches = Vec::new();
    let marketplaces =
        fs::read_dir(root).map_err(|error| format!("无法读取 Claude Marketplace：{error}"))?;
    for entry in marketplaces.flatten() {
        let manifest = entry.path().join(".claude-plugin/marketplace.json");
        let Ok(bytes) = fs::read(&manifest) else {
            continue;
        };
        let Ok(document) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let directory_name = entry.file_name().to_string_lossy().to_string();
        let marketplace_name = document
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&directory_name);
        if requested_marketplace
            .is_some_and(|requested| requested != marketplace_name && requested != directory_name)
        {
            continue;
        }
        let Some(items) = document.get("plugins").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if item.get("name").and_then(Value::as_str) == Some(plugin) {
                matches.push(resolve_source(&entry.path(), item.get("source"))?);
            }
        }
    }
    match matches.len() {
        0 => Err(format!("Claude Marketplace 中未找到插件：{input}")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "多个 Marketplace 包含 {plugin}，请使用 {plugin}@marketplace 指定来源"
        )),
    }
}

fn split_plugin_id(input: &str) -> Result<(&str, Option<&str>), String> {
    let value = input.trim();
    if value.is_empty() {
        return Err("请输入 Claude Marketplace 插件名称".to_string());
    }
    let (plugin, marketplace) = value
        .rsplit_once('@')
        .map(|(plugin, marketplace)| (plugin, Some(marketplace)))
        .unwrap_or((value, None));
    if plugin.is_empty() || marketplace.is_some_and(str::is_empty) {
        return Err("Claude 插件名称格式无效".to_string());
    }
    Ok((plugin, marketplace))
}

fn resolve_source(
    marketplace_root: &Path,
    source: Option<&Value>,
) -> Result<(String, String), String> {
    let source = source.ok_or_else(|| "Claude 插件缺少来源信息".to_string())?;
    if let Some(relative) = source.as_str() {
        return resolve_local_source(marketplace_root, relative);
    }
    let source_kind = source
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| "Claude 插件来源类型无效".to_string())?;
    if source_kind == "npm" {
        return Err("该 Claude 插件来自 npm；SkillMate 不执行第三方包管理器，请改用其 Git 仓库或本地 Skill 目录".to_string());
    }
    let repository = match source_kind {
        "github" => source.get("repo").and_then(Value::as_str),
        "url" | "git-subdir" => source.get("url").and_then(Value::as_str),
        _ => {
            return Err(format!(
                "SkillMate 暂不支持 Claude 插件来源类型：{source_kind}"
            ))
        }
    }
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "Claude 插件来源缺少仓库地址".to_string())?;
    let reference = source
        .get("sha")
        .and_then(Value::as_str)
        .or_else(|| source.get("ref").and_then(Value::as_str))
        .unwrap_or_default();
    let subdir = if source_kind == "git-subdir" {
        source
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
    } else {
        ""
    };
    Ok((
        git_locator(repository, reference, subdir),
        "git".to_string(),
    ))
}

fn resolve_local_source(
    marketplace_root: &Path,
    relative: &str,
) -> Result<(String, String), String> {
    let relative_path = Path::new(relative);
    if !relative.starts_with("./")
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("Claude Marketplace 本地来源必须是其目录内的相对路径".to_string());
    }
    let root = marketplace_root
        .canonicalize()
        .map_err(|error| format!("无法读取 Claude Marketplace 目录：{error}"))?;
    let path = marketplace_root
        .join(relative_path)
        .canonicalize()
        .map_err(|error| format!("无法读取 Claude 插件目录：{error}"))?;
    if !path.starts_with(&root) {
        return Err("Claude Marketplace 本地来源越过了 Marketplace 目录".to_string());
    }
    Ok((path.to_string_lossy().to_string(), "local".to_string()))
}

fn git_locator(repository: &str, reference: &str, subdir: &str) -> String {
    match (reference.is_empty(), subdir.is_empty()) {
        (true, true) => repository.to_string(),
        (false, true) => format!("{repository}#{reference}"),
        (true, false) => format!("{repository}#:{subdir}"),
        (false, false) => format!("{repository}#{reference}:{subdir}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    #[test]
    fn plugin_identifier_accepts_optional_marketplace() {
        assert_eq!(split_plugin_id("writer").unwrap(), ("writer", None));
        assert_eq!(
            split_plugin_id("writer@official").unwrap(),
            ("writer", Some("official"))
        );
    }

    #[test]
    fn resolves_supported_git_source_objects() {
        assert_eq!(
            resolve_source(
                Path::new("."),
                Some(&json!({ "source": "github", "repo": "example/skills", "ref": "v1" })),
            )
            .unwrap(),
            ("example/skills#v1".to_string(), "git".to_string())
        );
        assert_eq!(
            resolve_source(
                Path::new("."),
                Some(&json!({
                    "source": "git-subdir",
                    "url": "https://example.com/skills.git",
                    "path": "plugins/writer",
                    "sha": "0123456789012345678901234567890123456789"
                })),
            )
            .unwrap(),
            (
                "https://example.com/skills.git#0123456789012345678901234567890123456789:plugins/writer".to_string(),
                "git".to_string()
            )
        );
    }

    #[test]
    fn npm_source_is_explicitly_rejected() {
        let error = resolve_source(
            Path::new("."),
            Some(&json!({ "source": "npm", "package": "example" })),
        )
        .unwrap_err();
        assert!(error.contains("不执行第三方包管理器"));
    }

    #[test]
    fn local_source_stays_inside_marketplace() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-claude-marketplace-{}",
            crate::app_core::generate_id()
        ));
        let plugin = root.join("plugins/writer");
        fs::create_dir_all(&plugin).unwrap();

        let resolved = resolve_source(&root, Some(&json!("./plugins/writer"))).unwrap();
        assert_eq!(resolved.1, "local");
        assert_eq!(PathBuf::from(resolved.0), plugin.canonicalize().unwrap());
        assert!(resolve_source(&root, Some(&json!("../writer"))).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_source_can_be_the_marketplace_root() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-claude-marketplace-root-{}",
            crate::app_core::generate_id()
        ));
        fs::create_dir_all(&root).unwrap();

        let resolved = resolve_source(&root, Some(&json!("./"))).unwrap();

        assert_eq!(PathBuf::from(resolved.0), root.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_plugin_by_manifest_marketplace_name() {
        let root = std::env::temp_dir().join(format!(
            "skillmate-claude-marketplace-{}",
            crate::app_core::generate_id()
        ));
        let marketplace = root.join("checkout-name");
        let plugin = marketplace.join("plugins/writer");
        fs::create_dir_all(marketplace.join(".claude-plugin")).unwrap();
        fs::create_dir_all(&plugin).unwrap();
        fs::write(
            marketplace.join(".claude-plugin/marketplace.json"),
            serde_json::to_vec(&json!({
                "name": "official",
                "plugins": [{ "name": "writer", "source": "./plugins/writer" }]
            }))
            .unwrap(),
        )
        .unwrap();

        let resolved = resolve_plugin_source_in(&root, "writer@official").unwrap();

        assert_eq!(resolved.1, "local");
        assert_eq!(PathBuf::from(resolved.0), plugin.canonicalize().unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
