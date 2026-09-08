# Chimera 桌面 E2E 架构与证据边界

本文说明当前实现，替代早期“最小 smoke / 待 staged 文件”的历史说明。产品目标以 [AGENTS.md](../AGENTS.md) 和 [ref 对齐指南](ref-alignment-guide.md) 为准：共通实现跟随 ref，保留 legacy UI，以 E2E 为主要扩展，并增加 agent 能力。

本文记录的运行行为以相关源文件为依据，不代表本次已经运行或通过全部场景。具体命令与 suite 选择见 [E2E README](../tauri-e2e/README.md)。

## 1. 运行链路

```text
构建当前前端
  → Cargo e2e feature 构建到 backend/target/e2e
  → 启动真实 Chimera 桌面程序与 embedded WebDriver
  → WebdriverIO 连接 legacy 窗口（默认端口 4446）
  → 按 suite 执行；main 场景打开并切换到 main 窗口
  → 用户操作、状态读取及对应结果断言
  → onComplete 尝试清理并恢复主机代理状态
```

根目录 `pnpm e2e:tauri` 默认构建并运行 smoke；它不自动运行 unit tests，也不是全部桌面测试。`maxInstances: 1` 只限制同一个 runner 内的并发，不能防止多个独立 runner 共用二进制目录。

## 2. 文件职责

| 文件                                                          | 当前职责                                                                |
| ------------------------------------------------------------- | ----------------------------------------------------------------------- |
| [package.json](../package.json)                               | 根工作区 E2E 构建、默认测试入口                                         |
| [pnpm-workspace.yaml](../pnpm-workspace.yaml)                 | 注册 `tauri-e2e` workspace 和依赖策略                                   |
| [Cargo.toml](../backend/tauri/Cargo.toml)                     | 定义 optional WebDriver 依赖和 `e2e` feature                            |
| [lib.rs](../backend/tauri/src/lib.rs)                         | 在 `e2e` feature 下注册 embedded WebDriver                              |
| [dirs.rs](../backend/tauri/src/utils/dirs.rs)                 | 解析 E2E 配置和数据目录覆盖                                             |
| [resolve.rs](../backend/tauri/src/utils/resolve.rs)           | 生产初始化路径，包含 service/core/system proxy；不能假设 E2E 会全部跳过 |
| [E2E package.json](../tauri-e2e/package.json)                 | unit、typecheck、各桌面 suite 与升级 runner 命令                        |
| [wdio.conf.ts](../tauri-e2e/wdio.conf.ts)                     | 二进制、端口、目录、初始窗口、fixture、产物与退出编排                   |
| [spec-suites.ts](../tauri-e2e/spec-suites.ts)                 | base suite 与组合 suite 的实际成员                                      |
| [runtime-path.ts](../tauri-e2e/runtime-path.ts)               | 独立运行目录、显式目录覆盖及目录清理                                    |
| [process-cleanup.ts](../tauri-e2e/process-cleanup.ts)         | Windows 代理状态保存/恢复与测试进程清理                                 |
| [specs](../tauri-e2e/specs)                                   | 主界面、legacy UI、配置、运行时、agent 等场景                           |
| [upgrade-v0223-v0230.ts](../tauri-e2e/upgrade-v0223-v0230.ts) | 共用升级测试目录，编排旧版与新版两个阶段                                |
| [agent/e2e.rs](../backend/tauri/src/features/agent/e2e.rs)    | agent 的确定性诊断 fixture                                              |
| [e2e.yaml](../.github/workflows/e2e.yaml)                     | 当前 Windows CI 调度、构建、测试和产物上传                              |

`pnpm-lock.yaml` 和 `backend/Cargo.lock` 固定各自依赖；依赖变更时应按既有工具同步。本文不要求为了文档修改重新生成绑定、锁文件或构建产物。

## 3. 三种必须区分的隔离

### 文件隔离

