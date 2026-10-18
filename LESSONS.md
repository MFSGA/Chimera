# Chimera Engineering Lessons / 已验证工程经验库

供维护者和编码/审查 Agent 在相关模块工作前阅读。本文件记录**已证实的工程失败及其防复发措施**，不是 TODO 或推测清单。

- 遵循 [AGENTS.md](AGENTS.md)、[AI 治理规范](docs/ai-development-governance.md)、[ref 对齐](docs/ref-alignment-guide.md)、[测试验收规范](docs/testing/README.md)；经验记录不能自行放宽已有门禁。
- 只收录真实复现、已确定根因、有可核对的 Issue/PR/CI/commit 的缺陷。仅凭一次失败、CI 红灯或猜测不等于上游/本地已确诊。
- 区分“真正的产品回归”“参考实现缺陷”“测试边界/驱动限制”“环境/平台问题”；对于 ref 缺陷给出对应 SHA/路径和最小复现，记录必要的偏离与复查条件。
- 不收录密钥、完整订阅、用户配置、真实个人信息、未经脱敏的截图/日志；发生 host TUN/Service 副作用时注明所在隔离 VM 和恢复证明。
- 已验证问题优先补能检出旧实现的回归测试，并注册测试执行入口，确保 CI/独立审查能发现复发。
- 再次出现时审查重复原因、是否需要添加 CI 检查；修改 AGENTS/CI/Review 门禁属于 L3，只有维护者可批准。
- 对尚无根因的问题开 Issue 标记“待核实”，**不得填写本文件为已确认事故**。

## 已确认条目索引

| 编号                | 日期       | 模块           | 问题与证据                                                                                                      | 状态      |
| ------------------- | ---------- | -------------- | --------------------------------------------------------------------------------------------------------------- | --------- |
| LESSON-20261014-001 | 2026-10-14 | Rust / CI      | macOS epoch PID 集成用例第二次 spawn 收到 `NotFound`；PID 文件读取竞态是候选根因，PR #479 远端复验待完成        | Open      |
| LESSON-20261014-002 | 2026-10-14 | Proxies / E2E  | Windows 桌面 E2E 激活配置后仍显示旧代理组图标；后端缓存未随 profile 刷新事件失效，PR #479 新 SHA 远端复验待完成 | Open      |
| LESSON-20261014-003 | 2026-10-14 | Profile / E2E  | PR #481 的 Linux 定时刷新测试和 Windows 流量菜单 E2E 未等待异步状态收敛；修复待新 SHA 复验                      | Open      |
| LESSON-20261014-004 | 2026-10-14 | Development    | PR #481 首次提交钩子未使用外接盘 Cargo target，写满主卷；显式传入 target 后 hooks 通过                          | Mitigated |
| LESSON-20261015-001 | 2026-10-15 | E2E            | main-first 套件误用仅存在于 main UI 的 `app-root` 作为 legacy 就绪标记；本地隔离 WebKit 运行复现并修正          | Verified  |
| LESSON-20261015-002 | 2026-10-15 | E2E            | PR #481 Windows 规则图标用例的路由就绪等待超时，具体条件未定位；目标用例已缩小等待范围，待新 SHA 复验           | Open      |
| LESSON-20261015-003 | 2026-10-15 | Development    | PR #481 本地验证误用了 PR #482 Cargo target，混入另一分支类型产物；已确认并隔离构建目录                         | Mitigated |
| LESSON-20261016-001 | 2026-10-16 | Development    | 在未准备好的 worktree 调用 `pnpm exec` 触发依赖安装；已中断，须先核对依赖状态并用已安装工具运行检查             | Mitigated |
| LESSON-20261016-002 | 2026-10-16 | Frontend / CI  | PR #484 transform-chain 行组件首次遗漏 `isLast` 解构，提交钩子的类型检查在提交前发现并阻止了错误                | Mitigated |
| LESSON-20261017-001 | 2026-10-17 | E2E / CI       | Windows Edge 154 对 `data:` no-cors fetch 返回 `basic`；兼容 readable `basic` 与标准 `opaque` 响应              | Verified  |
| LESSON-20261017-002 | 2026-10-17 | Profile / Rust | `start_producers()` 原先只投递 actor cast；await actor 回复后 Profile 测试 72/72、PR #485 CI/E2E 通过并已合入   | Verified  |
| LESSON-20261017-003 | 2026-10-17 | E2E            | Traffic 路由 E2E 修正窗口准备及侧栏状态读取；精确 PR SHA 的 Windows hermetic 和隔离套件通过                     | Verified  |
| LESSON-20261017-004 | 2026-10-17 | CI / Linux     | 前端-only job 误跑依赖 GTK/GLib 的 Rust 测试；已移入后端门禁，PR #489 的 Ubuntu 测试通过                        | Verified  |
| LESSON-20261017-005 | 2026-10-17 | E2E / Windows  | PR #484 的平滑滚动在 Windows Edge 154 只到达 49/820px；改用即时定位并在清理时复位视口                           | Verified  |
| LESSON-20261017-006 | 2026-10-17 | E2E / Windows  | PR #490 Dashboard 右键 E2E 漏发 `pointerdown`；驱动已补合成 PointerEvent，待 Windows 复验                       | Open      |
| LESSON-20261017-007 | 2026-10-17 | E2E / Windows  | PR #490 新运行确认输入 CSS 高度为 56px、窗口矩形为 53px；Dashboard 用例增加 fresh-mount 与故障调用证据，待复验  | Open      |

## 单条模板

```markdown
### LESSON-YYYYMMDD-001：简短标题

- 日期与时区：
- 状态：Open / Mitigated / Verified
- 严重度：Critical / High / Medium / Low
- 模块：main UI / legacy UI / agent / IPC / Rust / Profile / Core / Service / TUN / E2E / CI / 发布
- master commit / ref gitlink SHA / 平台与测试环境：
- 关联 Issue / PR / CI / 运行日志（脱敏）：
- 预期行为：
- 真实异常、复现步骤与证据：
- 确认的根因与排除项：
- 修复提交、差异映射及保留的兼容性边界：
- 原实现失败、新实现通过的回归测试，及测试注册位置：
- 验证命令、实际用例数量、结果、未覆盖/跳过项：
- 风险/回滚和清理证据（如涉及宿主状态）：
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：
```

_历史条目只按可追溯证据收录，不从既有 Git 历史自动推断事实。_

### LESSON-20261017-001：data URL E2E 应验证请求结果而非固定响应类型

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #478 已合入；对应 CI 与 Tauri E2E 均通过）
- 严重度：Low
- 模块：E2E / CI / WebView CSP
- master commit / ref gitlink SHA / 平台与测试环境：master `79c7cb8aa53686cd67effc3d021bd06aaad3e914`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Windows runner，Microsoft Edge 154。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #478](https://github.com/MFSGA/Chimera/pull/478)，[Tauri E2E run 39003668695](https://github.com/MFSGA/Chimera/actions/runs/39003668695)。Hermetic 53 个 spec 中 52 个通过；唯一失败来自新加 smoke 断言，实际 `Response.type` 为 `basic`，而测试预期 `opaque`。单独的 Profile auto-update E2E 通过。
- 预期行为：两个桌面窗口均能成功读取 same-origin 页面，并通过 `connect-src data:` 完成 data URL 请求；no-cors 响应可以是标准 opaque 响应，也可以是 WebView 暴露的 readable basic 响应。
- 真实异常、复现步骤与证据：Windows Edge 154 在 `fetch('data:text/plain,chimera-csp-probe', { mode: 'no-cors' })` 上返回 `type: 'basic'`，导致测试在类型断言处失败；此前同一窗口的请求已返回 HTTP 成功响应。失败不表示 CSP 拒绝请求。
- 确认的根因与排除项：测试将 `no-cors` data URL 的响应类别误设为固定的 `opaque`。响应类型不是所测 CSP 功能的产品合同；工作流日志没有显示请求被阻止，也没有其他 smoke 或 hermetic spec 失败。
- 修复提交、差异映射及保留的兼容性边界：`tauri-e2e/specs/smoke.e2e.ts::assertConnectSourcesAreAvailable` 接受 `opaque`（状态码 0、空 body）或 `basic`（状态码 200、完整 probe payload），其他响应类型失败。CSP 产品策略保持不变。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原断言在上述 Windows Edge run 失败；修正用例仍注册于默认 smoke 和 `hermetic`。PR #478 合并 SHA `b0c6beb65c3fc6099735c0b61cd913287b77964d` 的 CI 与 Tauri E2E 均通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：原 Windows Edge run 的 hermetic 套件为 52/53 通过；修正后 CI run [39005445410](https://github.com/MFSGA/Chimera/actions/runs/39005445410) 与 Tauri E2E run [39005445412](https://github.com/MFSGA/Chimera/actions/runs/39005445412) 均在 SHA `b0c6beb65c3fc6099735c0b61cd913287b77964d` 通过。此测试修复不声称证明 macOS WebKit 或其他 WebView 的响应类型。
- 风险/回滚和清理证据（如涉及宿主状态）：只改测试断言和经验记录，不改变宿主代理/TUN、持久化或 CSP 策略；可回滚为恢复原 `type/status` 断言并移除此条记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：后续 `data:` no-cors 测试先断言 fetch 已 resolve；只有 WebView 返回 readable basic 响应时才断言 payload，标准 opaque 响应应检查其受限元数据。无 CI 门禁变更。

