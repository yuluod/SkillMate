use serde::{Deserialize, Serialize};

/// 受管 Skill 的领域描述:归属助手、来源、目标与固定引用。
///
/// 该类型是 manifest、profile 与受管注册表共用的货币类型。
/// 字段名即 TOML/JSON 持久化格式,变更需保持向后兼容。
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct SkillDescriptor {
    pub assistant: String,
    pub source: String,
    pub source_kind: String,
    pub target_name: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub install_mode: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub subdir: Option<String>,
    #[serde(default)]
    pub resolved_ref: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
}
