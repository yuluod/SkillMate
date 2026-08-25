# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

SkillMate 面向同时使用一个或多个 AI 编程助手的开发者。他们需要盘点本机 Skills、判断安装来源是否可信、在不同助手或项目之间分发内容，并在更新、迁移和恢复时避免覆盖未知的手工内容。

## Product Purpose

SkillMate 是本地优先的 AI Skills 生命周期管理器。它让用户能够发现、检查、安装、组织、同步、更新和复现目录型 Skills；成功意味着用户能在写入前理解风险和变更，在写入后追溯来源，并能在另一台设备或项目中重建同一组 Skills。

## Positioning

SkillMate 的差异化机制是将严格的 Agent Skills 规范校验、静态风险检查、来源与提交追踪、声明式目标状态以及可回滚事务放在同一条安装和同步链路中。市场或跨平台分发不能绕过这条安全链路。

## Operating Context

产品作为 Tauri 桌面应用运行，读取用户主目录和用户明确选择的项目目录中的 Skills。主要工作流包括本机盘点、市场或 Git/本地来源安装、跨平台副本比较、Git 上游更新、标签与组合组织、`skillmate.toml` 对齐、Profile 切换以及 Git 备份。

## Capabilities and Constraints

- 只管理可落到本地目录的 Skills，不把 IDE 扩展或通用包管理器作为同一概念。
- 所有外部来源在安装前必须经过结构预览、风险检查、安装策略和写入计划。
- 自动删除仅作用于 SkillMate 明确登记的受管内容；普通卸载必须可恢复。
- 内置平台约定应由同一数据模型驱动，并允许后续增加经过验证的平台。
- 应用保持本地优先，不要求 SkillMate 账号；市场访问和 Git 操作可以使用网络。
- MCP 管理和团队 Registry 不属于当前 P0/P1 范围。

## Brand Commitments

保留 SkillMate 名称、当前蓝色主色、明暗主题和偏专业工具的克制语气。界面文案提供简体中文和英文，不使用夸张营销语言。

## Evidence on Hand

- 当前功能与边界记录在 `README.md`。
- 安装策略、结构校验、来源追踪、事务恢复和 Git 备份均有仓库内实现及测试。
- `src/assets/brands/` 包含现有 Agent 品牌资产。
- 当前没有客户案例、性能基准或商业化证明，界面不得虚构这些内容。

## Product Principles

- 先看清，再写入。
- 来源可追溯，状态可复现。
- 默认保护用户手工内容。
- 深能力必须通过清晰的日常操作被用户感知。
- 扩展平台不能削弱安全与事务语义。

## Accessibility & Inclusion

所有核心操作必须支持键盘访问、清晰焦点、非颜色状态提示和减少动态效果；中英文切换不能依赖重启应用。