### LESSON-20261014-001：epoch PID 记录可能在检查后、读取前被并发清理

- 日期与时区：2026-10-14 UTC
- 状态：Open（本地已修复，等待 PR CI/E2E 在新 SHA 上复验）
- 严重度：Low
- 模块：Rust / CI
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `e9e3fab1913773a1ecd6ad6a689175e523208cc2` 的 ref gitlink 为 `69353fc9a7d7e4e427211a85cdaa0b9c07a4f01a`；失败 PR SHA `ef96125f05584bf2dce017ce2e2266c10bdee372` 的 ref gitlink 为 `813673a8e84b51265467f02277fc7e739837f61e`，runtime 子模块为 `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7` / macOS GitHub Actions、Apple Silicon 本地开发机
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #479](https://github.com/MFSGA/Chimera/pull/479)、[macOS CI job 115762372590](https://github.com/MFSGA/Chimera/actions/runs/38536302199/job/115762372590)；日志显示 `residual_process_is_killed_before_spawn` 在第二次 `spawn().unwrap()` 收到 `Io(NotFound)`，旧 SHA 上该集成目标 11 passed、1 failed。
- 预期行为：旧 epoch 的 PID 记录在回收或旧进程退出时消失，应按“无记录”继续启动新 epoch。
- 真实异常、复现步骤与证据：旧实现先调用 `symlink_metadata`，再 `read_to_string`。macOS CI 的第二次启动失败，错误为 OS code 2；日志没有指出产生错误的具体系统调用。进程清理路径可以并发移除同一记录；若删除发生在元数据检查成功后、读取前，`read_to_string` 会把 `NotFound` 传播为启动错误。
- 最可能根因与排除项：检查与读取之间的文件生命周期竞态与 CI 错误及并发清理路径一致，但 CI 日志没有证明 `NotFound` 确实来自该次 `read_to_string`。测试没有确定性地钩住两个文件系统调用之间的间隔，因此不能声称已在本机重现该窄竞态。
- 修复提交、差异映射及保留的兼容性边界：PR #479 在 `backend/chimera-utils/src/process/pid_file.rs::read_epoch_pid_file` 将读取阶段的 `NotFound` 映射为 `Ok(None)`，其他 I/O 错误仍传播。ref `backend/nyanpasu-runtime/crates/nyanpasu-utils/src/process/pid_file.rs::read_epoch_pid_file` 在基线 SHA `e9f44e68b0c88981160f5b986f7ffcc9fb5b20a7` 有相同检查/读取顺序；这里保留最小本地修正，不改变符号链接检查、进程身份验证、kill 权限或记录格式。详见 [DIFF-051](docs/ref-alignment-progress.md#diff-051-修正-epoch-pid-文件检查与读取之间的删除竞态)。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为上述 macOS `process_pid_file::residual_process_is_killed_before_spawn`；新实现本地同一集成目标 12/12 通过，位置 `backend/chimera-utils/tests/process_pid_file.rs`。新 SHA 的 macOS 远端复验尚待执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo test --locked --manifest-path backend/Cargo.toml -p chimera-utils --features process --test process_pid_file`，12 passed、0 failed、0 ignored；`rustfmt --check backend/chimera-utils/src/process/pid_file.rs` 通过。未运行全量 `cargo fmt --all --check`（会报告既有 vendor/runtime 子模块格式差异），不声称该全量检查通过。
- 风险/回滚和清理证据（如涉及宿主状态）：不修改宿主状态；回滚为还原 `read_epoch_pid_file` 的读取错误分支及本条记录。CI 新 SHA 通过后将状态更新为 Verified。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：无；不改变治理门禁。

### LESSON-20261014-002：profile 刷新事件未使后端代理快照缓存失效

