---
name: SkillMate
description: 本地优先的 Skills 桌面工作台，三套皮肤共享清晰的操作层级
colors:
  standard-secondary-dark: "#96a2b0"
  standard-secondary-light: "#646b78"
typography:
  page-title:
    fontSize: "1.8rem"
    fontWeight: 780
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  skill-title:
    fontSize: "0.94rem"
    fontWeight: 650
  skill-meta:
    fontSize: "0.8rem"
spacing:
  content-top: "40px"
  content-inline: "44px"
  content-bottom: "48px"
components:
  sidebar:
    width: "216px"
  skill-row:
    padding: "12px 10px 12px 4px"
---

# SkillMate 视觉系统

## 方向

SkillMate 是用于日常操作的桌面管理工具，优先保证名称、来源、启用位置和操作结果易于辨认。安静列表是当前信息布局的基础；纸面、签章等材质只用于视觉表达，不应引入“申报、查验、盖章”等额外业务术语。

小范围优化保留现有信息结构，优先解决文字过小、截断、状态混淆与重复分隔。用户文案使用添加、接管、启用、检查更新等具体动作，避免为风格牺牲可读性。

三套界面风格共享同一套列表与签章结构，用户可在设置 → 外观中切换。显示名称保持直观，内部标识继续承载各自的设计方向：

| 显示名称 | 标识 | 设计方向 |
|---|---|---|
| 中性现代 | `standard`（默认） | 纯白底、近黑文字、单一靛蓝、8px 圆角，Linear/Raycast 一系 |
| 经典 | `ledger` | 纸面台账：单线清单、钢印/签章、打字机数字、签章蓝墨 |
| 现代 | `standard` | 净色网格、克制阴影、标准桌面工具 |
| 复古 | `cardbox` | HyperCard 致敬：像素边框、抖动灰、硬偏移阴影 |

## Token 体系

皮肤通过 `data-skin` 属性切换，明暗通过 `data-theme` 属性切换。所有颜色、圆角、边框宽度、阴影均以 CSS 自定义属性定义，按特异性层叠：

```
:root                          /* ledger 深色（standard 未生效时的回退） */
[data-theme="light"]           /* ledger 浅色 */
[data-skin="standard"]         /* standard 深色 */
[data-skin="standard"][data-theme="light"]  /* standard 浅色 */
[data-skin="cardbox"]          /* cardbox 深色 */
[data-skin="cardbox"][data-theme="light"]   /* cardbox 浅色 */
```

### 核心 Token

| Token | ledger 深色 | ledger 浅色 | standard 深色 | standard 浅色 | cardbox 深色 | cardbox 浅色 |
|---|---|---|---|---|---|---|
| `--bg` | `#151821` | `#efece2` | `#0f141d` | `#f3f6fa` | `#101010` | `#ffffff` |
| `--accent` | `#7d9ff2` | `#2450c4` | `#6ea8ff` | `#2563eb` | `#8fa9ff` | `#1d43c8` |
| `--text` | `#ede8db` | `#20263a` | `#f4f7fb` | `#101828` | `#f4f4f4` | `#0a0a0a` |
| `--text3` | `#94917f` | `#5d5f72` | 见 `colors.standard-secondary-dark` | 见 `colors.standard-secondary-light` | `#969696` | `#585858` |
| `--radius` | `4px` | `4px` | `10px` | `10px` | `0` | `0` |
| `--border-w` | `1px` | `1px` | `1px` | `1px` | `2px` | `2px` |
| `--shadow` | `0 3px 10px rgba(0,0,0,.3)` | `0 2px 8px rgba(60,54,38,.14)` | `0 4px 12px rgba(0,0,0,.22)` | `0 4px 12px rgba(15,23,42,.08)` | `3px 3px 0 rgba(214,214,214,.55)` | `3px 3px 0 rgba(10,10,10,.85)` |
| `--control-height` | `40px` | `40px` | `40px` | `40px` | `40px` | `40px` |

品牌蓝在所有皮肤中保持一致系：`#2450c4` / `#2563eb` / `#1d43c8`。

## 页面骨架

桌面侧栏宽度见 `components.sidebar`，日常入口居上，设置通过剩余空间置于底部。当前入口提供 `aria-current="page"`，主导航具有独立可访问名称。主体留白见 frontmatter 的 `spacing`，宽度不超过 1024px 时收至 24px，820px 时为 16px，768px 时为 12px；设置页保留独立布局。页面标题使用 `typography.page-title`，620px 以下缩为 1.35rem。

