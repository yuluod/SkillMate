---
name: skillmate
description: 使用 SkillMate CLI 盘点、添加、接管和启用本机 Agent Skills，或检查项目实际生效的 Skills。适用于需要跨 Agent 管理目录型 Skill 的任务。
---

# SkillMate

使用 `skillmate-cli` 管理目录型 Agent Skills。默认保持项目优先：通用能力才全局启用，项目相关能力启用到具体项目。

## 工作流

1. 用 `skillmate-cli scan` 盘点本机内容。
2. 用 `skillmate-cli project <项目目录>` 核对各 Agent 实际生效的 Skills。
3. 添加、接管或启用前先运行不带 `--plan-token` 的命令查看计划。
4. 只有用户确认后，才使用计划输出的令牌重新执行写入命令。

不要替用户猜测项目路径、覆盖外部实体目录或绕过安装策略。外部 Skill 需要转入统一库时使用 `adopt`，不要把它当成普通覆盖安装。

运行 `skillmate-cli --help` 查看当前支持的命令和参数。
