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

| 编号                | 日期       | 模块          | 问题与证据                                                                                                                     | 状态     |
| ------------------- | ---------- | ------------- | ------------------------------------------------------------------------------------------------------------------------------ | -------- |
| LESSON-20261014-001 | 2026-10-14 | Rust / CI     | macOS epoch PID 集成用例第二次 spawn 收到 `NotFound`；PID 文件读取竞态是候选根因，PR #479 远端复验待完成                       | Open     |
| LESSON-20261014-002 | 2026-10-14 | Proxies / E2E | Windows 桌面 E2E 激活配置后仍显示旧代理组图标；后端缓存未随 profile 刷新事件失效，PR #479 新 SHA 远端复验待完成                | Open     |
| LESSON-20261015-001 | 2026-10-15 | Rust / CI     | profile 导入后的调度测试在固定 200 次 yield 内未观察到第二次请求；改为等待提交结果并设置真实时间上限，PR #483 新 SHA CI 已通过 | Verified |

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

### LESSON-20261015-001：profile 导入后的调度测试使用固定轮询次数

- 日期与时区：2026-10-15 UTC
- 状态：Verified（测试修正后的 PR #483 SHA `1b976851cfe9b2735948204d5292d3b30e798018` 的 CI 已通过）
- 严重度：Low
- 模块：Rust / CI
- master commit / ref gitlink SHA / 平台与测试环境：master 基线 `c7d21d47a374009161f271c4512722c5c5cd8eae`，ref gitlink `813673a8e84b51265467f02277fc7e739837f61e`；失败 PR #483 SHA `6be5fe4fa75d0d53f775ae62cd38cddd1bb84b49` 更新 ref 至 `5ed238841d98b47ae5e2fdfeb433d4946ba43bbb`；GitHub Actions Ubuntu Rust workspace unit tests
- 关联 Issue / PR / CI / 运行日志（脱敏）：[PR #483](https://github.com/MFSGA/Chimera/pull/483)、[Ubuntu Lint job 116178475001](https://github.com/MFSGA/Chimera/actions/runs/38659904573/job/116178475001)。日志显示 551 passed、1 failed、1 ignored；失败为 `client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer`，`fetch_count >= 2` 在 200 次 `yield_now` 后仍为 false。
- 预期行为：虚拟时间越过导入时采用的服务器建议周期后，定时刷新会发起第二次请求、提交响应并保留用户有效的刷新周期。
- 真实异常、复现步骤与证据：导入首次请求后推进 Tokio 虚拟时钟 60 分钟加 1 秒；测试只轮询最多 200 次，未等到 profile snapshot 的 `subscription.upload == Some(2)` 时也继续执行计数断言，Ubuntu CI 因第二次请求尚未被调度而失败。失败的断言与固定轮询窗口直接对应；没有证据表明生产调度逻辑错误。
- 确认的根因与排除项：测试把异步 actor/scheduler/fetch 任务的完成时机绑定到固定 200 次调度让步，没有等待可观察的提交结果。该轮数不是时间或业务完成条件；在 CI 调度压力下用尽后便错误地把尚未运行的请求当作产品失败。该用例的上游实现使用 `wait_for_source(...ScheduledRefresh)` 和 job settlement 等待；没有发现 Chimera 的生产调度实现偏离预期。
- 修复提交、差异映射及保留的兼容性边界：在 `backend/tauri/src/client/profiles.rs::import_suggestion_is_committed_and_reschedules_the_timer` 中改为轮询已提交快照直到第二次订阅响应可观察，并以 `std::time::Instant` 提供 10 秒真实时间上限。ref `backend/tauri/src/client/profiles.rs` 的同名测试等待 source watch 中的 `ScheduledRefresh` 状态并 settlement；Chimera 当前没有相同的 source-status 测试 API，因此以已提交的 `subscription.upload == Some(2)` 作本地观测点。没有修改产品调度行为；此测试观测差异待 Chimera 提供等价调度状态后重新评估。
- 原实现失败、新实现通过的回归测试，及测试注册位置：原实现失败证据为上述 CI 单测；新断言仍经过实际第二次 fetch 和 commit，固定窗口不足会等待而非误报，真正不刷新时 10 秒后明确失败。测试由 `backend/tauri` 的 Rust 库测试入口执行。
- 验证命令、实际用例数量、结果、未覆盖/跳过项：初次本地尝试因隔离工作树缺少 `backend/chimera-runtime` 子模块而无法解析；初始化精确 gitlink `74ab9e425a41147a0d77c4f9b8e083a9962627f0` 后，首次内部盘构建因当时仅剩 116 MiB 可用空间而在 C 编译阶段失败（非测试失败）。随后将 target 放到扩展盘，并只读复用主工作区已准备好的 sidecar/resource 文件。期限检查前置前的首版有界等待修复定向测试执行 6 次，期限检查移到循环开头后的最终版本再执行 5 次；两版合计 11 次均为 1 passed、0 failed。命令为 `CARGO_TARGET_DIR=/Volumes/ChimeraBuild/codex-chimera-pr483-rust-target-20261015 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=4 cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::profiles::tests::import_suggestion_is_committed_and_reschedules_the_timer -- --exact`。`cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check`、`pnpm exec prettier --check LESSONS.md` 及 `git diff --check` 通过。新 SHA 的 Ubuntu/macOS/Windows lint、三平台 Tauri build 和 Desktop E2E 全部通过；例如 [Ubuntu lint job](https://github.com/MFSGA/Chimera/actions/runs/38663999818/job/116196637777)。
- 风险/回滚和清理证据（如涉及宿主状态）：只影响单测等待策略和经验记录，无产品运行时或宿主网络变更；回滚为恢复固定 200 次轮询并删除本条 lesson。为恢复磁盘空间，仅清理本隔离工作树 Cargo target 构建缓存，未删除源码或其他工作树数据。
- 关联重复事件、后续防复发措施、L3 门禁调整审批链接（如有）：无治理门禁调整；如后续遇到同类 scheduler 测试，等待业务状态/信号并使用有界 deadline，避免依赖固定 yield 次数。
