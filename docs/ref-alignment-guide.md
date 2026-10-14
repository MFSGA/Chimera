# Chimera 与 ref 实现对齐指南

本文与根目录 [AGENTS.md](../AGENTS.md) 共同约束后续开发、修复和迁移。最终目标是：共通功能的实现尽可能与 `ref/` 一致，包括架构、目录、文件名、类型名、函数名和变量名；保留品牌等必要差异，明确保留 legacy UI，以 E2E 测试为主要扩展，并扩展 agent 能力。

**迁移规模不设人为上限。** 一项完整功能涉及多个文件、目录或前后端调用链时，可以先大范围复制 `ref/` 的对应实现，再集中按映射替换上游产品品牌为 Chimera；不要求为了降低单次 diff 行数而逐文件搬运。除必要品牌和明确保留的扩展/兼容差异外，默认保留上游的结构、名称与业务行为。

可以分阶段完成迁移，但“行为相似、内部继续独立实现”不是最终验收标准。目录和命名对齐也是目标的一部分。本文不表示现有代码已完成对齐，也不把测试要求当作已通过的结果。

## 1. 参考基线

本次规则更新核对的本地参考版本：

```text
repository: https://github.com/libnyanpasu/clash-nyanpasu
branch:     main
commit:     69353fc9a7d7e4e427211a85cdaa0b9c07a4f01a
verified:   2026-10-13 fetched and fast-forwarded from origin/main
```

每个迁移切片开始前执行：

```powershell
git -C ref rev-parse HEAD
git -C ref status --short
```

记录实际 commit。它与本文不一致时，明确本次使用的版本及其原因，不得笼统写“参考最新 ref”。参考目录有未提交修改时，应区分 commit 内容与工作区差异，不能把修改后的内容误称为该 commit 的实现。

`ref/` 默认只读。开发共通功能时应主动读取其对应代码，不必等待用户再次要求“参考 ref”。不要为了开始一个功能而自动 fetch、pull 或重置参考目录。只有任务包含同步参考版本时才更新，并记录更新前后 commit。

主仓库通过 `ref/` 的 gitlink 固定参考版本，`.gitmodules` 提供克隆地址；新工作区可用 `git submodule update --init --recursive` 检出完全相同的基线。`ref/` 不应加入忽略规则。该 gitlink 固定的是经过核验的 commit，不会自动跟随上游 `main`；只有明确同步 ref 时才更新并提交新的 gitlink。本次将基线固定为上述 `main` commit，并递归初始化其子模块；CI 和新克隆应从该 gitlink 获得相同源码。参考目录有未提交修改时，应区分 commit 内容与工作区差异，不能把修改后的内容误称为该 commit 的实现。

## 2. 必须同时满足的目标

| 范围         | 目标                                                          | 不能作为完成依据的情况                 |
| ------------ | ------------------------------------------------------------- | -------------------------------------- |
| 业务实现     | 共通功能沿用 ref 的领域模型、算法、状态所有权、事务和错误处理 | 只保留相同接口，内部仍走另一套实现     |
| 目录与文件名 | 对应模块保留 ref 的相对结构和文件名，仅做必要品牌映射         | 自行拆分、移动、改名后认为功能相同即可 |
| 符号命名     | 类型、函数、变量和协议字段尽量与 ref 相同                     | 对不涉及品牌的概念批量替换命名         |
| 主界面       | 跟随 ref 的对应实现，并保留明确的扩展入口                     | 只检查外观相似                         |
| legacy UI    | 长期保留窗口、路由、布局、交互和既有用户流程                  | 因为 ref 没有它就删除或停用            |
| E2E          | 主要扩展，提供真实操作、状态和结果证据                        | 只增加测试文件，没有执行入口或实际运行 |
| agent        | 在共享业务能力上增加诊断、工具、提案和执行编排                | 另写配置、内核或服务管理逻辑           |

保留 legacy UI 不等于冻结它所调用的旧业务底层。应保留界面与用户流程，通过必要的 hook、数据展示或命令适配，让它使用与主界面、agent 相同的业务入口。旧业务桥接可以在依赖迁移后移除；legacy UI 本身不属于待删除的迁移债务。

## 3. 差异边界