概览、技能库、启用位置、应用场景、可更新与设置是六个主导航页面，共用 `.surface-header`：标题、说明、数量和右侧操作保持同一层级、间距与分隔线。顶栏只保留品牌、搜索与全局动作，不再重复页面标题；项目工作区以 `.ws-picker` 放在「启用位置」页的页头之下、项目核验之前，弹出面板在内容列内展开。场景筛选作为筛选片与状态、标签同排。应用场景面向写作、开发或审查等任务保存一组需要共同使用的 Skills；环境复现、导入导出和 Git 备份留在设置中渐进披露。页面内容统一放在最大宽度 `1180px` 的 `.view-shell` 中；设置页因包含二级导航，内容列保持 `680px` 的阅读宽度。

## 安静列表语言

技能库、可更新与启用位置共用同一套列表语言：筛选片（状态计数 + 标签 + 当前场景）在最上，行内只有图标块、名称、一行元信息与右侧信号，健康状态保持沉默，异常以小色点或签章提示；主要操作集中在行尾或详情栏，低频操作收进「···」菜单。技能库行点击展开右侧详情栏（`.skill-pane`），集中状态、来源、启用位置与更新动作；有更新的行在行尾直接给出「更新」入口，点击进入更新预览。

概览按状态摘要、待处理事项和「开始工作」组织，并保留直接添加入口。状态摘要为五列单线分隔条，技能数量、平台数量和更新数量可直接跳转；标签数和需处理数保持只读。待处理事项为主栏，常用入口为侧栏，分别使用 1.7fr 与 1fr，栏间距 36px，侧栏最小宽度 240px；1080px 以下纵向排列。常用入口指向技能库、启用位置与应用场景，并说明各自用途。待办条目跳转时带上筛选上下文：安全、结构、本地改动落到技能库「需处理」，更新落到可更新页。公开来源发现位于技能库内，按需展开；技能库提供直接添加和发现入口，但直接添加保持唯一主操作。

页面内的主要区块共用 `.surface-section-head`，数量统一使用等宽数字 `.surface-meta`。状态统一优先使用签章，标签仅表示用户定义的组织分类。

输入框、选择框与文本域共享 40px 基础高度、`--control` 背景、`--radius-sm` 圆角以及相同的悬停、焦点和禁用状态；文本域仅扩展纵向高度。带图标的搜索框只调整图标内边距，选择框使用统一的 CSS 箭头，避免不同桌面 WebView 使用各自的默认外观。复选框和颜色选择器保留其控件语义。

## 列表结构

技能库、可更新与启用位置共用 `.skill-list` + `.skill-row` 的安静列表，不再使用表格列头。筛选片（`.filter-chip`）位于列表上方，承载状态计数和标签；行内保持一条主信息线和右侧信号，低频操作收进「···」。

共享列表行最小高度为 68px，内边距见 `components.skill-row`，名称和元信息分别使用 `typography.skill-title` 与 `typography.skill-meta`；主题专用边框和内边距按皮肤覆盖。详情栏以边框分层，不再叠加阴影。

```
技能库行:   [选择] [图标] [名称 + 异常色点 / 一行元信息] [更新签章] [平台头像] [···]
可更新行:   [图标] [名称 + 来源签章 / 远端 · 落后 · 检查时间] [当前 → 最新] [状态] [检查/更新]
启用位置行: [头像] [名称 + 状态签章 / 目录 · 目录数 · 数量] [Skills 标签] [诊断] [展开]
```

- **技能库行**：点击或按 Enter/空格展开右侧 `.skill-pane`，集中显示状态签章、来源三要素、启用位置与标签；详情栏在 1080px 以下移到列表下方。桌面复选框在悬停、选中或聚焦时显现，620px 以下或无悬停能力的设备始终显示；批量操作作用于当前筛选结果。
- **可更新行**：只列落后、检查失败等需要处理的项，已是最新的 Skill 不进入列表；保留来源签章、`当前 → 最新` 引用列和检查/更新操作，筛选片按落后、失败计数。
- **启用位置行**：保留平台头像、状态签章和 Skills 标签，诊断与多目录详情按需展开。
- **空状态**：使用 `.empty-state` 独立区块，不再渲染列头或占位行；空库给添加入口，筛选无结果给清除筛选。

登记行支持选择当前筛选结果，并复用标签编辑器批量添加标签；批量添加不移除条目已有标签。

## 签章词汇

状态标签（`.stamp`）用文字和边框表达状态，不仅依赖颜色；签章材质由主题决定：

| 类 | 语义 | 颜色 |
|---|---|---|
| `.stamp` (default) | 中性信息 | `--text3` |
| `.stamp.success` | 通过/完成 | `--success` |
| `.stamp.warn` | 警告/落后 | `--warn` |
| `.stamp.error` | 错误/风险 | `--error` |
| `.stamp.muted` | 弱化 | `--text3` |
| `.stamp.stamp-source` | 来源钢印 | `--accent`（品牌蓝） |
| `.stamp.stamp-source.npm` | npm 来源 | `--success` |
| `.stamp.stamp-source.local` | 本地/未登记 | `--text3` |

