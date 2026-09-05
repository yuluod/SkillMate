<p align="center">
  <img src="src-tauri/icons/icon.png" alt="SkillMate" width="128" />
</p>

<p align="center">
  <a href="https://github.com/yuluod/SkillMate/actions/workflows/ci.yml"><img src="https://github.com/yuluod/SkillMate/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI" /></a>
  <a href="https://github.com/yuluod/SkillMate/actions/workflows/release.yml"><img src="https://github.com/yuluod/SkillMate/actions/workflows/release.yml/badge.svg" alt="Release" /></a>
  <a href="https://github.com/yuluod/SkillMate/releases/latest"><img src="https://img.shields.io/github/v/release/yuluod/SkillMate?sort=semver" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/yuluod/SkillMate" alt="License" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-59636e" alt="macOS, Windows and Linux" />
</p>

<p align="center">
  简体中文 · <a href="README.en.md">English</a>
</p>

# SkillMate

SkillMate 是一个跨平台的 **AI Skills 管理器**，用于统一盘点、添加、接管、启用和维护散落在不同 AI 编程工具中的目录型 Skills。

它不试图替代每个 Agent 自己的插件系统，而是解决更基础的问题：同一个 Skill 从哪里来、由谁管理、在哪些平台和项目中生效，以及如何避免多份复制带来的内容漂移。

## 为什么需要 SkillMate

Claude Code、Codex、Gemini CLI、Cursor 等工具使用不同的全局目录和项目目录。手工复制同一个 Skill 很容易产生以下问题：

- 不知道当前项目实际加载了哪一份 Skill
- 同名 Skill 在项目级和全局范围相互覆盖
- 同一份能力散落在多个目录，内容逐渐不一致
- Git、外部 CLI 和手工目录的更新责任混在一起
- 更换电脑或项目时难以复现原有组合

SkillMate 用一份受管主副本和显式启用关系整理这些内容，同时保留对外部安装内容的可见性。

## 核心模型

SkillMate 将 Skill 的生命周期拆成几个含义明确的动作：

- **添加**：从 Git、本地目录或 Claude Marketplace 解析内容，复制到 SkillMate 统一库，不选择平台或项目
- **接管**：把现有 Agent 目录中的实体 Skill 加入统一库，再将原位置替换为受管目录连接
- **启用**：从统一库向某个平台的全局目录或项目目录创建目录连接
- **停用**：移除启用位置，统一库中的主副本保持不变
- **删除**：删除 SkillMate 管理的主副本；普通删除先进入自有垃圾箱并提供 60 秒撤销

```text
Git / 本地目录 / Claude Marketplace
                 │
                 │ 添加
                 ▼
       SkillMate 统一库
          （唯一主副本）
                 │
          ┌──────┴──────┐
          │             │
       全局启用       项目启用
          │             │
          ▼             ▼
   Agent 全局目录   项目 Skill 目录
          └────── 目录连接 ──────┘

现有实体 Skill ── 接管 ──► 统一库 + 原位置受管连接
```

统一库负责内容、来源和更新状态，Agent 目录只表示“在哪里启用”。SkillMate 不会自动接管或覆盖外部内容，接管必须由用户发起并确认写入计划。

## 支持的平台与目录

