# SkillMate 开发指引

## 按任务读取

- 产品术语、功能范围：先看 [PRODUCT.md](PRODUCT.md)，用户操作和 CLI 用法见 [README.md](README.md)。
- 界面修改：看 [DESIGN.md](DESIGN.md)，并核对现有组件和 `src/styles.css`；保留经典、现代、复古三套主题的差异。
- 环境、验证和贡献要求：见 [CONTRIBUTING.md](CONTRIBUTING.md)，工具版本以仓库配置为准。
- 分析、审查请求只输出结论；实现请求完成修改与适当验证。提交、推送和打 Tag 需用户明确要求。

## 业务约束与代码入口

- 添加仅进入统一库；启用才选择平台和全局／项目范围。接管是显式操作，外部内容不得因扫描而自动变成受管内容。相关入口为 `skill_library.rs`、`skill_adoption.rs` 和 `lib.rs`。
- 平台目录连接和库主副本是不同路径身份。读取内容、比较指纹和探测来源时复用 `resolve_library_path`；停用和启用记录仍针对平台路径，不能把链接删除变成主副本删除。
- `can_check` 表示可检查来源，`can_sync` 表示可执行受管更新，两者不可互相代替。来源及更新规则集中在 `skill_origin.rs`，前端实时状态复用 `useUpdateFlow.js`。
- 安装、更新和删除复用现有操作协调、计划校验与事务恢复机制，不新增平行实现。涉及失败路径时验证文件、数据库和 sidecar 的一致性。
- 平台目录约定集中在 `app_core.rs`；调整时同步核对全局／项目范围、发现顺序、路径矩阵测试和两份 README。路径删除复用现有跨平台工具。

以上 Rust 文件位于 `src-tauri/src/`，前端流程位于 `src/lib/`。先定位已有实现，再决定是否需要新抽象。

## 界面与文档

- 共享表单、页面标题和列表优先复用现有组件与 CSS Token；复古主题的直角不应被通用圆角覆盖。
- 用户文案同步维护 `src/locales/zh-CN.js` 和 `src/locales/en.js`；改变用户流程时同步相关中英文 README。
- `PRODUCT.md` 和 `DESIGN.md` 记录持续有效的约束；个人截图与设计过程记录放在已忽略的 `.impeccable/`，不复制全局 Skill 到仓库。
- `src-tauri/resources/skills/skillmate/SKILL.md` 是供用户的 Agent 操作 CLI 的内置 Skill，不是开发本项目的指令。更改它时核对 `cli.rs` 实际支持的命令。

## 验证范围

- 文档修改检查内容、链接和差异格式；内置 Skill 修改还需验证 frontmatter 与 CLI 用法。
- 前端行为修改运行相关测试与前端构建；Rust 行为修改运行相关测试、格式检查和 Clippy。具体命令见贡献指南。
- 发布修改同步 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json` 与 CHANGELOG，并运行发布元数据测试。