| 差异类别       | 允许范围                                                                     | 约束                                                              |
| -------------- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| 品牌           | 产品显示名、包或 crate 标识、应用 ID、图标和必要发行地址                     | 不借品牌替换重命名无关领域概念；不删除上游版权和许可证信息        |
| legacy UI      | 保留的窗口、路由、组件、交互及必要展示适配                                   | 复用共通业务底层，不维护独立状态源或事务                          |
| E2E            | harness、spec、fixture、隔离目录、feature-gated WebDriver 和必要稳定定位属性 | 测试控制能力不能无条件进入发布构建；标明 fixture 与真实运行的区别 |
| agent          | feature 模块内的工具契约、诊断、提案和执行编排，以及必要 UI/IPC 入口         | 通过共享应用 API 读写状态；生产业务语义跟随 ref                   |
| 兼容或缺陷修正 | 现有内核/服务兼容、数据迁移，以及有证据的 ref 缺陷最小修正                   | 单独记录原因、适配边界、证明测试和重新评估条件                    |

例如 `frontend/nyanpasu/` 与 `frontend/chimera/`、`NyanpasuClient` 与 `ChimeraClient` 可以记录为品牌映射；不能因此把不含品牌的文件名、变量名或数据模型也全部改为自定义形式。**统一替换必须先建立白名单映射并审阅所有匹配项**，不能简单对整个仓库执行无边界的文本替换：协议字段、序列化键、已有用户存储内容、第三方包名、API/IPC 契约、下载地址与签名标识是否能改，必须逐项核实。原有 LICENSE/NOTICE、版权声明和必要署名不得因复制或改品牌而丢失。

现有差异不自动成为永久例外。Chimera Client/Service 支持不能为减少 diff 而被删除；应核对保留的能力，把必要兼容限制在 ref 对应的 adapter/port 边界。历史多 Profile 语义、脚本执行器差异等应记录为待收敛项，迁移时安排数据兼容和回归，不默认按原样永久保留，也不在无关任务中移除。

对新增或暂存的实现差异使用以下记录。简单切片可写在变更说明中，跨任务迁移应保存为可版本控制的记录：

```markdown
### DIFF-XXX：差异标题

- ref commit：
- ref 路径和符号：
- Chimera 路径和符号：
- 类别：品牌 / legacy UI / E2E / agent / 兼容 / 缺陷修正 / 临时迁移
- 差异及必要性：
- 共通业务入口及适配边界：
- 影响的主界面、legacy UI、agent、数据、内核和平台：
- 实际验证结果或未验证原因：
- 收敛、移除或重新评估条件：
```

发现 ref 缺陷时，先用测试或可复现步骤证明，再做最小修正。不能复制已知错误，也不能仅以“本地实现更好”为由扩大分叉。

## 4. 每个切片的迁移流程

### 4.1 读取参考并建立对应关系

沿用户入口读取相关 ref 实现：UI、hook、IPC、application client、状态管理、持久化、运行时和失败处理。只读与任务有关的源文件及架构说明，不遍历 ref 的依赖和构建产物。

开始修改前列出路径与符号映射，区分已对齐、必要扩展和待迁移部分：

| ref 文件 / 符号    | Chimera 文件 / 符号 | 差异类别          | 本次动作与验证               |
| ------------------ | ------------------- | ----------------- | ---------------------------- |
| 填写实际路径和名称 | 填写实际路径和名称  | 对齐或第 3 节类别 | 迁移 / 保留 / 暂存及对应测试 |

### 4.2 固定受影响的用户流程

记录主界面、legacy UI 和已有 agent 调用入口，确认持久化数据、内核、系统副作用与平台范围。已有测试可作为当前行为证据；新增 characterization test 不能把所有历史偏离都固化为永久产品要求。

保留 legacy UI 的交互和数据可用性。涉及用户数据格式变化时，明确迁移、失败恢复和升级验证路径，不直接丢弃旧数据。

### 4.3 可以大范围复制完整实现，再统一做品牌映射

**允许按完整功能范围批量迁移上游代码**：当 ref 的模块紧密关联时，可以一次复制整个子目录、多个包、Rust/IPC/React 的连续调用链、相应资源与测试，不以文件数或代码行数设限。优先从固定的本地 `ref/` 复制原始实现，保持上游目录组织、文件名、非品牌类型/函数/变量名、状态所有权与错误处理，而不是逐个手工重写成“功能相似”的代码。

推荐迁移顺序：