- 日期与时区：2026-10-14 UTC
- 状态：Open（本地已修复，等待 PR #479 新 SHA 的 Windows Desktop E2E 复验）
- 严重度：Low
- 模块：Proxies / IPC / E2E
- master commit / ref gitlink SHA / 平台与测试环境：PR #479 旧 SHA `2188073f4ee0f749777e3a8c21e1c7aef54520c3`，ref gitlink SHA `813673a8e84b51265467f02277fc7e739837f61e`；GitHub Actions Windows Desktop E2E；本机 macOS WebKit 定向复跑
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #479 Desktop E2E run](https://github.com/MFSGA/Chimera/actions/runs/38544336107)，52 项中 1 项失败；归档截图显示 Rules 页的新 profile 导航已显示，但 proxy-group icon 使用无图标 fallback。
- 预期行为：profile 激活后，已挂载的 main UI 及时取得新运行时代理组快照并展示对应图标。
- 真实异常、复现步骤与证据：`rules-main-proxy-icon.e2e.ts` 先打开 Rules 页面、再激活含原始 SVG 图标的本地 profile。Windows CI 在 30 秒等待后仍观察到该 group 存在但 `<img>` 缺失；直接 runtime 查询已确认新 profile 的代理组已激活。相同定向用例在 macOS 上通过，因此此处保留 Windows 失败为回归证据，不声称本机可稳定复现。
- 确认的根因与排除项：Chimera profile 事件会使前端代理 query 失效，但 `get_proxies` 可能在 3 秒 TTL 内从 `ProxiesGuard` 返回旧快照。该 IPC 读取路径刷新缓存时不广播新的 proxies 事件，因此该次旧 query 没有后续 UI 刷新信号。另有跨越失效点的旧快照请求可能在稍后写回缓存；generation 栅栏现会拒绝这类响应。检查截图和事件/IPC 实现后确认该竞态路径；不是 SVG 图片尺寸或测试选择器问题。
- 修复提交、差异映射及保留的兼容性边界：Chimera 在 `ChimeraClient::refresh_profiles` 中递增 `ProxiesGuard` generation、标记缓存过期，再发 profile mutation；写回前核对 generation，IPC 查询在检测到并发失效时重试。后台刷新在 API 解析前捕获 generation；代理选择在 API 获取后、HTTP 选择前检查 generation，并将同一 token 用于后续缓存写回。profile 在 API 获取期间变化时选择会被拒绝；若 profile 在 HTTP 选择请求进行中切换，该请求仍可能作用于正在退出的 runtime，但缓存刷新会检测失效并返回错误，IPC 不会报告成功或继续发代理事件/连接中断处理。Chimera 尚未具备 ref actor 的实例级撤销。手动 profile mutation 和无当前配置时的自动激活完成路径都调用该函数。详见 [DIFF-053](docs/ref-alignment-progress.md#diff-053-profile-变更后使代理快照缓存失效)。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败为上述 Windows 桌面用例；新增 `ProxiesGuard::invalidate` 单元用例验证缓存过期且保留最后快照。该桌面用例由现有 `tauri-e2e` 套件注册；新 SHA 的 Windows 复验待完成。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：初始 generation 修复的定向 Rust 测试为 3 passed；独立复审要求补上
  选择请求期间发生 profile 切换时的错误返回后，第一次重跑因工作树构建缓存占满主机磁盘，在链接前失败。清理该工作树的 Cargo
  生成产物后，以 `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --manifest-path
backend/Cargo.toml -p chimera --lib core::clash::proxies::tests:: -- --nocapture` 重跑，最终代码 4 passed、0 failed。
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera --all-targets
--all-features`、`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、Prettier、`git diff --check`、
  `pnpm test:manifest`（8 passed）和 Deno check 通过。最新 e2e 构建上的本机
  macOS WebKit 定向 WDIO 用例首轮在主页面路由动画稳定检查失败（未进入 profile/代理断言），第二次独立运行 1/1 通过；
  旧实现的本机用例也为 1/1 通过。记录该首轮失败，重试通过不等于干净首轮通过。Windows Desktop E2E 仍需新 SHA 远端复验；
  未声称 macOS 结果能代替 Windows。全工作区 typecheck 因隔离工作树缺少生成的 Paraglide 文件而失败。
- 风险/回滚和清理证据（如涉及宿主状态）：L2，共享 profile/proxy 刷新时序。回滚为恢复 profile call sites 直接触发事件并移除缓存失效函数和对应测试；不触及持久化数据、宿主网络、TUN 或 Service 状态。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：现有 E2E 即为跨平台防复发用例；无治理门禁调整。

### LESSON-20261014-003：异步 Profile 刷新与流量菜单测试必须等待状态收敛

- 日期与时区：2026-10-14 UTC
- 状态：Open（测试修复待 PR #481 新 SHA 的 CI/E2E 复验）
- 严重度：Low
- 模块：Profile / E2E / CI
- PR head commit / ref gitlink SHA / 平台与测试环境：PR #481 初始 head `b9329eee6e6a20d64b960800c8a6cefa81439836`；ref `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；GitHub Actions Ubuntu Rust tests 与 Windows Edge hermetic suite
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)，[CI run 38592275892](https://github.com/MFSGA/Chimera/actions/runs/38592275892)，[Tauri E2E run 38592275993](https://github.com/MFSGA/Chimera/actions/runs/38592275993)。Ubuntu 在 551 passed 后，`client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer` 因 `fetch_count >= 2` 超时失败；Windows 52/53 specs 通过，`traffic-analysis-main.e2e.ts` 未观察到 Connections 菜单项，失败截图中的 metric trigger 仍为 `aria-expanded=false`。Profile transform-chain 用例在该 E2E run 中通过。
- 预期行为：计划刷新到期后，测试等待 fetch 与提交后的 Profile snapshot；选择流量 scope 后等待控制状态更新，再打开 metric 菜单并选择指标。
- 真实异常、复现步骤与证据：Linux 测试以 200 次 `yield_now` 轮询 actor snapshot，CI 失败时仍未观察到第二次 fetch。Windows 测试只等待 URL 中的 `scope=closed`，随即点击 metric trigger 并等待 Connections 项；归档 HTML/截图显示测试结束时 scope 已选中、菜单已关闭。失败是测试证据，不代表 Profile scheduler 或流量菜单产品行为已被证明损坏。
- 确认的根因与排除项：Rust 用例将调度 actor 与异步提交误当成可在固定次数的协作让出内完成；计数是调度任务的异步副作用，有限 `yield_now` 不提供完成保证。Windows 用例没有等待 scope 控件反映 URL 状态，也没有验证点击后菜单确实打开，故失败位置不能区分 UI 事件未触发与断言时序。新测试改为等待 fetch `Notify`、提交后的 authoritative snapshot、scope `aria-checked` 和菜单 `aria-expanded`；仍须以新 CI 结果确认这组边界足以稳定通过。
- 修复提交、差异映射及保留的兼容性边界：Rust 测试用 `Notify` 等待第二次真实 fetch，并以有界等待观察已提交的 subscription；ref `backend/tauri/src/client/profiles.rs::wait_for_source` 与 `jobs::settled` 提供类似完成同步，Chimera scheduler 没有 jobs registry，因此本测试用 fetch 信号和 snapshot 适配，不改产品调度器。流量 E2E 在打开菜单前等待 scope 和 Radix `aria-expanded` 状态；不改变流量 UI 或查询行为。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为上述 Ubuntu 与 Windows runs。新测试修复已在 `backend/tauri/src/client/profiles.rs` 和 `tauri-e2e/specs/traffic-analysis-main.e2e.ts`；待新 SHA 运行 `pnpm test:backend` 与 hermetic E2E 后才能标记修复通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、`pnpm --filter @chimera/tauri-e2e typecheck`、`cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib --all-features client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer -- --exact`、Prettier 和 `git diff --check` 通过；针对性 Rust 单测 1/1 通过。Mac 桌面 E2E 构建成功，但 `traffic-analysis-main.e2e.ts` 在 `beforeEach` 的既有 `waitForMainRouteSettled` 等待中超时（0/1 specs 到达目标交互断言），因此本机未验证本次修改的菜单步骤；Windows Edge hermetic CI 是该步骤的权威验证。外接盘独立 target 避免了主卷空间限制。
- 风险/回滚和清理证据（如涉及宿主状态）：测试可靠性修正，不改变运行时或持久化行为；回滚为还原上述两个测试文件。本次只下载并检查了 PR E2E 诊断工件，未运行宿主网络或 Service/TUN 操作。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：新断言在 CI 通过后作为防复发检查；无治理门禁调整。

### LESSON-20261014-004：提交钩子必须继承隔离的 Cargo target 路径

- 日期与时区：2026-10-14 UTC
- 状态：Mitigated（本地提交 hook 已在外接盘 target 上通过；后续大仓库提交沿用该环境）
- 严重度：Low
- 模块：Development / CI / Rust
- PR head commit / 平台与测试环境：PR #481 `51aa4b2d733733ecdd8a89e00f451dfa1e7d9209`；Apple Silicon macOS，隔离 worktree
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)；首次 `git commit` 的 `lint-staged` 在 `pnpm lint:clippy` 子步骤因 `No space left on device` 失败。失败前主卷剩余 116 MiB；清理该 worktree `backend/target` 后释放 701.5 MiB。
- 预期行为：在本机磁盘紧张时，commit hook 的 Clippy 构建应复用调用者指定的外接盘 Cargo target，不在默认 worktree target 重建完整依赖。
- 真实异常、复现步骤与证据：先前 Rust/E2E 构建以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/codex-pr481-ci-fix.CC4isH` 执行；首次 `git commit` 未在该进程环境中传入此变量。lint-staged 因而创建 `backend/target`，空间耗尽后 Clang 和 rustc 报错。Git 钩子留下自动备份 stash，但已暂存的四个目标文件仍完整；专用 target 清理后主卷空间恢复。
- 确认的根因与排除项：`CARGO_TARGET_DIR=... cargo test` 只为那次 Cargo 子进程设置环境，变量没有自动成为交互 shell 的导出变量，所以后续独立的 `git commit` 钩子使用 Cargo 默认路径。证据是失败产物位于该 worktree 的 `backend/target`，清理后释放 701.5 MiB；未发现产品代码或 CI 构建失败。
- 修复提交、差异映射及保留的兼容性边界：失败后使用 `cargo clean --manifest-path backend/Cargo.toml` 清理本次专用 worktree target，再以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/codex-pr481-ci-fix.CC4isH CARGO_INCREMENTAL=0 AWS_LC_SYS_PREBUILT_NASM=1 git commit ...` 重跑 hooks。代码未修改 hook 或其门禁，也未改动主工作区 target。
- 原实现失败、新实现通过的回归测试，及测试注册位置：没有新增产品回归测试；hook 的 `pnpm lint:clippy` 在修正环境变量后实际执行并通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：首次提交 hook 失败；第二次提交 hook 的 oxlint、Prettier、`pnpm typecheck:e2e`、rustfmt、`pnpm lint:clippy` 全部通过。验证前后分别观察到主卷仅剩 116 MiB，以及清理后 792 MiB；未清理主工作区或其大型 Cargo target。
- 风险/回滚和清理证据（如涉及宿主状态）：清理只针对本任务 worktree 中由失败 hook 新建的 Cargo 构建目录，`cargo clean` 报告移除 3614 文件 / 701.5 MiB；无宿主网络、Service 或 TUN 操作。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：执行 Rust 构建后若 hook 也需编译，`git commit` 命令必须显式继承 `CARGO_TARGET_DIR`；无治理门禁调整。

