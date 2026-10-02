# macOS 顶部 UI 对齐记录（2026-10-02）

本次修复针对主窗口额外显示灰色原生标题栏的问题。最初编辑的 checkout 与实际开发进程启动的 checkout 不同；应通过进程的可执行文件路径核对运行来源，不能仅以当前工作目录或前端热更新作为确认。

## 参考基线和范围

- 初始源码对照基线：5331747c06a5f42eeabb3e225a1e77a83f480549。
- 实际运行 checkout 的本地 ref 基线：cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1；工作区干净，未修改或更新 ref。
- 已读取两个基线的 macOS overlay 分支和 Tailwind screens，对应实现一致。
- 本次为标题栏与响应式布局切片。整个窗口管理架构仍有差异，不宣称完整窗口模块已迁移。

| ref 路径 / 符号                                                                               | Chimera 路径 / 符号                                                                                                      | 本次处理                                                             |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| backend/tauri/src/window.rs / WindowConfig.decorations、AppWindow::create                     | 同路径、同符号                                                                                                           | macOS 使用 decorations、hidden_title 和 TitleBarStyle::Overlay       |
| frontend/nyanpasu/tailwind.config.ts / theme.screens                                          | frontend/chimera/tailwind.config.ts / theme.screens                                                                      | 复制 600/900/1200/1536px，修正无效的 screen 字段                     |
| frontend/nyanpasu/src/hooks/use-is-moblie.tsx / useIsMobile                                   | frontend/chimera/src/hooks/use-is-moblie.tsx / useIsMobile、frontend/ui/src/materialYou/themeConsts.ts / MUI_BREAKPOINTS | CSS 与主 UI hook 使用一致的 900px md 断点                            |
| frontend/nyanpasu/src/pages/(main)/_modules/header.tsx 和 components/window/window-header.tsx | 对应 Chimera 路径 / Header、WindowHeader                                                                                 | 既有标题组件样式已一致，保留 Chimera 名称和图标                      |
| frontend/nyanpasu/src/pages/(main)/_modules/navbar.tsx / MobileNavbar                         | frontend/chimera/src/pages/(main)/_modules/navbar.tsx / MobileNavbar                                                     | 保留 legacy 切换入口和 agent 扩展入口                                |
| 无对应 ref legacy 界面                                                                        | frontend/chimera/src/pages/(legacy)/route.tsx / Layout                                                                   | 800px 旧版 drawer 阈值作为边界展示适配，防止共享断点改变原有侧栏流程 |

## 实际验证

- 实际运行 checkout 的 pnpm typecheck 通过。
- 本次配置与 legacy route 的 Prettier 检查、涉及文件的 git diff --check 通过。
- Tauri dev watcher 已重新编译并启动新进程；使用新编译二进制生成本地 debug app，用 CUA 获取真实窗口截图。
- 主 UI 截图与已运行 Clash Nyanpasu 参考窗口对比：均为单条 40px 标题栏；取样背景 RGB 为 (169, 200, 255)。额外灰色原生标题栏消失。
- 通过实际控件完成主 UI 更多菜单 → legacy → 设置 → CONTINUE → 主 UI，确认侧栏、内容和返回入口可用。
- 切换及 HMR 期间，开发日志出现 Tauri event unlisten 的 handlerId 清理错误；窗口已成功切换，但这不是无控制台错误的完整回归，事件生命周期问题需另行修复。
- 对比截图只保存主窗口，不保存含个人配置或 IP 信息的 legacy 设置截图。
- 截图期间使用 visual-check Vite mode 去除 React Scan 重绘覆盖；应用仍运行开发二进制和开发服务器，未改变生产工具装配。
- 截图完成后关闭验证用 debug app 与其子进程，并恢复实际运行 checkout 的 pnpm dev:diff。

## 保留差异和验证限制

- Chimera 品牌、静态应用图标、legacy UI、agent 和 Chimera Client 支持保留。
- TanStack Router 开发入口与参考生产窗口中的配置状态入口不同；它不出现在生产构建。
- 本切片未移植 ref 的 macOS traffic-light 自定义 delegate。系统截图共享指示器遮盖了窗口控制区，未宣称按钮坐标逐像素一致。
- 未执行桌面 E2E suite、跨平台测试或真实网络/TUN 测试；界面验证不能证明内核或网络功能已对齐。
- 后续窗口基础设施迁移应继续以对应 ref WindowManager/Registry 和 macOS delegate 为基线，保留现有 geometry persistence 与 legacy entry points。