1. **确认范围和基线**：记录本地 ref commit、目录/符号映射、需要复制的完整调用链，以及现有 Chimera-only 功能/旧数据/权限边界；用独立 Worktree 隔离未提交改动。
2. **先复制后替换**：将一组相关上游源码、必要资源及测试复制到 Chimera 对应目录，暂不额外重构；然后使用经过审核的**品牌替换白名单**集中替换产品名、包名、应用标识、必需资源名等，例如 `NyanpasuClient` → `ChimeraClient`。替换脚本如需使用，必须限定目标文件和匹配模式，并检查实际替换结果。
3. **检查不能随意改的标识**：保留协议字段、配置键、IPC/API 公共契约、序列化数据和第三方标识，除非确有必要、已经核实所有消费者并提供兼容/迁移处理。保留上游许可证、版权头和必要声明；生成文件通过项目原有生成器再生，不复制过期产物替代生成验证。
4. **接回 Chimera 扩展**：在共通实现外的狭窄适配边界接回 main/legacy UI、Agent、Chimera Client/Service 等必要功能，不因整目录复制而覆盖已有扩展、权限确认或用户数据恢复能力。
5. **统一核验再收尾**：对照原始 ref 清单审阅新增、替换和删除文件，检查迁移后剩余品牌名、导入路径、符号引用、构建与测试；覆盖受影响的调用入口、失败路径、平台和实际 E2E 边界。无法验证的部分明确标记，不以“大部分复制成功”宣称功能已对齐。

ref 的 Actor、typed persistent state 或 runtime builder 如果属于该功能的实际实现，就应纳入迁移目标，不能无限期排除。可按架构边界分成几个可验证阶段，也可一次完成大批量迁移；依据是完整性、兼容性及可验证性，**不是任意的单次改动行数限制**。临时桥接仍须说明原因和退出条件，不顺带修改无关模块。

让 legacy UI 的必要适配接到同一应用 API。agent 调用这些共享能力，不直接修改另一套持久化状态，不绕过普通 UI 使用的事务和并发控制。

### 4.4 同时验证实现对应关系和行为

检查路径、文件名、符号、实际生产装配及调用链；确认新实现已被生产入口使用。测试通过不能代替这一步，文本相似也不能代替行为测试。

按作用范围更新并运行测试。主/legacy 两个入口都受影响时，应覆盖各自用户操作及共同结果；agent 使用同一能力时补相应回归。不要求仅因存在 legacy UI 就向它添加未请求的新功能。

### 4.5 报告阶段状态

报告基线、映射、差异、入口、测试及剩余步骤。仍由旧业务实现提供核心语义的阶段应标为“部分迁移”，不称作该功能已完成 ref 对齐。提交时只包含相关文件，不将其他未提交改动带入；本文不要求在用户未请求时自动提交。

## 5. E2E 是主要扩展

测试按所提供的证据分类：

| 类型                         | 可以证明                                       | 不能替代                         |
| ---------------------------- | ---------------------------------------------- | -------------------------------- |
| 单元 / 契约 / 集成测试       | 确定性计算、错误分支、事务、并发和接口契约     | 桌面用户操作是否可用             |
| Fixture UI / 编排测试        | 页面、交互、提案流及确定性状态展示             | 真实系统授权、主机修改、内核行为 |
| 真实桌面 E2E                 | 控件 → hook → IPC → 共享业务实现的实际用户链路 | 未检查的网络、重启或恢复结果     |
| 真实运行时 / 网络 / 升级测试 | 内核回读、网络请求、持久化、重启或升级结果     | 未执行的其他内核和平台           |

配置、TUN、代理等场景按实际影响验证 UI、持久化、运行时配置和系统状态。明确区分持久化输入、生成产物和内核当前状态；具体类型名称及结构采用对应 ref 实现，不为概念说明另造平行模型。

例如 Allow LAN 的真实验证应包括用户开启/关闭、刷新后状态、内核回读、实际 LAN 请求及恢复；涉及持久化时补重启验证。相关场景仍需验证 Mihomo 与 Chimera Client 的兼容，未运行的环境应明确报告。局域网测试使用环境配置的客户端，不硬编码私人主机。

对无用户入口的纯逻辑修改，用合适的单元/集成测试验证并说明原因，不人为添加生产测试入口来凑 E2E。仅修改文档或注释时执行文档和差异检查，无需启动桌面或修改系统状态。

新 spec 必须注册到 suite；新 unit test 必须有执行入口。核对 CI 实际选择的集合，不能把“文件存在”“typecheck 通过”“clippy 通过”或“已跳过”写成断言通过。

