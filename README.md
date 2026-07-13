# SkillMate

SkillMate 是一个跨平台桌面应用，用来盘点、安装、组织和更新本机的目录型 **AI 助手 Skills**。

它关注的是可以落到本地目录的 Skill，而不是整包 IDE 扩展、插件市场或通用包管理器。目标是让不同助手下的 Skills 更容易查看、迁移、备份和复用。

## 当前支持的 AI 助手

- Claude Code
- Codex
- OpenClaw
- Gemini CLI

## 核心能力

### Skill 盘点

- 扫描受支持助手的本地 Skill 目录
- 展示名称、路径、来源、递归目录大小、入口文档预览和更新状态
- 按 Agent Skills 规范识别大小写精确的 `SKILL.md`
- 校验 YAML frontmatter、必填 `name` / `description`、名称格式及目录名一致性
- 读取 `compatibility`、`license`、`metadata`、`allowed-tools` 等可选字段
- 识别可选的 `references/`、`scripts/`、`assets/` 资源目录
- 用 `符合规范` / `需要修复` / `非 Skill` 标记结构状态；小写 `skill.md` 和仅 README 的目录会提示修复
- 检查脚本、依赖清单、软连接、隐藏文件、网络访问和环境变量引用等静态风险

### 安装 Skill

安装入口接受 Git 仓库和本地目录：

- Git 仓库
- 本地目录

Git 来源支持普通仓库地址、GitHub shorthand、GitHub tree URL，以及通过 `#ref:path` 指定分支 / 标签 / 提交和仓库子目录：

- `https://github.com/example/skills.git`
- `example/skills`
- `https://github.com/example/skills.git#main:skills/writer`
- `https://github.com/example/skills/tree/main/skills/writer`

安装前会先生成结构预览、风险提示和写入计划。Git 来源的预览会临时克隆仓库；安装时不会把仓库的 `.git`、`.hg`、`.svn` 目录带入助手目录，但会单独保存来源、引用、子目录和已安装提交，普通仓库与仓库子目录都可以继续检查和更新。

本地目录默认复制到目标助手目录。对于本地目录来源，也可以选择“链接到项目”，把 Skill 软连接到对应助手的项目目录：Codex 使用 `.agents/skills`，Claude Code 使用 `.claude/skills`，Gemini CLI 使用 `.gemini/skills`，OpenClaw 使用 `skills`。项目级安装同样会写入受管注册，后续可以安全预览、导出或解除软连接。

全局安装会写入各助手当前推荐的目录。为兼容既有安装，扫描时还会发现 Codex 的 `~/.codex/skills`，以及 OpenClaw、Gemini CLI 可复用的 `~/.agents/skills`；兼容目录只参与发现，不会改变新安装的目标路径。

安装输入会先经过本地规则识别，不需要模型 API。规则无法判断的自然语言或复杂说明会标记为“可用模型辅助识别”，但当前版本不会自动调用模型。

设置页可以选择安装安全策略：仅提示、阻止关键风险，或只允许可信 Git 主机和本地根目录。策略会参与安装预览、Manifest / Profile 应用和 Git 更新；修改策略后，旧的写入计划会自动失效。静态风险检查和信任列表都只是安装决策辅助，不会执行 Skill 中的脚本。

### 更新视图

更新页会展示每个 Skill 的来源和同步状态，包括：

- `git`
- `legacy_npm`
- `legacy_pip`
- `local`

更新视图会展示来源类型、远端来源、当前版本引用、最新可用引用、落后提交数、同步状态和最近检查时间。当前只有 Git 来源支持一键更新，通常需要状态为可更新且系统判定 `can_sync = true`。

`local` 来源仅做可用性检测与展示，不参与自动更新。`legacy_npm` 和 `legacy_pip` 只作为历史来源 / 外部环境来源探测，不作为安装入口，也不会在 SkillMate 内执行全局 npm/PyPI 升级。

### 应用更新

设置页提供应用更新入口，可以检查 GitHub Releases 上的最新版本，并通过 Tauri updater 校验签名后安装更新。

### 组织与迁移

- 标签：为 Skill 添加标签并筛选
- 场景：保存一组 Skill 路径，查看缺失状态，回填编辑器或复制路径
- 导入 / 导出：导出标签、场景和受管 Skill 清单，导入前可预览变更
- Git 备份：把去重后的全局和受管项目 Skill 内容快照到本地 Git 仓库，并可推送到远端
- SkillMate manifest：导出 / 预览 / 应用 `skillmate.toml`，以 `install` / `keep` / `remove` 计划把受管 Skill 对齐到目标状态
- Skill Set Profile：保存一组当前 Skill 来源组合，支持预览、应用和一次性回滚

项目级受管 Skill 可以导出到项目根目录的 `skillmate.toml`。清单会稳定排序，并记录来源、目标助手、固定引用和内容哈希；项目内路径尽量使用相对路径。重新应用项目清单时，只会对齐同一项目的受管 Skill，不会波及全局或其他项目。

SkillMate 只会自动移除自身记录的受管 Skill，不会删除手工放入助手目录的内容。安装、Manifest 和 Profile 应用失败时会尽量恢复文件、受管状态和数据库记录，并明确报告未能完成的回滚步骤。

Git 备份用于保存 Skill 内容，不是完整应用恢复。快照不会包含 SkillMate 数据库、标签、场景、Profile、受管 sidecar、凭据、运行时缓存或软连接；每次同步都会在仓库中写入清单，记录来源根、复制统计和排除原因。

## 当前边界

为了保持语义清晰，当前版本暂不做：

- VSCode / Cursor / Windsurf / Zed 整包扩展管理
- npm / PyPI 安装入口
- 市场搜索

这些能力后续是否加入，取决于目录型 Skill 管理闭环是否足够稳定。

## 项目信息

- [GNU AGPL v3 或更高版本](LICENSE)
- [安全策略](SECURITY.md)
- [参与贡献](CONTRIBUTING.md)
