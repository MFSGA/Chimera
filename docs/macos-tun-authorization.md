# macOS TUN 授权边界

- 参考提交：`cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`，参考工作树干净。
- 参考映射：`ref/backend/tauri/src/core/manager.rs::grant_permission` →
  `backend/tauri/src/utils/dirs.rs::grant_macos_tun_permission`；
  `ref/frontend/nyanpasu/src/hooks/use-proxy-settings.ts::useTunMode` →
  `frontend/chimera/src/features/system-proxy/use-proxy-settings.ts::useTunModeAction`。
- 差异类别：缺陷修正、临时迁移。当前 ref 的授权函数只有定义，没有调用点；
  重现步骤为在未授权核心的 macOS 环境点击 TUN，沿 `useTunMode` 的设置写入路径
  检查调用链，无法到达 `grant_permission`。这是源码调用链证据，尚无真实桌面失败证据。
- Chimera 已有授权辅助函数，本次将调用从提交后运行时处理移至
  `client/application.rs::patch_legacy_uncoordinated` 的提交前检查。
  主界面、legacy UI、快捷键及 agent 均通过共享 `patch_verge` 入口。
- 开启 TUN 且目标未使用已连接的服务时，对目标核心请求 macOS 管理员授权。
  已授权核心无需重复弹框，关闭 TUN 无需授权。授权拒绝或失败时不提交设置。
  密码框在 `spawn_blocking` 中等待，避免阻塞异步运行时线程。
- 保留差异：安全引用二进制路径、仅设置用户 setuid 位、验证 root/admin 所有权及
  setuid 位；不复制 ref 的路径转义和未经回读的授权成功判断。
- 部分迁移：保留现有 typed state 与 legacy 兼容桥，本次未迁移最新 ref 的完整
  application workflow。ref 恢复授权调用链时应重新核对入口、命名与事务阶段。

## 验证契约与边界

现有 `macos_tun_permission_tests` 验证 shell 路径及 AppleScript 字符串引用，
不证明系统授权或实际 TUN 联网。运行入口：

```sh
cargo test --manifest-path backend/tauri/Cargo.toml macos_tun_permission_tests --lib
```

真实桌面验收应在隔离 runner 上分别通过主界面和 legacy UI：使用未授权的测试核心，
开启 TUN 后拒绝管理员密码框，检查设置与持久化仍为关闭；再次开启并授权，检查
目标核心权限、生成配置、核心 `/configs` 回读与受控网络请求；关闭 TUN 并恢复测试状态。
agent 应通过已确认的提案调用相同 API，并保留其运行时回读与失败恢复。
本次未执行这些主机级验收，不把引用测试或编译结果报告为真实 TUN 成功。

本次实际检查：`macos_tun_permission_tests --lib` 2 项通过；
`client::application::tests --lib` 4 项通过（通用设置边界，未覆盖真实授权拒绝）；
修改的 Rust 文件 `rustfmt --check` 通过，本次差异 `git diff --check` 通过。
完整工作树的差异检查报告已有 `backend/chimera-config/Cargo.toml` 尾随空白，未修改该文件。