当前运行入口及局限见 [E2E README](../tauri-e2e/README.md) 和 [E2E 架构说明](tauri-e2e-minimal-architecture.md)。这些文档描述现状，不代表后续验收要求已实现。

## 6. Agent 扩展边界

工具契约、诊断投影、提案和任务编排集中在 agent feature。模型接入、会话和自主规划可以按任务扩展，但不应改变共通业务实现来服务另一套执行系统。

网络修改复用共享应用 API 和事务，并检查：

1. 用户确认的动作、范围和执行参数一致，提案具有过期及归属检查。
2. 执行前状态检查与共享写入的并发边界一致，不用旧快照覆盖其他合法修改。
3. 执行后读取实际状态；部分应用、验证失败和回滚失败具有可区分结果。
4. 失败时更新展示状态并提供恢复信息，不能继续展示过期快照作为实际结果。
5. 审计记录实际可读取，不能仅因有日志调用就认为已具备审计能力。
6. Fixture 流程与真实授权、系统变更及恢复测试分别报告。

这些是目标要求，不能据此宣称当前 agent 已拥有全部能力。当前 fixture 测试的边界见 E2E 文档。

## 7. 验证与环境约束

按修改范围选择检查；下面列出的命令并不表示每次都要全部运行：

```powershell
git diff --check
pnpm typecheck
pnpm lint:frontend-boundaries
pnpm --filter @chimera/tauri-e2e test:unit
cargo test --manifest-path backend/tauri/Cargo.toml
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
```

需要桌面验证时构建当前代码的 E2E 二进制，再运行对应 suite。不要用旧二进制验证新代码后报告通过。说明实际运行的命令、环境、通过/失败/跳过数量及未覆盖部分。

隔离 config/data/runtime 路径不等于隔离系统代理、服务和 TUN。主机级测试使用专用 runner 或 VM，遵守已授权的系统修改范围，保存并恢复测试前状态。清理只针对测试资源；Windows、WSL、远程主机等环境遵守用户的执行约束。不提交用户配置、订阅地址、凭据或未脱敏的原始日志与截图。

当前 `test:unit`、PR suite 和升级入口的覆盖限制见 E2E 文档。CI 覆盖调整应作为明确实现任务完成，不能只改文档就视为接入。

## 8. 切片完成标准

- [ ] 已记录实际 ref commit，处理了参考工作区差异。
- [ ] 对应路径、文件名、类型名、函数名、变量名及必要品牌映射已核对；批量复制时记录范围和替换白名单，检查误替换、许可/版权声明、遗漏的调用方和原有扩展。
- [ ] 生产调用链、领域模型、状态所有权和失败处理与 ref 对齐，或已明确标为部分迁移。
- [ ] 每项剩余差异均有类别、理由、边界及后续条件。
- [ ] legacy UI 保留可用，受影响流程通过共享底层验证。
- [ ] agent 与主/legacy UI 复用共通业务能力，没有新建平行业务实现。
- [ ] 受影响测试已更新、注册并实际执行；fixture、真实运行、跳过和未验证范围分别报告。
- [ ] 涉及的数据迁移、持久化、重启、网络、副作用恢复及内核/平台范围已有对应证据或明确缺口。
- [ ] 相关规范文档与代码现状一致，修改范围未混入他人的工作。

后续工作应让每个共通模块逐步收敛到对应 ref 实现，同时保留 legacy UI，并让 E2E 和 agent 成为其上的明确扩展。收敛的对象是共通业务分叉和临时桥接，不是 legacy UI。

### DIFF-CLIENT-ALPHA：Chimera Client prerelease channel

