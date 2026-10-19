# 测试依赖上游依据与版本边界

核对日期：2026-10-18。本文支撑[测试规范](README.md)，仅研究测试依赖，不以业务项目 `ref/` 作为驱动行为依据。

## U01 锁定版本与证据优先级

| 依赖                                                | 当前依据                                                                                                                    | 核对版本                                                    |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| WebdriverIO、WDIO runner / Mocha adapter 等直接依赖 | [测试包](../../tauri-e2e/package.json)、[pnpm 锁文件](../../pnpm-lock.yaml)                                                 | WDIO / runner / adapter / reporter 9.32.0（globals 9.31.3） |
| `@wdio/tauri-service`                               | 同上                                                                                                                        | 1.4.0                                                       |
| `tauri-plugin-wdio-webdriver`                       | [Cargo manifest](../../backend/tauri/Cargo.toml)、[vendored source](../../backend/tauri/vendor/tauri-plugin-wdio-webdriver) | crates.io 1.4.0 + 上游兼容修复 `e4bdb66`                    |
| Mocha                                               | pnpm-lock.yaml 中传递依赖                                                                                                   | 11.8.0                                                      |
| expect-webdriverio                                  | pnpm-lock.yaml 中传递依赖                                                                                                   | 6.0.10                                                      |
| Node / pnpm                                         | [根 package.json](../../package.json)                                                                                       | Node 24.21.0；pnpm 12.3.4                                   |

**产品期望由需求/契约决定；工具能力由实际锁定版本决定。** 判断顺序：锁文件与实际安装包 → 同版本发布源码/发布产物 → 对应源码提交 → 官方当前文档。官网会更新，不能默认其中每个 API 已在本项目可用。传递包并非全部与直接依赖同版本；例如 `@wdio/tauri-service` 的运行时依赖可能与工作区直接声明的 WDIO 版本不同，实际解析仍需看锁文件。

本文将事实与规则分开：标为“源码事实”的内容来自实际阅读；“项目要求”是本项目为保证测试可信度作出的约束，并非伪称上游强制规定。

## U02 WebdriverIO 等待与重建会话

核对 `webdriverio@9.32.0` 的发布元数据、锁定完整性和 `waitUntil`、`reloadSession`、元素命令源码；npm 发布元数据给出的源码提交是 `5c2a318d9a5e8424a2f2de95df9374b89bda7936`，发布完整性为 `sha512-A7r7SnvZ6eEJROIWAG6GMOlV82W2Pl7FDjXLn/tJ4L8BEc3yjCrDWzY4EB9i4MtPZwlNZoHgNV9fYYKn7JWAew==`。安装目录可能因工作区尚未重新安装而暂时保留旧包，不能以旧包文件冒充锁定版本依据。

