# SkillMate 视觉系统

## 方向

SkillMate 是一间海关——任何 Skill 落盘前先申报、查验、盖章。界面即台账文件，拒绝品类默认的「侧栏 + 同尺寸卡片墙」排布。

三套界面风格共享同一登记册结构，用户可在设置 → 外观中切换。显示名称保持直观，内部标识继续承载各自的设计方向：

| 显示名称 | 标识 | 设计方向 |
|---|---|---|
| 经典 | `ledger`（默认） | 纸面登记册：单线表格、钢印/签章、打字机数字、签章蓝墨 |
| 现代 | `standard` | 净色网格、克制阴影、标准桌面工具 |
| 复古 | `cardbox` | HyperCard 致敬：像素边框、抖动灰、硬偏移阴影 |

## Token 体系

皮肤通过 `data-skin` 属性切换，明暗通过 `data-theme` 属性切换。所有颜色、圆角、边框宽度、阴影均以 CSS 自定义属性定义，按特异性层叠：

```
:root                          /* ledger 深色（默认） */
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
| `--text3` | `#94917f` | `#5d5f72` | `#98a3b3` | `#556278` | `#969696` | `#585858` |
| `--radius` | `4px` | `4px` | `10px` | `10px` | `0` | `0` |
| `--border-w` | `1px` | `1px` | `1px` | `1px` | `2px` | `2px` |
| `--shadow` | `0 3px 10px rgba(0,0,0,.3)` | `0 2px 8px rgba(60,54,38,.14)` | `0 4px 12px rgba(0,0,0,.22)` | `0 4px 12px rgba(15,23,42,.08)` | `3px 3px 0 rgba(214,214,214,.55)` | `3px 3px 0 rgba(10,10,10,.85)` |
| `--control-height` | `40px` | `40px` | `40px` | `40px` | `40px` | `40px` |

品牌蓝在所有皮肤中保持一致系：`#2450c4` / `#2563eb` / `#1d43c8`。

## 页面骨架

概览、Skills、平台、集合、更新与设置内容区共用 `.surface-header`：标题、说明、数量和右侧操作保持同一层级、间距与分隔线。页面内容统一放在最大宽度 `1180px` 的 `.view-shell` 中；设置页因包含二级导航，内容列保持独立的阅读宽度。

页面内的主要区块共用 `.surface-section-head`，数量统一使用等宽数字 `.surface-meta`。状态统一优先使用签章，标签仅表示用户定义的组织分类。

输入框、选择框与文本域共享 40px 基础高度、`--control` 背景、`--radius-sm` 圆角以及相同的悬停、焦点和禁用状态；文本域仅扩展纵向高度。带图标的搜索框只调整图标内边距，选择框使用统一的 CSS 箭头，避免不同桌面 WebView 使用各自的默认外观。复选框和颜色选择器保留其控件语义。

## 登记册结构

Skills、平台和 Updates 页使用统一的登记册（`.registry`）结构，替代原有的卡片网格。

### 列布局

```
registry-colhead:  [登记条目] [平台] [体积] [操作]
registry-row:      [名称+签章+路径] [平台] [体积] [操作按钮]
```

Updates 页的列布局略有不同，用「当前 → 最新」版本列和状态列替代平台/体积列。

平台页使用「平台 / 目录 / 状态 / Skills」四列；空状态继续保留登记册列线与说明，避免与其他资源页形成独立的卡片语言。

### 登记行内容

每条 Skill 登记行包含：

- **身份行**：名称（`h3`，截断省略）与来源签章
- **审查行**：结构结论、共享状态、本地改动和静态风险；阻断性风险使用 error 色，并由右侧文字动作进入审查
- **标签行**：普通组织标签独立于审查状态，最多显示 2 个，超出显示 `+N`
- **来源签章**（`.stamp.stamp-source`）：git/github 为品牌蓝，npm/pip 为绿色，local/unmanaged 为灰色
- **结构状态签章**（`.stamp` + tone）：complete/success、nonstandard/warn、invalid/error
- **共享签章**（`.stamp`）：跨助手共享时显示
- **变更签章**（`.stamp.error`）：受管内容已变更且覆盖被阻止
- **风险签章**（`.stamp.error`）：安全警告数量
- **描述**（`.registry-desc`，可选，截断省略）
- **路径**（`.registry-path`，等宽字体，截断省略，symlink 显示箭头指向源）
- **平台**（`.registry-platform`）：AI 助手头像 + 可用性列表
- **体积**（`.registry-size`，等宽 tabular-nums）
- **操作**（`.registry-actions`）：常驻“查看/审查”文字动作；标签、目录和移除收进原生展开区，减少图标猜测