### LESSON-20261015-001：跨窗口 E2E 就绪检查必须使用窗口对应的 UI 标记

- 日期与时区：2026-10-15 UTC
- 状态：Verified
- 严重度：Low
- 模块：E2E / main UI / legacy UI
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `c7d21d47a374009161f271c4512722c5c5cd8eae`、PR #482 测试前 head `24ea62a3f42458c31d0860b172396e34340050d3`、ref `c08aa9d9a17953aa2464cdae61390d5c3f9afbc7`；macOS Apple Silicon、WebKit 605.1.15，使用隔离 E2E runtime 和本地专用 app ID。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #482](https://github.com/MFSGA/Chimera/pull/482)；本地失败信息为 `The legacy Profile window did not render after navigation.`，运行产物位于本次任务的外接盘临时目录。
- 预期行为：main-first 套件在 main UI 验证完成后创建 legacy 窗口；测试须确认 legacy 前端已挂载，再打开 Profile 列表并验证共享 Profile。
- 真实异常、复现步骤与证据：首次添加就绪等待时检查 `[data-slot="app-root"]`。该标记只属于 main UI；legacy 页面有自己的根节点与 `data-testid="sidebar-route-profiles"` 导航项。main-first 本地运行在进入 legacy Profile 断言前超时，确认等待条件引用了错误窗口的 DOM 合同。
- 确认的根因与排除项：根因为 E2E 等待器误用 main-only 标记，不是 Profile 后端状态问题。首次尝试也遇到内部磁盘空间不足；将本次生成的构建缓存及临时目录放到外接盘后，排除了该环境限制。
- 修复提交、差异映射及保留的兼容性边界：PR #482 在 `tauri-e2e/specs/profile-auto-update.e2e.ts` 改为等待 legacy 专属侧栏项挂载后再导航；`tauri-e2e/README.md` 说明隔离 main-first 套件与默认 legacy 窗口生命周期的区别。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现为上述 main-first 本地 E2E 超时；修复后 `profile-auto-update-main` 在同一主机、独立 app ID 和外接盘 runtime 下通过 1/1。套件由 `isolatedE2eSuites` 注册，并由 `spec-suites.test.ts` 检查只运行一次。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --suite profile-auto-update-main --logLevel error`，1/1 通过；`pnpm --filter @chimera/tauri-e2e typecheck` 和 25 项 harness unit tests 通过。更新后的 Windows CI 仍须确认平台行为。
- 风险/回滚和清理证据（如涉及宿主状态）：用例只使用隔离 runtime 与 loopback 服务，并注册 Profile/窗口清理；没有启用系统代理或 TUN。回滚时可恢复上一版测试与 suite 配置。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：main 与 legacy 使用不同 DOM 标记；新增跨窗口用例前须从目标页面源码确认挂载信号。没有调整门禁。

### LESSON-20261015-002：路由就绪等待应限定到当前用例需要的出口

- 日期与时区：2026-10-15 UTC
- 状态：Open（PR #481 的 Windows 新 SHA 复验待完成）
- 严重度：Low
- 模块：E2E / main UI
- master commit / ref gitlink SHA / 平台与测试环境：master `c7d21d47a374009161f271c4512722c5c5cd8eae`；PR #481 旧 head `4431e7e641626c73c44ded27a8f56220d05a3d62`；ref `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；GitHub Actions Windows Edge hermetic E2E，另有本机 macOS WebKit 定向运行。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #481](https://github.com/MFSGA/Chimera/pull/481)，[Windows Desktop E2E run 38668811814](https://github.com/MFSGA/Chimera/actions/runs/38668811814)。该 run 的 53 个 specs 中 52 通过；`rules-main-proxy-icon.e2e.ts` 的 `before` 在 `openMainRoute('/main/rules')` 等待 15 秒后报 `The Chimera main route animation did not settle.`
- 预期行为：路由进入后按当前用例需要确认页面准备完成；目标业务结果另行等待并断言。
- 真实异常、复现步骤与证据：规则图标用例在 profile 创建、激活和图标断言前失败。它调用的共享 helper 会等待主路由透明度，并要求 settings、providers、profiles、proxies 等所有当前存在的嵌套 outlet 都恰有一个子节点。CI 日志只证明整体等待超时，没有指出哪个条件未满足；因此尚不能确认嵌套路由是失败原因，也不据此认定 Rules UI 或 profile 图标行为异常。
- 确认的根因与排除项：根因尚未确认；主路由透明度条件、嵌套 outlet 条件及 Windows 时序都不能从现有日志排除。由于该规则图标用例的契约不依赖其他嵌套路由，修复仅让它跳过这些额外 outlet 等待，同时保留其后对 runtime proxy group 与已加载 SVG 图标的明确等待。本机定向 WebKit 运行达到 1/1。该诊断运行使用当前 profile-sync E2E 二进制；Rules 页面源文件与 PR #481 相同，但导航/button/route tree 有分支差异，Windows 新 SHA 仍是决定性复验。
- 修复提交、差异映射及保留的兼容性边界：PR #481 的 `tauri-e2e/specs/main-window.ts::openMainRoute` 增加仅供特定测试选择的 `waitForNestedOutlets` 选项，默认行为不变；`rules-main-proxy-icon.e2e.ts` 关闭无关嵌套出口等待，随后继续执行原有 runtime 与 SVG 加载断言。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败为上述 Windows run；修改后的 `rules-main-proxy-icon.e2e.ts` 已注册在 `main` / `hermetic`。本机定向命令 `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/rules-main-proxy-icon.e2e.ts --logLevel error` 结果 1/1 通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：E2E harness TypeScript 检查通过，注册的 harness 单测 25/25 通过，Prettier 和 `git diff --check` 通过；本机 WebKit 定向规则图标用例 1/1 通过。新 SHA 的 Windows Edge hermetic run 尚未执行；不将 Mac WebKit 结果当成 Windows 复验。
- 风险/回滚和清理证据（如涉及宿主状态）：仅测试 helper 与单个用例，未改变应用 runtime 或宿主网络；回滚为恢复原先无参数的 `openMainRoute` 调用。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：LESSON-20261014-003 也记录过主路由动画等待失败；后续将结合 Windows 新 SHA 结果判断应否进一步调整共享等待条件。当前约束是每个测试只等待其契约所需的 outlet，业务结果使用独立权威断言。无门禁变化。

### LESSON-20261015-003：Cargo 构建产物必须按 worktree 隔离

