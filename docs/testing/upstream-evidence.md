# 测试依赖上游依据与版本边界

核对日期：2026-09-13。本文支撑[测试规范](README.md)，仅研究测试依赖，不以业务项目 `ref/` 作为驱动行为依据。

## U01 锁定版本与证据优先级

| 依赖                                                | 当前依据                                                                    | 核对版本                                            |
| --------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------- |
| WebdriverIO、WDIO runner / Mocha adapter 等直接依赖 | [测试包](../../tauri-e2e/package.json)、[pnpm 锁文件](../../pnpm-lock.yaml) | WDIO 9.31.7（globals 9.31.3、spec-reporter 9.31.2） |
| `@wdio/tauri-service`                               | 同上                                                                        | 1.4.0                                               |
| `tauri-plugin-wdio-webdriver`                       | [Cargo 锁文件](../../backend/Cargo.lock)；Cargo.toml 仅声明 `1`             | 1.3.0                                               |
| Mocha                                               | pnpm-lock.yaml 中传递依赖                                                   | 11.8.0                                              |
| expect-webdriverio                                  | pnpm-lock.yaml 中传递依赖                                                   | 6.0.10                                              |
| Node / pnpm                                         | [根 package.json](../../package.json)                                       | Node 24.21.0；pnpm 12.3.4                           |

**产品期望由需求/契约决定；工具能力由实际锁定版本决定。** 判断顺序：锁文件与实际安装包 → 同版本发布源码/发布产物 → 对应源码提交 → 官方当前文档。官网会更新，不能默认其中每个 API 已在本项目可用。传递包并非全部与直接依赖同版本；例如 `@wdio/tauri-service` 的运行时依赖可能与工作区直接声明的 WDIO 版本不同，实际解析仍需看锁文件。

本文将事实与规则分开：标为“源码事实”的内容来自实际阅读；“项目要求”是本项目为保证测试可信度作出的约束，并非伪称上游强制规定。

## U02 WebdriverIO 等待与重建会话

核对 `webdriverio@9.31.7` 的发布元数据、锁定完整性和 `waitUntil`、`reloadSession`、元素命令源码；npm 发布元数据给出的源码提交是 `ba488fa25aaac3122a676405969a4f790615757f`。安装目录可能因工作区尚未重新安装而暂时保留旧包，不能以旧 `node_modules` 的文件冒充锁定版本依据。

- 源码事实：`waitUntil` 使用计时器调用条件函数，接收的是 `timeoutMsg` 字符串；它不会替调用者持续重新计算模板字符串。项目要求：失败时另外附上最新快照，见 T05。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/ba488fa25aaac3122a676405969a4f790615757f/packages/webdriverio/src/commands/browser/waitUntil.ts)
- 源码事实：元素命令的隐式等待只解决元素定位/重新获取，不代表业务事务完成。项目要求：等待具体业务结果。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/ba488fa25aaac3122a676405969a4f790615757f/packages/webdriverio/src/utils/implicitWait.ts)
- 源码事实：`reloadSession` 删除旧会话后请求协议层创建新会话，并调用 `onReload` hooks；其中并没有针对 Chimera 的应用进程退出、磁盘持久化或内核重启验证。项目要求：冷启动契约必须另外验证进程生命周期。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/ba488fa25aaac3122a676405969a4f790615757f/packages/webdriverio/src/commands/browser/reloadSession.ts)

