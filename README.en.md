# Chimera

> A modern desktop proxy client focused on stable, clear, and maintainable daily use.

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-4c8bf5)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24c8db)](#)
[![Rust](https://img.shields.io/badge/Rust-stable-f74c00)](#)
[![React](https://img.shields.io/badge/React-19-61dafb)](#)
[![pnpm](https://img.shields.io/badge/pnpm-workspace-f69220)](#)

[简体中文](README.md) | English | [Русский](README.ru.md) | [فارسی](README.fa.md)

---

## Overview

Chimera is a modern desktop proxy client built for users who want a stable, clear, and maintainable experience on Windows, macOS, and Linux/NixOS. It is built with Tauri, Rust, and React, keeping the responsiveness and lightweight feel expected from a desktop app while providing reliable support for proxy cores, subscription profiles, system proxy integration, service mode, and update workflows.

If you want a tool focused on daily use, Chimera is not trying to make the interface complicated. The goal is to keep the common workflow smoother: importing subscriptions, switching nodes, checking connections, adjusting rules, managing cores, and handling updates should all stay direct, understandable, and low-friction.

## Why Choose Chimera

- Cross-platform desktop experience: focused on real-world issues across Windows, macOS, and Linux/NixOS.
- Multi-core support direction: centered around running, switching, updating, and managing cores such as `chimera-client`, `clash-rs`, and `mihomo`.
- Full protocol support: aimed at common proxy combinations and practical core compatibility.
- Complete proxy console: manage subscriptions and profiles, inspect proxy groups and nodes, observe connections, and adjust runtime configuration.
- Service mode support: suitable for users who want safer background execution, system-level proxy capabilities, and long-term stable usage.
- Troubleshooting-friendly: provides access to logs, connections, rules, config directories, and core directories to reduce the cost of locating problems.

## Currently Supported Protocols

Chimera currently focuses on common proxy combinations and core compatibility, including:

- `trojan + ws + tls`
- `reality + tcp`
- `hysteria2`
- `xhttp`

## Main Capabilities

- Subscription and profile management: import, update, delete, view, and edit remote profiles.
- Proxy node management: inspect proxy groups, select nodes, and refresh runtime state after switching.
- Runtime configuration: read and adjust Clash-related runtime configuration for debugging and fine-tuning.
- Core management: check core status, switch cores, restart the sidecar, and fetch or update core versions.
- System integration: read system proxy settings, support deep links, single-instance startup, notifications, dialogs, and process management.
- Service mode: install, uninstall, start, stop, and restart the service.
- Update support: check application updates and keep a dedicated management path for core version updates.

## Common Development Commands

Install dependencies:

```bash
pnpm install
```

Prepare core metadata:

```bash
pnpm generate:manifest
```

Prepare core binary resources and run checks:

```bash
pnpm check
```

Start the desktop application:

```bash
pnpm dev:diff
```

Build the application:

```bash
pnpm build
```

## Contributing

- If you encounter usage issues or implementation problems, issues and pull requests are welcome.
- Even if you are completely new, please read the [wiki](https://mfsga.github.io/Proxy_WIKI/) first and then ask focused questions.
- Another major goal of this project is to attract more developers to participate.

---

If it helps, feel free to give it a star 🧡