| 平台 | 默认全局启用目录 | 其他发现目录 | 项目目录 |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/skills` | — | `.claude/skills` |
| Codex | `~/.agents/skills` | `~/.codex/skills` | `.agents/skills` |
| OpenClaw | `~/.openclaw/skills` | `~/.agents/skills` | `skills` |
| Gemini CLI | `~/.gemini/skills` | `~/.agents/skills` | `.gemini/skills` |
| Cursor | `~/.cursor/skills` | — | `.cursor/skills` |
| OpenCode | `~/.config/opencode/skills` | — | `.opencode/skills` |
| GitHub Copilot | `~/.copilot/skills` | — | `.github/skills` |

SkillMate 会同时扫描平台声明的发现目录。项目核验功能会把项目级与全局内容合并，并按项目优先原则展示实际生效的 Skills 和被同名内容覆盖的数量。

Windows 创建受管目录连接需要开启开发者模式。

## 来源、管理者与更新责任

“内容来源”“当前管理者”和“更新方式”是三个不同概念，SkillMate 会分别展示：

| 场景 | 内容来源 | 管理者 | 更新方式 |
| --- | --- | --- | --- |
| SkillMate 添加的 Git Skill | GitHub / Git | SkillMate | SkillMate 检查并更新 |
| SkillMate 添加的本地 Skill | 本地目录 | SkillMate | 手工维护 |
| npm / pip 等外部工具安装的 Skill | 外部 CLI | 原安装器 | 使用原安装器更新 |
| Agent、插件或用户手工放置的 Skill | Git / 本地 / 未知 | 外部或手工管理 | 外部工具或手工维护 |
| 用户显式接管后的 Skill | 保留原来源 | SkillMate | 按可重建来源决定 |

SkillMate 不会因为“发现了一个目录”就宣称拥有它。只有加入统一库并完成登记的内容，才会被视为 SkillMate 受管内容。

## 主要功能

### 盘点与项目核验

- 扫描所有受支持平台的全局 Skill 目录
- 输入项目路径，计算每个平台在该项目中实际可见的 Skills
- 区分项目级、全局、同名覆盖和共享发现目录
- 展示内容来源、管理者、更新方式和启用平台
- 汇总结构问题、静态风险、本地修改、内容差异和扫描诊断

### 结构与安全检查

- 按 Agent Skills 规范识别大小写精确的 `SKILL.md`
- 校验 YAML frontmatter、必填 `name` / `description`、名称格式和目录名一致性
- 读取 `compatibility`、`license`、`metadata`、`allowed-tools` 等可选字段
- 识别 `references/`、`scripts/`、`assets/` 等资源目录
- 检查脚本、依赖清单、软连接、隐藏文件、网络访问和环境变量引用
- 使用 `符合规范`、`需要修复`、`非 Skill` 区分结构状态

静态检查只用于预览和安装决策。SkillMate 不执行 Skill 中的脚本，也不会为了识别来源而运行第三方安装命令。

### 添加来源

#### Git 仓库

支持普通仓库地址、GitHub shorthand、GitHub tree URL，以及 `#ref:path` 子目录语法：

```text
https://github.com/example/skills.git
example/skills
https://github.com/example/skills.git#main:skills/writer
https://github.com/example/skills/tree/main/skills/writer
```

Git 预览会临时克隆仓库。添加后不会保留 `.git`、`.hg`、`.svn`，但会单独记录仓库、引用、子目录和已安装提交，用于后续检查与更新。

如果仓库中包含多个 Skills，必须显式选择需要添加的目录。

#### 本地目录

本地单 Skill 目录可以直接添加。包含多个 Skills 的目录同样需要显式选择，SkillMate 只复制选中的内容。

#### Claude Marketplace

高级来源中可以使用：

```text
plugin
plugin@marketplace
```

SkillMate 会读取本机 Claude Marketplace 清单，将插件解析为本地目录或 Git 来源，再进入同一套结构预览、风险检查和写入计划。来自 npm 的 Marketplace 插件会明确提示使用原安装器，SkillMate 不会执行 npm 安装。

