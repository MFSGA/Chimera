# Chimera

> 面向桌面用户的现代代理客户端，强调稳定、清晰、可维护的日常使用体验。

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-4c8bf5)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24c8db)](#)
[![Rust](https://img.shields.io/badge/Rust-stable-f74c00)](#)
[![React](https://img.shields.io/badge/React-19-61dafb)](#)
[![pnpm](https://img.shields.io/badge/pnpm-workspace-f69220)](#)

简体中文 | [English](README.en.md) | [Русский](README.ru.md) | [فارسی](README.fa.md)

---

## 项目简介

Chimera 是一款面向桌面用户的现代代理客户端，目标是在 Windows、macOS 与 Linux/NixOS 上提供稳定、清晰、可维护的代理使用体验。它基于 Tauri、Rust 与 React 构建，既保留了桌面应用应有的轻量与响应速度，也为代理核心、订阅配置、系统代理、服务模式和更新能力提供可靠支撑。

如果你希望有一个更专注于日常使用的代理管理工具，Chimera 的重点不是把界面做得复杂，而是把常用流程整理得更顺：导入订阅、切换节点、查看连接、调整规则、管理核心、处理更新，都应该尽量直接、可理解、少打扰。

## 为什么选择 Chimera

- 跨平台桌面体验：关注 Windows、macOS 与 Linux/NixOS 环境下的真实使用问题。
- 多核心支持方向：主要围绕 `chimera-client`、`clash-rs`、`mihomo` 等核心提供运行、切换、更新和状态管理能力。
- 全协议支持：覆盖常见代理协议组合，兼顾核心兼容性。
- 完整代理控制台：管理订阅与配置，查看代理组和节点，观察连接状态，调整运行时配置。
- 服务模式支持：适合追求更安全、更稳定后台运行、系统级代理能力和长期使用场景的用户。
- 日常排查友好：提供日志、连接、规则、配置目录和核心目录等入口，降低定位问题的成本。

## 当前支持协议

Chimera 当前重点关注常见代理组合与核心兼容，包括：

- `trojan + ws + tls`
- `reality + tcp`
- `hysteria2`
- `xhttp`

## 主要能力

- 订阅与 Profile 管理：支持导入、更新、删除、查看和编辑远程配置。
- 代理节点管理：支持查看代理组、选择节点，并在切换时同步刷新运行状态。
- 运行时配置：支持读取和调整 Clash 相关运行配置，方便排查和微调。
- 核心管理：支持查询核心状态、切换核心、重启 sidecar，并获取或更新核心版本。
- 系统集成：支持系统代理读取、应用深度链接、单实例启动、通知、对话框和进程管理。
- 服务模式：支持服务安装、卸载、启动、停止和重启。
- 更新能力：支持应用更新检查，并为核心版本更新保留独立管理链路。

## 常见开发命令

安装依赖：

```bash
pnpm install
```

准备核心信息：

```bash
pnpm generate:manifest
```

准备核心二进制资源并执行检查：

```bash
pnpm check
```

启动桌面应用：

```bash
pnpm dev:diff
```

构建应用：

```bash
pnpm build
```

## 贡献

- 有任何使用上的问题，或者代码实现上的问题，欢迎提 Issue 或 PR。
- 即使你是完全的新手，在查阅完 [wiki](https://mfsga.github.io/Proxy_WIKI/) 后，也可以继续有针对性地提问。
- 本项目也希望吸引更多开发者一起参与完善。

---

如果觉得有帮助，欢迎点个 star 🧡
