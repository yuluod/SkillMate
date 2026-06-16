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
- 展示名称、路径、来源、入口文档预览和更新状态
- 识别 `SKILL.md` / `skill.md`、README 和 `references/`、`scripts/`、`assets/` 资源目录
- 用 `完整` / `部分` / `非标准` 标记 Skill 结构状态
- 从 `SKILL.md` frontmatter 中轻量读取标题、说明、标签和兼容信息

### 安装 Skill

安装入口接受 Git 仓库和本地目录：

- Git 仓库
- 本地目录

Git 来源支持普通仓库地址、GitHub shorthand、GitHub tree URL，以及通过 `#ref:path` 指定分支 / 标签 / 提交和仓库子目录：

- `https://github.com/example/skills.git`
- `example/skills`
- `https://github.com/example/skills.git#main:skills/writer`
- `https://github.com/example/skills/tree/main/skills/writer`

安装前会先生成结构预览和写入计划。Git 来源的预览会临时克隆仓库；安装仓库子目录时，SkillMate 只复制目标子目录，并保留来源信息，后续可通过重新拉取该子目录更新。

本地目录默认复制到目标助手目录。对于本地目录来源，也可以选择“链接到项目”，把 Skill 软连接到具体项目下的助手目录，例如 `.codex/skills` 或 `.claude/skills`。

安装输入会先经过本地规则识别，不需要模型 API。规则无法判断的自然语言或复杂说明会标记为“可用模型辅助识别”，但当前版本不会自动调用模型。

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

当前发布包覆盖：

- macOS Apple Silicon
- Windows x64
- Linux x64

Intel Mac 暂不作为 v0.x 发布目标。

### 组织与迁移

- 标签：为 Skill 添加标签并筛选
- 场景：保存一组 Skill 路径，查看缺失状态，回填编辑器或复制路径
- 导入 / 导出：导出标签、场景和受管 Skill 清单，导入前可预览变更
- Git 备份：把受支持助手目录快照到本地 Git 仓库，并可推送到远端
- SkillMate manifest：导出 / 预览 / 应用 `skillmate.toml`
- Skill Set Profile：保存一组当前 Skill 来源组合，支持预览、应用和回滚

## 当前边界

为了保持语义清晰，当前版本暂不做：

- VSCode / Cursor / Windsurf / Zed 整包扩展管理
- npm / PyPI 安装入口
- 市场搜索

这些能力后续是否加入，取决于目录型 Skill 管理闭环是否足够稳定。
