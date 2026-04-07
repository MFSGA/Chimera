# Chimera

[简体中文](README.md) | English

Chimera is a modern desktop proxy client for users who want a stable, clear, and maintainable proxy experience on Windows, macOS, and Linux/NixOS. Built with Tauri, Rust, and React, it keeps the lightweight feel and responsiveness expected from a desktop app while providing reliable support for proxy cores, profiles, system proxy integration, service mode, and updates.

If you want a proxy management tool that focuses on daily use, Chimera is designed to keep the common workflow smooth instead of making the interface unnecessarily complex. Importing profiles, switching nodes, checking connections, adjusting rules, managing cores, and handling updates should all be direct, understandable, and unobtrusive.

## Why Choose Chimera

- Cross-platform desktop experience: focused on real-world usage across Windows, macOS, and Linux/NixOS.
- Multiple core support direction: built around running, switching, updating, and checking cores such as `chimera-client`, `clash-rs`, and `mihomo`.
- Complete proxy control center: manage subscriptions and profiles, inspect proxy groups and nodes, observe connection status, and adjust runtime configuration.
- Service mode support: suitable for users who need more stable background operation, system-level proxy capabilities, and long-term daily usage.
- Troubleshooting-friendly: provides access to logs, connections, rules, config directories, and core directories to reduce the cost of locating issues.

## Main Features

- Subscription and Profile management: import, update, delete, view, and edit remote profiles.
- Proxy node management: inspect proxy groups, select nodes, and refresh runtime state after switching.
- Runtime configuration: read and adjust Clash-related runtime configuration for debugging and tuning.
- Core management: check core status, switch cores, restart sidecars, and fetch or update core versions.
- System integration: system proxy access, deep links, single-instance behavior, notifications, dialogs, and process management.
- Service mode: install, uninstall, start, stop, and restart the service.
- Update support: check app updates and keep a separate management path for core version updates.

## Supported Direction

Chimera currently focuses on common proxy combinations and core compatibility, including:

- `trojan + ws + tls`
- `reality + tcp`
- `hysteria2`

The project will continue to iterate on cross-platform desktop experience, proxy core stability, TUN/service mode, configuration enhancement, and user workflows.

## User Experience

Chimera organizes the interface around practical usage scenarios:

- Dashboard: view the overall status.
- Proxies: select proxy nodes.
- Profiles: manage subscription profiles.
- Connections: observe connections.
- Rules and Providers: inspect rules and providers.
- Logs: troubleshoot runtime issues.
- Settings: adjust application behavior.

Regular users can focus on importing profiles and switching nodes. Advanced users can continue into connections, rules, runtime configuration, core versions, and service state.

## Technology

Chimera uses Tauri 2 for the desktop shell, Rust for backend capabilities and system integration, and React/Vite for the frontend. The frontend and backend communicate through Tauri IPC, with generated type bindings to reduce maintenance issues caused by interface drift.

Compared with traditional Electron apps, Tauri has a lighter runtime footprint. Compared with command-line tools, Chimera provides a more intuitive management entry point. It brings the proxy capabilities desktop users need into one application while keeping the underlying implementation clear and extensible.

## Development

Install dependencies:

```bash
pnpm install
```

Prepare core information:

```bash
pnpm generate:manifest
```

Prepare core binary resources and run checks:

```bash
pnpm check
```

Start the desktop app:

```bash
pnpm dev:diff
```

Build the app:

```bash
pnpm build
```

## Contributing

If you run into usage issues or implementation problems, issues and pull requests are welcome.

Even if you are completely new to computers, please read the wiki first and then ask targeted questions. I will take the time to answer them one by one.

Another major goal of this project is to attract more developers to participate.

## Summary

Chimera is a modern desktop proxy client focused on cross-platform support, core compatibility, and daily usability. It brings profile management, node switching, connection observation, service mode, core updates, and system integration into one clear workflow, making it suitable for desktop users who want a proxy tool they can rely on over time.

If you find it helpful, feel free to give it a star.
