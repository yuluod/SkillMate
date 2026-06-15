# SkillMate

[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)
![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-2021-000000?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-18-61DAFB?logo=react&logoColor=0b1020)
![Node.js](https://img.shields.io/badge/Node.js-%3E%3D22-5FA04E?logo=nodedotjs&logoColor=white)
![pnpm](https://img.shields.io/badge/pnpm-11.6-F69220?logo=pnpm&logoColor=white)
[![Release](https://github.com/yuluod/SkillMate/actions/workflows/release.yml/badge.svg)](https://github.com/yuluod/SkillMate/actions/workflows/release.yml)

一个跨平台桌面应用程序，用来统一管理目录型 **AI 助手 Skills**。

当前版本刻意收窄了边界：只处理有明确本地 Skill 目录的助手，不再把整包 IDE 扩展和 Skill 混为同一种对象。

## 当前支持的 AI 助手

- Claude Code
- Codex
- OpenClaw
- Gemini CLI

## 当前核心能力

### 1. 统一盘点

- 扫描受支持助手的本地 Skill 目录
- 展示名称、路径、来源、README 预览和更新状态
- 用标签对 Skill 做筛选和组织

### 2. 安装到目标助手

当前版本只支持两类安装来源：

- Git 仓库
- 本地目录

Git 仓库安装支持普通仓库地址，也支持通过 `#ref:path` 指定分支 / 标签 / 提交和仓库子目录，例如：

- `https://github.com/example/skills.git`
- `https://github.com/example/skills.git#main:skills/writer`
- `https://github.com/example/skills/tree/main/skills/writer`

Git 来源可以先执行结构预览；预览会临时克隆仓库并识别 `SKILL.md`、README 和资源目录。安装仓库子目录时，SkillMate 会只复制该子目录内容；这类安装会保留来源信息，后续可通过重新拉取该子目录完成一键更新。

安装输入会先经过本地规则识别，不需要模型 API。规则能识别本地目录、Git URL、GitHub `owner/repo` 简写、GitHub tree URL、`repo#ref:path` 和暂未支持的压缩包链接；规则无法判断的自然语言或复杂 README 说明会标记为“可用模型辅助识别”，但当前不会自动调用模型。

安装时必须明确选择目标助手，SkillMate 会把内容真正落到对应助手的受管目录。

### 3. 更新视图

更新页会统一展示每个 Skill 的来源和同步状态，当前支持的来源识别类型为：

- `git`
- `legacy_npm`
- `legacy_pip`
- `local`

其中自动更新只针对：

- Git 仓库

`legacy_npm` 和 `legacy_pip` 只作为历史来源 / 外部环境来源探测，不作为当前安装入口，也不会在 SkillMate 内执行全局 npm/PyPI 升级。

### 4. 场景管理

- 可以把多个 Skill 路径保存成场景
- 可以查看场景中每个 Skill 的存在状态
- 可以把场景回填到编辑器继续修改
- 可以复制场景中的路径列表

### 5. Git 备份

- 保存本地备份仓库路径、远端地址和分支
- 同步时会把受管助手目录快照到备份仓库
- 配置了远端时会自动推送
- 未配置远端时也可以只做本地快照提交

### 6. 组织数据导入 / 导出

- 可以把当前标签、场景和受管 Skill 清单导出为 JSON 文件
- 可以从导出的 JSON 文件重新导回标签和场景
- 导入前可以先预览新增、覆盖和清空摘要
- 导入时支持“合并”和“替换现有组织数据”两种模式
- 导入 / 导出不会直接覆盖本地 Skill 文件内容

## 当前不做的事情

为了保持产品边界清晰，当前版本暂时不做：

- VSCode / Cursor / Windsurf / Zed 整包扩展管理
- npm / PyPI 作为安装入口
- 市场搜索

这些能力后续是否恢复，要等目录型 Skill 闭环足够稳定之后再评估。

## 快速开始

### 前置要求

- Rust
- Node.js 22+
- Tauri 2.x
- pnpm

### 开发

```bash
pnpm install
pnpm dev
```

`pnpm dev` 会自动执行：

- 启动 Vite 开发服务器（`http://localhost:1420`）
- 启动 Tauri 开发进程并连接该前端地址

### 构建

```bash
pnpm build
```

### 发布构建

GitHub Release 构建通过 `.github/workflows/release.yml` 执行，当前会为 macOS、Windows 和 Linux 生成安装包。

推荐发布流程：

```bash
git tag v1.0.0
git push origin v1.0.0
```

workflow 会创建草稿 Release，并上传：

- macOS: `dmg`
- Windows: `nsis`
- Linux: `deb`、`rpm`

也可以在 GitHub Actions 页面手动运行 Release workflow，并填写要发布的 tag。

## 技术栈

- Tauri 2.x
- Rust + SQLite
- Vite + React

## 前端脚本

- `pnpm frontend:dev`
- `pnpm frontend:build`

## 更新系统说明

更新系统会根据 Skill 的来源进行探测与状态判断。

### 已展示的关键信息

- 来源类型
- 远端来源
- 当前版本引用
- 最新可用引用
- 落后提交数
- 同步状态
- 最近检查时间

### 一键更新出现条件

通常需要同时满足：

- 来源类型支持自动处理
- 当前状态为可更新
- 系统判定 `can_sync = true`

### 已知限制

- `local` 来源仅做可用性检测与展示，不参与自动更新
- `legacy_npm/legacy_pip` 的版本探测依赖本机环境与网络可用性，结果只作提示
- 不同 Skill 的元数据完整度可能不同，个别字段可能为空

## 许可证

AGPL-3.0-or-later。详见 [LICENSE](LICENSE)。