- 日期与时区：2026-10-15 UTC
- 状态：Mitigated（交付记录后在 PR #482 自己的 worktree 重建其 target）
- 严重度：Low
- 模块：Development / Rust / E2E
- master commit / ref gitlink SHA / 平台与测试环境：PR #481 head `4431e7e641626c73c44ded27a8f56220d05a3d62` 与 PR #482 worktree `/Volumes/ChimeraBuild/chimera-profile-sync`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：PR #481、PR #482；本地尝试精确构建 PR #481 的 E2E binary 时失败，类型错误涉及 `RemoteSyncInterval` 与 `u64`。
- 预期行为：每个分支/worktree 使用仅由该 worktree 构建的 Cargo target，避免依赖产物与另一 checkout 的源码/API 不一致。
- 真实异常、复现步骤与证据：在 PR #481 worktree 运行 `pnpm --filter=chimera-ui build` 后，以 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/chimera-profile-sync/backend/target/e2e` 编译 PR #481。其 `chimera-config` 源码使用 `u64`；但该 target 中 `chimera_config` 的 dep-info 指向 PR #482 worktree，归档 rlib 还包含 `RemoteSyncInterval` 符号，随后 Tauri 编译产生 4 个与该 API 不匹配的错误。
- 确认的根因与排除项：把 PR #482 的 target 目录复用于另一 worktree，造成 Cargo 复用了来自不同源码树的 workspace crate 产物。该失败不是 PR #481 Windows CI 日志中的规则图标超时，也不是 source tree 修改；此前 PR #481 的远端 Build Tauri 在旧 SHA 通过。
- 修复提交、差异映射及保留的兼容性边界：没有产品代码修复。后续为 PR #481 使用独立 target；已经受污染的 PR #482 target 仅在其原 worktree 内重建恢复。cargo target 不再跨 worktree 共享。
- 原实现失败、新实现通过的回归测试，及测试注册位置：这是验证环境的过程错误，没有新增产品测试；在各自 worktree 用独立 target 成功构建后才可宣告该构建通过。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：首次跨 worktree 构建失败（E0308/E0369/E0277）；PR #481 目标用例在单独已有 E2E binary 上通过 1/1，但这不能作为 PR #481 本地 build 成功证据。PR #482 自己的 target 后续在原 worktree 恢复构建成功，二进制 SHA-256 `dd0c94f945119e9ce5e72839585deaf96349f36fa155f4a0200bdb082383d8ad`；随后该分支的 profile suite 在此二进制上通过 1/1。隔离构建目录并分别验证两项已完成；该过程错误没有产品测试覆盖。
- 风险/回滚和清理证据（如涉及宿主状态）：没有清理或删除任何 Cargo target；只产生/复用了 Cargo 编译产物，主源码未改。以后需要的 target 放在各自的 worktree 子目录或独立外接盘目录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：LESSON-20261014-004 涉及 hook 未继承 CARGO_TARGET_DIR；本次另外确认 target 本身也不能跨 worktree 共用。无 CI 门禁变化。

### LESSON-20261016-001：未准备的 worktree 中 pnpm 命令会触发依赖安装

- 日期与时区：2026-10-16 UTC
- 状态：Mitigated（本次安装已中断；留下的虚拟 store 未清除，以免误删未知的预存依赖）
- 严重度：Low
- 模块：Development / E2E / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #486 head `8d6a6dfb9e2d0ec19bde94f8dd77a2eeb6e9c78d`；ref gitlink `685042aaedaea26a591900be0b494dfe8efd79b0`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #486](https://github.com/MFSGA/Chimera/pull/486)，本机检查期间 `pnpm exec prettier` 输出自动依赖验证与安装进度；此前同日一次 `pnpm --filter @chimera/tauri-e2e exec wdio` 也触发依赖安装，并在 `Chimera_Client/target/debug` 启动 Rust 编译。
- 预期行为：运行检查时只调用选定的检查工具，不启动隐式安装或与检查无关的 Rust 构建。
- 真实异常、复现步骤与证据：在新 E2E worktree 中并行启动 `pnpm --filter @chimera/tauri-e2e typecheck` 与 `pnpm exec prettier --check ...`。后者输出 lockfile verification、依赖解析与 package cloning 进度；两条命令立即以 Ctrl-C 中断。此前的 WDIO exec 安装/编译也在约 84 秒后中断。当前 worktree 内 `.pnpm` 虚拟 store 显示约 1.8 GiB 逻辑内容；系统可用空间从约 1.2 GiB 降到 1.1 GiB。中止后未发现 pnpm、cargo 或 rustc 进程，也没有新增跟踪文件。
- 确认的根因与排除项：调用 pnpm 的 exec/run 入口时，当前环境因该 worktree 依赖链接未准备完成而开始自动依赖验证和安装；并行 pnpm 命令共用这一准备过程。该现象是本地验证流程失误，不是产品失败或 CI 失败。
- 修复提交、差异映射及保留的兼容性边界：没有产品代码改动。本次改用预装的 Prettier 和 TypeScript 二进制，并在 `node_modules` 中临时链接主 checkout 的准备依赖完成检查，随后移除这些临时链接；没有清理无法区分归属的 `.pnpm` 虚拟 store。
- 原实现失败、新实现通过的回归测试，及测试注册位置：这是本地执行流程错误，不适用产品回归测试；不能通过重试 pnpm 命令来证明该流程安全。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：中断后确认没有本次依赖安装或构建进程；直接 TypeScript harness 检查通过、unit tests 25/25、Prettier 与 `git diff --check` 通过。没有本机构建最新 E2E binary；该分支的 Windows CI 才能验证当前 app/test 组合。
- 风险/回滚和清理证据（如涉及宿主状态）：未修改跟踪源码以外的配置；测试 runtime/artifacts 使用仓库 `cleanupRuntimeDirectory` 安全删除。依赖虚拟 store 保留在隔离 worktree 中，系统空间仍约 1.1 GiB 可用；未触及其他 worktree 或用户 app。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：此前另一 worktree 的 WDIO exec 已出现相同安装副作用，故记录为重复流程错误。开始运行 pnpm 检查前须确认工作树依赖已准备；否则从同一 checkout 的现成二进制运行单项检查，避免 `pnpm exec` 和并行安装。没有修改 CI 或治理门禁。

### LESSON-20261016-002：行边界状态必须在渲染组件中完整解构

- 日期与时区：2026-10-16 UTC
- 状态：Mitigated（提交钩子已拦截并修复；PR #484 的新 SHA 桌面 E2E 仍待验证）
- 严重度：Low
- 模块：Frontend / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #484 基线 master `6b5fba41c867ab6c1e39f501f78aa037db96e320`、ref gitlink `e81eeaea46a9c229478cd29c9555b5c072f7e6cc`；macOS arm64 本地开发环境；上游 UI 源码读取自 ref `1a0372280a405a824353ac2fa6a5f48a3bca4e18`。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #484](https://github.com/MFSGA/Chimera/pull/484)；本地首次提交尝试的 git hook `pnpm typecheck:chimera` 输出报告 `transform-chain-column.tsx` 中找不到名称 `isLast`。该失败发生在提交创建前，因此没有错误版本的 commit SHA；修复后的实现提交为 `88ea658a3954ffd3df73b94168d4c798e1d46154`。
- 预期行为：transform-chain 最后一行的 Move down 操作禁用，其他行按行索引计算边界；每个行组件都必须接收并解构用于该边界的 `isLast` 属性，且提交前类型检查应阻止未定义标识符进入提交。
- 真实异常、复现步骤与证据：首次提交尝试的工作区中，`ItemContent` 渲染逻辑引用了 `isLast`；提交钩子的 `pnpm typecheck:chimera` 报告该标识符未定义并拒绝提交。失败现场没有保存，因此不记录当时参数解构的其他字段。修复代码将 `isLast` 纳入组件参数解构，类型检查通过。
- 确认的根因与排除项：`ItemContent` 使用的行尾状态未在该组件作用域中声明。不是 ref 的行为差异，也不是生成文件或运行时状态造成的错误。
- 修复提交、差异映射及保留的兼容性边界：PR #484 的 `frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/transform-chain-column.tsx` 将 `isLast` 加入 `ItemContent` 解构，保持 ref 按行计算首尾状态的分解，并禁用最后一行的无效下移操作。修复包含在提交 `88ea658a3954ffd3df73b94168d4c798e1d46154` 中。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现被提交钩子 `pnpm typecheck:chimera` 拒绝；修复后的 `pnpm typecheck` 与生产构建均通过。边界行为也由 `tauri-e2e/specs/profile-transform-chain-ui.e2e.ts` 的重排断言覆盖，该 spec 属于 `profiles`、`critical` 和 `hermetic`。新 SHA 的 Windows Edge 桌面结果待 PR #484 CI。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：修复后 `pnpm typecheck`、`pnpm --filter=chimera-ui build`、E2E harness typecheck、harness 单测（25/25）、`pnpm lint:frontend-boundaries`、格式检查、`pnpm prepare:check` 和 `git diff --check` 通过。初始 SHA 的远端 Windows Edge profiles E2E 尚在排队；加入本经验记录会产生新提交 SHA，必须重新验证新 SHA，不复用旧 SHA 的结果。
- 风险/回滚和清理证据（如涉及宿主状态）：L3 工程经验记录；不改产品运行行为、用户数据或宿主状态。回滚只需还原本条索引和记录。维护者已明确批准添加并推送该记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：已有提交 hook 将 `pnpm typecheck:chimera` 作为 staged frontend 文件检查；继续保留该门禁，UI 边界状态变更需复核 prop 声明、传参和渲染解构三处。无 CI 门禁变化。

