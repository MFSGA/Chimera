# Frontend UI reference migration inventory — 2026-11-05

## Verified source baseline

- Chimera `master`: `fe7dd4e6c610acdb2fe05631cceafdabc109fa7a`.
- Read-only `ref` gitlink: `5ed3c019e13015fc5b06fc8d7069b330607d9036`.
- Runtime gitlink: `5b7717ee9ec6c962ae6923de81a4c5d28c94ced2`.
- Evidence: recursive GitHub trees for those exact commits; neither tree was truncated.
- Local `ref` and checkout status: **not checked** (DevSpace connections unavailable). No `ref` write.
- This is a **path inventory**, not source/behavior parity, UI walkthrough, or final acceptance.

## Shared UI TSX paths

Pinned ref has **52** `frontend/ui/src/**/*.tsx` files; Chimera has
**70**. All **52** ref-relative paths exist
in Chimera; **0** ref paths are absent. The **18**
Chimera-only paths need separate classification before any deletion.

Reference TSX paths (relative to `frontend/ui/src/`):

```text
accordion.tsx
action-swap-text.tsx
animated-item.tsx
animated-tabs.tsx
button.tsx
card.tsx
chip.tsx
circle.tsx
context-menu.tsx
date-time-field.tsx
dnd-grid/dnd-grid-item.tsx
dnd-grid/dnd-grid-root.tsx
dnd-grid/dnd-grid-source.tsx
dnd-grid/dnd-grid.tsx
drawer.tsx
dropdown-menu.tsx
empty-state.tsx
expressive-choice.tsx
fab-menu.tsx
glass-control-group.tsx
glass-surface.tsx
hct-color-picker.tsx
highlight-text.tsx
input.tsx
kbd.tsx
launcher-menu.tsx
lazy-image.tsx
material-shape.tsx
menu-surface.tsx
menubar.tsx
modal.tsx
navigation-list.tsx
navigation-sidebar.tsx
number-stepper.tsx
popover.tsx
progress.tsx
ripple.tsx
scroll-area.tsx
search-field.tsx
segmented-button.tsx
select.tsx
separator.tsx
shape-toggle.tsx
shared-tooltip.tsx
sidebar.tsx
slider-sidebar.tsx
slider.tsx
snackbar.tsx
sparkline.tsx
switch.tsx
text-marquee.tsx
tooltip.tsx
```

Chimera-only shared UI TSX paths:

```text
hooks/index.tsx
linear-progress.tsx
materialYou/components/baseCard/index.tsx
materialYou/components/baseDialog/index.tsx
materialYou/components/basePage/baseErrorBoundary.tsx
materialYou/components/basePage/header.tsx
materialYou/components/basePage/index.tsx
materialYou/components/expand/index.tsx
materialYou/components/expandMore/index.tsx
materialYou/components/floatingButton/index.tsx
materialYou/components/item/baseItem.tsx
materialYou/components/item/menuItem.tsx
materialYou/components/item/switchItem.tsx
materialYou/components/lazyImage/index.tsx
materialYou/components/loadingButton/index.tsx
materialYou/components/loadingSwitch/index.tsx
materialYou/components/sidePage/index.tsx
materialYou/components/sparkline/index.tsx
```

## Main interface path alignment

Mapping for path comparison only:
`ref/frontend/nyanpasu/src/pages/(main)/` →
`frontend/chimera/src/pages/(main)/`.

- Reference TSX paths: **263**.
- Chimera main TSX paths: **196**.
- Identically mapped paths in master: **130**.
- Reference paths without an identical Chimera path: **133**.
- Chimera-only/relocated main paths: **66**.

Missing **path correspondence**, not necessarily missing functionality
(relative to `frontend/chimera/src/pages/(main)/`):

```text
main/logs/_modules/file-logs.tsx
main/logs/_modules/log-file-menu.tsx
main/logs/_modules/log-json.tsx
main/logs/_modules/log-viewer.tsx
main/logs/_modules/logs-layout.tsx
main/profiles/$type/detail/_modules/profile-overview-card.tsx
main/profiles/_modules/create-profile-fields.tsx
main/profiles/_modules/create-profile-modal.tsx
main/profiles/_modules/page-toolbar.tsx
main/profiles/_modules/profiles-navigation-item.tsx
main/profiles/global/_modules/column.tsx
main/profiles/global/_modules/leave-dialog.tsx
main/profiles/global/_modules/library.tsx
main/profiles/global/_modules/pipeline.tsx
main/profiles/inspect/_modules/changed-fields.tsx
main/profiles/inspect/_modules/diff-viewer.tsx
main/profiles/inspect/_modules/find-bar.tsx
main/profiles/inspect/_modules/inspect-toolbar.tsx
main/profiles/inspect/_modules/lane-gutter.tsx
main/profiles/inspect/_modules/only-changed-switch.tsx
main/profiles/inspect/_modules/status-icon.tsx
main/profiles/inspect/_modules/step-detail.tsx
main/profiles/inspect/_modules/step-graph.tsx
main/profiles/inspect/_modules/step-panel-skeleton.tsx
main/profiles/inspect/_modules/step-rail.tsx
main/profiles/inspect/_modules/yaml-viewer.tsx
main/providers/_modules/use-proxies-provider-healthcheck.tsx
main/proxies/_modules/group-folder.tsx
main/proxies/_modules/group-list-actions.tsx
main/proxies/_modules/group-move-menu.tsx
main/proxies/_modules/group-nav-item.tsx
main/proxies/_modules/proxies-dnd-provider.tsx
main/proxies/_modules/proxies-layout-provider.tsx
main/proxies/group/$name.tsx
main/proxies/group/_modules/delay-test-button.tsx
main/proxies/group/_modules/group-header.tsx
main/proxies/group/_modules/group-meta.tsx
main/proxies/group/_modules/node-list-toolbar.tsx
main/proxies/group/_modules/proxy-node-button.tsx
main/rules/_modules/rule-row.tsx
main/settings/_modules/app-update-changelog.tsx
main/settings/_modules/changelog-markdown.tsx
main/settings/_modules/settings-slider-row.tsx
main/settings/about/_modules/help-links.tsx
main/settings/about/_modules/nyanpasu-version.tsx
main/settings/about/credits/_modules/credit-count-badge.tsx
main/settings/about/credits/_modules/credit-license-modal.tsx
main/settings/about/credits/_modules/credit-list.tsx
main/settings/about/credits/index.tsx
main/settings/about/index.tsx
main/settings/clash/ai/_modules/core-agent-skill-card.tsx
main/settings/clash/ai/index.tsx
main/settings/clash/control/_modules/control-channel-settings.tsx
main/settings/clash/control/_modules/core-secret-config.tsx
main/settings/clash/control/_modules/external-controller-config.tsx
main/settings/clash/control/_modules/port-strategy-selector.tsx
main/settings/clash/control/_modules/web-ui.tsx
main/settings/clash/control/index.tsx
main/settings/clash/core-manager/_modules/core-manager-card.tsx
main/settings/clash/core-manager/index.tsx
main/settings/clash/core/_modules/break-when-mode-change-switch.tsx
main/settings/clash/core/_modules/break-when-profile-change-switch.tsx
main/settings/clash/core/_modules/break-when-proxy-change-selector.tsx
main/settings/clash/core/_modules/latency-test-config.tsx
main/settings/clash/core/_modules/managed-guard-field-selector.tsx
main/settings/clash/core/index.tsx
main/settings/clash/network/_modules/allow-lan-switch.tsx
main/settings/clash/network/_modules/ipv6-switch.tsx
main/settings/clash/network/_modules/mips-congestion-controller-selector.tsx
main/settings/clash/network/_modules/proxy-port-config.tsx
main/settings/clash/network/_modules/tun-stack-selector.tsx
main/settings/clash/network/index.tsx
main/settings/clash/runtime/_modules/enable-builtin-enhanced-switch.tsx
main/settings/clash/runtime/_modules/expand-include-all-switch.tsx
main/settings/clash/runtime/_modules/field-filter-card.tsx
main/settings/clash/runtime/_modules/field-filter-switch.tsx
main/settings/clash/runtime/index.tsx
main/settings/debug/_modules/changelog-preview.tsx
main/settings/debug/_modules/configuration-status.tsx
main/settings/debug/_modules/crash-simulation.tsx
main/settings/debug/_modules/error-reporting-debug.tsx
main/settings/debug/_modules/http-server.tsx
main/settings/debug/_modules/mock-connections-switch.tsx
main/settings/debug/_modules/mock-traffic-switch.tsx
main/settings/nyanpasu/_modules/icon-set-store.tsx
main/settings/nyanpasu/_modules/tray-icon-config.tsx
main/settings/nyanpasu/_modules/tray-icon-editor.tsx
main/settings/nyanpasu/interface/_modules/custom-css-card.tsx
main/settings/nyanpasu/interface/_modules/language-selector.tsx
main/settings/nyanpasu/interface/_modules/theme-color-config.tsx
main/settings/nyanpasu/interface/_modules/theme-mode-selector.tsx
main/settings/nyanpasu/interface/index.tsx
main/settings/nyanpasu/logs/_modules/app-log-level-selector.tsx
main/settings/nyanpasu/logs/_modules/core-log-level-selector.tsx
main/settings/nyanpasu/logs/_modules/core-log-storage-config.tsx
main/settings/nyanpasu/logs/_modules/local-ip-probe-switch.tsx
main/settings/nyanpasu/logs/_modules/log-file-config.tsx
main/settings/nyanpasu/logs/_modules/traffic-retention-selector.tsx
main/settings/nyanpasu/logs/index.tsx
main/settings/nyanpasu/shortcuts/_modules/hotket-manager.tsx
main/settings/nyanpasu/shortcuts/index.tsx
main/settings/nyanpasu/tray/_modules/tray-menu-mode.tsx
main/settings/nyanpasu/tray/_modules/tray-proxies-selector.tsx
main/settings/nyanpasu/tray/_modules/window-close-behavior.tsx
main/settings/nyanpasu/tray/index.tsx
main/settings/nyanpasu/updates/_modules/about-card-header.tsx
main/settings/nyanpasu/updates/_modules/release-channel-preference.tsx
main/settings/nyanpasu/updates/_modules/update-controls.tsx
main/settings/nyanpasu/updates/_modules/update-preferences-card.tsx
main/settings/nyanpasu/updates/_modules/update-source-selector.tsx
main/settings/nyanpasu/updates/_modules/update-status-card.tsx
main/settings/nyanpasu/updates/index.tsx
main/settings/nyanpasu/widget/_modules/network-statistic-widget-icon-config.tsx
main/settings/nyanpasu/widget/_modules/network-statistic-widget-logo-follow-state-switch.tsx
main/settings/nyanpasu/widget/_modules/network-statistic-widget-logo-source-selector.tsx
main/settings/nyanpasu/widget/_modules/network-statistic-widget-selector.tsx
main/settings/nyanpasu/widget/index.tsx
main/settings/system/proxy/_modules/current-system-proxy.tsx
main/settings/system/proxy/_modules/proxy-bypass-config.tsx
main/settings/system/proxy/_modules/proxy-guard-config.tsx
main/settings/system/proxy/_modules/proxy-guard-switch.tsx
main/settings/system/proxy/_modules/proxy-mode-selector.tsx
main/settings/system/proxy/index.tsx
main/settings/system/service/_modules/auto-launch-switch.tsx
main/settings/system/service/_modules/dns-cache-button.tsx
main/settings/system/service/_modules/slient-launch-switch.tsx
main/settings/system/service/_modules/system-service-ctrl.tsx
main/settings/system/service/_modules/system-service-switch.tsx
main/settings/system/service/_modules/uwp-tools-button.tsx
main/settings/system/service/index.tsx
main/topology/$dimension/_modules/usage-list.tsx
main/topology/$dimension/index.tsx
main/topology/_modules/single-value-card.tsx
```

Additional or relocated Chimera paths (same relative root):

