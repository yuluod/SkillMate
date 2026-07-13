# 参与贡献

感谢你改进 SkillMate。提交改动前，请先确认问题范围明确，并尽量保持 diff 小而完整。

## 开发环境

- Node.js 22.13.0（见 `.node-version`）
- pnpm 11.6.0（见 `package.json#packageManager`）
- Rust 1.96.0（见 `rust-toolchain.toml`）
- 当前平台所需的 Tauri v2 系统依赖

安装依赖：

```bash
pnpm install --frozen-lockfile
```

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
