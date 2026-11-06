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