```text
main/assistant/route.tsx
main/connections/_modules/connections-content.tsx
main/profiles/$type/_modules/chain-profile-import.tsx
main/profiles/$type/_modules/sortable-profile-item.tsx
main/profiles/$type/detail/_modules/action-card.tsx
main/profiles/$type/detail/_modules/update-option-editor.tsx
main/profiles/_modules/error-item.tsx
main/profiles/_modules/transform-chain-column.tsx
main/proxies/$name.tsx
main/proxies/_modules/delay-chip.tsx
main/proxies/_modules/delay-test-button.tsx
main/proxies/_modules/group-header.tsx
main/proxies/_modules/proxy-node-button.tsx
main/settings/_modules/settings-card.tsx
main/settings/_modules/settings-title.tsx
main/settings/about/_modules/chimera-version.tsx
main/settings/chimera/_modules/break-when-mode-change-switch.tsx
main/settings/chimera/_modules/break-when-profile-change-switch.tsx
main/settings/chimera/_modules/break-when-proxy-change-switch.tsx
main/settings/chimera/_modules/enable-builtin-enhanced-switch.tsx
main/settings/chimera/_modules/hotket-manager.tsx
main/settings/chimera/_modules/lighten-animation-effects-switch.tsx
main/settings/chimera/_modules/log-file-config.tsx
main/settings/chimera/_modules/log-level-selector.tsx
main/settings/chimera/_modules/traffic-retention-selector.tsx
main/settings/chimera/_modules/tray-proxies-selector.tsx
main/settings/chimera/route.tsx
main/settings/clash/_modules/allow-lan-switch.tsx
main/settings/clash/_modules/core-manager-card.tsx
main/settings/clash/_modules/field-filter-card.tsx
main/settings/clash/_modules/field-filter-switch.tsx
main/settings/clash/_modules/ipv6-switch.tsx
main/settings/clash/_modules/log-level-selector.tsx
main/settings/clash/_modules/mixed-port-config.tsx
main/settings/clash/_modules/random-port-switch.tsx
main/settings/clash/_modules/tun-stack-selector.tsx
main/settings/clash/route.tsx
main/settings/system/_modules/auto-launch-switch.tsx
main/settings/system/_modules/current-system-proxy.tsx
main/settings/system/_modules/dns-cache-button.tsx
main/settings/system/_modules/proxy-bypass-config.tsx
main/settings/system/_modules/proxy-guard-config.tsx
main/settings/system/_modules/proxy-guard-switch.tsx
main/settings/system/_modules/proxy-mode-selector.tsx
main/settings/system/_modules/slient-launch-switch.tsx
main/settings/system/_modules/system-service-ctrl.tsx
main/settings/system/_modules/system-service-switch.tsx
main/settings/system/_modules/uwp-tools-button.tsx
main/settings/system/route.tsx
main/settings/user-interface/_modules/custom-css-card.tsx
main/settings/user-interface/_modules/language-selector.tsx
main/settings/user-interface/_modules/theme-color-config.tsx
main/settings/user-interface/_modules/theme-mode-selector.tsx
main/settings/user-interface/route.tsx
main/settings/web-ui/_modules/core-secret-config.tsx
main/settings/web-ui/_modules/external-controller-config.tsx
main/settings/web-ui/_modules/port-strategy-selector.tsx
main/settings/web-ui/_modules/web-ui.tsx
main/settings/web-ui/route.tsx
main/topology/_modules/ranking-modal.tsx
main/topology/_modules/ranking-view.tsx
main/topology/exit/index.tsx
main/topology/inbound/index.tsx
main/topology/process/index.tsx
main/topology/source/index.tsx
main/topology/target/index.tsx
```

## Supported legacy interface

Chimera legacy TSX files: **11**. The legacy presentation remains
supported; it has no required one-to-one ref path counterpart. Keep the same
application APIs and data ownership as the main UI.

```text
connections.tsx
dashboard.tsx
index.tsx
logs.tsx
profiles.tsx
providers.tsx
proxies.tsx
route.tsx
rules.tsx
settings.tsx
subscription-onboarding.tsx
```

## Differences requiring behavioral review

- `DIFF-UI-COMPONENT`: paths match, but component props, exports, animation,
  focus, keyboard handling, and production consumers still require comparison
  against the pinned ref and real interaction checks.
- `DIFF-MAIN-PATH`: 133 unmatched ref paths and
  66 Chimera-only paths may represent relocation, unfinished
  migration, or genuine product extensions; classify by functional call chain.
- `DIFF-LEGACY-ENTRY`: keep all 11 legacy files, existing
  navigation, and user data; adapt presentation to shared APIs without
  introducing parallel state or runtime logic.
- `DIFF-E2E-UPDATER`: a real update modal covered the legacy Settings entry
  during PR #596 desktop E2E. Preserve the update prompt; when present, close
  it through the visible control before testing sidebar navigation. Re-run the
  desktop suite and record whether the prompt was actually exercised.
- `DIFF-UI-HIGHLIGHT`: PR #607 aligns `HighlightText` props and four
  legacy connections consumers; do not count it in master until reviewed and
  merged. PR #608 proposes a shared UI test gate, also unmerged.

## Acceptance and next inventory update

A matching file path does not prove identical symbols, implementation, wiring,
or observable behavior. Review main routes and shared/legacy/agent consumers
by feature; test common APIs, regression paths, E2E, persistence and desktop
platforms as relevant. Track signed commits, checked CI, independent review,
and any accepted Chimera-only compatibility differences separately.

Update these counts on each new `master`/gitlink baseline and replace path
questions with grounded, per-feature status as code and test evidence arrives.

## 2026-11-06 continuation — current master and next feature slices

