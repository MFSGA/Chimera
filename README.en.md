# Chimera

> A next-generation host networking manager: simple, reliable, and designed to work quietly in the background.

[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-4c8bf5)](#)
[![Tauri](https://img.shields.io/badge/Tauri-2.x-24c8db)](#)
[![Rust](https://img.shields.io/badge/Rust-stable-f74c00)](#)
[![React](https://img.shields.io/badge/React-19-61dafb)](#)
[![pnpm](https://img.shields.io/badge/pnpm-workspace-f69220)](#)

[简体中文](README.md) | English | [Русский](README.ru.md) | [فارسی](README.fa.md)

> **Chimera Proxy** — a Rust-native desktop client for the Chimera proxy ecosystem.

## Chimera Ecosystem

```text
Chimera Desktop
       ↕
Chimera Client (Rust client core)
       ↕ VLESS / REALITY
Chimera Proxy Server (Rust server core)
       ↕
AChimera · Proxy Wiki
```

- [Chimera Client](https://github.com/MFSGA/Chimera_Client) — Rust client core with Clash / Mihomo compatibility
- [Chimera Proxy Server](https://github.com/MFSGA/Chimera_Service) — Pure Rust Xray-compatible server core
- [AChimera](https://github.com/MFSGA/AChimera) — Android client
- [Proxy Wiki](https://mfsga.github.io/Proxy_WIKI/) — documentation and deployment notes

---

## Overview

Chimera's goal is to become the easiest-to-use next-generation host networking manager. It brings proxy cores, system network settings, and everyday connection management together on Windows, macOS, and Linux/NixOS, making networking reliable while handling routine work quietly in the background. Built with Tauri, Rust, and React, Chimera also keeps status, logs, and controls clear so users can understand and manage what is happening when they need to.

Chimera is designed to be AI-agent-friendly. The main UI, legacy UI, and agents should share the same application APIs and business capabilities instead of maintaining parallel configuration or runtime logic. The project plans to support local-first agent tools that can help inspect network state, explain issues, and assist with configuration on the user's machine. Mutating actions will remain under user control and provide results that can be checked. This capability is planned work; it does not mean every agent tool is available today.

## Protocol Compatibility Matrix

| Capability                        | Status       |
| --------------------------------- | ------------ |
| VLESS + REALITY                   | Stable       |
| XHTTP                             | Stable       |
| TUN / DNS / REST API              | Stable       |
| Clash / Mihomo core compatibility | Partial      |
| VMess / Vision                    | Experimental |

Chimera Desktop provides the desktop orchestration layer; exact protocol availability depends on the selected core and its version.

Chimera focuses on everyday network management without making the interface complicated. Importing subscriptions, switching nodes, checking connections, adjusting rules, managing cores, and handling updates should stay direct and reliable. The long-term goal is to take care of routine network management in the background and surface it clearly when the user needs to make a decision or inspect it.

## Why Choose Chimera

- Cross-platform desktop experience: focused on real-world issues across Windows, macOS, and Linux/NixOS.
- Multi-core support direction: centered around running, switching, updating, and managing cores such as `chimera-client`, `clash-rs`, and `mihomo`.
- Full protocol support: aimed at common proxy combinations and practical core compatibility.
- Complete proxy console: manage subscriptions and profiles, inspect proxy groups and nodes, observe connections, and adjust runtime configuration.
- Service mode support: suitable for users who want safer background execution, system-level proxy capabilities, and long-term stable usage.
- Troubleshooting-friendly: provides access to logs, connections, rules, config directories, and core directories to reduce the cost of locating problems.
- Agent-friendly direction: shared application APIs let the UI and agents use the same business capabilities, with local-first agent tools planned and user control preserved.

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
pnpm prepare:check
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