技能库中的 [skills.sh](https://skills.sh/) 和 GitHub Repository Search 只负责发现来源，真正添加时仍会经过完整检查。

### 接管外部 Skill

扫描到的外部实体目录默认只读。用户可以选择“接管”，流程为：

1. 检查目录所属平台和全局 / 项目范围
2. 生成来源、结构、安全策略和文件动作预览
3. 将内容复制到统一库
4. 将原实体目录替换为指向主副本的受管连接
5. 迁移来源与更新状态

Git 仓库中的嵌套 Skill 会保留相对仓库根目录的子路径，避免后续更新错误地指向整个仓库。任一步失败都会尝试恢复原目录和相关登记信息。

### 统一库位置

默认统一库位于系统应用数据目录下的 `skillmate/skills`。设置页可以修改统一库位置，也可以使用环境变量：

```text
SKILLMATE_LIBRARY_DIR=/absolute/path/to/skills
```

约束如下：

- 必须使用绝对路径
- 只有当前统一库和目标目录都为空时才能更换
- 不能位于任何 Agent 的全局 Skill 发现目录内
- 环境变量生效时，设置页只读

这些限制用于避免已有启用连接失效，也避免“添加到库”意外等同于“全局启用”。

### 更新

- SkillMate 管理且来源可重建的 Git Skill 支持检查和一键更新
- Git 仓库子目录与固定引用会保留在来源记录中
- 本地来源只检查原始位置是否仍可用，不执行自动更新
- `legacy_npm` / `legacy_pip` 只用于识别历史来源和提示原更新方式
- 外部内容保持可见，但由原安装器或用户维护

更新前会重新执行安全策略。内容或策略在预览后变化时，旧计划令牌会失效。

### 应用更新

SkillMate 可以通过 GitHub Releases 和 Tauri updater 检查、验证并安装应用更新。设置页支持启动时自动检查；发现新版本后，可以直接从提示中安装，也可以暂时忽略本次安装提示。

### 组织、场景与迁移

- **标签**：为 Skill 添加标签并筛选
- **应用场景**：按写作、开发、审查等任务保存一组 Skills，查看缺失状态并复用组合
- **批量启用**：在技能库勾选已入库的 Skills，统一选择平台及全局／项目范围。无需创建场景；场景筛选后也可使用同一入口，不会自动停用其他 Skills。
- **导入 / 导出**：导出标签、应用场景和受管 Skill 清单，导入前预览变化
- **Git 备份**：把明确登记的受管内容快照到本地 Git 仓库，并可推送远端
- **SkillMate manifest**：使用 `skillmate.toml` 以 `install` / `keep` / `remove` 计划对齐目标状态
- **环境快照**：保存一组 Skill 来源组合，支持预览、应用和一次性回滚

项目级受管 Skill 可以导出到项目根目录的 `skillmate.toml`。清单会记录来源、目标平台、固定引用和内容哈希；重新应用时只对齐同一项目，不影响全局或其他项目。

## 命令行

仓库同时提供 `skillmate-cli`，用于盘点、脚本化管理和声明式对齐：

```text
skillmate-cli scan [--json]
skillmate-cli list [--json]
skillmate-cli project <项目目录> [--json]
skillmate-cli add <来源> [--source git|local|claude_marketplace] [--skill <相对路径>]... [--plan-token <令牌>] [--json]
skillmate-cli enable <统一库Skill目录> --assistant <Agent> [--project <项目目录>] [--plan-token <令牌>] [--json]
skillmate-cli adopt <Skill目录> --assistant <Agent> [--project <项目目录>] [--plan-token <令牌>] [--json]
skillmate-cli maintain [--json]
skillmate-cli library [--set <绝对路径>] [--json]
skillmate-cli agent-skill [--install <Skills根目录>]
skillmate-cli plan <skillmate.toml> [--json]
skillmate-cli verify <skillmate.toml> [--json]
skillmate-cli apply <skillmate.toml> --plan-token <令牌> [--json]
```

`add`、`enable` 和 `adopt` 采用两阶段写入：第一次运行生成计划，确认后重复原命令并附带输出的 `plan-token`。声明式流程先运行 `plan`，再把令牌交给 `apply`。来源、目标、策略或当前状态变化后，旧令牌会失效。

示例：

```bash
# 先预览
skillmate-cli add example/skills --skill skills/writer

# 确认后重复原参数并附带令牌
skillmate-cli add example/skills --skill skills/writer --plan-token <令牌>

# 查看某个项目中各 Agent 实际生效的 Skills
skillmate-cli project /path/to/project

# 将统一库中的 Skill 启用到 Codex 项目
skillmate-cli enable /path/to/library/writer --assistant Codex --project /path/to/project
```

`agent-skill --install` 可以把随应用提供的 SkillMate Agent Skill 安装到指定 Skills 根目录；如果目标已经存在，包括断开的软连接，命令会拒绝覆盖。

## 可靠性与数据边界

- 文件系统、SQLite 和 sidecar 状态通过补偿式事务协调，失败时执行回滚并报告未完成步骤
- 写入操作使用绑定来源、目标和当前状态的计划令牌，避免执行过期计划
- SkillMate 只自动修改自己明确登记的受管内容
- 已存在的外部目标会阻止启用，不会被静默覆盖
- 受管内容发生本地修改时，破坏性操作会被阻止或要求重新确认
- 普通删除进入 SkillMate 自有垃圾箱，恢复时不会覆盖原路径中新出现的内容
- Windows、macOS 和 Linux 使用各自的安全路径与目录连接处理

Git 备份只保存受管 Skill 内容，不是完整应用恢复。快照不包含数据库、标签、应用场景、环境快照、sidecar、运行时缓存或目录连接，并会排除常见凭据与密钥文件。

为保证提交内容和安全扫描结果一致，Git 备份直接提交已经验证的 Git tree。Git clean filter 会生效，但不会执行提交 hooks，也不会自动应用 `commit.gpgSign`。

## 当前边界

SkillMate 当前专注于目录型 Agent Skills，暂不提供：

- VS Code、Cursor、Windsurf、Zed 等整包扩展管理
- npm / PyPI 第三方安装入口
- MCP 配置与服务器管理
- 团队私有 Registry 与账号同步
- 模型驱动的自动安装决策

规则无法识别的自然语言或复杂输入可能显示“可用模型辅助识别”，但当前版本不会自动调用模型 API。

## 本地开发

需要 Node.js 24.20.0+、pnpm 11、Rust，以及当前系统对应的 [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install
pnpm dev
```

测试和构建：

```bash
pnpm test
pnpm build
```

## 获取与项目信息

- [下载 Releases](https://github.com/yuluod/SkillMate/releases)
- [GNU AGPL v3 或更高版本](LICENSE)
- [安全策略](SECURITY.md)
- [参与贡献](CONTRIBUTING.md)