- ref commit：`def0dbf2716ff972255be47924401b60db7d482c`（本地工作区干净）。
- ref 路径和符号：`ref/frontend/query/src/ipc/use-clash-cores.ts::ClashCores`；`ref/backend/nyanpasu-config/src/application/clash_core.rs::ClashCore::{ClashRs, ClashRsAlpha}`。ref 提供 Clash-rs 稳定版和 Alpha 两项，但没有 Chimera Client 产品或渠道。
- Chimera 路径和符号：`frontend/interface/src/ipc/use-clash-cores.ts::ClashCores`；`backend/chimera-config/src/application/clash_core.rs::ClashCore::{ChimeraClient, ChimeraClientAlpha}`；`backend/tauri/src/config/chimera/mod.rs::ClashCore`；`backend/tauri/src/core/updater/`；`scripts/manifest.ts` 和 `scripts/check.ts`。Service wire identity is `chimera_utils::core::ClashCoreType::{ChimeraClient, ChimeraClientAlpha}` from `MFSGA/Chimera_Utils`.
- 类别：产品扩展。
- 差异及必要性：Chimera Client 提供 `latest` 预发布，与稳定版并列显示，沿用 ref 的稳定版/Alpha 选择模式。两者保留相同 Chimera Client runtime kind、配置和 TUN 合约；IPC 保留不同渠道身份，Alpha 使用独立 sidecar 与更新目标，防止更新一个渠道覆盖另一个。
- 共通业务入口及适配边界：两个界面选项复用现有 `changeClashCore`、`updateCore`、共享 runtime builder 和 core lifecycle API。Service 将两个 wire variant 映射到同一 `CoreKind::ChimeraClient`，再用原始 variant 选择稳定版或 Alpha 可执行文件；没有另建 runtime manager 类型。
- 影响的入口：主界面与 legacy 设置页共享 `useClashCores`；runtime 启动、updater、打包 sidecar 和支持的桌面平台受影响。agent 复用当前选中的 runtime，不新增独立实现。
- 实际验证：前端 `pnpm typecheck`、`pnpm lint:frontend-boundaries` 和 Prettier 检查通过；Rust 应用契约、legacy bridge、runtime builder、local host、updater 和 Alpha 版本展示测试通过。`pnpm prepare:check` 成功下载并登记 macOS arm64 Alpha sidecar（版本戳 `0.26.3-alpha+sha.229218a`），二进制 `-V` 输出一致。完整 manifest 生成器确认解析出该 Alpha 版本，但在无关的 Mihomo Alpha GitHub 请求处因 TLS peer 关闭而失败，因此版本清单保留了已核对的 Alpha 版本和资源模板。`pnpm build` 已生成 `.app` 和 `.dmg`，但命令最后因缺少 `TAURI_SIGNING_PRIVATE_KEY` 在 updater 签名步骤返回失败；构建包内已确认存在可执行的 Alpha sidecar。Service 扩展测试通过：`cargo +nightly test --locked -p chimera-ipc chimera_client_channels_keep_distinct_wire_and_executable_identities`、`cargo +nightly test --locked -p chimera-service service_resolves_the_alpha_sidecar_while_sharing_the_chimera_manager_kind`、`cargo +nightly test --locked -p chimera-service chimera_client_keeps_its_manager_kind`，以及主仓的 Alpha application-contract、Service wire-identity 和 reconcile-wire 测试；受影响 Rust 文件的 nightly rustfmt 检查通过。Service `v1.10.2` 发布 workflow `38156677500` 的 macOS、Linux、Windows 全部构建和上传 job 成功；主仓 `pnpm check:service-runtime` 报告 IPC/Service 版本对齐至 `v1.10.2`。`pnpm prepare:check` 再次成功并验证 Service macOS arm64 发布包 SHA-256，二进制报告 `chimera-service v1.10.2 (Release Build)`。Service 单元测试仍是源码级验证，不等于已安装系统服务或真实 TUN 端到端验证；桌面 UI E2E 尚未执行。
- 已知边界：Service IPC 类型、manager 映射和独立 Alpha 可执行文件均已进入源码及 `v1.10.2` 发布二进制，主仓 submodule 也指向该版本。已经安装旧版 Service 的主机仍需更新其 Service 二进制；真实系统服务重启、授权和 TUN 流量尚未端到端验证。
- 重新评估条件：在 Chimera Client 持续发布 `latest` 预发布期间保留此产品选项；其发布 workflow 改动时重新核对 asset 名称、压缩包布局和 tag/ref 约定。对已安装服务完成版本升级后，在专用 macOS runner/VM 验证服务模式启动、重启和 TUN，再把该平台验证标记为完成。UI 与 updater 继续按 ref 稳定版/Alpha 结构维护。

### DIFF-MAIN-SCROLL-VIEWPORT：主界面虚拟列表滚动视口

