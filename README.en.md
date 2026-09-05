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
  <a href="README.md">简体中文</a> · English
</p>

# SkillMate

SkillMate is a cross-platform **AI Skills manager** for inventorying, adding, adopting, enabling, and maintaining directory-based Skills used by different AI coding tools.

It does not try to replace each agent's own plugin system. Instead, it solves a more fundamental problem: where a Skill came from, who manages it, which platforms and projects use it, and how to avoid content drift caused by duplicated copies.

## Why SkillMate

Claude Code, Codex, Gemini CLI, Cursor, and other tools use different global and project directories. Manually copying the same Skill between them quickly creates problems:

- You cannot easily tell which copy a project is actually loading.
- Project-level and global Skills with the same name may shadow each other.
- Multiple copies of the same capability gradually diverge.
- Responsibility for updates from Git, external CLIs, and local directories becomes unclear.
- Reproducing the same setup on another computer or project is difficult.

SkillMate organizes these files around one managed master copy and explicit enablement links, while keeping externally installed content visible.

## Core model

SkillMate divides the Skill lifecycle into explicit operations:

- **Add**: resolve content from Git, a local directory, or Claude Marketplace and copy it into the SkillMate library without selecting a platform or project.
- **Adopt**: move an existing physical Skill from an agent directory into the library, then replace its original location with a managed directory link.
- **Enable**: create a directory link from the library to a platform's global or project directory.
- **Disable**: remove an enablement link while keeping the master copy in the library.
- **Delete**: remove a master copy managed by SkillMate. Normal deletion first moves it to SkillMate's own trash and provides a 60-second undo window.

```text
Git / local directory / Claude Marketplace
                    │
                    │ Add
                    ▼
            SkillMate library
            (one master copy)
                    │
             ┌──────┴──────┐
             │             │
       Global enable   Project enable
             │             │
             ▼             ▼
   Agent global dir   Project Skill dir
             └──── directory links ────┘

Existing physical Skill ── Adopt ──► Library + managed link at original location
```

The library owns content, provenance, and update state. Agent directories only describe where a Skill is enabled. SkillMate never adopts or overwrites external content automatically; adoption must be explicitly requested and confirmed from a write plan.

## Supported platforms and directories

| Platform | Default global enablement directory | Additional discovery directories | Project directory |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/skills` | — | `.claude/skills` |
| Codex | `~/.agents/skills` | `~/.codex/skills` | `.agents/skills` |
| OpenClaw | `~/.openclaw/skills` | `~/.agents/skills` | `skills` |
| Gemini CLI | `~/.gemini/skills` | `~/.agents/skills` | `.gemini/skills` |
| Cursor | `~/.cursor/skills` | — | `.cursor/skills` |
| OpenCode | `~/.config/opencode/skills` | — | `.opencode/skills` |
| GitHub Copilot | `~/.copilot/skills` | — | `.github/skills` |

SkillMate scans every discovery directory declared for a platform. Project inspection merges project-level and global content, applies project-first precedence, and shows both the effective Skills and the number shadowed by a same-named project Skill.

Creating managed directory links on Windows requires Developer Mode.

## Source, manager, and update responsibility

Content source, current manager, and update mechanism are separate concepts in SkillMate:

| Scenario | Content source | Manager | Update mechanism |
| --- | --- | --- | --- |
| Git Skill added by SkillMate | GitHub / Git | SkillMate | Checked and updated by SkillMate |
| Local Skill added by SkillMate | Local directory | SkillMate | Maintained manually |
| Skill installed by npm, pip, or another CLI | External CLI | Original installer | Updated with the original installer |
| Skill placed by an agent, plugin, or user | Git / local / unknown | External or manual | External tool or manual maintenance |
| Skill explicitly adopted by the user | Original source retained | SkillMate | Depends on whether the source can be reconstructed |

Discovering a directory does not make SkillMate its owner. Only content added to the library and registered by SkillMate is considered managed.

## Features

### Inventory and project inspection

- Scan global Skill directories for every supported platform.
- Inspect a project path and calculate the Skills visible to each platform.
- Distinguish project, global, shadowed, and shared discovery entries.
- Display content source, manager, update mechanism, and enabled platforms.
- Summarize structure issues, static risks, local changes, content drift, and scan diagnostics.

### Structure and safety checks

- Detect the exact, case-sensitive `SKILL.md` filename required by the Agent Skills specification.
- Validate YAML frontmatter, required `name` and `description` fields, name format, and directory-name consistency.
- Read optional fields such as `compatibility`, `license`, `metadata`, and `allowed-tools`.
- Recognize resource directories such as `references/`, `scripts/`, and `assets/`.
- Inspect scripts, dependency manifests, symbolic links, hidden files, network access, and environment-variable references.
- Classify structures as `Conformant`, `Needs attention`, or `Not a Skill`.

Static checks are used only for previews and installation decisions. SkillMate never executes scripts inside a Skill or runs third-party installation commands to infer provenance.

### Add sources

#### Git repositories

SkillMate accepts regular repository URLs, GitHub shorthand, GitHub tree URLs, and the `#ref:path` subdirectory syntax:

