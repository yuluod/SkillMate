---
name: skillmate
description: 使用 SkillMate CLI 盘点、添加、接管和启用本机 Agent Skills，或检查项目实际生效的 Skills。适用于需要跨 Agent 管理目录型 Skill 的任务。
---

# SkillMate

使用 `skillmate-cli` 管理目录型 Agent Skills。本 Skill 用于操作 SkillMate，不用于指导修改 SkillMate 项目代码。

## 准备

先运行 `skillmate-cli --help` 确认可执行文件及当前支持的命令。若命令不可用，说明需要安装或提供 CLI 路径，不假定安装桌面应用后 CLI 已在 PATH 中。需要解析输出时，对支持的命令使用 `--json`。

## 选择操作

- `list --json` 查看统一库，`scan --json` 盘点本机平台；项目相关任务用 `project <项目目录> --json` 核对生效范围。
- `add <来源>` 只把内容加入统一库，不需要平台或项目。多 Skill 包按用户选择传入可重复的 `--skill <相对路径>`。
- `enable <统一库Skill目录> --assistant <Agent>` 启用到平台；带 `--project <项目目录>` 为项目范围，省略则为全局范围。
- `adopt <Skill目录> --assistant <Agent>` 用于将已有外部实体 Skill 转入统一库，项目级内容同时指定 `--project`。

范围以用户意图为准。项目相关能力建议在项目中启用，这不是 CLI 自动选择项目的行为；已有明确项目路径可直接使用，范围或路径不明确时先澄清。仅要求添加时，不额外要求选择项目或自动启用。

## 工作流

1. 添加、接管或启用前，先运行不带 `--plan-token` 的命令查看计划；声明式对齐先运行 `plan <skillmate.toml>`。
2. 核对来源、所选 Skills、目标平台、范围、冲突和安全策略是否符合用户要求。
3. 用户已明确授权该写入且计划未超出范围时，使用返回的令牌和相同参数执行，不重复索要同一授权。若计划涉及未授权的删除、替换或新的目标范围，先说明具体影响并确认。
4. 令牌失效后重新预览并核对变化，不绕过校验。检查结果中的成功标记和错误信息，不能仅凭进程退出码宣称成功；必要时用 `list` 或 `project` 核对结果。

不要替用户猜测项目路径、覆盖外部实体目录或绕过安装策略。外部 Skill 需要转入统一库时使用 `adopt`，不要把它当成普通覆盖安装。

## 更新边界

`maintain --json` 检查已知 Skills 的更新，不安装更新。可检查来源不等于可自动更新：SkillMate 受管且来源可重建的 Git Skill 可通过桌面端执行更新；本地来源只检查原始位置是否可用，历史 npm/PyPI 和其他外部内容由原安装器或用户维护。当前 CLI 没有独立的 Skill 更新安装命令，不编造 `update` 子命令，也不把重新添加当作更新。