ledger 皮肤的签章有内阴影（`inset 0 0 0 1px color-mix`），模拟钢印压痕。cardbox 皮肤签章为 0 圆角。

## 皮肤特有材质

### ledger（经典台账）

- 页眉保留一处双线分隔（`border-bottom: 3px double var(--rule)`），这是当前实现的主题样式
- 列表行以 `--rule` 单线分隔、无外框，行内边距收窄到 6px，避免与页眉叠加成重复分隔
- 签章有内阴影压痕
- 导航激活项为索引签样式（右侧延伸、左圆角）
- 数字、路径、badge 使用等宽 tabular-nums
- 概览使用共享状态摘要条，保留单线分隔与等宽数字特性

### standard（极简现代）

- 10px 圆角
- 克制阴影
- 列表行使用 10px 圆角悬停块，不画外框
- 标准桌面工具语法

### cardbox（卡片盒）

- 0 圆角，2px 边框
- 硬偏移阴影 `3px 3px 0`
- 侧栏抖动纹理（inline SVG checkerboard）
- 导航激活项反相（深底浅字）
- 签章和筛选片为 0 圆角

## 深链与主题持久化

### 皮肤状态

- `localStorage` key: `skillmate-skin`，值为 `ledger` / `standard` / `cardbox`
- 默认值: `ledger`
- `document.documentElement.setAttribute("data-skin", skin)`

### 明暗主题

- `localStorage` key: `skillmate-theme-mode`，值为 `system` / `light` / `dark`
- `system` 跟随 `prefers-color-scheme`
- `document.documentElement.setAttribute("data-theme", resolved)`

### URL 参数覆盖

用于测试和深链：

- `?skin=ledger|standard|cardbox` 覆盖皮肤
- `?theme=system|light|dark` 覆盖明暗

URL 参数优先级高于 localStorage，但不写回 localStorage。

### 视图深链

视图和设置页签同步到 `location.hash`：

- 主导航：`#/dashboard`、`#/skills`、`#/ai`、`#/scenarios`、`#/updates`、`#/settings`
- `#/settings/appearance`、`#/settings/language` 等

前进后退恢复浏览位置。初始视图从 hash 读取，避免与 hash 同步 effect 竞态。

## 响应式

- `max-width: 820px`：搜索进入顶栏第二行；列表行自动换行，行尾菜单操作目标不小于 44×44px
- `max-width: 768px`：侧栏收窄但保留文字标签，不使用只有图标的导航
- `max-width: 620px`：导航移到底部，六个主入口继续同时显示图标和文字
- `max-width: 620px`：概览状态摘要改为三列，次行取消首项左分隔线；设置入口取消沉底间距
- 无悬停能力的设备：行尾菜单、展开按钮与更新动作至少 44×44px
- 移动端（390×844）：列表纵向堆叠，无横向溢出

## 错误恢复

- 数据加载错误先说明影响和下一步，不在主文案中暴露 `TypeError`、`invoke` 等实现细节
- 核心加载失败明确说明未执行写入，现有内容保持不变
- 原始异常仅放入可展开、可选择复制的诊断区

## 无障碍

- 状态标签使用文字说明，不仅依赖颜色表达状态
- 皮肤选择器使用 `radiogroup` 语义
- 键盘焦点可见（`outline: 2px solid var(--accent)`）
- `prefers-reduced-motion` 支持
- light 主题 `--text3` 对比度 ≥ 4.5:1

## i18n

外观与列表相关文案在 `src/locales/zh-CN.js` 和 `src/locales/en.js` 中同步提供：

- `settings.appearance` / `settings.appearanceHint`
- `settings.skin` / `settings.skin.{ledger,standard,cardbox}` / `settings.skin.{ledger,standard,cardbox}Hint`
- `settings.theme` / `settings.theme.{system,light,dark}`
- `skills.filter{All,NeedsAttention,Updates}` / `updates.filter{Behind,Failed}`
- `skills.update{Ready,Action}` / `skills.detail{Status,Source,Agents,Close,Label,NoAgents}`

## 涉及文件

- `index.html` — 方向契约注释
- `src/App.jsx` — 皮肤状态、持久化、URL 覆盖、深链
- `src/styles.css` — 三套皮肤 token、安静列表、签章、皮肤选择器、皮肤特有材质
- `src/components/InventoryViews.jsx` — SkillsView / UpdatesView / AssistantsView 列表行与详情栏
- `src/components/SettingsView.jsx` — 外观设置 tab
- `src/components/DashboardView.jsx` — 可跳转状态摘要、待办与常用入口
- `src/locales/zh-CN.js` / `src/locales/en.js` — 外观与列表文案