```text
https://github.com/example/skills.git
example/skills
https://github.com/example/skills.git#main:skills/writer
https://github.com/example/skills/tree/main/skills/writer
```

Git previews use a temporary clone. The added copy excludes `.git`, `.hg`, and `.svn`, while repository, ref, subdirectory, and installed commit metadata are stored separately for later update checks.

If a repository contains multiple Skills, you must explicitly select the directories to add.

#### Local directories

A directory containing one Skill can be added directly. A directory containing multiple Skills also requires an explicit selection, and SkillMate copies only the selected entries.

#### Claude Marketplace

The advanced source selector accepts:

```text
plugin
plugin@marketplace
```

SkillMate reads the local Claude Marketplace manifest, resolves a plugin to a local directory or Git source, and passes it through the same structure preview, risk checks, and write plan. Marketplace plugins sourced from npm are explicitly delegated to their original installer; SkillMate does not execute npm installation.

The [skills.sh](https://skills.sh/) catalog and GitHub Repository Search are discovery channels only. Adding a result still goes through the complete inspection flow.

### Adopt external Skills

External physical directories found during scanning remain read-only by default. The adoption flow:

1. Verifies the owning platform and global or project scope.
2. Builds a provenance, structure, safety-policy, and file-action preview.
3. Copies the content into the library.
4. Replaces the original physical directory with a managed link to the master copy.
5. Migrates provenance and update state.

Nested Skills from Git repositories retain their path relative to the repository root, so later updates do not accidentally target the whole repository. If any step fails, SkillMate attempts to restore the original directory and registration data.

### Library location

The default library is stored under the system application-data directory at `skillmate/skills`. You can change it in Settings or use an environment variable:

```text
SKILLMATE_LIBRARY_DIR=/absolute/path/to/skills
```

The following constraints protect existing installations:

- The path must be absolute.
- The library can move only when both the current and destination directories are empty.
- It cannot be placed inside any agent's global Skill discovery directory.
- Settings become read-only while the environment variable is active.

These rules prevent existing enablement links from breaking and ensure that adding a Skill never implicitly enables it globally.

### Skill updates

- Git Skills managed by SkillMate can be checked and updated when their source is reconstructable.
- Repository subdirectories and pinned refs are retained in provenance metadata.
- Local sources are checked only for availability; SkillMate does not update them automatically.
- `legacy_npm` and `legacy_pip` identify historical sources and direct users to the original updater.
- External content remains visible but is maintained by its installer or the user.

Safety policy is evaluated again before an update. If content or policy changes after a preview, the old plan token becomes invalid.

### Application updates

SkillMate checks, verifies, and installs application updates through GitHub Releases and the Tauri updater. Automatic startup checks can be configured in Settings. When a new release is found, it can be installed directly from the notification or ignored for the current prompt.

### Organization, scenarios, and migration

- **Tags**: tag and filter Skills; create, rename, recolor, and delete tags.
- **Scenarios**: save groups of Skills for writing, development, review, or other tasks, inspect missing entries, and reuse the group.
- **Bulk enablement**: select library Skills and choose platforms and global or project scope together. No scenario is required; scenario-filtered results use the same action. Other Skills are not automatically disabled.
- **Import / export**: export tags, scenarios, and the managed Skill inventory, with a change preview before import.
- **Git backup**: snapshot explicitly managed content into a local Git repository and optionally push it to a remote.
- **SkillMate manifest**: use `skillmate.toml` to reconcile a target state through `install`, `keep`, and `remove` actions.
- **Environment snapshots**: save a set of Skill sources, preview and apply it, and perform a one-time rollback.

A project-managed Skill can be exported to `skillmate.toml` in the project root. The manifest records source, target platform, pinned ref, and content hash. Reapplying it reconciles only that project and does not affect global or unrelated projects.

## Command line

The repository also includes `skillmate-cli` for inventory, scripted management, and declarative reconciliation:

```text
skillmate-cli scan [--json]
skillmate-cli list [--json]
skillmate-cli project <project-directory> [--json]
skillmate-cli add <source> [--source git|local|claude_marketplace] [--skill <relative-path>]... [--plan-token <token>] [--json]
skillmate-cli enable <library-skill-directory> --assistant <Agent> [--project <project-directory>] [--plan-token <token>] [--json]
skillmate-cli adopt <skill-directory> --assistant <Agent> [--project <project-directory>] [--plan-token <token>] [--json]
skillmate-cli maintain [--json]
skillmate-cli library [--set <absolute-path>] [--json]
skillmate-cli agent-skill [--install <skills-root>]
skillmate-cli plan <skillmate.toml> [--json]
skillmate-cli verify <skillmate.toml> [--json]
skillmate-cli apply <skillmate.toml> --plan-token <token> [--json]
```

`add`, `enable`, and `adopt` use a two-phase write flow. The first invocation produces a plan; after confirmation, repeat the same command with the emitted `plan-token`. Declarative operations run `plan` first and pass its token to `apply`. A change to the source, target, policy, or current state invalidates an older token.

Examples:

```bash
# Preview the operation
skillmate-cli add example/skills --skill skills/writer

# Repeat the same arguments with the confirmed token
skillmate-cli add example/skills --skill skills/writer --plan-token <token>

# Inspect the Skills that are effective for each agent in a project
skillmate-cli project /path/to/project

# Enable a library Skill in a Codex project
skillmate-cli enable /path/to/library/writer --assistant Codex --project /path/to/project
```

`agent-skill --install` installs the bundled SkillMate Agent Skill into a specified Skills root. If the destination already exists, including as a broken symbolic link, the command refuses to overwrite it.

## Reliability and data boundaries

- Compensating transactions coordinate filesystem changes, SQLite, and sidecar state; failures trigger rollback and report incomplete recovery steps.
- Write operations use plan tokens bound to source, target, and current state to prevent stale plans from executing.
- SkillMate automatically modifies only content explicitly registered as managed.
- Existing external destinations block enablement and are never silently overwritten.
- Destructive operations are blocked or require a fresh confirmation after local changes to managed content.
- Normal deletion uses SkillMate's own trash, and restoration never overwrites new content at the original path.
- Windows, macOS, and Linux use platform-appropriate safe path and directory-link handling.

Git backup stores managed Skill content, not a full application backup. Snapshots exclude the database, tags, scenarios, environment snapshots, sidecars, runtime caches, and directory links, as well as common credential and secret files.

To ensure the committed bytes match the security scan, Git backup commits the verified Git tree directly. Git clean filters still apply, but commit hooks are not executed and `commit.gpgSign` is not applied automatically.

## Current scope

SkillMate currently focuses on directory-based Agent Skills and does not provide:

- Full extension management for VS Code, Cursor, Windsurf, Zed, or similar editors.
- Third-party npm or PyPI installation entry points.
- MCP configuration or server management.
- A team-private registry or account synchronization.
- Model-driven automatic installation decisions.

Natural-language or complex inputs that cannot be classified by local rules may be shown as eligible for model-assisted detection, but the current release does not call a model API automatically.

## Local development

You need Node.js 24.20.0+, pnpm 11, Rust, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your operating system.

```bash
pnpm install
pnpm dev
```

Tests and production build:

```bash
pnpm test
pnpm build
```

## Downloads and project information

- [Download releases](https://github.com/yuluod/SkillMate/releases)
- [GNU AGPL v3 or later](LICENSE)
- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