This section supersedes **only the old status and candidate priority**, not the
original file-count snapshot above. Actual base:
`master@9932f66ab7f2bc28e6bfa553edc0d502544faddb` (#612 is merged).
Reference UI sources were read from the local **pinned**
`ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`. The reference
checkout has a pre-existing modified nested `backend/nyanpasu-runtime`;
no reference content, submodules, or user files were changed.

### Implemented, and explicitly partial

- `ref/frontend/ui/src/search-field.tsx` →
  `frontend/ui/src/search-field.tsx`: Material Symbols search/close icons
  with existing input, clear button, disabled, readonly, glass, accessible-name
  and Escape contracts preserved; new SSR regression coverage.
- `ref/frontend/ui/src/navigation-sidebar.tsx` →
  `frontend/ui/src/navigation-sidebar.tsx`: use `./slider-sidebar` as in
  ref, with `SidebarProvider`, rail-width, label and content SSR assertions.
- `ref/frontend/nyanpasu/src/pages/(main)/main/logs/_modules/{consts.ts,log-level-badge.tsx}` →
  corresponding `frontend/chimera` files: seven log levels
  (`trace/debug/info/warning/error/fatal/unknown`), reference badge palette
  and warning display. `main/logs/route.tsx` gains Material Symbols icons
  and fixes level links to absolute `/main/logs`, preventing the known
  wrong-page navigation when a visible filter is clicked.
- `tauri-e2e/specs/logs-main-layout.e2e.ts`: verifies seven visible level
  controls, click-to-select Fatal, click-to-reset All, pathname, URL search,
  and selected state. Layout assertions intentionally describe the retained
  Chimera `SliderSidebar` implementation rather than claiming a copied
  ref sidebar.

**Not yet aligned:** ref's Logs route uses `NavigationSidebar`,
`ScrollArea`, `source=core|app|service` navigation and additional
`_modules/file-logs.tsx`, `log-viewer.tsx`, `log-file-menu.tsx`,
`logs-layout.tsx`. These require actual log-source hooks/typed client
mapping, not a visual-only copy. Keep Chimera's Main/Legacy log routes
working while that larger feature is migrated.

**Checks executed on the current source tree:** shared UI unit 16/16, E2E
harness unit 33/33, Legacy connections 9/9, TypeScript typecheck,
Prettier, Oxlint, Stylelint, frontend boundary validation and main UI
production build passed. The new Logs WebDriver spec **has not run as a
desktop app in this local worktree**; exact-head GitHub CI/E2E, independent
review and actual visible interaction evidence remain necessary.

### Remaining UI migration order

| Priority | Target and existing proposal | Required dependency and acceptance |
| --- | --- | --- |
| P0 | Complete Logs viewer, source chooser and file modules (partial #613) | Map actual app/core/service logs, preserve Legacy logs and state; verify live arrival, changing source/level and restart |
| P1 | Shared ScrollArea, Tooltip and Modal (#590/#591/#595) | Re-extract only component deltas from pinned ref; verify keyboard/focus, portal, resize, reduced motion and Legacy overlays |
| P1 | Slider, Sparkline and DnD Grid (#592/#593/#594) | Test pointer commit-on-release, keyboard adjustment, real drag and dashboard data invariants |
| P2 | Profile Global/Inspect/Create pages and modules | First map typed profile API, edit/undo and persisted data; validate migration/restart and both UIs |
| P2 | Proxy-group navigation and drag components | Preserve Core snapshots, latency/state updates and Legacy proxy selection; verify real user drag |
| P3 | Clash Network/Control and System Proxy/Service settings | Explicit host-effect boundaries; mock contracts and approved disposable VM tests for proxy/TUN/Service |

Draft #614 contains redundant content already on master from #612 plus the
remaining UI candidate and unsigned GitHub API commits. Old #589–#595 PRs are
also based on a stale mainline with unrelated backend/gitlink edits. **Do not**
merge any of those full history trees into current master. Extract the exact
ref-mapped functionality onto new signed commits, review its changed APIs and
run complete exact-SHA tests before routine merge. Preserve the original dirty
primary checkout, read-only ref, Legacy UI, Agent and v0.28.0 release tags.

## 2026-11-11：Logs / Settings 导航结构后续对齐（部分迁移）

- **基线**：Chimera `master@3813878613292ef35b27f72c89c788eceb766c67`；主仓 pin 的 ref 为 `59724e85c34abb6995c22d101c876cc06dc16576`，实际检出的 ref 是 `5a2e2baba3bdf2c3db3b9ffe1c941ac45dfa9ba5`。本批参考的 Logs `route.tsx`/`index.tsx`、Settings `route.tsx`/`settings-navigate.tsx` 在这两个 ref 提交之间没有源码差异；未更新 ref/gitlink。ref 内嵌 runtime 与根仓库 `pnpm-lock.yaml` 在开始时已脏，未触碰这两类修改。
- **目标和边界**：L2，主 UI 的 Logs 级别导航、搜索工具栏和 Settings 分组样式与 ref 收敛。非目标：完整日志持久化/分页 RPC、Core/App/Service 来源切换、上游细分 Settings 子路由、系统代理/TUN/Service 行为改变；Legacy UI 和 Agent 继续共用原接口。
- **Logs 路径映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/logs/route.tsx` → `frontend/chimera/src/pages/(main)/main/logs/route.tsx`：以 `NavigationSidebar`、`NavigationSidebarContent`、`NavigationSidebarItem`、`NavigationSidebarToggle`、`LogLevelsLayout` 取代旧手工 Sidebar，保留七级别 URL 及 E2E slot。`ref` 的 `logs/_modules/logs-layout.tsx`、`log-viewer.tsx` 所使用的 `DataPage`、浮动 toolbar、玻璃分组、`SearchField`/Clear 模式在 Chimera `logs/index.tsx` 部分复用。保留现有虚拟日志列表及 context menu 清除；新搜索实时过滤时间、级别与内容，显示项不会只高亮不筛选。
- **Logs 有意差异**：本地 `@chimera/interface::useClashLogs()` 从全局 WS provider 获取内存日志，上游 `@nyanpasu/query::useClashLogs(level,keyword,following)` 使用 CoreLogCursor/RPC 分页。没有新的 RPC/Service 契约就不能声称已支持历史加载、来源切换与文件日志，也不显示无法工作的来源按钮。这些仍是 P0 后续完整调用链任务。
- **Settings 路径映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/settings/route.tsx` → Chimera 同路径：使用 `@chimera/ui/sidebar`，对齐 `min-w-72`、`bg-surface-variant/50` 和 `flex-[2.5_1_0%]`。上游 `settings/_modules/settings-navigate.tsx` → Chimera 同路径：改为 `NavigationList` / `NavigationListGroup` / `NavigationListItem` / `NavigationListItemText`，用 ref 的 `useLinkProps` 保留包含子路由的活动状态与搜索参数无关的选中行为。Chimera 尚只有七个实际可用设置主分类，继续使用现有 URL、核心图标和品牌，不引入悬空上游子页面链接。
- **主/Legacy/Agent**：本批仅改 Main UI 和对应 E2E。Legacy UI 路由与组件未变；共用接口、后端、配置及内核生命周期不变。没有添加第二份业务状态源。
- **实际验证**：`pnpm typecheck` 全前端、E2E TS、`pnpm lint:frontend-boundaries`、改动文件 Oxlint/Prettier、`git diff --check`、`pnpm --filter=chimera-ui build` 均通过，构建仅有已知大 chunk 提示。共享 UI 原 `test:unit` 脚本在当前主机因包级 `tsx@4.22.4` 链接目标缺失而不能启动，改用仓库根可用的 `tsx@4.23.13` 对脚本列明的同一组 14 个测试文件执行，**43/43 passed**。此绕过不代表节点依赖已经修复。
- **E2E 验收状态**：现有注册于 `main`、`settings` suite 的 `logs-main-layout.e2e.ts` 与 `settings-main-layout.e2e.ts` 已更新为用户可见的导航点击、搜索清除、当前 URL/选中状态、分组布局及浮动工具栏契约。**尚未实际执行桌面 WebDriver，不能宣称交互/视觉已经验证**；需在隔离 Windows runner 运行并检查失败截图，不能以 UI 类型检查代替。Legacy UI、代理/网络、重启及跨平台验证未覆盖。
- **回滚**：仅恢复本批 4 个主 UI 源文件、2 个 E2E 文件及本段记录；保留开始时原有的 `pnpm-lock.yaml`、`ref/` 差异和所有其他工作。
- **下一批顺序**：先落实 Logs 的 CoreLogCursor、App/Service 文件日志查询和 LogRecord/文件选择器的完整前后端切片及实际 E2E；然后拆分 Settings 子路由并测试具体设置，再继续 Profiles → Proxies。每批先确认上游 commit、真实调用路径及现有客户端兼容，再做源码和测试迁移。

## 2026-11-11：Logs 记录查看器与跟随行为

- **基线与范围**：主仓库 master `3813878613292ef35b27f72c89c788eceb766c67`，参考子模块实际 checkout `5a2e2baba3bdf2c3db3b9ffe1c941ac45dfa9ba5`。本批在前一批 Logs / Settings UI 迁移基础上继续修改。L2：仅主 UI 日志显示和多语言资源，不改 MMDB、配置生成、内核、服务、系统代理、TUN、Legacy UI 或 Agent，不改原有 `ref/` 或 `pnpm-lock.yaml` 差异。
- **ref 源与落点**：`ref/frontend/nyanpasu/src/pages/(main)/main/logs/_modules/log-viewer.tsx` → `frontend/chimera/src/pages/(main)/main/logs/_modules/log-viewer.tsx`，`log-json.tsx` 同名迁移；将共享 LogRecord 详情/复制、按需 JSON 高亮、LogVirtualItem、LogClearButton、LogFollowButton 复用到现有 `logs/index.tsx`；增加 `utils/shiki.ts::highlightJson` 与五种 locale 字串。迁移仅替换 @nyanpasu/@chimera 包别名和现有 `@tauri-apps/plugin-clipboard-manager` 复制调用，保持 ref 的局部组件形状。原有 E2E 的 logs search/clear data-slot 保留。
- **临时桥接**：Chimera 目前仍由 `@chimera/interface::useClashLogs()` 的 WebSocket 内存快照提供 `type,time,payload`，没有 ref 的 `CoreLogCursor`、原始分页详情、App/Service 日志文件查询；没有捏造后端接口、日志源选项或服务端搜索。虚拟列表用 `WeakMap<ClashLog,number>` 提供同一个 WebSocket 对象的稳定 key（含定长历史滑窗），后续有真实游标时移除。旧流可能发出 `warn`，而导航为 `warning`，增加匹配适配及单测；其余级别沿用现有值。日志上滚停止自动跟随；主动恢复时滚到末尾，尊重 reduced motion；展示 dock 底部留白避免遮住最新日志。
- **验证**：新 `frontend/chimera/tests/logs-level-filter.test.ts` 2/2 通过；`pnpm typecheck`、`pnpm --filter=chimera-ui build`、改动文件 Oxlint/Prettier、`pnpm lint:frontend-boundaries`、共享 UI 对应 9 个单测均已在本轮执行并通过；最终源码已重新通过全前端类型检查、生产构建、Oxlint、Prettier 和新增回归测试。构建仍仅提示已有大 bundle chunk。**桌面 E2E 未运行**，因为 `backend/target/e2e/debug/chimera.exe` 当前不存在；现有 main suite 的 WebDriver 日志测试不能被静态检查代替。日志复制、JSON 展开、跟随滚动、虚拟尺寸和重启的真实交互仍待专用 E2E runner 验收。
- **回滚和下一步**：恢复本轮新增两个 logs `_modules` 文件、`logs/index.tsx` 本轮增量、`logs/_modules/consts.ts`、`utils/shiki.ts`、五个 locale 文件及新增单测，不动之前已经暂存的 Logs/Settings、lockfile、ref 和私有数据。下一批优先 ref 后端 Logs actor → RPC typed client → `useClashLogs` 游标/文件日志 UI → 真实桌面 E2E；然后完成 Settings 子页面、Profiles 和 Proxies。

## 2026-11-11：Profiles 分组导航与响应式布局迁移

- **基线 / 风险**：Chimera `master@3813878613292ef35b27f72c89c788eceb766c67`；上游 ref 实际 HEAD `5a2e2baba3bdf2c3db3b9ffe1c941ac45dfa9ba5`（主仓 pin `59724e85c34abb6995c22d101c876cc06dc16576`）。L2，主 UI 的 Profiles 导航、拖拽目标和响应式布局；不触碰 Profile 的持久化/API、Legacy UI、Agent、服务/TUN、MMDB，亦不改变已有的 `pnpm-lock.yaml` 或 `ref/` 工作区状态。
- **完整路径对齐**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/_modules/profiles-navigation-item.tsx` → Chimera 同名新增文件（`useLinkProps`、NavigationListItem/图标/文本，与 ref 相同的子路由活动选中语义）。`profiles-navigate.tsx` 引入 ref 的 Configs / Transforms / Pipeline 三组、按类型计数及图标色调，并保留真实可用路由。`consts.ts` 补 `PROFILE_TYPE_COLORS`。共用的 `@chimera/ui/navigation-list` 不重复实现。
- **必要差异**：Chimera 原有 `folder-nav-item.tsx` 的文件夹链接携带同一个 DnD ID 和 `useSortable` 传入 ref；文件夹移动、重命名与删除仍使用已有显式 `FolderActionsMenu`（上游主要通过右键菜单），避免取消已有可发现的操作和 Legacy 的共享行为。`profiles-all-nav-item` 和 `global-transform-chain-nav` E2E slot 仍在。新导航标记为 `profiles-navigation`；已有 `profiles-navigate` 属于卡片渐进列表，不能复用，否则会让 E2E 误认对象。
- **布局与语言**：`profiles/route.tsx` 改用 ref 已有 `@chimera/ui/sidebar`，调整背景、宽度比例（侧栏固定基数、内容 `flex-[2.5_1_0%]`），保留应用的 `validateSearch` 与 `subscribeUrl` 深链导入约束；`profiles/index.tsx` 对齐上游移动端页面不透明叠层。新增五语种的 Config / Transform / Pipeline、数量、Inspect 标签。
- **测试边界**：补充到现有 `tauri-e2e/specs/profiles-main-layout.e2e.ts` 的用户可见导航点击、URL、选中状态及文件夹菜单可见性断言（真实 E2E 待 runner）；前端 TypeScript、E2E TypeScript、Vite 生产构建及架构边界本轮均已通过。未以类型检查冒充真实拖拽、文件夹排序或桌面布局通过。无 E2E 专用二进制时需先按测试规范编译、隔离本机真实配置，再运行 WebDriver。
- **后续与回滚**：如异常仅恢复上述 Profiles 相关文件、新增的 `profiles-navigation-item.tsx`、对应五语种新增文案与 Profiles E2E 增量。下一阶段才开始完整的 Inspect 页面、上游变换链工具栏/排序、Logs CoreLogCursor 后端迁移和独立桌面 E2E；不一次性替换共享状态所有权。

## 2026-11-11：Profiles → Inspect 真实快照查看器

- **目标与边界**：用上游 Inspect 的步骤分组、字段搜索、变更字段、YAML 与 diff 结构替换 Chimera 的 "coming soon" 占位页，直接读取本地已有的只读 `inspect_runtime` / `inspect_runtime_node` 后端契约。L2：主 UI 路由及共享前端查询元数据，未修改 Rust/Service、核心生命周期、运行时配置生成逻辑、系统代理/TUN、Legacy UI 或 Agent；原有 `pnpm-lock.yaml` 和 `ref/` 工作区修改不动。
- **上游文件映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/profiles/inspect/_modules/inspect-steps.ts`、`text-find.ts`、`changed-fields.tsx` → Chimera 同名模块，保留上游阶段分组、组合分支缩进、稳定步骤键、changed-fields 筛选及 YAML 字段路径定位。上游 `inspect-toolbar.tsx`、`step-rail.tsx`、`step-detail.tsx` 以已有 `@chimera/interface` 和布局组件迁移。上游 `yaml-viewer.tsx` / `diff-viewer.tsx` 的虚拟滚动被适配到 `inspect-document.tsx`；本批不复制尚不存在的 YAML tokenizer / 双侧语法高亮基础设施。
- **数据与状态路径**：在 `frontend/interface/src/ipc/query-bindings.ts` 对已有生成命令声明 `queries.inspectRuntime()` 与 `queries.inspectRuntimeNode(snapshotId,nodeId)`，由 `unwrapQueryOptions` 统一解包 Result；绝不手改 `bindings.ts`。选择的 step 通过 `?step=` URL 保存，节点详情请求用 `snapshot_id` / `node_id` 做缓存键，快照更换后不会把旧详情当新数据；原后端拒绝过期 snapshot id 时显示真实错误，用户可刷新。
- **已有接口不同于最新 ref**：Chimera 暂无 ref `inspectAppliedRuntime` / `applied` / `effective_pending` / `final_id` / `max_log_level`，因此不显示无法工作的已应用快照切换、错误状态汇总或完整图视图。实际终点优先识别 `builtin_step: finalizing`，旧快照退化到最后一个叶节点，并由单测覆盖；步骤详情仍显示日志真实级别。用户需手动按刷新拉取新快照，本批没有新增独立事件订阅。
- **交互**：侧栏选择各阶段步骤，勾选“仅变更步骤”；输入 `mmdb` / `dns.nameserver` 搜索步骤变更字段，Enter / Shift+Enter 定位下一/上一匹配；点击字段胶囊进入对应输出 YAML 并跳到行；三个选项卡展示每步的差异、最终只读 YAML 和步骤日志。较长文档采用 TanStack Virtual 滚动渲染。无快照、请求错误、步骤无差异均有不同用户反馈；新增五语种 Inspect 文案。
- **测试实际结果**：`pnpm typecheck`（含前端、E2E TypeScript）、`pnpm --filter=chimera-ui build`、`pnpm lint:frontend-boundaries`、改动文件 Oxlint、`pnpm exec tsx --test frontend/chimera/tests/inspect-steps.test.ts frontend/chimera/tests/profile-layout.test.ts frontend/chimera/tests/resolve-profile-drop.test.ts` **25/25** 通过；Inspect 单测 4/4（终点识别、步骤筛选、字段定位、差异文本）。`tauri-e2e/specs/profiles-main-layout.e2e.ts` 加入可见点击侧栏 Inspect → 实际后端快照 vs 空状态 → 只读 YAML 视图的验收，但**真实桌面 WebDriver E2E 未执行**，不能声称交互完成验证；打包和真实核心运行时回归亦未覆盖。
- **回滚及下一阶段**：仅恢复本批 `profiles/inspect/` 新增文件和路由、`frontend/interface/src/ipc/query-bindings.ts` Inspect 两条查询、五语种 Inspect 文案与测试增量。下一批先通过隔离的桌面 runner 实际验证 Inspect 和 Profiles 链接，再按上游迁移 `inspect-graph`、细粒度 YAML 高亮、已应用快照的完整后端契约；之后继续 Profiles 变换库和 Proxies，不把尚未完成的能力说成已有。

## 2026-11-11：Inspect 后端和 effective-config 双快照（本地待提交）

- **源码基线及授权范围**：当前任务分支 `codex/ui-alignment-batches-20261111@806b1b9a`，按仓库固定基线 `ref@59724e85c34abb6995c22d101c876cc06dc16576` 对照 `backend/nyanpasu-core/src/client/runtime_inspection.rs`、`backend/nyanpasu-config/src/redact.rs` 和 Inspect 前端。L2 只读诊断变化，配置生成、用户配置写入、核心和服务状态所有权、系统代理/TUN/Legacy UI/Agent 不变。不动原有 `pnpm-lock.yaml`、`ref/` 工作区差异。固定 ref 的 `redact_config` 未覆盖代理 `uuid`/`auth` 与订阅 URL 凭据；本次为 Chimera 增加遮蔽并以回归用例覆盖，后续在上游修复时复核并收敛差异。有效配置差异仅导出字段路径，因此在原始值上计算路径，再对 YAML 与 diff 脱敏。
- **上游路径对应**：`RuntimeInspectionData.final_node` 和摘要 `final_id`、`max_log_level` 在 Chimera `backend/tauri/src/client/runtime_inspection.rs` 中对齐；新增 `inspect_applied_runtime` Tauri RPC/ Specta 生成客户端。进一步对齐 `EffectiveInspection`：在 RuntimeSnapshot 的一次性只读克隆中添加实际核心配置的虚拟 `CoreController` 子节点（不污染持久化图）。`backend/chimera-config/src/redact.rs::redact_config` 按上游源迁入，生成与核心视图都先脱敏再输出 YAML/Diff；添加 `changed_fields_between` 与 `BuiltinStepKind::CoreController`。
- **真实生效校验**：`CoreLifecyclePort::active_effective_config()` 默认不支持并返回 `None`；生产 LegacyCoreBridge 通过 CoreFacade 和已有 Local/Service ControlEndpoint 调用 `effective_config`，前后比对 CoreStatusSnapshot 的 host、instance_id、回执和完整 ConfigRevisionInfo。失败/过期/停止情况不生成有效 Core 节点；有活动回执但无法取回文档时显示 pending 状态。独立 effective snapshot_id 基于原检查 ID + revision/hash 生成；过期或切换核心后详情请求失败关闭。
- **Main UI 与共享缓存**：Inspect Toolbar 改为 Generated / Last applied 切换，显示 Applied / Pending / Not applied；运行时步骤保留原 `final_id`，额外 CoreController 展示在 Result 组。五种语言增加状态标签；`MutationProvider` 在配置、Profiles、运行时诊断事件时同时使两种只读快照缓存失效。接口类型由 Rust Specta 生成，不手工编辑 `bindings.ts`。Legacy 和 Agent 继续通过共享业务入口读取已存在的 API，不改其交互。
- **已运行验证**：此前记录的 Rust Inspect 6/6（含有效 Core 节点、凭据脱敏、MMDB 变化和过期 revision）、Runtime Snapshot 23/23、Executor 50/50、Specta 合约 1/1；前端 Inspect 5/5、MutationProvider 缓存 4/4；`pnpm typecheck`、Vite 生产构建、架构边界、Oxlint、Prettier、Rustfmt 定向检查通过。本次分批提交过程未重跑这些检查；新增的 `uuid`/`auth`/订阅 URL 脱敏及秘密字段变化路径回归用例也尚未执行。已加入用户可见的 E2E 切换与空状态断言，但 **Windows WebDriver 尚未执行**（缺少 `backend/target/e2e/debug/chimera.exe`），不把单测等同于真实服务端到端验证。
- **回滚 / 后续**：本批 Rust 只读 Inspect、Config Snapshot 脱敏与 CoreFacade 端点、绑定增量、Inspect Toolbar/步骤模型、MutationProvider/测试增量逐 hunk 恢复，不覆盖之前已提交的 Logs、Settings、Profiles UI，也不涉及用户 lockfile 或 ref。后续先在隔离 Windows Runner 验证真实本机/Service 实例切换、停止后快照、重启后旧 ID 拒绝与 MMDB 字段差异，再推进上游完整图形化 Inspect / Logs。


## 2026-11-12：Profiles Transform Chain / Library / Pipeline 迁移

- **基线与范围**：从已推送的 Chimera `fb5f40911b412c5eccc9ec2d7c36965cfedf5ccf` 开始，工作分支 `codex/ref-profile-transform-migration-20261112`。本轮按当前实际检出的参考源码 `ref@ea74290f98300a3b24e750e9eb59b91615f9c201` 迁移；父仓库记录的 ref gitlink 仍为 `59724e85c34abb6995c22d101c876cc06dc16576`，本轮没有提交或重置该 gitlink，也没有把 ref checkout 前移到 fetch 到的 `origin/main`。L2：只调整 Main Profiles UI、现有 Profile API 调用和对应 E2E；不改 Profile 持久化格式、Rust/IPC/Service、运行时转换执行、Legacy UI、Agent、核心生命周期、系统代理/TUN。另一个 `codex/ref-tun-auto-route-ownership-20261111` 工作树保持不动。
- **共享编辑模型**：上游 `profiles/_modules/use-transform-chain-draft.ts` / `transform-chain-list.tsx` / `transform-library.tsx` 迁移到 Chimera 同路径。Active/Inactive 双列表改为单条有序 enabled chain；Library 独立承担候选搜索、类型过滤、使用位置和加入/移除；保存继续调用现有 `setGlobalTransforms` / `replaceDefinition`，没有改变后端 UID 列表结构。draft 保留服务端顺序；删除的候选从 draft 中剔除，新发现的候选不会自动加入；重复 UID 禁止编辑；拖拽期间冻结候选快照，避免 refetch 卸载拖拽目标。新增纯模型 `transform-chain-model.ts` 与 5 个回归用例，覆盖已保存顺序、未知 UID 清理、候选刷新、增删及重排摘要。
- **全局 Pipeline**：新增 `global/_modules/{pipeline.tsx,column.tsx,library-column.tsx,builtin-steps.ts}`，展示 Source → Profile Scoped Transforms → Global Transforms → Chimera built-in stages，并从当前配置映射哪些 Scoped transforms 会被使用。内置阶段标签复用现有 Inspect tag 文案和 `BuiltinStepKind`，不新增 IPC；Stage 4 链接到 Chimera 实际存在的 Clash Settings 页面，而不是引用不存在的上游子路由。
- **Scoped transforms**：Profile Detail 增加可打开的 transforms side panel；宽布局为侧栏，窄布局使用 Add Transform Drawer。Panel 通过 URL `?panel=transforms` 保存打开状态，未应用的更改通过 LeaveDialog 拦截导航；原 ActionCard 的滚动定位改为打开该 Panel，避免面板迁移后留下失效的 DOM 滚动路径。Scoped 链仍通过现有 `replaceDefinition` 写回同一 profile config。
- **创建与编辑**：新增本地 Create Transform Modal，支持本地 JavaScript、Lua 和 Merge 模板；创建请求抽取到 `transform-profile-request.ts`，并复用于原类型页导入组件，避免模板/请求结构分叉。代码仍通过现有 editor window 编辑。**未迁移 ref CreateProfileModal 的 remote/external source 选择和远程 URL 创建流程**；这不是后端契约缺失，而是本轮有意限制在 Chimera 当前本地创建路径，后续需单独补齐并验证远程来源。
- **Inspect 与 Chimera 扩展**：每个已启用 transform 增加 `Trace in Inspect`，step key 使用已有 Inspect 语义 `global:<uid>:<index>` / `scoped:<profile_uid>:<uid>:<index>`，不增加第二套诊断数据。保留 Chimera 已有的运行时 revision、transform logs/failure 摘要，并保留明确的 Move Up/Down 键盘/辅助操作；它们是现有诊断 API 与可访问性能力，不是旧双列表回退路径。详细步骤和生成配置仍以 Inspect 为准。
- **删除旧实现**：移除旧 `transform-chain-column.tsx` 和 `transform-add-menu.tsx`；新的全局和 Scoped 页面共用同一个单链 draft、sortable chain 与 Transform Library，不保留旧 Active/Inactive 编辑器作为备用实现。
- **E2E 变更**：更新 `profile-transform-chain-ui.e2e.ts`，将 Scoped editor 的可见性契约改为点击 Profile action 后打开 wide panel；新增 Trace link 精确选中 Inspect scoped step 的断言，以及 dirty chain 离开时选择 Stay 后 draft 顺序保持的断言。既有 UID 顺序、add/remove/reorder、save/reset、runtime revision/log/failure 和外部运行时更新场景继续保留。
- **实际验证**：`transform-chain-model` 5/5、Profile layout/drop 21/21、Transparent Proxy interfaces 3/3、E2E harness unit 44/44；完整 `pnpm typecheck`（含 interface、UI、Chimera、E2E） 、`pnpm lint:frontend-boundaries`、全仓 Prettier、Oxlint 和 `pnpm --filter=chimera-ui build` 均通过。Oxlint 0 errors、1 条既有 warning 位于未修改的 `inspect-document.tsx`（`jump` / `jump.start` exhaustive-deps）。Vite 仅有既有 `__dirname` configLoader 和大 chunk 警告。`git diff --check` 通过。没有 Rust 变更，因此未运行 Cargo 测试。
- **E2E 验收边界**：桌面 WebDriver 尚未执行，`backend/target/e2e/debug/chimera` 不存在；本机 E2E harness 文档明确说明隔离配置/数据目录不能隔离系统网络状态，当前主机还有独立 `clash-rs` 系统服务进程，因此本轮没有启动真实桌面 E2E 应用。E2E TypeScript 检查通过不等于用户交互通过；需在隔离 runner/VM 构建匹配 E2E binary 后执行 `profile-transform-chain-ui.e2e.ts` 和 Profiles suite。
- **回滚与后续**：回滚本轮新增的 Transform Library、sortable、panel、create modal、pipeline/model/test 文件，以及本轮修改/删除的 Profiles、messages、package script、E2E 文件；不要覆盖本批开始时已存在的 `ref/` gitlink 差异，也不要操作另一个活动 worktree。下一步先在隔离桌面 runner 实际验证宽/窄 Panel、拖拽、LeaveDialog、Inspect trace 与 runtime diagnostics；随后补齐 remote/external transform creation/import parity，再按 ref 最新源码复核是否有后续 Pipeline / Library 变化。

## 2026-11-19：Rules 主界面按固定 ref 完整组件链迁移（本地待验收）

- **目标/基线/风险**：在 `master@f9f68c473f3a40e539d4a4cf7b5aada2b8fe07d2` 上迁移主界面 Rules；固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 干净且未前移。L2（共享只读前端查询接口）。不修改 Rust/IPC、运行时/Service、代理/TUN、存储格式、Legacy Rules、Agent、CI、子模块指针，也不修改另一活动工作树。UI 和查询展示的更改可按本条映射回滚。
- **ref 主页面**：`ref/frontend/nyanpasu/src/pages/(main)/main/rules/{index.tsx,route.tsx,_modules/rank-by-value.ts,_modules/rule-row.tsx,_modules/use-rule-stats.ts}` → `frontend/chimera/src/pages/(main)/main/rules/` 同路径。按 ref 复制 `NavigationSidebar`、`DataPage`、`DataPageToolbar`、`SearchField`、两个 ScrollArea、固定高度虚拟规则行、实时连接/速度/累计流量、排序、URL 搜索与跨页面 Connections/Topology 跳转；删除主 UI 的旧 `react-table` 布局，Legacy UI 表格维持不变。
- **必要兼容边界**：Chimera 的 `@chimera/interface::useClashProxies` 返回的是 Clash 展示模型，没有上游 `iconConfigured` 字段，因而规则行代理组图标复用已经存在且带该字段的 `useNativeProxies`。在 `frontend/interface/src/ipc/use-profile.ts` 按 `ref/frontend/query/src/ipc/use-profile.ts::useCurrentProfileUid` 新增只读 select，使用相同 `queries.getProfiles` 缓存键；`use-traffic.ts::useTrafficUsageByKeys` 允许传入 ref 的 `refetchInterval` 选项，仍走已有 `queryTrafficUsageByKeys`，没有第二份统计实现。品牌替换仅限 `@nyanpasu` → `@chimera` 模块/产品标识；保留其余上游算法及类型、命名与文件划分。
- **语言和测试**：五个 locale 从 ref 各补七项 Rules 新增文案，不删旧文案。移植 `ref/frontend/nyanpasu/tests/rule-rank.test.ts` 至 `frontend/chimera/tests/rule-rank.test.ts`，仅将 Vitest 断言适配项目原有 Node `test`/tsx 执行器并接入 `chimera-ui test:traffic`。调整已注册的 `tauri-e2e/specs/rules-main-layout.e2e.ts`，基于用户 WebDriver 导航、输入、清除、点击排序表头和侧栏展开/收起，以及 DataPage 真实布局状态；保留代理图标 E2E 和 Legacy 入口。
- **已执行**：`pnpm --filter chimera-ui test:traffic` **18/18 passed**（含新增排序 2/2）；`pnpm typecheck:e2e` 通过；变更文件 Prettier 和 `git diff --check` 通过。对主界面 `tsc` 已执行，**43** 项错误全部来自忽略生成目录 `src/paraglide/messages` 的陈旧消息定义：五语源 JSON 新增键存在，旧生成器产物也缺少多个此前已添加的 Dashboard/Proxies keys。`pnpm --filter chimera-ui generate:i18n` 及直接 SDK `loadProjectFromDirectory` 在本机均长时间挂起后人工中断，非产品编译成功；另一次 `tsc` 分类确认除生成文件缺键外 **0** 项非 Paraglide 类型错误。完整 Chimera 前端类型检查/生产构建仍待能正常再生 Paraglide 的环境复验。
- **未覆盖**：桌面 WebDriver E2E 未实际运行，截图/真实滚动/排序/跨页面跳转没有用户可见实机证据，不能将 E2E 类型检查或 mock 排序代替桌面验收。Rules 与 Connections/Topology 共用的统计/导航需由后续隔离桌面 runner 验证；`ref` 中潜在的后续 UI 修改需重新固定基线后单独审核。当前未创建提交、未推送。

## 2026-11-19：Proxies 组详情头部、搜索及节点交互继续按 ref 对齐（本地待验收）

- **基线/范围**：Chimera `master@822e04c5`，同步远端四个 Runtime/Reconciler 提交后恢复上一批未提交 Rules UI；固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。风险 L2，改 Main Proxies 页面、相关展示 hook、locale、单元/E2E；不改 Rust、IPC/Service、TUN、Legacy UI、Agent、锁文件及子模块 SHA。不修改另一工作树，之前 Rules 改动完整保留；本次只做本地代码，不提交/推送。
- **上游映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/proxies/group/_modules/{group-header,group-meta,node-list-toolbar}.tsx` → Chimera `frontend/chimera/src/pages/(main)/main/proxies/_modules/`（保留 Chimera 现有 `/main/proxies/$name` 路由和 `@/components/ui/sidebar` 兼容入口）；主 `group/$name.tsx` 上游显示部分迁入 Chimera `$name.tsx`。上游的 GroupMeta 宽度测量/窄时状态-速度切换、`SearchNodesButton` 动画、`NodeSearchOverlay`、窄屏覆盖式搜索、搜索历史路由同步和滚动行容器复用已有页面实现。`proxy-node-button.tsx` 对齐上游 hover 状态层、圆形 feature chip、单词搜索高亮及固定节点文案，不影响现有选中和延迟动作 API。
- **共用数据与必要适配**：上游 `useGroupTrafficSpeed` 消费 `ClashConnectionsSummary.memberRates`，Chimera 当前共享 API 未提供该字段且旧实现错误地只取第一条连接的累计流量显示为速率。`_modules/group-traffic-speed.ts::latestGroupTrafficSpeed` 保留上游功能名称，以本地已存在的 `useClashConnectionDetails()` 最新帧中每连接 `downloadSpeed/uploadSpeed` 对经过本组的所有连接求和；查询仅触发已有公共 provider，不新增私有 WS/IPC 或 Core 状态源。同一连接包含重复组名只计算一次。上游 `memberState/resolveChain` 对应的完整原生组解析尚未迁入，当前组状态从 `useNativeProxies` 的已有 `memberDelay` 和嵌套组 `now` 链计算，遇环停止。这是待收敛的展示适配，不声称完全等同于上游 `memberRates` 或所有嵌套组结果。
- **可验收状态**：新增 `proxies_group_available`、`proxies_group_fixed_label`、`proxies_group_clear_fixed_button`、`proxies_node_view_save_failed` 五语文案（从 ref 对应 locale 复制，保留原文案）。`frontend/chimera/tests/group-traffic-speed.test.ts` 3 个用例验证只读取最后帧、多个连接/嵌套链聚合及空帧归零；`test:proxies` 同时跑已有节点排序/过滤及文件夹拖放纯函数测试。已注册的 `tauri-e2e/specs/proxies-node-main.e2e.ts` 增加状态展示、窄屏覆盖式搜索/关闭验证，并将旧的宽屏搜索关闭按钮假设改为 SearchField 用户可见清空操作。
- **验证与边界**：本地 `test:proxies` **17/17 passed**，本地化测试 **6/6 passed**；E2E TypeScript、前端 IPC 边界、目标文件 Oxlint（0 error/0 warning）、Prettier 和 `git diff --check` 均通过。Chimera 全体 `tsc` 最终复核报告 **48** 项错误，全部是上轮已定位的陈旧 `.gitignore` Paraglide 生成结果中缺少源 JSON 已有消息导出；没有新增非 Paraglide TypeScript 错误。Inlang 项目加载/生成器在本机挂起，前端完整构建和真实桌面 WebDriver/E2E（包括触发样式和搜索动画）**仍未验证通过**。此前的旧基线并不能证明新代码 UI 实际可用。
- **后续**：优先在可生成 Paraglide 的环境复跑全体前端 TypeScript/Vite build，再使用隔离桌面运行器验证搜索展开/关闭、窄屏点击命中、实时组速率变化、代理节点高亮、拖拽及 Legacy 保持可用；最后按 ref 迁移原生等效的 member state/leaf、DelayHistory、权限边界和完整分组路由/节点详情，使差异进一步收敛。回滚只涉及本段列出的 Proxies 文件、新增四组五语文案、新增单测和 E2E 对应增量，不撤销前一批 Rules 变更。

## 2026-11-19：Proxies 嵌套节点状态、DelayHistory 与 Paraglide 构建复验（本地未提交）

- **基线/边界**：在 Chimera `master@822e04c5`、固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` 上继续两批未提交的 Rules/Proxies Main UI 迁移。只改 `frontend/interface/src/ipc/` 的只读 Native Proxies 投影、Main Proxies 纯状态和展示组件、五语 JSON、回归/E2E；不改 Rust、Service、IPC 生成合约、系统代理/TUN、Legacy MUI 组件、Agent、子模块、其他工作树及远程主线。
- **上游算法逐项对齐**：`ref/frontend/query/src/ipc/proxy-delay.ts` → `frontend/interface/src/ipc/native-proxy-delay.ts`：`collectDelayKeys`、`groupDelayKey`、`nodeDelayHistory`、`latestDelay`、`resolveProxyChain`、`nativeMemberState` 和 `nativeMemberDelay`；后两个保留 Chimera 前缀以免与 Legacy Query 导出重名。原生节点 `all` 可为 null：有 `all` 的嵌套代理组沿 `now` 递归找到叶节点；使用**叶节点直接父组的 opaque `delayKey`** 取历史；遇断链、循环、无选中返回无延迟；0=失败、undefined=未测试；共享历史数组保持引用稳定。隐藏组从可见分组列表过滤时，`selectVisibleNativeProxies` 额外保留完整 `delayKeys`，不泄露真实探测 URL，不建立第二套数据源。
- **节点列表/性能与视图**：`proxies/_modules/node-list.ts` 的排序/隐藏不可用使用统一的 `nativeMemberDelay`，允许按上游 `delayOf` 重用单次 `memberStates` 快照。页面 `$name.tsx` 一次计算所有组成员的 leaf/历史/可用状态并使用 `resolveProxyChain` 构造 GroupStatus，单个虚拟行显示 `leaf` 与对应延迟。按 ref 用 `useDeferredValue(true,false)` 在首帧后再挂载虚拟卡片，避免路由切换同步生成大量子节点。
- **DelayHistory 源码**：`ref/frontend/nyanpasu/src/components/proxies/delay-history.tsx` → `frontend/chimera/src/components/proxies/delay-history.tsx`，迁入上游 `SharedTooltip`、`SharedTooltipTrigger`、历史前 10 条、locale 时间、失败及空状态。一个列表共享一个 Tooltip，不在每个节点挂 Popper。由于 Chimera `components/proxies/delay-chip.tsx` 仍由 Legacy MUI 使用，其源码保留不动；把 Main UI 原轻量 `delay-chip.tsx` 下沉到 `components/proxies/proxy-delay-chip.tsx` 并将旧 Main UI 路径改为导出兼容，避免共享组件反向依赖路由模块。五语新增历史标题/空态/数量标签，失败文案重用已有键。
- **测试与局限**：新增 `frontend/chimera/tests/proxies-member-state.test.ts` 8 项 ref 风格回归；`test:proxies` 扩至 **25/25**；`test:localization` **6/6**；`pnpm lint:frontend-boundaries`、E2E TypeScript、Oxlint（0 error / 0 warning）、`pnpm typecheck` 全仓和 `pnpm --filter chimera-ui build` 均通过。E2E 文件新增节点 hover 后共享历史 Tooltip 断言，**桌面 WebDriver 未实际运行**，不能宣称真实视觉/交互验收通过。
- **语言生成问题已定位并在本机绕过**：根因是 Inlang 第二个 CDN 插件请求已返回 HTTP 200、但正文读取长时间停滞，使 SDK `network-first` 缓存逻辑不进入失败回退分支。仅在本机 `/tmp/chimera-inlang-fetch-diagnostic.mjs` 用 Node `--import` 为该下载设置 2.8s 超时，使请求抛 AbortError 后由 SDK 自己回退到 `project.inlang/cache/plugins/`；Paraglide 成功生成，原 48 项陈旧代码缺失报错消失。**未更改项目生成脚本或手写忽略生成代码**；其他没有本地缓存且 CDN 正文同样卡住的机器仍需独立修复。Vite 只剩已知 `__dirname` configLoader 和大 chunk 警告。
- **待完成**：真实隔离桌面 E2E 验证嵌套组叶节点、历史 Tooltip/键盘焦点和大量节点的滚动性能；随后按 ref 核对权限 `proxy:write`、Group delay 历史图和 Detail 页路由结构、状态加载/错误边界。所有当前代码仍属未提交的 UI 迁移批次，未推送。

- **2026-11-19 本批末尾远程快进**：检查发现 origin/master 从 `822e04c5` 前移四个 Runtime/Reconciler 相关提交到 `1bff62fe`，文件变更全部位于 `backend/tauri/` 和 `docs/design/`，与本批 UI 路径无交集；将含未跟踪文件的 UI 工作区临时 stash 后以 `git merge --ff-only origin/master` 快进，成功无冲突还原当前 Rules/Proxies 所有文件。主仓库和 origin/master 此时均在 `1bff62fe`；仍未提交、未推送这两批 UI 源码。

## 2026-11-19：Proxies 操作反馈、能力边界与桌面交互验收（本地待提交）

- **目的与基线**：在干净的固定 `ref@c4b35e04`、Chimera `master@1bff62fe` 的基础上继续未提交的 Rules/Proxies Main UI 迁移；只修改主界面 Proxies 的本地化操作反馈、E2E 交互和五语消息，不变更其他 Agent、Legacy、Service、Runtime/TUN、IPC 生成合约或 ref 指针。风险分级 L1（前端通知与测试），独立项目权限 API 迁移属于未来单独审批的高风险跨层工作。
- **上游能力边界确认**：`ref/frontend/nyanpasu/src/components/auth/capabilities.tsx` 使用 `AuthBootstrapGate` 启动期间获得的 `rpcDescribe()` `permissions['proxy']` 授权快照，控制 `proxy:write`，并同时保留 `ProxyGroup.capabilities.select/clearFixed`。Chimera `backend/tauri/src/client/clash_api.rs` 明确记录现有桌面 IPC **没有同等的 per-scope permissions 描述接口**，主 UI 只有原生代理组 `capabilities.select` / `clearFixed`，因此不能以硬编码桌面恒为可写或伪造 `useCapabilities()` 宣称完成 `proxy:write` 迁移。保持现有真实能力检查，延迟测试仍由原生 mutation 在后端验证可用性和组成员；该跨层权限描述、动态只读 UI 与 read-only banner 留作单独的架构迁移。
- **ref 操作回执与用户反馈**：`ref/frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx` 中的 `proxies_select_failed_message`、`proxies_clear_fixed_failed_message` 两项失败消息按原文复制到 Chimera 的 en/ja/ru/zh-cn/zh-tw。当前主 UI 使用已存在的 `Notice.error(..., 5000)`（Chimera 的临时 toast 兼容边界）在选中节点或清除固定选择被后端拒绝时显示本地化错误，代替此前未处理的选择拒绝和原始后端错误文本弹窗；保存节点视图失败改用已引入的 `proxies_node_view_save_failed`。不将“返回未确认”错误视为已成功，不改变 Native Proxy Query 缓存或乐观提交语义。
- **真实交互 E2E 的新增契约**：在既有已注册 `tauri-e2e/specs/proxies-node-main.e2e.ts` 的专用合成 SOCKS5 Profile 中，新增 WebDriver `click()` 选择另一节点、检查可见活动卡片的 `data-active`、并用 WebDriver 点击恢复先前节点的用例。`proxy-node-button.tsx` 添加稳定 `data-slot="proxy-node-card"` 与 `data-proxy-name` 选择器，允许外层测试定位真实控件，而非直接调用 IPC 来通过测试。仍需在含 WebDriver Feature 的隔离桌面 runner 中执行，只有用例实际跑过且通过后方可宣称验收。
- **隔离执行准备**：这台 NixOS 有 Xvfb 和 `dbus-run-session`，通过 `unshare --user --map-root-user --net` 无权限测试确认可以创建独立网络命名空间，普通 `backend/target/debug/chimera` **未包含 E2E WebDriver**，测试专用 `backend/target/e2e/debug/chimera` 起初不存在。仅在独立网络命名空间、独立 HOME、独立 XDG 运行时目录和 Xvfb 中尝试真实 E2E，不让测试应用读取主机个人配置或使用真实网络；禁止执行 Windows 系统/TUN 破坏性 suite。即使有 E2E 结果也不声称覆盖真正跨进程权限授权（当前无描述接口）。
- **回滚与后续**：本轮只需撤回 Proxies 页面中新添的 UI 提示/节点卡片测试选择器、五语言新增文案及 E2E 测试插入部分，不撤销上一批 Rules、原生嵌套组、DelayHistory 迁移。下轮再处理 `rpcDescribe` 分级授权描述、只读范围和错误 UI，必须先取得明确跨层安全模型设计授权并同步 Backend/Agent 的同源权限。

- **2026-11-19 WebKitGTK 真实桌面验收（本轮新结果）**：使用独立 `unshare --user --map-root-user --net --pid --fork --kill-child --mount-proc`、已启动的隔离 loopback、虚拟 Xvfb `:85`、独立 D-Bus、临时 HOME/XDG 和 E2E-only 配置，不接入主机 TUN/Service/Proxy 的真实网络。`pnpm e2e:tauri:build:rust` 通过，产物 `backend/target/e2e/debug/chimera`；Rules `rules-main-layout.e2e.ts` **4/4** 真正点击/输入/排序/侧栏切换通过（本地临时日志 `...oHjFce/wdio.log`）。Proxies `proxies-main-layout.e2e.ts` 首次失败时立即捕获并实际打开测试截图：可见正确的空态容器、空态文案与跳转按钮，而测试请求外窗 `1240×638` 只产生内窗 `1189×611`，第 86 行 `innerWidth >= 1200` 断言先行失败；调整前后两次 `browser.setWindowSize` 为 `1270×700`，**保留全部原断言**后同一真实桌面套件 **2/2** 通过，包含由可见按钮跳到 Profiles。依赖实际 Core 的 `proxies-node-main.e2e.ts` 在 `before all` 调用 `activate_profile` 后被 `appliedValue` 安全断言拦截，**节点 UI 点击、延迟历史及搜索未执行**。真实 app 诊断日志证实隔离网络的 Service probe 为 `Unknown` / status-query exit 1，Runtime 拒绝并行启动第二个 Core（`the Service host (Unknown) may still hold a Core`），不代表 Proxies UI 用例断言失败；没有绕过 `appliedValue` 或缩小校验。此隔离会话还出现系统辅助服务 glibc 2.42/2.43 不兼容警告，但不阻碍已通过的 Rules/Proxies 空态测试。

## 2026-11-19：隔离 E2E 的真实 Core 启动与 Proxies 交互闭环（本地未提交）

- **最新复验基线**：主仓库 `master = origin/master = bc97e3e3`（从 `1bff62fe` 快进，最新两个提交仅修改 Runtime/CoreLifecycle 和设计文档；UI 工作区临时 stash 后原样恢复，无冲突），固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。所有更改仍限 Main UI、只读 IPC projection、E2E/测试与文档，不改 Rust、Service、安全模型、Legacy UI、Agent、子模块与远端。
- **恢复真实 Service 探测而不绕过安全门禁**：上一批的 `unshare --user --map-root-user --net` 把内层 uid 变为 0；`chimera-service status --json` 内部使用 `service_manager::ServiceManager::native()`，因此会误入不适合此无系统管理器环境的探测路径，CLI exit 1 被 `ServiceActor` 严格识别为 `Unknown`。本轮仅把**本机临时 E2E runner** 改为 `unshare --user --map-current-user --keep-caps --net --pid --fork --kill-child --mount-proc`，可用 rootless namespace 能力启用自己命名空间的 loopback，仍以宿主 uid 1000 运行桌面 app；真实同目录 `chimera-service status --json` 输出 `not_installed`，`ServiceActor` 启动 `NotInstalled` 并正常协调 `Local`，不制作伪造状态、不修改主机 Service/TUN、网络或权限规则。Xvfb、D-Bus、HOME/XDG、配置、数据、日志均为运行 ID 独立目录。
- **正确处理 ref 首份 Config 的自动激活**：`ref/backend/nyanpasu-core/src/client/mod.rs::create_profile` 在 `current=None` 时执行 `set_current_if_none`；`ref/backend/nyanpasu-core/src/client/profiles.rs::set_current_commits_without_runtime_work_when_reselected` 明确同 UID 再选只产生文档版本提交。此时应用 Core 尚未运行，实际 `create_profile` 两张 receipt 均为 `unchanged`。`tauri-e2e/specs/proxies-node-main.e2e.ts` 修复准备代码：创建前登记恢复选中状态的责任；创建后读取 `profiles.current`，仅在未自动选中时显式激活；已自动选中且未应用时仅接受 `unchanged/saved_inactive`，通过正式 `restart_sidecar` 让已选配置经真实 CoreLifecycle 协调，随后强制等待 `get_core_status === Running`，再用 `get_proxies` 证实合成组及节点确实加载。**不允许仅靠创建成功或伪造 applied receipt 通过**，不使用 mock Core、系统代理或真实公网。
- **修复 E2E 的真实 UI 定位/交互问题**：`ref GroupMeta` 为自适应排版同时渲染 `aria-hidden` 的隐藏尺寸测量行和显示行；旧用例用全局第一个 `proxies-group-status`，故真实截图中状态可见却因选中隐藏副本超时。测试改用 `[data-slot="proxies-group-meta"] > div:not([aria-hidden])` 下的可见状态。节点选择 E2E 初次失败是 Rust E2E binary 内嵌的旧 HTML/JS（Rust 编译时间早于最后一次 Vite 构建），缺少已在新源码中的 `data-slot="proxy-node-card"`；本轮**严格先执行完整前端构建，再编译 E2E Rust**，不放宽节点状态断言。代理组拖拽按 ref `FOLDER_ITEM_DRAG_SENSORS` 的 `Distance(8)` 先在原节点内产生实际 W3C pointer 移动，再移到目标标题并提交；失败时提前保存未清理的截图/DOM，防止 after hook 清理后丢失现场。W3C WebDriver 的 embedded driver 发送的是不可信合成指针事件，不冒称实体鼠标硬件输入。
- **2026-11-19 最新执行证据**：`pnpm e2e:tauri:build`（完整 UI 构建随后 Rust `e2e` feature）退出 0；隔离桌面 `proxies-node-main.e2e.ts` **7/7 通过**，包括节点类型+UDP、共享 DelayHistory、点击切换并恢复、组延迟按钮、搜索、窄屏搜索、真实 W3C pointer 拖拽后 Profile 布局持久化和恢复。同一编译构建下另独立执行 `rules-main-layout.e2e.ts` **4/4** 与 `proxies-main-layout.e2e.ts` **2/2**：三组 **13/13**。完整端到端通过的边界是隔离真实 Core + embedded WebKitGTK 的这些交互；没有验证生产 TUN、网络实际传输、`proxy:write` 远端授权或真实鼠标硬件，WebKitGTK 的辅助系统 DBus 服务 glibc 2.42/2.43 不匹配警告仍存在但未影响目标断言。
- **保留的迁移边界**：`proxy:write` 需要 RPC describe 式权限元数据，Chimera 桌面现有 IPC 无该协议；完整跨层授权和 `ReadOnlyScope` 仍另案设计。需要后续验证 Windows/macOS 桌面真实平台表现、动态探测性能、键盘焦点链、大量节点的滚动基线。前端本地使用临时 `NODE_OPTIONS=--import=/tmp/chimera-inlang-fetch-diagnostic.mjs` 解决 Inlang CDN 正文悬挂后的缓存回退，仅本机有效，不修改产品构建脚本。

- **2026-11-19 最终远程快进和再次实机复验**：随后 `origin/master` 新增两个仅触及 `backend/tauri/` 与 `docs/design/` 的 Runtime/Product Recovery 提交 `02fd36a0`、`ac0d4e04`。在校验路径与 UI 工作区完全无交叉后，临时 stash 含未跟踪的 UI 内容、`git merge --ff-only origin/master` 到 **`ac0d4e04`**，无冲突恢复并删除该临时 stash（其它 stash 不动）；`ref` 与 Runtime 子模块仍在原 SHA。使用 `NODE_OPTIONS=--import=/tmp/chimera-inlang-fetch-diagnostic.mjs` 执行完整 `pnpm e2e:tauri:build`（先最新前端再 Rust E2E）成功。按同样的真实隔离 user/net/pid+Xvfb+WebKitGTK 方案，重新执行 `proxies-node-main.e2e.ts` **7/7**（含真实 Core、节点选中、拖拽持久化和清理）、`rules-main-layout.e2e.ts` **4/4**、`proxies-main-layout.e2e.ts` **2/2**，精确最新 SHA 合计 **13/13**，三个独立测试程序都退出 0。前一轮 `bc97e3e3` 的 13/13 是单独的旧 SHA 证据，不能替代本次结果。静态测试此前在同一 UI 代码下完成：Proxies 25/25、Rules 18/18、本地化 6/6、E2E Harness 44/44、全仓 TypeScript、边界检查、Oxlint 0 error/0 warning；最新远端提交仅变动 Rust/设计文档。仍未提交/推送 UI 迁移代码。

## 2026-11-19：Main Proxies 组详情路由目录对齐 ref（本地待提交）

- **基线和范围**：主仓库 `master@ac0d4e04`、固定 `ref@c4b35e04`；在此前已通过 13/13 的 Rules/Proxies UI 工作区继续迭代。目标为 Main Proxies 详情路由、模块路径、顶栏/侧栏导航及旧深链接兼容。风险 **L1：UI 路由与跳转**；不改 Core、Service、Runtime、系统代理/TUN、Legacy UI、Agent、权限/IPC 生成合约或子模块。
- **源代码映射**：`ref/frontend/nyanpasu/src/pages/(main)/main/proxies/group/$name.tsx` → `frontend/chimera/src/pages/(main)/main/proxies/group/$name.tsx`；相应 `group/_modules/{delay-test-button,delay-chip,group-header,group-meta,node-list-toolbar,node-list,proxy-node-button}` 均移入新目录。侧栏 `group-nav-item.tsx` 与顶部 `_modules/navbar.tsx` 改为 `to="/main/proxies/group/$name"`；测试引用跟随迁移，`group-meta` 的共享 hooks 仍在上层 `proxies/_modules/hooks.ts`，而组状态/速度代码与本地延迟显示适配保留现有 Chimera 实现。TanStack 生成路由树由 Vite 自动更新，避免手动维护。
- **旧链接兼容**：原 `proxies/$name.tsx` 改为无重复 UI 的 `beforeLoad` redirect，`replace: true`，携带原 `params.name` 和父路由验证后的 `search`。已有 `/main/proxies/<group>` 书签不会因本轮路径迁移而 404，也不新增 Profile/Runtime 修改；静态 `/group/$name` 路由成为规范入口。
- **真实 WebKitGTK 检查和失败留证**：新测试先通过侧栏可见链接 `WebDriver.click()` 进入详情，并断言 href 是规范 `/main/proxies/group/... `。首次测试在点击后只确认 pathname/文本，未等待 `AnimatedOutletPreset` 动画，故测试在过渡中测量卡片；真实失败截图显示左右叠层、卡片超出视口，Tooltip 随之可能失效。保留实际用户点击，然后调用现有 `openMainRoute(canonicalPath)` 仅等待嵌套 outlet 的动画完全收敛（路径已相同、不会再次跳转），恢复几何和 Tooltip 验证；未降低任何阈值。兼容测试起初误用 `openMainRoute(oldPath)`，该辅助函数按设计要求输出 pathname 与输入完全相同；改成真实 `browser.url(oldUrl)` 后等待 redirect，验证最终规范路径、保留 `?q=SOCKS`、且真实节点列表只剩匹配项，测试 8/8 通过。WebDriver `pointer` 仍为 embedded 驱动合成事件，不能声称覆盖实体鼠标硬件。
- **验证和回滚**：`pnpm --filter chimera-ui build`、`pnpm typecheck`、`pnpm typecheck:e2e`、`pnpm --filter chimera-ui test:proxies`（25/25）、`pnpm e2e:tauri:build` 均通过；新规范+兼容路径在独立 user/net/pid、Xvfb 和真实 Mihomo 的 `proxies-node-main.e2e.ts` **8/8** 通过。其它 Rules/Proxies 空态 E2E 另行复验。回滚只需恢复原 `proxies/$name.tsx`、原组组件位置、导航目标及类型生成文件；无需变更持久数据、服务和系统设置。已保留本轮初始失败现场和成功复验日志于本机临时目录。仍未提交或推送。

- **2026-11-19 最终远程快进及最新 Runtime 复验**：在完整 UI/路由测试后发现 `origin/master` 又增加 `4f28cbc7`、`deaca068` 两条仅涉及 `backend/tauri/` 和 `docs/design/` 的 Service release debt 提交。确认无 UI/测试/路由文件重叠，使用临时 stash（含未跟踪组详情模块）保全全部 UI 改动，执行 `git merge --ff-only origin/master` 到 **`deaca068`**，恢复工作区并删除本次专用 stash；`ref` 仍为 `c4b35e04`，Runtime gitlink 保持原值。再执行全量 `pnpm e2e:tauri:build` 退出 0，并重新运行隔离真实 WebKitGTK/Mihomo 三个套件：**Proxies group 8/8、Rules 4/4、Proxies 空态 2/2，最新 SHA 合计 14/14 通过**。这与旧 SHA `ac0d4e04` 的测试结果是两组独立证据。旧书签兼容、新规范侧栏点击、组拖拽和延迟 Tooltip 都包含在 8 项中；本轮 UI 代码仍未提交或推送。

## 2026-11-20：Proxies 延迟测试失败反馈与 ref 文案对齐（未提交）

- **基线与范围**：在已保留的 Main Rules/Proxies UI 工作区上，先确认 `origin/master` 由 `deaca068` 快进两条 Backend/设计文档提交至 **`fc50370d`**，变更与 UI 目录无交叉，临时 stash（含未跟踪文件）后 `git merge --ff-only`、恢复并仅移除本次临时 stash。固定 `ref@c4b35e04` 未变，Runtime/Service/Agent/Legacy UI/TUN 均不修改；此次风险级别 **L1（Main UI 错误状态和 E2E）**。
- **接口语义（不复制第二套参数）**：固定 `ref/frontend/query/src/ipc/use-clash-proxies.ts` 提供 `ClashDelayOptions.expected` 和 `ClashProxyDelayOptions.group`。Chimera 已有 `frontend/interface/src/ipc/use-native-proxy-actions.ts` 使用 `testNativeProxyGroupDelay(group)` 和 `testNativeProxyNodeDelay(group,name)`，由 Core 的 `backend/chimera-core/src/proxies.rs` 从**同一实时 API 快照**独立解析组 URL/expectedStatus/provider 与成员关系，固定组选用最大 8 并发且不会意外清除 pin。该参数逻辑已经在 Core 统一，故本批**不再仿造 ref 前端参数或暴露认证 URL**，也没有改动 typed IPC schema；核心判定的真正授权差异仍另案处理。
- **实际 UI 缺陷与修正**：此前 `DelayTestButton.handleClick` 与 `ProxyNodeButton.handleDelayClick` 直接 `await blockTask.execute()`，IPC 报错时产生未处理的 async click rejection，用户也没有错误反馈。现在均在真实 `useBlockTask` 捕获错误，展示 `Notice.error(m.error_clash_read_delay_test_failed(),5000)`；五语文案逐字复制固定 `ref/frontend/nyanpasu/messages/{en,ja,ru,zh-cn,zh-tw}.json`。整组测试仅在 mutate 真正成功时显示 1s 绿色完成动画；错误时清掉 success，持久 task 状态为 error（**成功完成不代表节点可连通或延迟非零**）。单节点延迟控件新增可访问的名称及稳定测试选择器，保留 `stopPropagation` 不误触父节点选中。
- **真正的 WebKitGTK/Native Core E2E**：在 `unshare --user --map-current-user --keep-caps --net --pid --fork --kill-child --mount-proc` + 隔离 HOME/XDG、Xvfb、D-Bus 中运行真实 Mihomo、合成 SOCKS5 127.0.0.1:65535 Profile（无生产网络/系统代理更改）。首次测试的 `GROUP_DELAY_UI_RESULT error`、`NODE_DELAY_UI_RESULT error` **均为实时 native 结果**；测试检查 UI 错误标记、确实可见的本地化消息、不会发出成功状态，并将选中先切至 DIRECT 再点击离线 SOCKS 节点的内部延迟控制，核对没有改变选中且最终恢复原选择。原有 8 个节点/组导航 E2E 仍通过，加上 2 个新用例节点套件 **10/10**；同一构建下 Rules **4/4**、Proxies 空态 **2/2**，合计 **16/16**。最新测试又收紧断言为离线 SOCKS5 必须走 error 分支，并以隔离 WebKitGTK/Mihomo 实测 **10/10** 重新通过；调试输出同时确认 GROUP_DELAY_UI_RESULT=error、NODE_DELAY_UI_RESULT=error。相邻 Rules 4/4、Proxies 空态 2/2 已独立通过，总计 **16/16**。
- **构建和边界**：已通过 `pnpm e2e:tauri:build`、Proxies 单元测试 25/25、本地化 6/6、全仓 TypeScript、E2E TypeScript、Oxlint、前端边界与 diff 检查；本机仍需临时 `NODE_OPTIONS` 超时 shim 避免 Inlang CDN 文本读取悬挂，本次没有变更脚本。Linux WebKitGTK E2E 不能证明其他平台、真实公网成功分支或远端 `proxy:write` 授权。回滚本轮仅涉及 Main UI 两个延迟控件、五语键和新增 E2E 用例，之前尚未提交的 Rules/Proxies 迁移完整保留。

- **2026-11-20 最新主线复验（`5bbeec75`）**：再次检查到远程两条新提交 `8c9b11e5`、`5bbeec75` 只修改 `backend/tauri/src/client/application_workflow/startup.rs` 和 `docs/design/`，与所有 UI/测试/未跟踪文件无交叉；经临时 stash、`git merge --ff-only`、安全恢复并只删除本次 stash，`master` 与 `origin/master` 均为 `5bbeec75`，原本未提交的 Rules/Proxies 改动保持完整。完整重建 `pnpm e2e:tauri:build` 成功；在同样隔离 WebKitGTK/Mihomo 上重新运行 Proxies 节点 **10/10**（含真实整组及离线 SOCKS 节点错误反馈，日志均为 `error`）、Rules **4/4**、Proxies 空态 **2/2**，**最新 SHA 合计 16/16 通过**，三套均退出 0。Proxies 用例此次耗时 1m03.8s，前一版约 20.2s；独立应用日志出现一次 `active core has no confirmed mixed port for application effects` 警告，但真实 Core 最终 Running、组 API 可访问、所有 UI 测试通过。**只能记录启动延迟线索，不能据此断言新 Runtime 代码回归或 Service 故障**；留待启动性能专题实机复核。本轮仍未提交和推送 UI 迁移。

## 2026-11-20：Providers 列表、订阅统计与共享字节单位向固定 ref 收敛（本地未提交）

- **目标、基线与边界**：主仓库 `master@31cb63a9`（经 `git fetch origin master` 确认与远程一致、工作区原先干净）；只读固定 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。本轮为 **L1：Main UI 表示层与本地化辅助函数**，不修改后端、Runtime、TUN、Service、Profile 持久化、IPC 写入权限、Agent 或 Legacy UI；无新的外部 API、存储格式或签名配置变更。目标是用上游既有布局和计算，不另造同功能实现。
- **上游文件映射**：从 `ref/frontend/nyanpasu/src/pages/(main)/main/providers/index.tsx` 复制 Providers 两组列表、分组操作与上游 `EmptyState` 形态，在 Chimera 原路径保留原组件/函数名称；`_modules/use-proxies-subscription.tsx` 同步上游 `clampPercentage`、`nonNegative`、`Upload/Download/Total` 计算，并继续使用已对齐的 `@chimera/interface` 类型。共享单位代码从 `ref/frontend/nyanpasu/src/utils/{format-bytes,information-unit}.ts` 复制到 `frontend/chimera/src/utils/`，仅作 Paraglide 导入风格兼容（`import * as m`），将 ref 五语言原文 `unit_b` 等 18 项缺失单位消息分别导入 `messages/{en,ja,ru,zh-cn,zh-tw}.json`；列表和 `proxies/_modules/subscription-card.tsx` 共用 `formatBytes()`，不再维持页面内和详情卡片两套临时格式化。更新时间改调用现有 `formatRelativeTime(value, Date.now(), getLocale())`，而非直接使用未按当前用户语言配置的 `dayjs(...).fromNow()`。
- **必要适配和非目标**：`@nyanpasu/query` → `@chimera/interface`、`@nyanpasu/ui` → `@chimera/ui`、hook/locale 路径维持现有工程布局；`ref` 的通知接口能接受原始 `error` 选项，Chimera `message()` 类型不能，故保留同样的错误文本与流程，但不传不受支持的 `error` 属性，并 `await message()` 避免浮动 Promise。刷新数据仍由 Chimera 现有 `useUpdateClash*Provider` 及原生 Core 提供；没有声称迁移上游 RPC 授权模型。为用户可见的两组刷新入口补充 `data-slot` 和 aria 标签（E2E 稳定定位）。若同步订阅字段结构或持久化 API 将另行评估，避免本次影响真实配置。
- **测试及开发环境**：新增 `test:providers` + `tests/providers-subscription.test.tsx`，包含无订阅、正常统计、负数和超额钳制、零/负配额及 `formatBytes(0/1024/1MiB)` 五项。此测试需要本地化生成的 `unit_*` 函数，测试命令先执行仓库原有 `generate:i18n`；Node 侧没有浏览器 locale，自定义测试仅在字节用例内用 Paraglide `overwriteGetLocale(()=>'en')` 并在 `finally` 恢复原解析器，不修改生产国际化行为。`pnpm --filter chimera-ui test:providers` **5/5**、本地化 **6/6**、`pnpm typecheck`、前端边界、Oxlint **0 错误/0 警告**、`pnpm --filter chimera-ui build` 均已通过。真实独立 WebKitGTK/Xvfb/user-net-pid namespace 桌面 UI 已验证上游 `EmptyState[role=status]`、两组布局及空列表下分别点击全部刷新而不跳转 **2/2**；加入共享字节单位后重新编译原生 E2E 程序并再次验证。原版 UI E2E 的卡片高度断言已改为真实 EmptyState 或确有数据的 grid 二选一，不以“没有卡片”绕过空态断言。
- **回滚与后续**：若要撤回，仅恢复 Providers `index.tsx`、订阅 hook、详情 SubscriptionCard、`package.json`、五语言 `unit_*` 追加项、两个格式化工具、Provider 单元/E2E，其他已合并 Rules/Proxies UI 不受影响。尚未执行生产 provider 下载/更新成功场景，也没有声明 Windows/macOS、真实订阅源网络刷新或服务端权限通过。下批可继续对齐 Providers 详情与已有更新 hooks，或进入 Connections 和 Logs，但后两者仍有 RPC/权限边界需单独处理。

- **2026-11-20 本批最终原生桌面复测**：共享 `formatBytes()` 和 18×5 上游本地化消息并入后，重新执行完整 `pnpm e2e:tauri:build` 成功，将最新 Paraglide 产物/前端嵌入新 Rust E2E 应用。重新运行隔离 user/net/pid namespace + Xvfb + D-Bus + WebKitGTK 的 `providers-main-layout.e2e.ts`，**2/2 通过**：布局/EmptyState 和两组无数据刷新控件（点击后没有错误导航）；本次独立运行用时 31.9s。原样保留辅助桌面服务 GLIBC 2.43 不兼容的警告证据，主程序和目标测试均正常完成。最终增加完整单位本地化后 Providers 5/5、五语资源 6/6、全仓 TypeScript、前端边界、Oxlint 0 error/0 warning、Vite 生产构建均通过。本次不对需要真实 Provider 数据的更新成功、订阅配额卡片视觉截图或生产连接作未经执行的验收断言。

- **2026-11-20 最新 Runtime 复验**：结束前 `origin/master` 增加两条仅触及 CoreLifecycle/Reconciler 与 `docs/design/` 的提交，本地安全保存含 3 个新文件的 Providers 未提交工作区并 `git merge --ff-only origin/master`，无冲突恢复，当前 `master=origin/master=7cae712c`。原 `ref` 与 Runtime gitlink 未改。在该 SHA 下重新执行完整 `pnpm e2e:tauri:build`（前端本地化重新生成、随后 Rust E2E）成功，独立执行 WebKitGTK `providers-main-layout.e2e.ts` **2/2**，涵盖空态显示和点击两组刷新而不跳转，耗时 1.8s，测试程序退出 0。此前同一 UI 文件状态下执行的 Providers 5/5、本地化 6/6、全仓类型检查、边界/格式检查、Oxlint 和生产构建结果均通过，新远程提交不影响这些前端文件；尚未执行生产 provider 更新成功场景或真实订阅详情卡片视觉断言。本批代码尚未提交/推送。

- **2026-11-20 本轮提交和最新版 E2E 证据**：在上述 Providers 切片完成后，`origin/master` 又由 `7cae712c` 增加两个仅修改 `backend/tauri/src/client/core_lifecycle/mod.rs` 和设计文档的提交，先安全保存全部 14 个未提交文件并 `git merge --ff-only` 到 **`c6b8f0a1`**、无冲突恢复。随后在该基线上重新执行 Providers **5/5**、本地化 **6/6**、全仓 TypeScript、前端边界、Oxlint **0 error/0 warning**、Vite 生产构建，全部通过；完整 `pnpm e2e:tauri:build` 成功，并在真实隔离 WebKitGTK + Native Runtime 复跑 `providers-main-layout.e2e.ts` **2/2 通过**，耗时 1.9s，进程退出 0。这次复验包含最新启动相关的后端提交，不复用 `7cae712c` 的 E2E 结论。仍未执行非空订阅源实网更新或跨平台端到端验证。本批采用三组可独立审查的功能提交（单位工具与翻译、订阅统计与测试、Providers 页面及 E2E）再单独提交本记录，签名和推送以实际 Git 返回为准。

### 后续前端迁移的集成与发布习惯（用户于 2026-11-20 明确确认）

- **每轮默认推进到远程主线**：先核对和安全同步 `origin/master`，基于固定 `ref` 对齐一组完整前端功能，按作用面跑 TypeScript、Lint、构建和相关真实 E2E（未运行的步骤如实注明）。修复后把代码、测试和迁移文档**分批**签名提交到 `master`，核实新提交 GPG 状态为 `G`，再次 fetch 检查远程，如有新提交则 rebase 后重新核验签名及变化，最后**正常 push、核对远程 SHA 和干净工作区**。默认不再只停留在本地未提交状态，也不新建 PR/worktree、不 squash、不强制推送；有真实阻塞（测试失败、签名失效、远程冲突或缺乏权限）时明确报告，绝不伪称已推送。保留 `ref` 与 Runtime 子模块的固定 gitlink，除非本轮明确包含经审核的更新。

## 2026-11-20：Providers 详情 InfoCard、更新失败状态与真实 Provider E2E（待提交发布）

- **基线与只读参考**：当前主仓库 `master@ec30e30d`（启动时已 fetch 确认与 `origin/master` 一致且无工作区修改），固定只读 `ref@c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`。本轮为 **L1：Main UI 信息卡片、Hook 错误状态、本地化和隔离测试**，未修改 IPC 合约、Core、Service、系统代理/TUN、Legacy UI、代理连接、Profile 业务实现或任何 gitlink。
- **上游具体映射**：`ref/.../providers/proxies/_modules/info-card.tsx` 和 `.../rules/_modules/info-card.tsx` → Chimera 原路径 InfoCard，恢复 `formatRelativeTime(updatedAt, Date.now(), getLocale())` 代替无 locale 上下文的 `dayjs.fromNow()`；Proxy 健康检查按钮由速度计图标/组延迟测试文案恢复为 `MonitorHeartRounded` 及上游 `providers_healthcheck_provider`。五语 `providers_healthcheck_provider`、`providers_healthcheck_failed_message` 均直接从固定 `ref/messages/{en,ja,ru,zh-cn,zh-tw}.json` 复制。保留 Chimera 现有“更新与健康检查互斥 pending”的保护，追加细粒度 `data-slot`/错误状态标记，稳定标识供 E2E 使用。
- **实际失败状态修复**：原 `_modules/use-{proxies,rules}-provider-update.tsx`、`use-proxies-provider-healthcheck.tsx` 把 `mutateAsync` 的失败捕获并弹出 Dialog 后不再抛出，导致 `useBlockTask.run` 将业务失败标为 success。新纯函数 `provider-action.ts::runProviderAction` 在调用现有通知的同时**保留原始拒绝理由重新抛出**，即使通知本身失败也不会制造假成功；三条 Hook 均沿用实际 `@chimera/interface` Native mutation、原错误文案/弹窗。健康检查错误改为 ref 的带名称本地化消息。两张 InfoCard 的 `useLockFn` click handler 捕获已由 Hook 向用户报告的拒绝，避免未处理 Promise；`useBlockTask` 正确记录 error，保留 Core 在回复不确定时重新查询 Provider/Proxy 缓存的既有逻辑。没有依赖网络失败反推出资源实际未更新。
- **验证边界**：`test:providers` 追加 `tests/providers-actions.test.ts`，覆盖操作成功无误报、操作失败通知一次并保留原始 Error、通知失败仍保留原 Error、`formatRelativeTime` 在中英文与空更新时间上的语义；在既有 5 项订阅/单位测试上合计 **9/9**。五语资源 `6/6`、全仓 `pnpm typecheck`、`lint:frontend-boundaries`、Oxlint 0 warning/0 error、`chimera-ui build` 均已通过。
- **新增非空 Provider 真正桌面测试**：`tauri-e2e/specs/providers-detail-main.e2e.ts` 注册至 `spec-suites.ts` 的 `main`（继而 hermetic/all）并复验注册单元约束；在本次独立 `CHIMERA_E2E_RUNTIME_DIR` 的 `app/data/providers-detail-fixture` 下创建可控 Proxy+Rule 文件，使用 `create_profile`/生产 `restart_sidecar` 激活真实 Config，要求 `Core Running`、从 Providers 列表读到两个真实资源，再打开两种详情并检查卡片、按钮与本地化更新时间；Proxy 健康检查文案**严格匹配当前 Paraglide 语言包键**，不会再把原来的“测速”误当正确。测试完成从原 Profile 恢复、删除只属于本轮的 Profile 和 Provider 文件。独立 WebKitGTK 实测 **2/2**，原空态/刷新入口 **2/2**，合计 **4/4**。此测试没有直接触发真实 Provider 下载/健康检查 mutation，也不声称 Windows/macOS 或生产网络验证。
- **回归过程中的安全发现**：第一次真实 Provider Fixture 把本地文件放在 `app/config/`，Core 候选配置检查按 Mihomo `SAFE_PATHS` 返回 `config_check_failed: path is not subpath of home directory or SAFE_PATHS`；实际允许根目录包括本轮隔离 `app/data`。测试将文件放到 `app/data/providers-detail-fixture` 后 **2/2 通过**，无修改/放宽 Core 的 SAFE_PATHS 安全策略。具体失败与修正追加 `LESSONS.md` 供未来类似隔离 E2E 使用。
- **提交与剩余工作**：按用户确认的“同步远程→对齐 ref→验证→分批 Verified 提交→必要时 rebase→正常推送→核实 SHA”流程完成本轮，不 squash、不 PR/worktree/force push。下一组可继续研究 Provider 更新实际成功、并发取消/超时和 Connections/Logs 的 RPC/授权边界；这些非本次测试通过的事实。

- **2026-11-20 最新主线和真实 Provider E2E 复验**：执行提交前 `origin/master` 已由 `ec30e30d` 新增两条仅涉及 `backend/tauri/src/client/core_lifecycle/mod.rs` 和 `docs/design/` 的启动协调提交，确认与前端无重叠后采用临时 stash、`git merge --ff-only origin/master` 无冲突快进本地 `master` 至 **`7b61f150`** 并恢复所有前端、测试与文档；未改变 `ref` 和 Runtime gitlink。前端详情/失败状态及 E2E 三个批次均通过 Husky 的 Oxlint、Prettier、Chimera/E2E TypeScript，三个签名均为 `G`。使用最新 Vite 产物完整重新构建 `backend/target/e2e/debug/chimera` 成功，隔离真实 WebKitGTK/Mihomo 下 `providers-detail-main.e2e.ts` **2/2 通过（37.9s）**，`providers-main-layout.e2e.ts` **2/2 通过（31.8s）**，共 **4/4**。非空 Provider 来自实际配置文件，测试数据在隔离 `app/data/providers-detail-fixture` 下，运行结束后清理完成。新 Runtime 启动期间出现过一次 `the active core has no confirmed mixed port for application effects` 警告，但随后 Core Running 且两套 UI 用例通过；等待变长的原因仍未经定位，不能把它当成已经解决的 Runtime 性能问题。Providers 单元 **9/9**、语言 **6/6**、E2E harness **44/44**、全仓类型/Lint/边界和生产构建均通过。未测试生产网络更新成功、Provider healthcheck 网络路径或 Windows/macOS，不能扩大结论。本轮按此前约定最终执行文档签名提交、远程核验与推送。

## 2026-11-20：Providers 列表 Native 更新与批量刷新状态对齐（本地待提交）

- **基线与范围**：本地 `master@c66be3d4` 启动时与 `origin/master` 一致；固定 `ref@c4b35e04` 只读。本轮围绕已迁移的 `frontend/chimera/src/pages/(main)/main/providers/index.tsx` 补足 ref 的列表单项更新/分组更新交互在 Chimera Native mutation 边界的错误状态。只涉及 Main UI、真实 E2E 和文档；不修改 Runtime、Service、TUN、系统代理、权限或 submodule gitlink。保留上游现有路由/卡片结构，Native Proxy 与 Rule API 操作仍经原本的 `@chimera/interface`。
- **真实缺陷**：上一轮已修正详情 Hook 中被 `try/catch` 吞掉的失败，但 Providers 列表上单项刷新和两组 `Update All` 仍直接使用 `useLockFn(blockTask.execute)`（异常造成未处理 Promise）；分组 `Promise.all` 失败时只显示错误 Dialog 并返回，`useBlockTask` 错误标记为 `success`。本批统一在批量操作复用已验证的 `runProviderAction`：原样显示失败对话框、保留原始 rejected Promise，使 Task 最终处于 error；列表单项和分组按钮 click handler 只捕获已报告的拒绝，不改变 Native action 本身。增加 `data-slot="providers-item-update"`、`data-provider-name`、`data-error` 和 `data-loading` 供 UI 用户状态及 E2E 精确定位；两个 bulk 按钮追加 `data-error` / `data-loading`，没有加隐藏的测试专用业务分支。沿用 ref 的 `Promise.all` 并发处理，后续可单独评估它的首错返回/其余操作继续执行语义。
- **真实 HTTP Provider E2E（已运行）**：新增并注册 `tauri-e2e/specs/providers-update-main.e2e.ts` 到 `main` 测试套件。每轮在独立 user/net/pid namespace、Xvfb/WebKitGTK 和隔离 `CHIMERA_E2E_RUNTIME_DIR/app/data/remote-providers-fixture` 中启动真实 127.0.0.1 HTTP YAML 服务；Core 配置声明一组 HTTP Proxy Provider 和一组 HTTP Rule Provider，经生产 Profile 激活、`Core Running`、两组查询均得到初始 count=1。随后经**真实 UI 点击**分别在详情更新 Proxy（1→2）、列表单项更新 Rule（1→2，URL 不跳转）、两组 bulk 更新（2→3）；每次同时检查 HTTP 服务收到新的请求、Native Core 权威 `clash_api_get_providers_{proxies,rules}` 数量变化，Proxy 详情 UI 不刷新页面即反映新数量。另以服务端真实 HTTP 503 测试详情及 bulk Proxy 更新拒绝，观察原生错误 Dialog，确认点击完成后按钮 `data-error=true`、Core 仍保留上次成功版本（数量为 3）及路由不变。**真实 Linux/X11 原生桌面 E2E 5/5 通过**，执行后 Profile 与所有临时 Provider/HTTP 服务均清理。
- **原生 Dialog 测试边界**：Tauri 错误 Dialog 是 OS 窗口，不位于 WebDriver DOM。第一次直接 `xdotool key --window` 没真正关闭 Xvfb 里的无窗口管理器 GTK Dialog，导致 Task 保持 pending；随后只在测试侧使用 `xdotool search --onlyvisible` 核对正确错误对话框标题、`windowfocus --sync`、真实 `Return` 键确认，之后验证 Task 变为 error。Linux 有 DISPLAY 且安装 xdotool 时执行两条失败路径；Windows/macOS/没有可用 X11 driver 时**明确 Mocha skip 这两条**，成功下载三条仍执行，不声称跨平台失败 Dialog 已验证。没有 monkeypatch Tauri IPC、伪造 HTTP 响应或绕过 native 用户动作。真正生产公网订阅与真实代理站点不属于此测试。
- **检查**：新增前端变更后执行 Providers 单元 **9/9**、E2E Harness 单元 **44/44**、全仓 TypeScript、Oxlint 0 errors/warnings、前端边界、Vite 生产构建通过；完整 `pnpm e2e:tauri:build` 成功，新 HTTP Provider E2E 最终 **5/5 通过**。原有 Providers 空态和详情套件已在同一构建下分别 **2/2** 通过，合计真实桌面 E2E **9/9**；之后按用户约定分批 Verified 提交、同步最新远程、正常推送、核对远程 SHA；有阻塞如实报告。未做 Windows/macOS 或 CI 验证。

- **2026-11-20 同版相邻 E2E 复核**：采用与 HTTP 更新用例同一轮 `pnpm e2e:tauri:build` 的真实 Linux WebKitGTK 产物，另独立运行既有 `providers-detail-main.e2e.ts` **2/2**、`providers-main-layout.e2e.ts` **2/2**，与 HTTP 更新正负场景 **5/5** 合计本轮真实桌面 E2E **9/9**。其中空态 suite 初始化较慢（约 31.7s），并有历史已知的桌面辅助服务 GLIBC 提示；测试最终均正常退出。应用 Runtime/Service 实现未改，本轮仍不对发行构建或非 Linux 平台作覆盖承诺。

- **2026-11-20 最新 `master@885fb90b` 独立复验**：提交前再次 `git fetch` 发现远端增加两条仅修改 `backend/tauri/src/client/core_lifecycle/mod.rs` 与 `docs/design/` 的启动协调提交。核对本轮 UI/测试文件无交叉后，使用临时 stash（含未跟踪 E2E）保护全部内容，再 `git merge --ff-only` 并安全恢复，移除且仅移除该临时 stash，本地 `master=origin/master=885fb90b`；固定 ref/Runtime gitlink 仍未变。重新执行完整 `pnpm e2e:tauri:build` 后，最新原生应用在隔离 Linux/X11 + Xvfb + Mihomo 环境运行 `providers-update-main.e2e.ts` **5/5**，包含真实 HTTP 503 下的两种错误任务状态、Native Dialog 和旧 Core Provider 计数不变量；本次套件约 10.3s，正常退出 0。此前对旧 Runtime 基线执行的相邻 Providers 详情 2/2、空态 2/2 仍如实保留为旧 SHA 证据，不把它们算作最新 Runtime 的重新执行；本轮前端回归单元、全仓类型检查、边界、Lint 和构建均已通过。发布前提交签名及最终 GitHub SHA 以实际结果为准。

## 2026-11-20：Connections Main 对齐固定 ref 的 DataPage 浮动 dock（本轮）

- **基线/范围**：原本本地 master 与远程主线一致于 3cfe02d2，启动时 fetch 发现新 Runtime-only 提交 f6b9d83b、ab6ee626；无前端路径重叠后 git merge --ff-only 至 ab6ee626。只读固定 ref=c4b35e04，backend/chimera-runtime gitlink=9cf8e01a。本轮仅迁移 Main Connections 页面/共享表示层、关联 E2E 与文档，未修改 Core、Service、TUN、系统代理、Native IPC、配置格式及子模块。
- **ref 对照**：复制 ref/frontend/nyanpasu/src/pages/(main)/main/connections/_modules/connections-toolbar.tsx 为 Chimera 的 connections-toolbar-ref.tsx，主要内容按原文保留，包括 DataPageToolbar、SearchField variant=glass、GlassControlGroup 状态页签和操作组、可换行筛选 chips、列设置 modal trigger 与关闭全部按钮。差异仅为 @nyanpasu→@chimera 依赖和 Paraglide namespace 适配、保留可测试的 data-slot。
- **Main 装配**：ref index.tsx 的 DataPage + DataPageSurface + keyed ScrollArea / 底部浮动 dock 结构经已有 ConnectionsContent 共享展示层落地。Main index.tsx 显式传入 referenceLayout，而 Legacy 页面继续沿用原有 ConnectionsToolbar 和顶部/底部容器结构；共享连接数据、搜索延迟、范围、路由回跳、详情、列设置均未重造。Main DataPageSurface 保留共享组件原有 data-page-surface 标识，并以 data-connections-container=true 增设独立定位，Legacy 继续保留其原有 connections-container 标识。
- **ref 虚拟化滚动**：从上游 connections-table.tsx 导入 useDataPageBottomInset，向 TanStack virtualizer 传入 paddingEnd 与 scrollPaddingEnd，确保 Main 最后一行可越过底部 dock。Legacy 不处于 DataPage Provider 内，仍接收默认零偏移。Main ScrollArea scrollbar 竖直偏移改用 ref 的 top-10、Legacy 保留 top-9。
- **权限/未迁移边界**：上游 Main 通过 useCapabilities().hasPermission(proxy:write) 隐藏无权用户关闭连接的写操作；Chimera 当前前端没有同等 auth capability hook，而保留了此前已有的 Native 关闭操作。**本批没有扩大/修改此类写操作或伪造权限结果**，该差异须在后续拥有与后端授权事实一致的契约后单独迁移、测试；本批只是 Main UI 纯表现层的对齐。其他连接表格数据源及 Legacy 兼容不因视觉迁移替换。
- **实际静态验证**：Connections model 单元 9/9、pnpm typecheck、pnpm lint:frontend-boundaries、Oxlint 零 error/warning、chimera-ui 生产构建、pnpm e2e:tauri:build 均通过。E2E 主页面覆盖 dock 相对页面底边的真实几何、两组玻璃控件、SearchField 清除并去掉 URL q、all/active/closed 范围路由、列设置 modal。
- **真实原生桌面 E2E**：同一最新编译 Tauri E2E 程序，Linux WebKitGTK + Xvfb 隔离环境中 Main Connections 4/4、Legacy Connections 3/3、原生 Connections detail subscription 1/1，总计 **8/8 通过**，三套均退出码 0。首次 Main E2E 中测试按默认 data-slot 找不到 DataPageSurface/GlassControlGroup，改为保留组件默认 slot 并核对上游明确指定的 data-page-controls / connections-actions；另实际 WebKit 内容区 1189px 小于请求外层窗口 1240px，将纯 viewport 阈值改为 >=1100，但保留 dock、scroll、搜索与按钮的所有位置尺寸断言。Legacy 初次断言 viewport.top >= header.bottom 遭 WebKit 非整数像素约 0.00001px 的相反舍入，改为 1px 相邻边界容差，其他 Legacy 几何与交互断言未降低。
- **发布与下轮目标**：按用户确认的流程拆分 UI+表格迁移、真实 E2E、文档为独立 Verified 提交，fetch/rebase 最新 origin/master、正常推送 master 并确认远程 SHA。不以本次成功宣称关闭连接授权层、非 Linux WebView 或生产网络负载已完成验证；后续继续处理 Connections proxy:write 能力接口与更完整的非空表格场景。

- **2026-11-20 最新 Runtime 复验与批次提交**：提交前远程新增两条后端队列饱和补丁 05268890、fd507823，变更仅涉及 backend/tauri/src/client/core_lifecycle/mod.rs 及 docs/design，与所有未提交的 UI/E2E/文档文件无交叉。使用本次独有 stash（包含新增 ref toolbar）保护并 git merge --ff-only 到 master=origin/master=fd507823，恢复完毕、零冲突、没有改动任何 gitlink。UI 源码第一批已签名提交 4f010ca5、Main/Legacy E2E 第二批已签名提交 51e09289，两批均经过 Husky Oxlint/Prettier/TypeScript 且 GPG 状态 G。最新 fd507823 下完整重新执行 pnpm e2e:tauri:build 成功，真实隔离 WebKitGTK Main Connections **4/4**、Legacy Connections **3/3**、Native Connection Detail Channel **1/1**，合计 **8/8**，全部退出码 0；其余单元 9/9、E2E Harness 44/44、全仓 TypeScript/边界/Oxlint/Prettier/Vite build 已通过。最后文档将单独签名提交；再 fetch/rebase（如有远程新增提交）并正常 push，远程 SHA 验证以实际 Git 返回为准。未测试跨平台关闭连接授权或高并发长列表性能。