官方参考：[等待](https://webdriver.io/docs/api/browser/waitUntil/)、[重建会话](https://webdriver.io/docs/api/browser/reloadSession/)、[元素点击](https://webdriver.io/docs/api/element/click/)、[稳健选择器与断言](https://webdriver.io/docs/bestpractices/)。通用点击文档描述的接口不能代替对具体 provider 的核对。

## U03 Tauri service 启动恢复和清理

核对 `@wdio/tauri-service@1.4.0` 官方 npm [发布包](https://registry.npmjs.org/@wdio/tauri-service/-/tauri-service-1.4.0.tgz)中的 `package/dist/esm/index.js`。上游仓库：[webdriverio/desktop-mobile](https://github.com/webdriverio/desktop-mobile/tree/main/packages/tauri-service)。仓库 main 仅作入口，不当成 1.4.0 的固定快照。

发布包的 npm integrity 为 `sha512-EUTS8tfMOXK1yPNEaRd6yiSdPoX19zeV+gtp2m7UuPTWFZGayPuMUoFyttP6Lx3OFvJC3lnbxjr1vcCNAOA7tg==`；下载 tarball 的 SHA-256 为 `6af19c4c7f1b61fb5ef691f8540e5da310dcb12528377f72ca3f3af12bf2be5c`。以发布包校验值为准，不能以当前 checkout 的 main 分支替代版本证据。

| 已核对函数/阶段                                          | 源码事实                                      | 项目要求                                               |
| -------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------ |
| `startEmbeddedDriver`                                    | 启动应用子进程并等待 embedded 服务            | WebDriver 就绪后仍需等待产品自己的就绪条件             |
| `ensureEmbeddedServersHealthy` / `restartEmbeddedServer` | worker 启动路径可检测服务不可达并重新拉起应用 | 必须记录意外重启，不能由后续通过推断全程无崩溃         |
| `beforeTest`                                             | 按 clear/reset/restore 配置处理 mocks         | 不把清空调用历史当成恢复原实现；不假定其他资源随之重置 |
| `afterSession`                                           | 尝试恢复 mocks、删除会话；部分错误只写警告    | 不能依赖此阶段读取失败页面或保证宿主状态恢复           |
| `onComplete`                                             | launcher 的最终回收阶段                       | 在用例/有效会话阶段采集现场，launcher 负责进程级兜底   |

当前项目还有[自定义 harness 编排](../../tauri-e2e/wdio.conf.ts)：在 `after` 中恢复 mocks，对失效 session 的 execute 加保护，并在 suite 前后重置窗口、在 `afterTest` 采集失败证据。它是限定在 harness 生命周期的兼容措施，不能复制为测试主体的“错误返回 undefined”或隐藏业务失败。

官方参考：[Tauri service 配置](https://webdriver.io/docs/desktop-testing/tauri/configuration/)、[WDIO hooks 与 runner 配置](https://webdriver.io/docs/configuration/)。工具的日志捕获开关不等于项目已保存并上传所有失败产物。

## U04 Rust 插件的点击与会话边界

直接下载并阅读官方 [1.3.0 crate 发布包](https://static.crates.io/crates/tauri-plugin-wdio-webdriver/tauri-plugin-wdio-webdriver-1.3.0.crate)，其 SHA-256 与本项目 Cargo.lock 完全相同：

`7ad665bda48567107b6a3d070fe6d58a98e21f3ebf52abe41308fa4d5919f61b`

包内 `.cargo_vcs_info.json` 记录提交 `53d8569e7db3706a814f1fa6d6e779f2b22d0333`、`dirty: true`、目录 `packages/tauri-plugin-webdriver`。因此不能把该 Git 提交当成与发布内容完全等同的证据；以下结论以校验过的 crate 文件为准。

| 发布包文件与位置                                      | 源码事实                                                      | 对测试结论的影响                                          |
| ----------------------------------------------------- | ------------------------------------------------------------- | --------------------------------------------------------- |
| `src/server/handlers/element.rs`，`click`，114 行起   | 路由将元素点击交给 executor                                   | 要继续检查 executor，而不是停留在 W3C 接口名称            |
| `src/platform/executor.rs`，`click_element`，541 行起 | 执行 scrollIntoView、DOM click、focus 脚本                    | 普通元素 click 不证明原生鼠标事件路径                     |
| 同文件 `dispatch_pointer_event`，1391 行起            | 构造 MouseEvent，使用 mousedown / mouseup / mousemove / click | 不能据此认定发送了 pointerdown / pointerup 或可信输入事件 |
| `src/server/handlers/session.rs`，`delete`，206 行起  | 从 session 集合删除记录后返回；该 handler 没有退出应用        | 新会话不能替代应用冷启动证据                              |

已检查平台实现中的相同方法：pointer 方法的其他覆盖出现在 Android/iOS；本文结论限定当前桌面路径，不延伸为移动平台结论，也不承诺未来版本仍然如此。

项目现有[下拉菜单测试](../../tauri-e2e/specs/main-dropdown-menu.e2e.ts)通过设置焦点再按 Enter 激活菜单，并等待动画后检查几何。这与上述限制相符，但它不能替代本次发布源码核对。键盘激活适合验证菜单布局，鼠标可用性仍需独立覆盖。

## U05 Mocha 异步与 hooks

核对 `mocha@11.8.0/lib/runnable.js` 的 `callFn` / `callFnAsync`；npm 发布元数据给出的提交是 `90c1bb3e183a262ac91d83fa45035d03ea9f6045`。[固定提交源码](https://github.com/mochajs/mocha/blob/90c1bb3e183a262ac91d83fa45035d03ea9f6045/lib/runnable.js)

源码事实：返回 Promise 的完成和拒绝被交给测试结果处理；callback 路径会拒绝同时返回 Promise 的完成方式。项目要求：统一 async/await，并等待所有子操作，不能让未等待的 Promise 在用例通过后才报错。

官方参考：[异步测试](https://mochajs.org/features/asynchronous-code/)、[hooks](https://mochajs.org/features/hooks/)。完整清理、错误聚合和用例状态隔离属于项目自己的 T07/T10 约束，不是 Mocha 自动保证。

## 升级依赖时必须复核

升级 WebdriverIO、service、Rust 插件、Mocha 或断言依赖时，同步核对锁文件并更新本记录。至少检查点击/输入事件、窗口切换、等待异常、session 与进程生命周期、mock 恢复、hook 次序和日志采集。用最小兼容性用例验证受影响行为，再按 T14 运行对应套件。

移除兼容措施时必须有新版本通过原始最小复现的证据；不能仅因为官网说“支持”就删除。源码事实发生变化时，更新对应限制和契约，不要保留已经失效的断言或过时平台结论。