- 源码事实：`waitUntil` 使用计时器调用条件函数；`timeoutMsg` 可为字符串或函数，传入函数时只在超时时求值。静态错误文本不会自动更新。项目要求：失败时附上最新快照；若使用函数，快照读取仍应只读，见 T05。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/5c2a318d9a5e8424a2f2de95df9374b89bda7936/packages/webdriverio/src/commands/browser/waitUntil.ts)
- 源码事实：`waitForDisplayed` 在同一个元素对象上轮询 `isDisplayed`，并明确不等待元素存在；`isDisplayed` 对没有 element ID 的对象直接返回 `false`。异步渲染的目标需要在 `browser.waitUntil` 条件内重新执行 `$()`，不能先保存一个不存在的元素对象再调用 `waitForDisplayed`。[固定提交源码：waitForDisplayed](https://github.com/webdriverio/webdriverio/blob/5c2a318d9a5e8424a2f2de95df9374b89bda7936/packages/webdriverio/src/commands/element/waitForDisplayed.ts)、[isDisplayed](https://github.com/webdriverio/webdriverio/blob/5c2a318d9a5e8424a2f2de95df9374b89bda7936/packages/webdriverio/src/commands/element/isDisplayed.ts)
- 源码事实：元素命令的隐式等待只解决元素定位/重新获取，不代表业务事务完成。项目要求：等待具体业务结果。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/5c2a318d9a5e8424a2f2de95df9374b89bda7936/packages/webdriverio/src/utils/implicitWait.ts)
- 源码事实：`reloadSession` 删除旧会话后请求协议层创建新会话，并调用 `onReload` hooks；其中并没有针对 Chimera 的应用进程退出、磁盘持久化或内核重启验证。项目要求：冷启动契约必须另外验证进程生命周期。[固定提交源码](https://github.com/webdriverio/webdriverio/blob/5c2a318d9a5e8424a2f2de95df9374b89bda7936/packages/webdriverio/src/commands/browser/reloadSession.ts)

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

## U04 Rust 插件 1.3.0 的点击与会话边界

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

当前项目使用的桌面驱动是 vendored `tauri-plugin-wdio-webdriver@1.4.0`，并带兼容补丁 `e4bdb66`。本地为 Dashboard 右键菜单另加了有限兼容处理：`PointerEventType::Down` 先在同一目标合成不可信 `PointerEvent("pointerdown")`，再发送原有 MouseEvent；其余路径仍按原实现合成 MouseEvent。在 `PointerAction::Move` 中，`dispatch_pointer_event(PointerEventType::Move, ...)` 映射为 `MouseEvent("mousemove")`。因此 `moveTo()` 可以验证驱动的合成 mousemove 路径，但不能证明浏览器收到真实 `PointerEvent`，也不能作为 Radix Tooltip 鼠标悬停行为的验收。对应的受限测试及移除条件见[Profile 导入动作测试契约](contracts/profile-import-accessible-action.md)。

## U05 Mocha 异步与 hooks

核对 `mocha@11.8.0/lib/runnable.js` 的 `callFn` / `callFnAsync`；npm 发布元数据给出的提交是 `90c1bb3e183a262ac91d83fa45035d03ea9f6045`。[固定提交源码](https://github.com/mochajs/mocha/blob/90c1bb3e183a262ac91d83fa45035d03ea9f6045/lib/runnable.js)

源码事实：返回 Promise 的完成和拒绝被交给测试结果处理；callback 路径会拒绝同时返回 Promise 的完成方式。项目要求：统一 async/await，并等待所有子操作，不能让未等待的 Promise 在用例通过后才报错。

官方参考：[异步测试](https://mochajs.org/features/asynchronous-code/)、[hooks](https://mochajs.org/features/hooks/)。完整清理、错误聚合和用例状态隔离属于项目自己的 T07/T10 约束，不是 Mocha 自动保证。

## U06 Windows WebView2 与 Tauri 版本兼容

CI 在 Tauri 2.12 下编译 `tauri-plugin-wdio-webdriver@1.3.0` 失败：Tauri 将 `webview2-com` 从 0.38 升到 0.39，而插件仍以 0.38 的接口接收 Tauri 传入的 0.39 WebView2 对象；插件还依赖旧的 `windows` 0.61。错误包括 WebView2 handler 的 `Param` trait 不匹配和 `StructuredStorage` 未启用。

在插件发布版 1.4.0 之后，上游提交 [`e4bdb66ec5b1d96b7fd766d70370a8e8361e0459`](https://github.com/webdriverio/desktop-mobile/commit/e4bdb66ec5b1d96b7fd766d70370a8e8361e0459) 修复了该边界：插件自身改用 `webview2-com` 0.39 / `windows` 0.62，并在 Tauri 的 `PlatformWebview` 进入插件时重新绑定 controller 类型；同时显式启用 `Win32_System_Com_StructuredStorage`。当前使用 SHA-256 为 `52c5e97428174a52b7f4357bbc4535fcce52c6a06c8ee7a379502f7cda1b6045` 的 crates.io 1.4.0 源码包，并移植该提交对 `Cargo.toml` 与 `src/platform/windows.rs` 的补丁；不依赖 Cargo 从 GitHub 克隆整个上游仓库。

这是临时的测试工具兼容 pin。上游发布包含此修复的 crate 后，更新 Cargo 依赖与锁文件到该 registry 版本，并重新核对 Windows E2E 构建；在那之前保留固定提交，避免 Tauri 次版本变化再次拆分 WebView2 COM 类型。

## U07 关闭 WebView 时的元素点击响应

核对当前 vendored `tauri-plugin-wdio-webdriver@1.4.0` 的 `src/platform/executor.rs::click_element` 与 `src/server/handlers/element.rs::click`。点击脚本同步执行 `el.click()` 并返回 `true`；CSS 编辑器的 Cancel 处理器会在同一点击中调用 Tauri `WebviewWindow.close()`。本机 WebKit E2E 的日志记录到：窗口确实关闭，但首个 click 响应返回 unsupported result type，WDIO 按锁定的 `connectionRetryCount: 1` 重试后得到 `No window could be found`，窗口句柄列表已不含 `editor-css`。

项目测试在这条特定边界接受该最终响应，仅当它匹配元素 click 的 no-window 错误且独立读取的句柄列表确认编辑器窗口消失。其他错误、或窗口仍存在时均失败。此断言证明取消按钮关闭了 WebView，不证明原生鼠标输入。升级 WDIO 或 vendored 驱动时重跑此场景；若点击响应能正常返回，移除错误兼容分支。

## U08 Tauri driver 的键盘输入与焦点边界

核对项目当前 vendored `tauri-plugin-wdio-webdriver` 源码（Cargo.lock 中为
1.4.0）及锁定的 `webdriverio@9.32.0`。WDIO 的 `Key.Tab` 序列化为 WebDriver
特殊字符 `\uE004`；普通字符串 `'TAB'` 是三个可打印字符。即使传入 `Key.Tab`，
vendored 插件的 `src/platform/executor.rs::dispatch_key_event` 也只在
`document.activeElement` 上调用 `dispatchEvent(new KeyboardEvent('keydown'...))`
和 `keyup`，没有让浏览器执行 Tab 默认焦点遍历。因此 `browser.keys(Key.Tab)`
不能证明链接可通过 Tab 顺序到达。

证据位置：`backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/server/handlers/actions.rs`
的 `KeyDown`/`KeyUp` 分支（约 210–240 行）将动作委派给 executor；
`backend/tauri/vendor/tauri-plugin-wdio-webdriver/src/platform/executor.rs`
的 `dispatch_key_event`（约 1216–1355 行）在活动元素上合成键盘事件。源码调用链中
没有浏览器级键盘输入或默认 Tab 导航。

受限测试可通过 `HTMLElement.focus()` 触发浏览器焦点变化，并验证 Radix
Tooltip 对焦点的响应，但必须将边界写成“焦点触发行为”，不得声称验证 Tab 顺序。
需要验收实际 Tab 顺序时，使用具备浏览器原生键盘输入的专用驱动/系统测试，并以
目标链路的最小复现确认其确实改变 `document.activeElement`。升级 WDIO 或 vendored
驱动后，重跑该复现并更新本条记录。

## U09 Tauri driver 合成 PointerEvent 与右键上下文菜单边界

锁定的 `webdriverio@9.32.0` 中，`element.click({ button: 'right' })` 使用
`performActions`。本项目 vendored `tauri-plugin-wdio-webdriver@1.4.0` 在 WebView
内合成事件不会触发浏览器原生右键默认行为。ref `d43819fbb24ecf05f0ef7cad8fbaf87d6d0072a3`
和 Chimera 的 `context-menu-provider` 都在文档捕获阶段监听 `pointerdown`，记录右键目标；
Dashboard 再通过该目标查找已注册的菜单项。Windows hermetic E2E 运行
`39072145858` 中，52 个 spec 有 2 个失败，Dashboard 等待没有看到菜单；失败截图显示
应用的通用剪贴板菜单，没有 Dashboard 项。固定 ref 与本地相关产品代码的对照表明，
这次差异在测试驱动事件序列，不在 Dashboard 菜单实现。

兼容实现只在按下阶段发送冒泡、可取消的不可信 `pointerdown`，随后发送原有 MouseEvent；
匹配的 secondary-button 按下/释放后仍发送 `contextmenu` MouseEvent。Windows E2E 重跑用于
验证该事件序列能进入应用菜单处理链，不能证明可信原生鼠标输入或完整 PointerEvent 序列。
升级 WebDriver 或 vendored 插件时，先用同一 Dashboard 右键最小复现核对 PointerEvent 与
contextmenu；若驱动原生提供菜单所需事件，再移除此兼容实现。

## U10 Tauri invoke 属性与 Dashboard IPC 故障注入边界

核对 Cargo.lock 锁定的 `tauri@2.12.0` crates.io 包，checksum 为
`cb20ef8e6f97d6fe03039666cd82861f09a38b56291fa2f0f9fe140023f61c1a`。
检查发布源码 `scripts/core.js` 中 `Object.defineProperty(window.__TAURI_INTERNALS__,
'invoke', { value: ... })`，以及 `src/manager/webview.rs` 对
`window.__TAURI_INTERNALS__` 的同类定义。两处均没有设置 `writable` 或
`configurable`；按 ECMAScript 默认值，已定义的 `invoke` 属性不可赋值替换或删除。

PR #490 的 Windows run `39116055509` 因 E2E 尝试替换该函数而在拦截器安装断言失败。
这项测试设置没有拦截到实际存储 IPC，不能证明 Dashboard 业务恢复逻辑失败。随后改用独立
`e2e` Tauri 插件后，Windows run `39160740767` 的 Dashboard 用例仍失败；失败包装只输出
外层 `AggregateError`，没有呈现内部 IPC 错误。核对 `main-capability.json` 和 Tauri ACL
后发现，该测试插件既没有权限清单，也没有加入窗口 capability，WebView 无权调用其命令。
当前改为在 `e2e` Cargo feature 下把测试控制命令分发到应用级 invoke handler，同时在真实
`get_storage_item` / `set_storage_item` 入口，仅对 `dashboard-widget-configs` 执行可观察、可清理的
故障注入；生产构建不包含测试命令或 fixture。失败诊断会递归保留 `AggregateError` 内部错误。
测试通过触发计数确认故障命中了真实 IPC，再由可见 UI Retry 验证恢复。新 SHA 的 Windows E2E
通过后更新该结论与 `LESSON-20261017-007` 状态。

## 升级依赖时必须复核

升级 WebdriverIO、service、Rust 插件、Mocha 或断言依赖时，同步核对锁文件并更新本记录。至少检查点击/输入事件、窗口切换、等待异常、session 与进程生命周期、mock 恢复、hook 次序和日志采集。用最小兼容性用例验证受影响行为，再按 T14 运行对应套件。

移除兼容措施时必须有新版本通过原始最小复现的证据；不能仅因为官网说“支持”就删除。源码事实发生变化时，更新对应限制和契约，不要保留已经失效的断言或过时平台结论。