默认使用唯一的 `tauri-e2e/.tmp/runtime/<id>/config` 与 `data`，通过 E2E 环境变量传入应用。`CHIMERA_E2E_RUNTIME_DIR` 指定的目录由调用者管理，harness 不按自建目录删除它。配置 fixture 只会导入空配置目录，并记录导入标记。

E2E 使用独立 Cargo target；指定 `CHIMERA_E2E_BINARY` 时仍应确认该二进制来自本次要验证的代码，不能将普通开发实例误作测试实例。

### 主机隔离

程序仍经过真实 service、core 和 system-proxy 初始化。文件目录隔离不能证明系统代理、TUN、端口或服务不会改变。

Windows runner 默认读取代理设置并在 `onComplete` 中恢复；强制结束进程可能使恢复无法执行。`CHIMERA_E2E_SKIP_PROXY_RESTORE=1` 会跳过这项恢复保护，不能在报告中仍声称恢复已验证。涉及主机状态的测试使用专用 runner/VM，并验证结束后的实际状态。

### 进程隔离

当前 Windows 清理按二进制目录和 WebDriver runtime 路径筛选进程，不是按本次 runId/PID 精确归属。同目录的独立测试可能互相影响。非 Windows 分支不执行这套进程清理和代理恢复；跨平台能力需有另外的实现与验证证据。

## 4. E2E 与 agent 的证据

真实功能场景通过控件触发业务，按作用范围读取真实内核、持久化产物、网络或重启结果。直接调用 IPC 可以用于测试准备和状态观察，但不能取代要验证的用户操作。

`agent`、`hermetic`、`all` 默认选择 `stale-proxy` fixture。当前 [agent actions](../backend/tauri/src/features/agent/actions.rs) 在 fixture 下绕过原生确认，修复操作只标记 fixture 状态，随后返回模拟的健康快照。因此该用例证明桌面引导 UI 和部分提案编排，不证明真实系统确认、代理修复或恢复。

保留这些确定性测试，并把真实授权、状态漂移、执行失败、部分应用和恢复场景单独验证。E2E 扩展应集中在 harness 与明确测试边界；不得为得到成功结果而改变发布构建中的业务语义。

legacy UI 是正式保留的界面。共通业务修改需要检查受影响的 main 和 legacy 入口及共享结果，不能通过删除 legacy 路由来让测试通过。agent 也复用这套业务入口，不拥有独立配置状态。

## 5. 当前执行入口的局限

- `test:unit` 只执行三个显式列出的 harness 文件，不覆盖目录内全部 `.test.ts`。新增测试需要同步可运行入口。
- PR 的 `critical` 是 smoke + runtime + profiles；agent、settings、main 在夜间 `hermetic` 中，不属于该 PR 集合。
- 桌面 CI 当前是 Windows；网络套件依赖外部客户端配置，不能据常规 CI 结果声称双内核或跨平台网络验证已完成。
- 升级 spec 在没有 `CHIMERA_E2E_UPGRADE_PHASE` 时跳过。普通 `test:all` 不能代替专门的两阶段升级命令。
- 当前 CI 配置中的 build/clippy 不等于执行 Rust tests。需要实际运行的测试命令和结果才能报告通过。

这些是现状与后续接入事项，文档更新不会自动改变 CI。新功能验收必须报告本次真正运行的套件、通过/失败/跳过及环境缺口。

## 6. 产物和维护

设置 `CHIMERA_E2E_ARTIFACT_DIR` 后，失败 hook 保存截图、页面源码与错误文本；部分 spec 另有 `.tmp` 截图。它们不是自动脱敏的公开产物，应使用合成数据并在分享前检查敏感内容。

本指南和 ref 对齐指南通过 `.gitignore` 的明确例外保持可版本控制。其他本地报告、临时配置、原始日志和私人证据仍保持忽略；如需保存新的长期迁移记录，应为该记录单独配置跟踪范围。

修改 harness 的窗口、端口、suite、fixture、初始化或清理行为时，同步更新本文和 README，避免再次把早期 smoke 设计当作当前运行保证。