### LESSON-20261017-003：桌面 E2E 应以目标窗口和用户可见状态准备路由

- 日期与时区：2026-10-17 UTC
- 状态：Verified（历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows Desktop E2E 及隔离套件通过）
- 严重度：Low
- 模块：E2E / main UI / WebKit
- master commit / ref gitlink SHA / 平台与测试环境：历史通过 PR #489 head `13699341fc9389b4c18369609618e2fc944dddc3`，当时 base `aed52c17959ff1a332e7a8c165cf8fda987b3faa`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Apple Silicon macOS/WebKit 605.1.15 与 GitHub Actions Windows Edge 154，使用隔离 Cargo target、runtime 和 artifact 目录。
- 关联 Issue / PR / CI / 运行日志（脱敏）：本地 `traffic-analysis-main.e2e.ts` 定向 WebKit 调试；早期 Windows 失败见 [CI run 39015988294](https://github.com/MFSGA/Chimera/actions/runs/39015988294) 和 [run 39034891250](https://github.com/MFSGA/Chimera/actions/runs/39034891250)；精确最终 SHA 的 [Windows Desktop E2E run 39037138976](https://github.com/MFSGA/Chimera/actions/runs/39037138976) 通过。
- 预期行为：用例先选中 main 窗口，再设置视口并通过可见导航进入稳定 Dashboard；菜单操作使用 WebKit 支持且可观察的真实键盘激活，随后断言 URL 与控件状态。
- 真实异常、复现步骤与证据：最初的路由 helper 在页面内容已渲染时仍因要求恰有一个 `app-content` host 超时，诊断读数为两个 host 且各有一个子节点、透明度均为 1。另在选择 main 窗口前设置视口会在切换后仍显示移动导航，Radix 动画期间直接查找选项也不稳定。Windows Edge 154 在展开侧栏后 `aria-expanded` 为 `true`，但通过此前获取的 WDIO 元素句柄读到的 `aria-label` 为空，失败于 `assert.ok(expandedLabel)`（run `39015988294`）。复核本机运行时发现 `captureFailedTest` 把 WebdriverIO 9.32/Mocha 9.32 `afterTest` 的第二个参数（Mocha context）误当作结果对象；`passed` 因而是 undefined，成功用例也生成截图/HTML。此前据这些产物判断本机失败没有依据。旧运行日志中的滚动 `execute/async` 超时随后走了 WebdriverIO 的 `scrollIntoView` fallback，不能单独作为用例失败证据。随后 Windows Edge run `39034891250` 在展开侧栏后仍观察到 `aria-expanded=false`、标签为折叠状态；截图只捕获到一个 traffic page，因此该产物不能证明失败由退出中的旧路由导致。
- 确认的根因与排除项：路由准备条件、窗口选择顺序和动画中选项定位问题已有证据。Windows 旧 SHA 的失败是否由跨重渲染复用元素句柄造成尚未独立证明；第二次展开失败的具体触发原因也未从产物中确认。应用的 `AnimatedOutletPreset` 使用 `AnimatePresence` 保留退出路由，旧页面短暂留在 DOM 是已确认的生命周期行为；历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows hermetic E2E 已通过新路由稳定等待和侧栏状态读取。没有证据显示早期失败触发了宿主网络或持久状态变化。
- 修复映射及保留边界：`tauri-e2e/specs/traffic-analysis-main.e2e.ts` 先调用 `ensureMainWindow()`，再设置视口；需要回到 Dashboard 时通过可见 navbar 导航并等待稳定页面标记，且等待退出中的 traffic page 离开 DOM；进入 Topology 后要求恰有一个 traffic page，再从该页定位侧栏控件。指标菜单用触发器 focus 与 Enter/ArrowDown/Enter；侧栏切换后的 ARIA 状态现在由当前 DOM 快照读取。所有点击仍由 WDIO 对可见控件执行，共享路由 helper 与业务断言未放宽。
- 回归验证及注册位置：用例保留在 `main` suite，覆盖 Traffic 导航、范围/指标 URL、Flow/Geography 筛选保留及侧栏标签切换。侧栏属性改为每次从当前 DOM 读取并重定位控件。历史通过 SHA `13699341fc9389b4c18369609618e2fc944dddc3` 的 Windows hermetic E2E run `39037138976` 为 53/53 通过；Profile auto-update 与 controller fallback 各 1/1 通过。
- 验证命令与覆盖：在修正 `afterTest` 参数顺序后，执行 `CHIMERA_E2E_BINARY=/Volumes/Ventoy/Chimera/traffic-analysis-target/debug/chimera CHIMERA_E2E_RUNTIME_DIR=/Volumes/Ventoy/Chimera/traffic-analysis-runtime-20261017-final-evidence CHIMERA_E2E_ARTIFACT_DIR=/Volumes/Ventoy/Chimera/traffic-analysis-artifacts-20261017-final-evidence pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/traffic-analysis-main.e2e.ts`，macOS WebKit 605.1.15 报告 1/1 通过；隔离产物目录仅含运行日志，没有失败截图/HTML。二进制 SHA-256 `920c0cec03ff6cd854dc13e1ffb02c167bc866fb136d8aa8cc56a4caebb5e84e`。GitHub Windows Desktop E2E run `39037138976` 精确测试 PR SHA，53/53 specs 通过；精确 SHA 的 CI run `39037138645` 中三平台 lint 和 Tauri 构建全部通过。这些链接只证明历史 SHA `13699341fc9389b4c18369609618e2fc944dddc3`；当前验收状态以 PR #489 最新检查为准。
- 风险/回滚和清理证据：用例只改变其隔离窗口尺寸和 route；runtime、artifact、cargo target 均位于本次独立目录，没有改用户 Profile、系统代理或 TUN。回滚为恢复该 spec 之前的路由准备和菜单选择步骤。
- 后续防复发措施：跨窗口用例先确保目标窗口被选中再设置视口；路由断言等待业务可见标记及退出路由清理；在交互前确认当前路由只有一个目标页；动画控件用稳定的用户输入流程；状态更新后从当前 DOM 读取属性，并在等待失败时保留最后观察值。E2E 失败钩子需按当前 WDIO 版本读取第三个结果参数，并用 reporter 状态与产物交叉核对；LESSONS 条目 ID 保持唯一，已删除重复的 `LESSON-20261017-003` 副本。没有调整 CI 或权限门禁。

### LESSON-20261017-004：前端-only CI 门禁不得直接编译 Tauri Rust 测试

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #489 当前 SHA 的 Ubuntu 后端工作区测试和前端 harness 测试通过；Desktop E2E 的独立失败记录在 LESSON-20261017-003）
- 严重度：Low
- 模块：CI / Linux / Rust
- PR head commit / ref gitlink SHA / 平台与测试环境：PR #489 失败 SHA `b46ae47111ad90ea2c3e869c3a9cf397b88fdd9a`；修复 SHA `4f5d216b4f64890b676a59e27a4821b2d3ce1398`；本地 ref `a4f706fe788e3570247ad2fa49636c6b4c55c98b`；GitHub Actions `ubuntu-latest`。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #489](https://github.com/MFSGA/Chimera/pull/489)，[失败 CI run 39014838489](https://github.com/MFSGA/Chimera/actions/runs/39014838489)，[修复后 CI run 39015789669](https://github.com/MFSGA/Chimera/actions/runs/39015789669)，[当前 SHA 的 CI run 39015988284](https://github.com/MFSGA/Chimera/actions/runs/39015988284)，[当前 SHA 的独立桌面 E2E run 39015988294](https://github.com/MFSGA/Chimera/actions/runs/39015988294)。初始失败 job 在 `Test E2E harness and frontend models` 执行 Rust 测试时，`glib-sys` 的 `pkg-config` 找不到 `glib-2.0`，退出码 101。
- 预期行为：前端-only job 只执行前端与 E2E harness 检查；需要 Tauri 系统库的 Rust 测试只在安装了 Tauri 依赖的后端 job 运行。
- 真实异常、复现步骤与证据：前端测试步骤额外执行 `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::history::tests`。Linux 的 `Tauri dependencies` 步骤仅在 `BACKEND == true` 时安装 GTK/GLib，因此纯前端改动未准备这些库便开始编译 Tauri crate。
- 确认的根因与排除项：该 agent history 单测被放在 `FRONTEND == true` 门禁中，和 Rust/Tauri 依赖的后端测试环境不匹配；并非测试断言或产品逻辑失败。后端门禁中的 `pnpm test:backend` 已运行 workspace 全部 crate 和 all features，包含该单测。
- 修复提交、差异映射及保留的兼容性边界：提交 `4f5d216b` 从 `Test E2E harness and frontend models` 移除重复 Rust 命令；保留受 `BACKEND == true` 控制的 `pnpm test:backend`，因此后端改动仍执行该覆盖并在 Ubuntu 先安装 Tauri 依赖。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现的失败证据为 CI run `39014838489`。PR #489 当前 SHA 的 CI run `39015988284` 中，Ubuntu `Test E2E harness and frontend models` 与 `Test backend workspace` 均通过；后者包含该 agent history Rust 单测。桌面 E2E run `39015988294` 的 Traffic 用例失败属于独立的测试交互问题，不影响此工作流修复结论。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：本地 `pnpm exec prettier --check .github/workflows/ci.yaml`、`actionlint -ignore SC2086 .github/workflows/ci.yaml` 与 `git diff --check` 通过；PR #489 当前 SHA 的 Ubuntu 后端工作区和前端 harness 测试均通过。未在本机执行 Ubuntu 后端工作区测试；其他平台构建仍在执行。未忽略时 actionlint 只报告原有 `STORE_PATH` 导出脚本的 SC2086，与本次差异无关。
- 风险/回滚和清理证据（如涉及宿主状态）：仅调整测试所在的 CI 步骤，不改变测试断言、产品行为或权限；回滚时可恢复该 Rust 命令到前端步骤。没有宿主网络、Service、TUN 或个人配置变更。
- 后续防复发措施：新增 Rust/Tauri 测试时，确认它位于 `BACKEND == true` 且 Tauri 系统依赖已安装的后端门禁；避免在前端-only job 重复执行 workspace 已覆盖的测试。

### LESSON-20261017-005：视口之外的编辑器不依赖平滑滚动完成定位

- 日期与时区：2026-10-17 UTC
- 状态：Verified（修复代码在 PR #484 head 的 Windows Desktop E2E 中通过；lesson-only 新提交仍需 CI）
- 严重度：Low
- 模块：Profile / E2E / Windows WebView2
- master commit / ref gitlink SHA / 平台与测试环境：PR #484 base `ae48f24ae5012a47533784695d7109cafb753d12`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Windows GitHub Actions，Microsoft Edge 154。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #484](https://github.com/MFSGA/Chimera/pull/484)；[失败 run 38962020671](https://github.com/MFSGA/Chimera/actions/runs/38962020671) 的 head 为 `b66eb2ffc8b08d3c6de8c249ea609fac9e4e0b5d`；[通过 run 39018873894](https://github.com/MFSGA/Chimera/actions/runs/39018873894) 的 head 为 `62e561fb149c675f1f39dc8e1ba4132d026de287`。
- 预期行为：用户打开 Profile transform-chain 编辑区时，即使编辑区在当前滚动视口之外，也应把目标定位到可见区域；测试清理应恢复视口位置，避免影响后续用例。
- 真实异常、复现步骤与证据：Windows Edge 154 的 360px 高窗口中，点击 Profile transform-chain 操作后等待编辑区出现。失败日志报告 `editorTop=335`、`visibleBottom=351`、`maxScrollTop=820`，但 `scrollTop=49`，因此编辑区仍未进入可用视口；目标断言超时，所属 hermetic 运行共 51 个 spec 通过、2 个失败。失败来自 E2E 对平滑滚动完成位置的预期，不代表本次 Profiles 数据或持久化失败。
- 确认的根因与排除项：在 CI 的小视口下依赖 `scroll-behavior: smooth` 动画没有稳定到达目标；有界等待结束时滚动仅推进 49px。没有证据表明 Profile transform-chain 状态或持久化被破坏。
- 修复提交、差异映射及保留的兼容性边界：`frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/action-card.tsx` 计算目标偏移后临时将视口 `scroll-behavior` 设为 `auto !important` 并直接设置 `scrollTop`，随后恢复原样式；`tauri-e2e/specs/profile-transform-chain-ui.e2e.ts` 在失败清理时将 Profile 视口复位到顶部。保留窗口尺寸和可见性断言，不把定位动画当成用户业务结果。
- 原实现失败、新实现通过的回归测试，及测试注册位置：失败 run 38962020671 在 `profile-transform-chain-ui.e2e.ts` 的视口定位等待处失败；修复后该 spec 随 hermetic suite 在 run 39018873894 对 PR head `62e561fb` 通过。测试继续注册在 `profiles`、`critical` 和 `hermetic` suite；没有新增跳过或重试。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：GitHub Windows Desktop E2E run 39018873894 对应精确实现 head `62e561fb149c675f1f39dc8e1ba4132d026de287`，结论 success。新增本条仅修改 LESSONS.md；该 lesson-only 提交的 CI/E2E 仍待新 SHA 运行，不能复用为新 SHA 的检查通过证据。
- 风险/回滚和清理证据（如涉及宿主状态）：仅新增工程经验记录，不改变产品状态、持久数据或宿主设置；回滚时移除本条及索引行即可。失败清理现在显式复位测试窗口滚动位置。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：为视口之外的元素设计桌面 E2E 时，断言目标可见状态与所需滚动距离；避免等待未经验证的平滑动画到达最终位置，并在失败清理中恢复测试拥有的滚动状态。维护者已批准此 L3 lesson；没有修改 CI 门禁。

### LESSON-20261017-002：Profile producer 启动必须等待 actor 注册处理完成

- 日期与时区：2026-10-17 UTC
- 状态：Verified（PR #485 精确 head `c29185ad9b5d62cf6802db5ee0448cf85c309168` 的 CI/E2E 通过，且已合并）
- 严重度：Low
- 模块：Profile / Rust / Core startup / CI
- master commit / ref gitlink SHA / 平台与测试环境：PR #485 修复提交 `d1c78138`、文档修正提交 `e0629eb9`；ref gitlink `31019da14006b384b3272c72b4088179a9e72c47`；Apple Silicon macOS。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #485](https://github.com/MFSGA/Chimera/pull/485)，精确 head `c29185ad9b5d62cf6802db5ee0448cf85c309168` 的 [CI run 39037029299](https://github.com/MFSGA/Chimera/actions/runs/39037029299) 与 [Desktop E2E run 39037029364](https://github.com/MFSGA/Chimera/actions/runs/39037029364) 均通过；合并提交 `5fc556b4dc1b4724c3486d537b3d239d89c97c5e` 与该已测 head 的 tree 相同。
- 预期行为：核心启动协调确认终态后，Profiles actor 完成后台 jobs、外部文件 watcher 和 journal reconciliation 的启动尝试，再让调用方继续；测试推进虚拟时间或修改被监视文件时，actor 必须已处理 producer 启动消息。
- 真实异常、复现步骤与证据：原 `ProfilesClient::start_producers()` 只向 actor cast `StartProducers` 后立即返回。新增 profile suite 中，`external_background_commit_does_not_bypass_the_mutation_participant` 在并行运行中出现竞态，定时刷新用例会等待超时；单独重跑外部文件用例可通过，而单独运行 `import_suggestion_is_committed_and_reschedules_the_timer` 会卡在虚拟时钟等待。测试在 actor 尚未处理 cast 前就推进 Tokio paused time 或写入文件，故本地结果取决于 actor 调度时序。
- 确认的根因与排除项：cast 只保证消息排入 actor mailbox，不保证 handler 已运行；调用方把消息投递误当作 scheduler/watcher 已就绪。不是 ref 的预期功能差异或持久化问题。ref `31019da` 的 `ProfilesClient::start_producers` 也使用 cast；本 PR 增加 Chimera 内部 request/reply await 作为启动同步边界，差异已记录在 `docs/ref-alignment-profiles.md` DIFF-PROFILE-06。
- 修复提交、差异映射及保留的兼容性边界：`d1c78138` 将 `StartProducers` 改为带 reply 的 actor call，`ProfilesClient::start_producers()` 等待回复，`ChimeraClient::init_core()` 在 lifecycle terminal reply 后等待该调用；重复 release 仍保持幂等。文档更正提交 `e0629eb9` 说明这是 best-effort 注册尝试完成，不保证各个 watcher/job 均成功注册。无公共 IPC、持久化格式或 agent contract 变化。
- 原实现失败、新实现通过的回归测试，及测试注册位置：cast-only 版本在本地 profile suite 中发生上述 scheduler/watcher 时序失败；await 回复后 `backend/tauri/src/client/profiles.rs` 的 Profile 测试全套通过。该修复没有新增独立测试入口，而是让现有生产者启动、定时刷新与外部 watcher 测试等待真实 actor 完成边界。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo +nightly test --manifest-path backend/Cargo.toml --locked -p chimera startup_reconcile -- --nocapture` 通过 2/2；`cargo +nightly test --manifest-path backend/Cargo.toml --locked -p chimera client::profiles::tests:: -- --test-threads=4` 通过 72/72；`cargo clippy --manifest-path ./backend/Cargo.toml --all-targets --all-features`、包级 rustfmt、Prettier 与 `git diff --check` 通过。提交钩子的 rustfmt/Clippy 也通过。Reviewer 静态审查无 P0–P2；其独立测试复跑因主机卷空间不足无法完成。本机桌面 startup/UI E2E 未运行；远端 CI/E2E 已对合并所用的相同 tree 完成验证。
- 风险/回滚和清理证据（如涉及宿主状态）：回滚为恢复原 cast-only 启动调用及无 reply 的 actor message；这会重新允许调用方早于 producer handler 完成而继续。未修改主机网络、持久化数据或用户 Profile。审查器新建的失败 Cargo target 已经 cargo clean 清理，未触碰其他构建缓存。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：仓库维护者已批准将已确认的竞态写入 `LESSONS.md`。保留 producer startup 测试中的 await 边界；PR #485 的精确 head CI/E2E 已通过后更新本条状态；不修改治理门禁。

### LESSON-20261017-006：Dashboard 右键测试必须先发送 pointerdown

- 日期与时区：2026-10-17 UTC
- 状态：Open（驱动修复已在 PR #490 工作树完成；新 SHA 的 Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：master `5fc556b4dc1b4724c3486d537b3d239d89c97c5e`；ref `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge hermetic E2E
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39072145858](https://github.com/MFSGA/Chimera/actions/runs/39072145858)，52 个 spec 中 50 个通过；Dashboard 编辑菜单和后续 Agent 页面导航失败。
- 预期行为：右键 Dashboard 后，上下文菜单显示已注册的 Dashboard 操作，后续编辑流程可继续。
- 真实异常、复现步骤与证据：Windows E2E 右键后菜单等待为 false；失败截图显示应用通用剪贴板菜单，没有 Dashboard 项。随后 Agent 页就绪等待也失败。
- 确认的根因与排除项：ref 与 Chimera 的共享菜单 provider 都在 document capture 阶段通过 `pointerdown` 记录右键目标，再沿 DOM 查找已注册菜单。vendored 驱动只合成 `mousedown`、`mouseup` 和 `contextmenu`，没有触发该记录逻辑。对照 ref `frontend/nyanpasu/src/components/providers/context-menu-provider.tsx` 与 Chimera `frontend/chimera/src/components/providers/context-menu-provider.tsx` 后，未发现 Dashboard 菜单实现差异；Agent 页面失败发生在前一用例之后，待菜单修复后复验是否为连带失败。
- 修复提交、差异映射及保留的兼容性边界：`backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/platform/executor.rs::dispatch_pointer_event` 在按下时、对应 MouseEvent 前合成冒泡且可取消的 `pointerdown`，保留匹配右键释放后的 `contextmenu`。该修复验证应用 DOM 事件路径，不代表完整 PointerEvent 序列或真实鼠标输入。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现由上述 Windows PR E2E 运行失败；新 SHA 的复验仍待执行。相关回归仍由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 的 Windows hermetic suite 运行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：`cargo check --locked --manifest-path backend/tauri/vendor/tauri-plugin-wdio-webdriver/Cargo.toml --target-dir /tmp/chimera-wdio-plugin-check` 通过；`rustfmt --edition 2021 .../executor.rs` 与 `git diff --check` 通过。Windows E2E 尚未对修复 SHA 运行；真实可信鼠标输入不在覆盖范围。
- 风险/回滚和清理证据（如涉及宿主状态）：仅测试驱动合成事件和测试证据文档；不修改产品菜单、持久化或宿主网络。回滚为还原驱动 PointerEvent 合成并移除此记录。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：更新 [U09](docs/testing/upstream-evidence.md#u09-tauri-driver-合成-pointerevent-与右键上下文菜单边界) 及 Dashboard 测试契约，后续升级驱动时重跑相同菜单最小复现；无 CI 门禁调整。

### LESSON-20261017-007：等待可观察的失败状态并在重渲染后重新查询控件

- 日期与时区：2026-10-17 UTC
- 状态：Open（修复已在 PR #490 工作树完成；新 SHA 的 Windows Desktop E2E 待复验）
- 严重度：Low
- 模块：E2E / Windows
- master commit / ref gitlink SHA / 平台与测试环境：失败 head `ce86b993fcc9f21d863abd0c59e61c00bf192d00`；ref gitlink `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`；Windows Edge hermetic E2E。
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #490](https://github.com/MFSGA/Chimera/pull/490)，[失败运行 39074305271](https://github.com/MFSGA/Chimera/actions/runs/39074305271) 与 [39077318693](https://github.com/MFSGA/Chimera/actions/runs/39077318693)，后者 52 个 spec 中 50 个通过；Dashboard retry 和 main input 两个 spec 失败。
- 预期行为：注入存储读取故障后，Dashboard 应显示 `load-error`，释放故障后 Retry 应恢复；外部控制器弹窗出现后，输入框聚焦应显示浮动标签。
- 真实异常、复现步骤与证据：两次运行均未观察到 Dashboard `load-error`；39077318693 的失败截图显示页面进入编辑模式但无错误提示。main input 在同一运行断言 DOM 矩形高度为 56px 时得到 53px；失败诊断同时显示计算样式高度为 56px。
- 确认的根因与排除项：Dashboard 前一版用例未证明新挂载的 Dashboard provider 实际调用了被注入失败的存储命令，因而没能证明错误路径执行；这不单独证明产品读错误处理存在缺陷。Windows WebView 的元素矩形与计算 CSS 尺寸不同，`getBoundingClientRect()` 的 53px 不表示 `h-14` 样式错误，计算样式仍为 56px。
- 修复提交、差异映射及保留的兼容性边界：Dashboard 用例先确认离开路由后容器已卸载，再安装故障并导航回来；增加失败调用计数，等待 `get_storage_item` 确实被拦截后再断言可见错误，仍在错误状态出现后才释放故障。main input 继续断言 `h-14` 的 56px 计算样式及浮动标签等可观察行为，不再要求平台缩放后的矩形恰好为 56px。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败于上述 Windows run；修复后的同一用例仍由 `tauri-e2e/specs/dashboard-main-layout.e2e.ts` 和 `tauri-e2e/specs/main-input.e2e.ts` 注册在 hermetic suite，精确新 SHA 的 Windows 复验待执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：修复后本机 Prettier 检查、`tsc --noEmit -p tauri-e2e/tsconfig.json` 和 `git diff --check` 通过；新 SHA 的 Windows E2E 尚未执行。未声称已验证真实鼠标输入或弹窗在其他平台的行为。
- 风险/回滚和清理证据（如涉及宿主状态）：只调整 E2E 故障注入和等待方式；恢复原 Tauri invoke、KV 配置及 UI 状态由现有清理逻辑负责。没有宿主网络、Service、TUN 或个人配置变更。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：等待被测状态而非单次注入后的固定时序；发生等待失败时结合失败前页面产物定位，避免只凭超时推断产品缺陷。新 SHA 的 Windows 复验通过后更新状态。