登记册支持选择当前筛选结果，并复用标签编辑器批量添加标签；批量添加不移除条目已有标签。

### 空状态

空状态保持登记册形态：仍渲染 `.registry` + `.registry-colhead` + 一条 `.registry-empty` 行。空行包含一个 muted 印章、空标题、提示文案和操作按钮。内容抽掉后仍能靠列线与印章认出这本登记册。

## 签章词汇

签章（`.stamp`）是关务台账世界的核心状态标记，用文字 + 边框表达状态，不依赖颜色 alone：

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

### ledger（关务台账）

- 页眉双线（`border-bottom: 3px double var(--rule)`）
- 登记册无圆角，页眉双线作为顶部主分隔，登记册自身仅保留底部单线
- 签章有内阴影压痕
- 导航激活项为索引签样式（右侧延伸、左圆角）
- 数字、路径、badge 使用等宽 tabular-nums
- dashboard-status 为签章行（非大数字条）

### standard（极简现代）

- 10px 圆角
- 克制阴影
- 登记册有边框和圆角
- 标准桌面工具语法

### cardbox（卡片盒）

- 0 圆角，2px 边框
- 硬偏移阴影 `3px 3px 0`
- 侧栏抖动纹理（inline SVG checkerboard）
- 导航激活项反相（深底浅字）
- 签章 0 圆角

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

- `#/dashboard`、`#/skills`、`#/ai`、`#/scenarios`、`#/updates`、`#/settings`
- `#/settings/appearance`、`#/settings/language` 等

前进后退可精确回溯查验轨迹。初始视图从 hash 读取，避免与 hash 同步 effect 竞态。

## 响应式

- `max-width: 820px`：搜索进入顶栏第二行；登记册列头隐藏，行堆叠为单列，操作目标不小于 44×44px
- `max-width: 768px`：侧栏收窄但保留文字标签，不使用只有图标的导航
- `max-width: 620px`：导航移到底部，六个入口继续同时显示图标和文字
- 移动端（390×844）：登记册纵向堆叠，无横向溢出

## 错误恢复

- 数据加载错误先说明影响和下一步，不在主文案中暴露 `TypeError`、`invoke` 等实现细节
- 核心加载失败明确说明未执行写入，现有内容保持不变
- 原始异常仅放入可展开、可选择复制的诊断区

## 无障碍

- 签章使用文字标签，不依赖颜色 alone 表达状态
- 皮肤选择器使用 `radiogroup` 语义
- 键盘焦点可见（`outline: 2px solid var(--accent)`）
- `prefers-reduced-motion` 支持
- light 主题 `--text3` 对比度 ≥ 4.5:1

## i18n

外观与登记册相关文案在 `src/locales/zh-CN.js` 和 `src/locales/en.js` 中同步提供：

- `settings.appearance` / `settings.appearanceHint`
- `settings.skin` / `settings.skin.{ledger,standard,cardbox}` / `settings.skin.{ledger,standard,cardbox}Hint`
- `settings.theme` / `settings.theme.{system,light,dark}`
- `registry.entry` / `registry.platform` / `registry.size` / `registry.actions`
- `registry.empty` / `registry.clear`

## 涉及文件

- `index.html` — 方向契约注释
- `src/App.jsx` — 皮肤状态、持久化、URL 覆盖、深链
- `src/styles.css` — 三套皮肤 token、登记册、签章、皮肤选择器、皮肤特有材质
- `src/components/InventoryViews.jsx` — SkillsView / UpdatesView 登记册行
- `src/components/SettingsView.jsx` — 外观设置 tab
- `src/components/DashboardView.jsx` — dashboard-status 签章块
- `src/locales/zh-CN.js` / `src/locales/en.js` — 外观与登记册文案