- ref commit：`69353fc9a7d7e4e427211a85cdaa0b9c07a4f01a`（工作区干净）。
- ref 路径和符号：`ref/frontend/ui/src/scroll-area.tsx::{ScrollAreaProvider, useScrollAreaViewport, useScrollAreaViewportElement}`；主界面 connections、logs、rules、proxy-group 对应页面的 `getScrollElement`。
- Chimera 路径和符号：`frontend/chimera/src/components/ui/scroll-area.tsx`；`frontend/chimera/src/pages/(main)/main/{connections/_modules/connections-table.tsx,logs/index.tsx,rules/index.tsx,proxies/$name.tsx}`。
- 类别：部分迁移。
- 差异及必要性：已沿用 ref 的稳定 viewport ref 与挂载后 viewport element 状态，并让虚拟器读取实际元素；这是避免子组件在 ref 尚未挂载时初始化虚拟器的必要修复。Chimera 当前 ScrollArea 仍保留 `offset` 状态、`type="always"` 行为和独立 `slot` 参数，ref 的 `isPastHeader` 与 `AppContentScrollArea` 尚未迁入；proxy-group 路由文件位置也仍不同。
- 共通业务入口及适配边界：主界面和 legacy UI 共用 Chimera ScrollArea，业务状态与数据查询没有分叉；只为现有组件增加 ref 对应的 viewport contexts。
- 影响的入口：主界面的 connections、logs、rules、proxy group；共享 ScrollArea 的 legacy 调用方经类型检查覆盖。主界面 E2E 已注册到 `test:main`。
- 实际验证：`pnpm typecheck:chimera`、E2E TypeScript typecheck、`pnpm lint:frontend-boundaries`、前端 build 和 E2E harness unit（25/25）通过；`test:main` 14/14 specs 通过，`test:smoke` 4/4 specs（9 条用例）通过，覆盖主界面和 legacy 入口。`test:settings` 整套为 12/16 specs：修改过的 `core-manager-main.e2e.ts` 单独执行通过；整套另有 4 个失败，分别是 `session-typed-state` 关闭按钮不可点击、`clash-settings-main` 未找到期望的布局节点、`custom-css-editor` WebDriver 窗口丢失、macOS 上不存在的 Windows UWP 卡片。后四项未由本次视口/代理缓存改动触发，仍需独立修复或按平台约束测试；不能据此宣称 settings 套件全绿。
- 收敛、移除或重新评估条件：后续对齐完整 ScrollArea API 和 AppContentScrollArea 时复核 `offset`、滚动条策略及组件位置；在确认主/legacy 的页面语义后再迁移或记录兼容边界。

### DIFF-PROFILE-PROXY-INVALIDATION：Profile 变更后的代理查询缓存

- ref commit：`69353fc9a7d7e4e427211a85cdaa0b9c07a4f01a`（工作区干净）。
- ref 路径和符号：`ref/frontend/query/src/provider/mutation-provider.tsx::mutationKeys.profiles`。
- Chimera 路径和符号：`frontend/interface/src/provider/mutation-provider.tsx::{PROFILES_MUTATION_KEYS,CLASH_CONFIG_MUTATION_KEYS}`。
- 类别：缺陷修正。
- 差异及必要性：ref 的 `profiles` 变更键未包含代理快照；本地复现显示，当主窗口已经缓存旧 Profile 的代理数据时，切换 Profile 后 IPC 返回的新代理组与主界面代理图标不一致。因此 Chimera 在 Profile 和运行时 Clash 配置事件中同时失效代理及代理提供者查询，避免继续显示旧快照。
- 共通业务入口及适配边界：仍由共享 `MutationProvider` 监听现有后端 mutation 事件并失效 React Query 缓存；未增加第二套代理状态源。
- 影响的入口：主窗口 navbar、proxies、rules 代理图标及 provider 代理列表；legacy UI 和 agent 继续读共享接口。`rules-main-proxy-icon.e2e.ts` 先挂载旧 Profile 查询，再通过测试 fixture 激活新 Profile，验证当前 UI 跟随运行时数据更新。
- 实际验证：`pnpm typecheck:chimera`、E2E TypeScript typecheck、`pnpm lint:frontend-boundaries`、前端 build 和 E2E harness unit（25/25）通过；`test:main` 14/14 specs 通过，其中该回归用例先挂载旧 Profile 查询再切换到 fixture Profile；`test:smoke` 4/4 specs（9 条用例）通过。该用例使用 IPC fixture 准备并由桌面 UI 观察，不代表真实网络流量验证。`test:settings` 整套 12/16，失败项和已知限制见 `DIFF-MAIN-SCROLL-VIEWPORT` 验证记录。
- 收敛、移除或重新评估条件：迁移到 ref 的 query/event 架构后，重新核对 Profile 事件的代理快照失效语义；只有新架构在“缓存旧 Profile 后切换”的回归场景仍能更新时，才移除此兼容修正。
