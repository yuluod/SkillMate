# 参与贡献

感谢你改进 SkillMate。提交改动前，请先确认问题范围明确，并尽量保持 diff 小而完整。

## 开发环境

- Node.js 24.20.0（见 `.node-version`）
- pnpm 12.3.4（见 `package.json#packageManager`）
- Rust 1.96.0（见 `rust-toolchain.toml`）
- 当前平台所需的 Tauri v2 系统依赖

安装依赖：

```bash
pnpm install --frozen-lockfile
```

请使用 `package.json#packageManager` 声明的 pnpm 版本，版本不匹配会直接报错。
pnpm 配置集中在 `pnpm-workspace.yaml`；缓存和 store 使用平台默认目录。
运行脚本前若依赖已过期，会提示错误；先执行 `pnpm install --frozen-lockfile` 再重试，
避免 `pnpm dev` 在启动过程中自动重建依赖目录。
需要镜像时请在个人 `.npmrc` 中配置 `registry`，不要把本机缓存路径提交到仓库。

Windows 的启用与接管依赖目录软链接权限，请先开启开发者模式。应用会在相关预览中检查
链接能力；实际执行时仍会验证目标目录权限。完整 Rust 测试也需要该能力。
Git 测试会创建多个本地仓库；资源紧张时可用 `-- --test-threads=2` 限制测试并行数，
但不要用跳过失败测试代替完整验证。

`src-tauri/gen/schemas/` 是 Tauri 自动生成的编辑器 schema，不纳入版本控制。
首次运行 `pnpm dev` 或执行 Rust 构建后会生成，无需手工编辑。

## 本地验证

```bash
pnpm frontend:test
pnpm frontend:build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-fail-fast
```

## Pull Request 要求

- 说明问题、方案、用户可见变化和验证结果
- 为修复或新增行为补充回归测试
- 不混入无关格式化、重命名或生成文件
- 不提交密钥、凭据、本地数据库、备份仓库或私有文档
- 涉及安装、删除、更新和同步时，明确失败回滚与路径安全策略

提交信息建议使用 Conventional Commits，例如 `fix: restore managed state after failed install`。

## 许可证

提交贡献即表示你有权提交该内容，并同意其按项目的 [GNU AGPL v3 或更高版本](LICENSE)发布。
