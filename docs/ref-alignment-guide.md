# Chimera 与 ref 实现对齐指南

本文与根目录 [AGENTS.md](../AGENTS.md) 共同约束后续开发、修复和迁移。最终目标是：共通功能的实现尽可能与 `ref/` 一致，包括架构、目录、文件名、类型名、函数名和变量名；保留品牌等必要差异，明确保留 legacy UI，以 E2E 测试为主要扩展，并扩展 agent 能力。

**迁移规模不设人为上限。** 一项完整功能涉及多个文件、目录或前后端调用链时，可以先大范围复制 `ref/` 的对应实现，再集中按映射替换上游产品品牌为 Chimera；不要求为了降低单次 diff 行数而逐文件搬运。除必要品牌和明确保留的扩展/兼容差异外，默认保留上游的结构、名称与业务行为。

可以分阶段完成迁移，但“行为相似、内部继续独立实现”不是最终验收标准。目录和命名对齐也是目标的一部分。本文不表示现有代码已完成对齐，也不把测试要求当作已通过的结果。

## 1. 参考基线

本次规则更新核对的本地参考版本：

```text
repository: https://github.com/libnyanpasu/clash-nyanpasu
branch:     main
commit:     c4b35e04870a9caa53f21d10af6fcfde4a6f4c01
verified:   2026-11-18; checked origin/main with git ls-remote over SSH, fetched the commit, and verified the detached checkout and nested submodules
previous gitlink: dd42ab212bcab4c61601a8c6922b26231609ba49
nested runtime: 3854f2c06331be0b36bb69adc59708802f661e57
enable-loopback: f1b9f8693a328ca0a6094951035b81fe302b0848
```

The target is 32 commits ahead of the previous gitlink. The reference root and
checked-out nested submodules are clean. These commits include the RPC resource
permission model, configuration sync setup, UI changes, and dependency updates.
The startup reconciler and Service endpoint recovery sources used by this
iteration are unchanged in this 32-commit range; the latest commit is still the
baseline for review. The pin records the reference implementation only; it does
not automatically apply upstream dependencies or product code to Chimera. Each
shared feature still needs its own path and symbol review.

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

### DIFF-RUNTIME-STORED-GRAPH：Runtime Pipeline 压缩快照与按需 Inspection

- ref commit：c4b35e04870a9caa53f21d10af6fcfde4a6f4c01（固定本地 ref，未修改）。
- ref 路径与符号：backend/nyanpasu-config/src/runtime/snapshot.rs 中的 StoredConfigSnapshotsGraph、StoredConfigSnapshotState.changed_fields、KeyframePolicy、apply_patch、ConfigSnapshotsBuilder.push_delta、restore / restore_with_parent；runtime/executor/{builtin,artifact,mod}.rs 中的 filter_fields、RuntimeArtifact.graph / final_node 和快照记录。
- Chimera 路径与符号：backend/chimera-config/src/runtime/{snapshot,executor}/；backend/chimera-core/src/client/{artifact_snapshot,runtime_build,runtime_inspection}.rs；backend/tauri/src/client/runtime_inspection.rs 和既有 runtime build 调用方。
- 类别：临时迁移 / legacy UI 兼容。Shared executor 现已向上游返回压缩存储图及确定性的 final_node，字段过滤和 Guard/TUN 已知变更直接写入 Delta；序列化版本升级为 v3，旧 v2 快照缓存按版本拒绝后需要从源配置重建。此缓存不属于用户 Profile 文档。
- 保留差异：Chimera 的 RuntimeInspectionData 已沿用 ref 的 StoredConfigSnapshotsGraph，RuntimeArtifact → Inspection 不再整图展开，而是使用 restore / restore_with_parent 按需恢复；主界面、legacy UI 和 agent 继续共用原有 inspection DTO/IPC，未修改外部协议或用户 Profile 持久化。Chimera 的 Finalizing、TUN 分阶段处理、Chimera Client MMDB、可选监听器及 DNS/TUN 策略仍与 ref 的执行顺序或字段所有权存在差异；不将这一阶段误认为 Runtime Pipeline 全量完成对齐。
- 验证边界：配置层单元测试（含 snapshot-persistence feature）271/271 通过；Core runtime build 7/7、artifact snapshot 3/3、Tauri Runtime Builder 11/11 通过。已覆盖顺序保持、负零符号、显式补丁重放、压缩图恢复、最终节点与配置一致、v2 缓存版本拒绝。编译警告仍存在；未执行真实桌面、内核、Service 或主机网络 E2E。
- 本轮补充验证：Core 的 Runtime Inspection 4/4、Artifact Snapshot 4/4、Runtime Build 7/7 通过；Tauri Runtime Inspection 7/7 通过。新增快照 Delta 恢复、敏感配置脱敏、补丁损坏错误返回、产物与最终节点不一致时拒绝发布等回归用例。
- 风险与回退：L2，共享 Inspector 的图表示和按需读取路径发生变化，未修改前端 RPC wire、Profile/订阅数据、Core/Service 配置所有权或宿主网络。若发生 Inspection 回归，回退本轮 Inspection/Artifact 适配提交即可恢复前一轮的完整图映射；若涉及旧快照缓存，按版本拒绝并从持久化源重建，绝不重写用户配置。独立审查与真机桌面/网络 E2E 仍是后续验证项。
- 收敛条件：下轮继续对齐 ref 的单次字段所有权计划与执行顺序，并评估可移除的旧完整图 materialize 兼容方法。保留 legacy UI 的展示能力和 Chimera Client/Service 兼容层，不复制新的平行运行时状态实现。

### DIFF-RUNTIME-OWNERSHIP-ORDER：ref 单一字段所有权计划与执行顺序

- ref commit：c4b35e04870a9caa53f21d10af6fcfde4a6f4c01（固定只读基线）。
- ref 路径与符号：backend/nyanpasu-config/src/runtime/executor/{plan,mod,builtin}.rs 中的 ownership / guard_ownership / tun_ownership、ManagedSettings、Defaults 和内建脚本顺序；post_process/OwnershipEntries；tests/orders.rs 的 #5678 未配置 HTTP/SOCKS 监听端口回归。
- Chimera 路径与符号：backend/chimera-config/src/runtime/executor/{plan,mod,builtin}.rs 及对应 tests/{orders,builtin}.rs。仍使用 ref 的 post_process apply_settings / apply_defaults / keep_inherited 语义。
- 类别：共通业务对齐 + Chimera Client 狭窄兼容。生产 Pipeline 改为一次构造 Guard+TUN+DNS+profile 缓存的所有权计划；过滤后一次恢复 Inherited，再执行 ManagedSettings、Defaults，随后才执行内建脚本。未启用的 port/socks-port 必须移除订阅所带的端口，不能与 mixed-port 争用；启用端口由 caller 已确认绑定值写入。Standard TUN stack 沿 ref 改为强制写入所选栈，防止订阅覆盖；DNS 的缺省值不覆盖既有 nameserver/enhanced-mode。去掉两段式 Guard/TUN 中间处理及已无人使用的旧测试 oracle。
- 保留差异：Chimera Client/Alpha 的 TUN 特有字段（route-all、dns-hijack bool、so-mark、device-id）和 nameserver/default-nameserver 缺省值由 TunFlavor::ChimeraClient 显式适配，未引入其不支持的 Mihomo stack/auto-route。ref 的 TunStackWrite 能力探测、Mips 拥塞控制器、降级告警及产品-specific Core feature matrix 尚未对齐；当前仍由 Core builder 的旧 derive_tun_flavor 决定目标栈。Chimera 的第二次按 Core 过滤、Finalizing 排序节点和 mmdb 兼容仍保留在内建脚本之后，目的是现有 Client Core 及检查界面兼容；这不等于完整复制 ref。
- 本轮验证：chimera-config（含 snapshot-persistence）271/271、chimera-core 完整 lib 429/429、Core Runtime Build 7/7、Tauri Runtime Builder 11/11 通过；Tauri lib cargo check 通过；针对 ref #5678 新增禁用监听端口回归，并验证 Defaults 前置、内建脚本覆盖次序、Chimera Client TUN/DNS 特殊字段及 Snapshot 最终节点一致性。已有编译 warning，未运行系统级 E2E、真实代理/TUN 和跨平台验收。
- 风险及回退：L2，可能改变生成配置的 TUN 栈、默认值时序、脚本输入及代理监听端口。若在特定平台/内核实测失败，回退本轮 config executor/plan 变更并重建配置快照，切勿改变用户订阅或进行宿主网络修改。真机 Core/TUN/DNS 和桌面 UI E2E 尚需验证。
- 收敛条件：迁移 ref 的 TunStackWrite + core capability matrix 到纯 Runtime Builder 并补充降级/unsupported 参数告警、适配 Chimera Client 特性；待平台回归稳定，再判断可否删除尾部重复过滤/Finalizing 兼容节点。主界面、legacy UI 和 agent 继续使用同一业务 API。

### DIFF-TUN-CORE-FEATURES：TUN 栈能力矩阵与 MIPS 配置迁移

- ref commit：c4b35e04870a9caa53f21d10af6fcfde4a6f4c01，固定本地只读基线；本轮未更新 ref gitlink。
- ref 路径及符号：backend/nyanpasu-config/src/clash/config/tun_stack.rs 的 TunStack::Mips、MipsConfig、SelectedTunStack、TunCongestionController；clash/features.rs 的 ConfigFeatures、FieldSupport、TunFeatures、UnsupportedConfigValue；runtime/executor/{mod,plan}.rs 的 TunStackWrite、managed_settings_logs、MIPS 参数所有权；backend/nyanpasu-profile-transform/src/runtime_builder.rs 的 derive_tun_flavor 与 final-config validation。
- Chimera 对应：backend/chimera-config/src/clash/{config/tun_stack.rs,features.rs}；clash/config/mod.rs；runtime/executor/{mod,plan}.rs；backend/chimera-core/src/client/runtime_build.rs；Tauri application_workflow/impact、state/mutation/impact、client/effects/plan 以及 enhance/runtime_builder；frontend/interface/src/ipc/bindings.ts 通过 Specta 测试生成。
- 类别：ref 共通业务迁移，附 Chimera 自有 Core 的必要能力适配。tun_stack.rs 与 ref 源文件逐字一致。Mihomo/MihomoAlpha 支持 system、gvisor、mixed、mips 和 MIPS controller cubic/reno/bbr/bbr3；ClashRs/Alpha、ChimeraClient/Alpha 不写 Mihomo 栈字段，后者继续使用原有的 route-all、so-mark 和 DNS 默认值。ClashPremium 暂按原有行为支持 system/gvisor，mixed 与 mips 选项回退到 gvisor 并在 ManagedSettings 记录 WARN；此 Premium 映射属于本地旧核的兼容推断，尚未在实核验证。
- 构建与数据：仅在选中 MIPS 时读取独立保存的 tun_mips.congestion_controller；空选项移除订阅原有 controller，交给核心决定默认值，切换到别的栈时保存值不丢失。稀疏 ClashConfigPatch 缺少 tun_mips 时保持可反序列化；旧 ClashConfig 文档缺少此字段时按默认值读取；显式 null 可清空 controller。Runtime Builder 在生成最终文档后根据 Core Feature Matrix 检查已知非法栈/控制器，遇到不支持值拒绝发布候选，而不是修改订阅。未知的未来字符串值仍交给核心决定，与 ref 语义一致。
- 运行时影响与接口：MIPS controller 只有在当前栈为 mips 时计入 runtime target digest 和 rebuild 分类，普通栈下修改其保存值不会意外重启 Core。前端 IPC TypeScript 绑定已通过仓库的 Specta 生成测试更新，Patch 仍可稀疏提交。主界面、legacy UI、agent 继续使用既有共享 API；Mihomo/MihomoAlpha 的主/legacy TUN 选择器显示 MIPS；本轮继续按上游迁入主界面 MIPS controller 选择器，使用同一个 useClashSetting("tun_mips") 共享配置写入（核心默认写入 null），并在主设置及网络子路由复用。Legacy UI 暂保留栈选择能力，未新增独立的 controller 配置业务逻辑。
- 验证边界：新增旧文档/补丁往返、矩阵覆盖、MIPS 切换恢复、空控制器删除、Premium 降级告警、最终非法配置拒绝、Client 专用字段和 runtime diff 等单元回归。配置层（含 snapshot-persistence）280/280、Core 433/433，通过；Tauri Runtime Builder 11/11、application_workflow impact 7/7、mutation impact 4/4、effect plan 32/32，通过；Specta 生成与 freshness、Tauri lib check、接口 TS typecheck、主界面 TS typecheck、前端 boundary 检查通过。真实桌面、多内核二进制、TUN/Service、Windows/Linux 主机网络 E2E 尚未验证，不能仅凭测试认定实机支持。
- 风险与回退：L2，扩展了保存的 tun_stack 枚举和 ClashConfig 结构；旧安装在回滚前可能无法解析 mips 栈选择，须先在新版切回 system/gvisor 或从备份恢复旧配置。不能直接删除用户的配置文件或重写现有订阅。若新路径在实机回归，应回退本轮能力矩阵/Runtime Builder 相关提交，保留用户数据并在降级前检查 MIPS 选择值。
- 待收敛：与真实各 Core 的 TUN 字段行为做专用平台验收，验证 Premium 的能力假设；审阅当前只用于旧测试的 Tauri enhance/tun.rs 是否可安全淘汰，并由独立 Windows Runner 验证新增 controller 菜单真实点击、刷新后持久化和回退，不能将 TypeScript 检查视为已通过桌面 E2E。API 保持同一实现来源，不建立第二套配置拥有者。

### DIFF-MIPS-CONTROLLER-UI：MIPS TCP 拥塞控制选择器

- ref 基线：c4b35e04870a9caa53f21d10af6fcfde4a6f4c01；ref/frontend/nyanpasu/src/pages/(main)/main/settings/clash/network/_modules/mips-congestion-controller-selector.tsx::MipsCongestionControllerSelector，network/index.tsx，及 ref/frontend/nyanpasu/tests/tun-stack-settings.browser.test.tsx。上游仅在所选栈为 MIPS 且 core feature matrix 包含 controller values 时显示；通过 tun_mips.congestion_controller 写入支持值或 null。
- Chimera：frontend/chimera/src/pages/(main)/main/settings/clash/_modules/mips-congestion-controller-selector.tsx，主 clash/route.tsx 与 network/index.tsx，共享 useClashSetting("tun_mips")、useRuntimeProfile()；5 种已支持语言新增上游对应标签。所有选择仍通过现有单一 Tauri IPC/配置持久化，未创建新的业务 owner。
- 已收敛（配置能力 RPC）：Chimera 已将 ref 的 get_config_features 接入 Tauri IPC / Specta，并通过 frontend/interface 的 useConfigFeatures、getConfigFeatures Query 与 nyanpasu_config 事件失效。主设置 TunStackSelector 与 MipsCongestionControllerSelector 现都直接使用后端 ConfigFeatures.tun 的 Values 列表；不再按 Core 名称硬编码选项。Chimera Client/Alpha 等自有内核依然由单一后端能力矩阵返回 Ignored 或支持值，不在前端复制第二套能力判断。
- 验证：前端 typecheck:chimera、typecheck:interface、localization 6/6、E2E harness 44/44 和类型检查、前端 boundary 检查通过；E2E settings 中新增 TUN 关闭时的实际菜单点击、MIPS 选择、BBR 保存、刷新与 null 还原回归（使用测试自身的隔离配置，finally 还原）。该新桌面用例尚未在 Windows 原生 WebDriver 上执行，不能算作 E2E 通过。
- 风险与回退：L1 UI 层，MIPS 写入可能让正在运行的 Core 重建；正常点击执行原有 mutation / query 的错误提示。回退 UI 选择器和本地化键不改变已存的 MIPS 设置，降低版本前请按 TUN 功能迁移记录验证保存的 stack 字段。

### CI-20261122：Windows Migration Test 与 Legacy Profile E2E 证据

- SHA：322bee212d993682be37c0ee71d08263fb5f3bad，CI https://github.com/MFSGA/Chimera/actions/runs/45067708384，E2E https://github.com/MFSGA/Chimera/actions/runs/45067708381；上两轮 SHA d73cc59e 和 a59a13e1 也存在同样的 E2E 测试失败（两条独立工作流）。
- 确认的 CI 缺陷：Windows Lint 的 Test backend workspace 在 migration_child::tests::rejects_unexpected_args_and_child_paths_outside_absolute_roots 失败；测试错误把 Unix `/tmp/config` 当作 Windows 绝对路径。生产代码坚持 PathBuf::is_absolute 的安全约束是正确的。本轮修复该单元测试为 tempfile::tempdir() 的平台本机路径，并断言解析结果；本机 macOS Rust 单测通过，Windows 原生测试仍需新 SHA CI 验证。
- E2E 观测而非根因结论：Desktop (push) 的 smoke 测试共 4 个文件通过、1 个文件失败，失败 spec 为 proxy-localization.e2e.ts；有失败时刻 PNG/HTML 证据，创建本地 Profile 对话框仍存在，OK 为 loading，侧栏显示正在应用配置。WebDriver execute/sync script timeout 随后触发清理时的第二次超时；后台记录了源版本冲突警告，但尚无证据证明哪一次 backend call 最初卡住。没有放宽断言、延长脚本超时、跳过场景或伪造创建成功。本轮在点击前后/列表显示和读取提交结果的边界增加不含敏感参数的阶段日志，下一次原生 E2E 用于定位。不同于 Rust 单测修复，E2E 失败仍视为未解决。

### CI-20261122-PROFILE-SERIALIZATION：Legacy 创建后文件保存竞态

- 证据：连续三个提交 d73cc59e、a59a13e1、322bee21 的 Windows smoke 都在 proxy-localization.e2e.ts 失败。run 45067708381 的堆栈精确指向第309行的 save_profile_file，而不是点击创建。测试只等 Profile 列表新增 UID，不等 UI 的 create.mutateAsync 完成和对话框关闭，可能导致自动激活与另一次文件写入同时进行。失败截图中的 OK loading 证明 UI 尚未完成，不能推导具体后端根因。
- ref 对应：ref/backend/nyanpasu-core/src/client/mod.rs 的 create_profile、try_auto_activate_if_none 与 Chimera backend/tauri/src/client/mod.rs 对应入口保持创建 Config 后空选择自动激活协议，不能绕过该业务行为。
- 本轮范围：只在 tauri-e2e/specs/proxy-localization.e2e.ts 调整测试边界；真正点击 OK 后等待创建对话框名称输入框消失，再调用 get_profiles、save_profile_file、activate_profile，最后通过可见 Profile 卡片切换。保留严格 UI/后端断言、脱敏日志和清理；未动生产 Runtime/持久化/CoreLifecycle。
- 验证：本地 E2E TS 和格式检查通过。此前 607b6fdf（run 45070826318）新增阶段日志证实点击 OK、配置行显示、get_profiles 返回全部完成，但随后第317行 save_profile_file 超时；该 SHA 尚未包含本轮等待对话框关闭的修复。Windows 原生 smoke 修复版新 SHA 尚未执行，不能视作问题已经修复。若等待对话框关闭超时，需结合该时刻截图与本机后端日志追踪创建自动激活；若保存仍超时，检查 actor commit_with_resources 与 runtime preflight 的交接。

### CI-20261122-PROFILE-CREATE-GATE：Windows 创建未返回的真实阻塞边界

- 证据：Windows E2E https://github.com/MFSGA/Chimera/actions/runs/45072061799（commit baa10cab）按“真正点击 OK、等待对话框内 name input 消失”执行；只出现点击前、点击后两个阶段日志。第 308 行的 waitForDisplayed(reverse) 在 30 秒内未完成，截图仍显示创建配置与 OK loading。原先 607b6fdf 的独立读能发现新 Profile，但保存命令超时，不能据此认为 create_profile mutation 已结束。上一轮所谓“仅测试串行化即可修复”已被反证：**问题不止测试抢跑，创建命令的某个异步阶段本身阻塞**。
- 当前已核对的真实链路：ProfileDialog.onSubmit → useProfile.create.mutateAsync → IPC create_profile → ChimeraClient.create_profile → ProfilesActor.Add（源文档持久化、物化与 jobs/reconcile）→ try_auto_activate_if_none → ProfilesActor.SetCurrentIfNone → run_state_write → CandidateCheck::validate（捕获、构建、Core -t）→ CoreLifecycleCheckedInbox → 版本条件提交。ref 具有同样创建后自动激活原则，不得绕开/跳过以获得绿色测试。
- 本轮仅 E2E 构建诊断：backend/tauri/src/state/profiles/actor.rs、client/reconciler/{check,adapters}.rs 加入 cfg(feature = "e2e") 的固定 stage 日志，涵盖 Add、源持久化、source reconcile、自动激活、候选捕获、CoreCheck 和 Checked inbox；不记录 Profile 名称、UID、路径、YAML、代理或凭证。默认生产构建完全不编入这些日志，避免改变核心工作流和耗时。
- 验证与限制：Tauri cargo check --features e2e、候选检验测试 13/13、Profiles 测试 94/94 通过（原有 warning 存在）。未修改生产业务语义，真实 Windows 新 SHA 尚未重跑；不能宣布 Profile 创建故障修复。下一次 E2E artifact 可用 stage 的最后一条确定阻塞边界；仅在有可复现的源/RuntimeGate 契约测试后再实施小范围正式修复。不得增加测试超时或绕开内核检查。

### CI-20261122-CORE-LIFECYCLE-MAILBOX：Windows Checked 接收延迟的原生证据

- 证据：Windows E2E run 45074585452，commit e7ef4ea7，已解包 tauri-e2e-diagnostics-45074585452 并读取原生 wdio 日志。22:01:37.014Z Profile 文件/文档发布完成，22:01:37.030Z Add Actor 正常回复；22:01:37.035Z SetCurrentIfNone 开始；22:01:37.079Z candidate 生成完成，22:01:37.449Z Local core check 已通过。candidate_checked_admission_start 在 22:01:37.449Z，candidate_checked_admission_finished succeeded=true 在 22:02:45.917Z，等待约 68.5 秒。随后 SetCurrentIfNone 的源提交立即完成。UI 等待创建对话框关闭只给 30 秒，因此重现超时。清理阶段仍有第二批 Checked 请求排队；不能将清理日志当作首因。
- 排除：不是初次 Add 的本地文件物化/Profiles 发布锁、不是候选配置构建、不是 Mihomo -t 验证，且 Checked 最终得到成功 ACK，并非永久关闭 Actor。只证明 CoreLifecycle Actor 的已校验候选接收/ACK 延迟，尚不能证明是哪个 Message 或哪个异步 I/O 阶段占用了 Actor 处理循环。
- 共享根因排查范围：backend/tauri/src/client/core_lifecycle/mod.rs 中 ApplicationWorkflowActor::handle 的 Message::Completed 包含 Core 状态/选中版本二次验证等 await；ServiceReadiness/ObserveService/ServiceRecoveryTick 分支也包含 await。Message::Checked 自身仅纳入 CheckedRuntimeInbox 并 ACK，正常应短。此处的 mailbox 串行 await 需要真实计时辨别，不能直接复制第二个 Core owner 或把 ACK 在数据尚未被 owner 保留时发出。
- 本轮变更：仅 cfg(feature = e2e) 下启用 E2eCoreMailboxProbe，记录相关消息开始、结束、超过 100ms 的 handler 耗时，以及 Completed 内主要异步观察边界，字段仅为固定 kind/stage/elapsed_ms，不含 URL、路径、Profile 文档、凭证。生产构建不编入。已通过 cargo check -p chimera --features e2e --lib 和 CoreLifecycle 159/159 个相关 lib 测试；并非完成根因修复。
- 后续验收门槛：新 Windows smoke 原生日志应先把约 68 秒定位到一个具体消息处理路径，再为该 I/O/队列引入可复现的 Actor mailbox ACK 并发回归；修复必须保留原子 Checked admission、source-version CAS 和不确定结果的拒绝语义，不得仅延长 WebDriver/IPC 超时或绕开自动激活。完整 Windows E2E 尚未重跑本轮探针提交。

### CI-20261122-SERVICE-WATCH-FLOOD：排除重复就绪事件导致的 Checked ACK 饥饿

- 根因证据：Windows E2E run 45076380623（SHA 69ccb525）上传的 wdio 原生日志记录，创建配置后在 22:20:34.535 UTC 启动 Checked admission，直到 22:21:35.902 UTC 才由 CoreLifecycle Actor 处理 Checked（耗时 0ms）并 ACK，队列等待约 61.4 秒。同一创建故障区间前约 32 秒共处理 **320 次** ServiceReadiness，其中 **138 次** handler 耗时超过 100ms；在整轮原生日志中，共有约 3961 条 ServiceReadiness entered。并非 Core -t、文件锁或 CheckedInbox 本身耗时，属于固定 Service 状态重复投递造成的 mailbox 积压。
- 原始路径：backend/tauri/src/client/core_lifecycle/mod.rs::spawn_with_profiles_and_content 中两个 Service watch 消费分支，原先每次 .changed() 即 cast ServiceReadiness，无论当前、前次状态是否包含新证据。ref ServiceActor 的 Probe 回答可每次发布 Status watch；重复的 NotInstalled 或不变的 Core owner 不能构成需要再执行 Service handoff 的事件。
- 修复：新增 service_followup_evidence_changed(previous,current) 对实际可影响 host/retry 的语义状态判定：phase、ready_generation、settled_ready、status、runtime_owned、compat、restart_attempts，以及 ServiceEvidence::from_status 对 Core running/stopped/foreign/identity/revision 的比较。只有其变化时才 cast ServiceReadiness；既有 startup_recovery_evidence_changed 继续独立驱动 Recovery，不被去重错误吞掉。正常状态快照的发布和只读消费不变；单一 CoreLifecycle owner、source CAS、CheckedInbox 接受/拒绝语义不变。
- 验证：重复 NotInstalled 1000 次不会生成新的服务 follow-up，真正的 daemon phase、ready_generation、ownership、retry、Core status 转换仍产生信号。macOS 本地两项新增单元测试及 E2E feature compile 通过；完整本地 Tauri lib 测试和新的 Windows smoke 仍需核对。验收要求：Windows 原生 E2E 创建必须在现有 30 秒断言内完成，且 ServiceReadiness 同状态洪泛消失；不能仅看编译/模拟测试或增加超时。
- 风险：ServiceActor 观测中非 ownership/status 的附属元数据变化不再触发 CoreLifecycle follow-up，这些字段本身不影响 host 决策。若后续增加依赖字段，需要同步扩展 service_followup_evidence_changed 和真实状态转换测试。回滚本次改动将重新引入重复就绪事件的排队风险。

### DIFF-DNS-CONTROL-BRAND：DNS 控制来源与管理页面

- ref commit：`4607a7379f0aa060daf55b369dc8103dda929d82`。
- ref 路径与符号：`backend/nyanpasu-config/src/clash/config/dns.rs::ControlSource::Nyanpasu`；`frontend/nyanpasu/src/pages/(main)/main/settings/clash/dns/`；`backend/nyanpasu-core/src/actor_v2/api.rs` 的 DNS 查询与缓存清理。
- Chimera 路径与符号：`backend/chimera-config/src/clash/config/dns.rs::ControlSource::Chimera`；`backend/chimera-core/src/actor_v2/api.rs`、`backend/tauri/src/client/{clash_api,clash_config}.rs` 与生成 bindings；`frontend/chimera/src/pages/(main)/main/settings/clash/dns/`。
- 类别：品牌映射 + 共通业务对齐。上游 `Nyanpasu` 序列化标识映射为 `Chimera`，保留旧值 `nyanpasu` 的反序列化别名。DNS、Hosts 草稿、控制来源、candidate validation 和 Core API 沿用共享设置/runtime/application API，未建立平行业务状态。
- ref 缺陷例外：ref 的 YAML shape check 在任一侧为 `null` 时跳过类型校验；这会允许 `dns.enable: null` 进入草稿，后续 typed save 才失败。Chimera 只允许 `listen`、`fake-ip-range`、`fake-ip-range6` 三个可空字符串接受 null，其余 typed DNS 字段仍校验类型。DNS YAML 桌面用例覆盖 `enable: null` 拒绝、未写入持久配置；若 ref 修复其可空字段 schema，应重新核对并收敛。
- ref 缺陷例外：`render_hosts` 在不支持列表值的 Core 上会检查整组 Hosts 目标；若后续未选中的目标是 alias，就连同有效的首个 IP 映射一起丢弃。Chimera 只校验该 Core 实际读取的首项，并继续警告只保留首项。既有 `hosts_values_are_filtered_per_core_without_losing_saved_draft` 回归用例覆盖“首项为 IP、后项为 alias”。若 ref 修复这一问题，应重新对齐。
- UI 决策：保留 ref 已有的 DNS/Hosts 页面、嵌套设置导航与页面标题。`04c668a` 基线和当前候选均没有 Chimera-only Applied DNS summary，因此本提交不记录该 UI 的删除。日志页不新增尚无共享 API 支持的来源标签。
- 验证与边界：见 `docs/tasks/ref-sync-alignment-2026-11-23.md`。桌面覆盖包含 profile-owned 草稿、YAML 草稿、DNS/Hosts 控制开关；真实解析成功、cold start、Service 核心覆盖矩阵仍未验证。
- 收敛条件：继续补齐所有支持 Core 的 feature matrix 和真实 DNS 网络 E2E；任何序列化变化需验证旧 `nyanpasu` 配置回读与回滚兼容。

### DIFF-SETTINGS-ACTIVE-STATE：嵌套设置导航选中态兼容

- ref commit：`4607a7379f0aa060daf55b369dc8103dda929d82`；ref 文件
  `frontend/nyanpasu/src/pages/(main)/main/settings/_modules/settings-navigate.tsx`。
- Chimera 路径：`frontend/chimera/src/pages/(main)/main/settings/_modules/settings-navigate.tsx`。
- 类别：窄 UI 兼容差异。导航目标、href 和点击继续由 ref 同款
  `useLinkProps` 提供；当前 Chimera 主设置路由的真实点击路径中，页面与浏览器地址
  已到达目标，但 Link 未提供 `aria-current`，侧栏项目也未选中。`useMatchRoute`
  只用于 `data-active`/`aria-current` 表现，并遵循 `activeOptions.exact`；不改变
  导航目标、跳转、访问边界或业务状态。这记录为当前运行时兼容边界，不认定为
  ref 产品缺陷。
- 验证：修正前现有 settings layout spec 在目标页面显示时观测到导航项全 inactive；
  修正后 Windows/Edge 157 完整 spec **6/6** 通过。ref checkout 的安装目录为
  TanStack Router `1.170.18`，而 ref 锁文件和 Chimera 锁文件 pin `1.170.46`；
  不能用陈旧 ref 安装证明锁定版本的运行时行为。
- 收敛条件：在可安全使用的同版本 ref runtime 与相同真实点击/路由场景下重验
  `useLinkProps` active 状态；若其能正常选中，则移除该适配。若也复现，再向后续
  ref 对齐评估提交这一窄修正。

### DIFF-LOG-SOURCE-API：日志来源切换与快照恢复的共享 API 差异

- ref commit：`4607a7379f0aa060daf55b369dc8103dda929d82`。
- ref 路径与符号：`frontend/nyanpasu/src/pages/(main)/main/logs/index.tsx::{LogsLayout,FileLogs,MeowLogbook}` 和 `frontend/query/src/ipc/use-clash-logs.ts::{retry,loadOlder,detail}`；来源包含 Core、Logbook、app、Service。
- Chimera 路径与符号：`frontend/chimera/src/pages/(main)/main/logs/`、`frontend/interface/src/ipc/use-clash-logs.ts`、`frontend/interface/src/provider/clash-ws-provider.tsx`。
- 类别：部分迁移。页面布局、加载状态、搜索与错误恢复对齐 Core 日志路径；Chimera 目前通过单个 WS snapshot + live event stream 提供 Core logs，没有共享文件、Logbook、应用或 Service 日志查询 API，因此暂不显示来源切换。
- 验证与边界：见 `docs/tasks/ref-sync-alignment-2026-11-23.md`。重试只重新读取 shared WS snapshot；没有验证文件/Service/Logbook 来源，且不会证明真实 Service 日志权限或系统生命周期。
- 收敛条件：共享日志源 API 与权限/错误语义可用后，沿 ref 恢复来源切换及对应分页详情能力。

### DIFF-CONFIG-FEATURES-RPC：对齐 ref 配置能力查询和 TUN 设置显示

- ref 基线：c4b35e04870a9caa53f21d10af6fcfde4a6f4c01。来源：nyanpasu-core/src/client/mod.rs::config_features、nyanpasu-rpc/src/commands/system/core.rs::get_config_features、frontend/query/src/ipc/use-config-features.ts，以及两个 TUN 选择器。上游依据后端实时核心能力矩阵判断 stack 和 MIPS congestion-controller 可选值。
- Chimera 迁移：backend/tauri/src/client/application.rs 直接读取已提交应用配置中的 core.config_features()；ipc.rs 新增只读 get_config_features，specta_export.rs 注册命令，frontend/interface/src/ipc/bindings.ts 对齐对应 Specta 类型与方法，query-bindings/use-config-features.ts 暴露共享查询。MutationProvider 对应用配置变更使能力查询失效；核心切换调用同样失效此查询。
- 显示语义：TUN Stack 和 MIPS 控制器选择器只遍历 ConfigFeatures.tun 的 FieldSupport.Values；Ignored 不展示选项，尚未获得能力查询时亦不推测。MIPS 仍仅在所选栈为 mips 时显示，保存时沿用现有配置接口和持久化逻辑。核心名称与自有 Chimera Client 特性由 backend/chimera-config/src/clash/features.rs 统一决定。
- 本轮边界：只迁移读能力、查询失效和界面选项来源；未修改 Core/TUN/DNS 生成、Profile 数据、Service 所有权及 UI 以外的调用方。未运行任何测试或编译验证（按用户要求）；完成了代码格式化与源码差异审阅。TypeScript Specta 绑定按上游结构同步，后续正式 CI 仍应检查生成器一致性和 Windows 原生菜单表现。
- 收敛：这是此前 MIPS 菜单标注的能力查询缺口，不再需要以 Mihomo/MihomoAlpha 名称检查代替 RPC。剩余特殊 UI/legacy 差异按独立迁移条目处理。

### DIFF-TRAFFIC-LOCAL-SOURCE-CACHE：迁入 ref 的受授权本机出口来源归因

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82；ref/backend/nyanpasu-core/src/client/traffic.rs::LocalSourceCache。上游把明确探测到的本机 IPv4/IPv6 出口地址注入 Traffic owner，并在用户关闭探测、启用 TUN 或本机透明代理时屏蔽；不会根据代理出口自行推断本机 IP。
- 迁移前：backend/tauri/src/client/traffic.rs 始终用 NoLocalSource 填充 TrafficArgs.local_source，虽然 backend/tauri/src/network_info.rs::probe_direct_egress 已有完整的 opt-in、STUN/HTTP echo 和 capture gate，Traffic 仍无法使用已授权探测结果。
- 迁移后：client::traffic::LocalSourceCache 使用当前唯一 ApplicationClient 与 ClashConfigClient 提交状态，显式 probe 的结果按地址族解析后写入共享缓存；ChimeraClientInner.local_source 是唯一缓存持有者，TrafficArgs 复用相同 Arc，未引入第二个网络探测或 Core/Service owner。明确禁止捕获模式、未授权时的本机来源归因，读出前重新执行和网络探测完全相同的 gate。
- Chimera 特有安全适配：地址与探测开始时的两个源版本一同保存。网络探测期间发生任意提交版本更新的结果不入缓存，已缓存数据在任一源版本变更后不再使用，防止用户关掉探测、切换网络捕获模式又恢复时误把旧代理出口认成本机地址。后续只有用户再次显式探测才能刷新缓存。
- 本轮仅后端流量来源归因，不改变网络探测端点、配置文件、DNS/代理/透明捕获运行时或前端接口。未执行任何单元测试、编译检查或桌面 E2E（用户指定）；已做 Rustfmt 和静态代码/差异审阅。当前所有异步任务仍由现有网络探测请求和 Traffic actor 持有，不新增长期后台 worker。

### DIFF-RUNTIME-FROZEN-PROFILE-CONTENT：收敛 Runtime 与 Profiles 的冻结内容边界

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82；上游 client/reconciler/inputs.rs 的 FrozenProfileContent 和 RuntimeBuildPort::capture_content 是候选构建及 Core 验证的唯一冻结输入。源文档的提交与 managed-file 的读取必须保持原有 ProfileContentReader 文件锁和版本边界。
- 本地旧差异：backend/tauri/src/client/reconciler/inputs.rs 重复定义 FrozenProfileContent + 依赖闭包读取，与 backend/chimera-core/src/client/runtime_build_port.rs 有两套等价逻辑；runtime_build.rs 又手写一次闭包遍历。Tauri 还保留 ref 不再使用的 RuntimeContentPort blanket adapter。
- 当前收敛：Reconciler inputs 直接 re-export chimera_core::client::runtime_build_port::FrozenProfileContent；RuntimeBuildService 的新构建路径与 legacy_preparation 的候选/已提交读取，都调用同一个 core::runtime_build_port::FrozenProfileContent::capture_content。删去冗余 RuntimeContentPort 与重复文件读取实现。legacy_preparation 仅保留现阶段必需的 staged_content 与 ProfileContentReader 的读锁/提交版本栅栏；没有新建 source owner。
- 原语义边界：闭包顺序、global/scoped transform、Composition 来源、读取错误保留、仅为活跃依赖替换 staged_content、Target identity 和 Core 校验字节均由共享 core 类型决定；没有修改 RuntimeBuildPort、Config 持久化、Service host、Source CAS 或 Core 运行时写入。同步维持 content source 的 Send + Sync 约束。
- 仅代码迁移，本轮没有执行单元测试、构建、Clippy 或 E2E（用户要求）；已执行 Rustfmt、git diff --check 和人工静态差异检查。下一次集中验证重点是 staged 文件优先级、源晋升时的内容读锁，以及 Reconciler 和 legacy gate 输入类型一致性。

### DIFF-CANDIDATE-GATE-PRODUCTION-FALLBACK：移除生产级旧 CoreLifecycle 候选校验回退

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82；ref 的 client/reconciler/check.rs::CandidateCheck 通过统一 RuntimePreparation、RuntimeBuildPort、CoreValidatorPort 为来源文档构建并校验候选，状态提交由 RuntimeGate 通知唯一运行时 owner。
- 生产现状：Chimera 已为 Profiles、Application、Clash 构建并安装 Core-owned CandidateCheck，且共享提交后的 Checked inbox / Core 状态所有权。Clash 的只读预检和停止状态 Profile TCC 已使用该路径，但此前遇到未安装 gate 或无有效 runtime preparation session 时仍可能调用 legacy_check::CandidateCheck::check 或 legacy_check::check_candidate，转向独立 CoreLifecycle 验证路径。
- 本轮收敛：Clash 生产预检只允许已安装、共享的 CandidateCheck::validate_only；Stopped Profile TCC 只允许共享 gate::validate_stopped_profile_tcc，并要求 Core-owned runtime session。任一前提缺失均返回明确的候选不可用错误，不再独立构建/校验另一份 Core 配置。旧 legacy_check 整个模块仅为 cfg(test) 编译，保留隔离 fixture 及类型化拒绝错误的回归测试。
- 兼容性边界：没有将仍在使用旧 postcommit Apply 的 Clash/Core 选择与 Service host 路径强行切换成 CheckedSourceCommitted；没有新建第二个 Core owner、改动 Profile 文件锁、暂存字节、Source CAS、Runtime 产品、Service handoff 或现有模式切换效果。legacy_preparation 继续服务运行时迁移期间的安全内容捕获，但旧校验器不再在生产编译。
- 本轮按用户指示未执行单元测试、编译、Clippy 或桌面 E2E，仅做源码路径审查、Rustfmt、文档格式整理与 git diff --check。后续迁移重点是把 Application/Clash 的旧 postcommit Apply 与 Core owner 生命周期一起移到上游 Checked 提交流程，不能仅替换接口后忽略 host 转换。

### DIFF-CHECKED-DNS-HOSTS-SOURCE-OWNERSHIP：将本地 DNS/Hosts typed 接管纳入统一 RuntimeGate

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82，统一 Source RuntimeGate 在提交之前准备完整候选、确认 Core 校验并由唯一 Core owner ACK，再用源版本通知提交，禁止两个并行的 Runtime Apply。
- 本地现状：Chimera 的 Clash typed patch 仅限 enable_clash_fields / expand_include_all 使用 checked inbox；DNS/Hosts 的接管与已接管表变更虽然由纯 RuntimeBuilder 渲染，却仍走 legacy validate-only、source CAS、inline rebuild 的兼容路径。Application 的 checked 准入只排除 Core/Service，可能将带有系统代理、启动项、热键等副作用的组合补丁错误纳入无后处理的 checked 路径。
- 本轮迁移：仅在非 Service 主机、已安装共享 gate 时，允许 DNS/Hosts 控制源选择器以及当前已接管或在同一请求中显式更改控制源的表值进入 typed CheckedSourceCommit。使用同一个已提交 Clash 源快照进行准入和 source CAS，预构建的 Core 候选经真实验证与请求收据后通知唯一 owner，不运行第二次 inline Apply。
- 仍为 Profile 控制的 DNS/Hosts 草稿编辑继续使用文档保存行为，不强迫 Runtime 重建；TUN、端口、控制器、代理模式及其他任意未知 typed 字段仍拒绝 checked 路由。Application checked 路由只接受独立 enable_builtin_enhanced 请求；其他所有组合字段（包括系统效果）保留原有流程。显式同值请求继续保留 Runtime 意图。
- 修改包括 backend/tauri/src/client/clash_config.rs、backend/tauri/src/client/core_lifecycle/workflow.rs；在已有测试模块加入准入判定回归用例，但按用户要求不执行测试、构建、Clippy 或 E2E，仅进行 Rustfmt 与静态差异审阅。下一阶段才处理 Core/Service 所有权转换与剩余旧的 inline Apply，不能直接放宽主机状态条件。

### DIFF-LEGACY-RUNTIME-CROSS-DOMAIN-SOURCE-FENCE：旧 Application/Clash Apply 栅栏对齐三域提交版本

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 Source RuntimeGate 的提交由唯一 RuntimeReconciler 消费，Core 应用前从 Application、Clash、Profiles 和 managed-file 闭包共同确认最新的已提交目标。当前 Chimera 的 legacy inline Apply 仍保留分阶段 host/TUN/port 效果，不能直接引入第二个写入者。
- 旧问题：reconciler::committed::apply_committed 只围绕触发变更的单个源版本做应用前后检查。当另一个源域在校验之后或 Core Apply 期间提交时，发起域版本未变，旧流程可能误报收敛，或者执行属于旧目标的模式连接中断。
- 本轮迁移：新增只读 SourceVersions::from_committed 与 apply_with_dependency_fence，所有版本从已有 source snapshot handle 读取，避免 source actor 正在等待 CoreLifecycle 时互相阻塞。Application legacy patch、Clash typed legacy patch、Clash raw/override legacy patch 都在候选验证前保存其他源域的版本，并在 Core Apply 前后核对。发起域的版本仍由既有 apply_committed 独立核验。
- 目标推进语义：其他源域版本不同则产生类型化 CommittedTargetAdvanced；旧 runtime_deferred 现在将此类错误标记为可重试，通过已有 CoreLifecycle SourceCommitted / coalesced Retry 路径收敛，不撤销已持久化来源、不新建 Runtime actor 或强行触发第二次 inline Apply。raw Clash 的显式 mode interruption 在 Core Apply 后额外核对依赖域版本，避免对已经过期的目标主动断开旧连接。
- 测试支持：加入模拟 Application apply 前发生 Profiles 提交、Core apply 期间发生 Clash 提交的回归用例，并断言新错误分类为可重试；按用户要求未运行任何测试、编译、Clippy 或 E2E，仅执行 Rustfmt、静态审阅与 git diff --check。
- 迁移限制：这是旧 Apply 的单调提交版本栅栏，不等价于 ref 完整内容摘要与 Core Applied-instance 证明。managed-file 内容与执行 Host 的所有权确认仍应由后续 RuntimeReconciler / RuntimeSourceAuthority 迁移接管，不能据此放宽 Service/TUN/port 路由。

### DIFF-SERVICE-RELEASE-LEASE-OWNER-FENCE：S5 Service daemon 释放绑定就绪代次与 Local Core 实例

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。ref client/reconciler/committed.rs::maintain_service_release 和 apply.rs::release_service 仅在 Local owner 已建立、Core 无未决操作且仍有待释放债务时处理 daemon stop，失败保留独立 pending_release。Chimera 的 ServiceActor 还存在多实例、错误停止的隔离语义，必须在完整复用 ref 前维护实例证明。
- 本地旧差异：CoreLifecycle 的 MaintainServiceRelease 已在唯一 Actor 内持有独立 release-owed debt，并在取得 Service transition lease 前后重验可释放状态；但对加锁前后不同的 Service Ready 代次或 Local Core 实例没有明确相等性判定。新的 Ready daemon 恰好同样报告 owned/stopped 时，旧的已排队释放意图可能继续用于新的 Service 状态。
- 本轮收敛：读取初始 ServiceEvidence 和 authoritative ready_generation；获取现有 Service 过渡锁后，重新验证 Local Core 状态与 instance_id/state_changed_at/run_type 完全相同，确认同一已接管但已停止的 Service Core、相同 ServiceEvidence 与 Ready 代次，然后才调用 stop_daemon；停止后继续要求新探针证明 daemon Absent，且 Local Core 仍为原有已验证实例。单一 CoreLifecycle actor、现有 release debt 和显式 Retry 机制未改变。
- 失败语义：代次、实例、运行状态、Service ownership 或 probe 发生变化时，拒绝过期维护命令，失败由 CoreLifecycle 的 ServiceRelease debt 记录；非显式 Retry 不会反复停止一个变化中的 Service。无新的 Core 写入者，不改变 host 选择、Source CAS、Runtime product 或 TUN/port 行为。
- 增补 same_release_owner 的回归用例代码覆盖 Ready 代次推进、foreign/running/unreadable/失去 runtime ownership 等情形；按用户要求未运行测试、编译、Clippy 或 E2E，只执行 Rustfmt、静态差异检查。后续仍需将完整 RuntimeReconciler 与 Service owner 流程按 ref 合并，当前只是 S5 daemon 释放的安全所有权栅栏。

### DIFF-OWNER-FACTS-S2-S3-COHERENT-SNAPSHOT：共享 ref S2/S3 Core/Service Facts 稳定性验证

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。ref client/reconciler/plan.rs、startup.rs 与 workflow.rs 以同一组已观察的 Core、Service 与运行意图组合 Facts，再由 plan(Facts) 决定 S2 owner handoff 和 S3/S4 Apply/Stop；未完成的外部操作与未知所有权不能作为动作许可。
- Chimera 差异：现有 CoreLifecycle::planned_committed_round 已使用 ref 计划器并核对 Core 实例，但只采样一次 ServiceActor 状态，不比对第二次 Service 读取及 Ready 代次。ApplicationWorkflow::service_host_change_due 使用一组独立 Core/Service 观察；异步 Service 探测期间 Core 或 daemon 所有权可能已经改变，单次 S2 决定无法证明原 Facts 仍有效。
- 本轮统一：在 reconciler::decision 增加纯只读 same_core_status、stable_owner_facts。要求同一 selected Core 的 instance_id、状态、状态变更时间、run_type 一致，同时同一 typed ServiceEvidence 和 ServiceActor ready_generation 不变。S2 跟进前和 S3/S4 已提交目标规划中复用这条规则，在异步读取后再次复核 Core；S3/S4 还保留已有的 active_runtime_receipt Arc 身份核对。
- 异常路径：S2 事实不稳定时不排入 FollowService handoff；S3/S4 则返回 typed OwnershipTransitionPending::waiting，不执行计划中的 Core Stop/Apply。没有建立第二个 Core 控制器、独立 Service 代次缓存或额外持久化来源；所有 host mutation 仍由唯一 CoreLifecycle actor 及原 Service 过渡锁负责。
- 回归测试代码覆盖 Core 实例替换、状态 epoch 变化、Service Ready 代次推进、外部 owner 和不可读 Service；本轮按用户要求未运行测试、编译、Clippy 或 E2E，只运行 Rustfmt、Prettier、git diff --check 并静态审阅。该迁移统一了 S2/S3 Facts 的稳定性门槛，尚不等于替换完整的上游 RuntimeReconciler 状态机。

### DIFF-PLANNED-S3-STOP-TERMINAL-FENCE：S3 未验证 Core 的 Stop 必须取得终态及最新源证明

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/startup.rs::stop_owner 在执行 S3 Step::Stop 后再次刷新 Core 状态；仅在观察到 Stopped 且无未结的外部操作时继续 S4，对 NotStarted 只在真实停止已确认时按幂等处理。完整的目标重建始终来自已提交源，而非旧内存候选。
- Chimera 旧差异：core_lifecycle/workflow.rs::planned_committed_round 在 Step::Stop {intent:false} 下仅调用 core.stop().await?，随后直接把控制权交给旧 runtime reconcile；一个成功的 RPC 不等于 Core 已停止，Core 可能已经换实例，且 Stop 期间可能已有新 Application/Clash/Profiles 文件或配置提交。
- 本轮修改：将 Stop 与后续 S4 的准入隔离为 stop_planned_unverified_core。停止前重新确认完整已提交 Runtime target、当前 Core 状态与计划时相同且为 Running、既有 active receipt 没有跨 host 或跨 revision 切换；停止后拒绝不确定终态，重新确认同一 selected host 的真实 Stopped 状态及第二次稳定观察，才允许继续旧的 S4。Stop 结束后再次确认完整 committed target，过期时返回 typed CommittedTargetAdvanced 由唯一 owner 处理最新源重试。
- 将 ref 的 Stop 终态规则抽入 reconciler::decision::confirmed_planned_stop，并保留已有 startup_stop_error_is_idempotent 的 NotStarted 专项处理：只有已证明 Stopped 才容忍此类并发幂等失败；其他真实 Stop 错误不静默吞掉。
- 现有 CoreLifecycle actor、host selection、Service lease、runtime receipt 与 Checked Apply 没有增加第二写入者。本轮继续保留 ref 对齐过程中的 legacy Core stop 端口，尚未把全部 S3/S4 转成上游 RuntimeReconciler::apply_prepared 的统一执行器。添加终态判断的静态回归用例代码，但按用户要求不运行测试、编译、Clippy 或 E2E；仅执行 Rustfmt、Prettier、git diff --check 与源码审阅。

### DIFF-PRODUCTION-S4-PREPARED-CANDIDATE-AND-SUBMISSION：S4 生产候选准备与 Core 提交迁入 ref apply 边界

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/apply.rs 的 PreparedCandidate 将 PreparedRuntime、Selection、SelectionChange 保存在同一候选里；submit_prepared 只提交已准备的字节，依据 Core 的真实终态区分 Applied、RolledBack、Unknown 等结果，再在同一 Actor 内发布已确认的 RuntimeProduct 和一次性连接中断效果。
- Chimera 旧差异：生产 CandidateCheck::prepare_committed_target 只构建 (PreparedRuntime, CoreSpec)，而 CoreLifecycleWorkflow 从独立的 RuntimeSourceAuthority 再次推断选择和连接中断；submit_prepared_verdict 将 Core 提交和各类终态映射内联在旧 Workflow。reconciler/apply.rs 原先的 ref 形态 PreparedCandidate 和 prepare_candidate 主要服务独立测试，未连接到生产 S4 提交流程。
- 本轮迁移：在 reconciler/apply.rs 增加生产用 prepare_committed_candidate，使用 Core-owned RuntimePreparation 冻结的同一份 RuntimeInputs 生成 PreparedRuntime、Selection、SelectionChange 与 CoreSpec。CandidateCheck 负责捕获最新已提交内容后调用它；旧只返回 runtime/spec 的辅助入口仅保留在测试中。生产 try_reconcile_prepared_selected 消费完整的 PreparedCandidate，并在 Core 操作前核对其选择及中断策略与独立的 bounded source-authority read 完全一致；提交成功后使用本次 PreparedCandidate 的选择与效果，不再从脱离候选的旧字段设置。
- 将生产的 CoreLifecyclePort::reconcile_prepared_submission 调用和 PreparedSubmission 的终态归类移到 reconciler/apply.rs::submit_prepared / settle_prepared_submission，由同一个 CoreLifecycle worker 调用；CoreLifecycleWorkflow 只保存真实 typed ApplyOutcome 给最终 Actor completion fence。保留 AppliedCandidate 的精确产品指针、替换判定、真实 Rollback 和 operation-id-bound Unknown，绝不凭 unobserved legacy adapter 补造已应用证明。
- 迁移边界：没有复制出第二个 Core actor、revision allocator 或端口解析缓存。CoreLifecycleWorkflow 仍持有 S4 的执行序列、RuntimeSourceAuthority 前后检查、Core receipt 身份验证、RuntimeProduct 发布及连接效果完成；上游 apply_prepared 的完整状态拥有权与旧 inline Apply 清理仍是后续迁移。加入冻结 Selection/Mode-interruption 和 Applied/RolledBack/Unknown/Unobserved 终态回归用例代码，未实际运行测试、编译、Clippy 或 E2E，仅执行 Rustfmt、Prettier 与 git diff --check。

### DIFF-S4-APPLY-POSTCOMPLETION-AND-PRODUCT-MAINTENANCE：S4 apply_prepared 的后置效果与发布修复迁入 ref apply

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/apply.rs::apply_prepared 在 Core terminal 后依据 Applied/Refused/Unknown 分支处理 applied selection、RuntimeProduct 发布以及一次性 Mode/Profile connection interruption；startup.rs::maintain 在 Retry 时可修复 pending product，而不重复提交 Core。
- 本地旧差异：Chimera 在 CoreLifecycleWorkflow 内保留 CheckedConnectionEffect 的完整完成/降级实现，CoreLifecycle Actor 额外实现一次独立的 checked_mode_effect_can_finish 和 effect.finish 调用；reconcile_with_trigger 在 S4 外围内联了 RuntimeProduct 的 pending target 检查与 republish 操作。上游 apply.rs 的已迁移提交边界尚未承担这些生产后置职责。
- 本轮对齐：将 CheckedConnectionEffect（含实例绑定旧 API、Core replaced 分流、Mode/Profile interruption 以及 ApiError 到 typed InterruptFailure 降级）移入 reconciler/apply.rs；增加 finish_connection_effect，统一在 Actor 最终检查 Core success、Running receipt、Source mailbox fence 后执行。过期/未确认结果只能生成带 Mode/Profile 分类的不可重试降级，不能错误中断替换后的新 Core。CoreLifecycle 的唯一 Completed mailbox 仍控制调用时点并将结果写入 status。
- 将 explicit Retry 的 pending RuntimeProduct-only repair 迁入 reconciler/apply.rs::retry_pending_product，保留触发器、Core outcome uncertain、最新 committed target 三项准入条件，真实的已确认 receipt 和 derived-file publication 仍由原 LocalRuntimeHost::republish_pending_runtime_product 验证与执行，绝不向 Core 再提交 runtime bytes。SourceCommitted 和普通 Tick 不重放旧 pending product。
- 保留旧 Worker 的 S4 prepared submit 与 final source/receipt 验证，不复制 RuntimeProduct writer 或 Core actor。新增未执行回归用例代码覆盖过期源、未确认 Running、Refused、replaced Core、旧 API Stale 和未知替换结果；原有 explicit_retry_republishes_confirmed_product_without_another_core_apply 用例仍覆盖发布债务重试路径。
- 本轮只执行 Rustfmt、Prettier、静态差异检查；按用户要求不运行测试、编译、Clippy 或 E2E。剩余完整上游 RuntimeReconciler state ownership、Legacy Apply 清理应继续分批迁移。

### DIFF-REF-RUNTIME-PROGRESS-AND-PUBLICATION-STATUS-OWNER：Attempt、applied_selection 与产品发布债务状态收敛

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/workflow.rs 的 RuntimeReconciler 同时持有按目标/host 分类的 attempt、已应用的 Selection、pending_product 和 run intent；由同一 state-machine 调用 plan(Facts) 并在已确认 Core 终态后更新状态。status.rs 的 Conclusion 和 maintenance 独立表达运行时收敛与后置债务。
- Chimera 旧差异：唯一 CoreLifecycleWorkflow 分别使用两个 Arc/Mutex 存储 AttemptPolicy 和 applied_selection；已确认 Core receipt 的双源读取验证也内联在旧 workflow。Actor 又单独持有 product_maintenance 字符串，并在 ReconcilerStatus 中重复保存产品发布失败的降级投影。原 LocalRuntimeHost 已独占真实 Core receipt 和 RuntimeProduct 文件写入，不应再复制发布器。
- 本轮收敛：新增 reconciler/state.rs::RuntimeProgress，作为克隆共享的唯一 AttemptPolicy + applied_selection 内存归属；所有生产 allow/record/record_prepared、Selection 读取、提升和失效均由它执行。将双次 bounded committed source 读取和 active Core receipt 最终核对移入 proven_selection_from_receipt；失去权威源、Core receipt、同一 source target/host/revision/digest 时 fail closed，不恢复已应用选择。不在状态锁内 await，也不增加独立的 Core writer。
- 将产品发布债务字符串从 ApplicationWorkflowState 删除，改由既有 ReconcilerStatus 持有单份 pending_product_target 投影和生成 maintenance/degradation 的只读逻辑。Actor 的 Completed 仍仅从唯一 LocalRuntimeHost 读取 pending product 身份并原子更新 ReconcilerStatus；RuntimeProduct 文件写入、确认 receipt 与显式 Retry 保持在原有唯一端口。该整理不把产品发布失败等价为新的 Core Submit。
- 所有最终 Conclusion/Unknown/Waiting/Converged 判定仍由唯一 CoreLifecycle Actor 的 Completed 消息核验 source 和 Core 后记录；S2 Service lease、Checked inbox、TCC/Legacy Apply、run intent 不在本轮改写。回归用例代码新增单状态的 attempt 与 selection 失效隔离、产品发布债务去重/清除且保持 Converged 的结论等；复用既有 receipt 恢复和 product retry 用例。
- 本轮只执行 Rustfmt、Prettier、静态源码和 git diff --check；按用户要求不运行测试、编译、Clippy 或 E2E。完整 ref RuntimeReconciler Actor 状态机尚未迁移完毕；下一轮继续收敛剩余 legacy Completion / Attempt terminal 记录。

### DIFF-ATTEMPT-TERMINAL-COMPLETED-OWNER：Attempt 终态统一交由 Actor Completed 观察后记录

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/workflow.rs 的 RuntimeReconciler 在同一序列化状态机内处理 Attempt、Core ApplyOutcome 与最终 Conclusion；上游 plan(Facts) 用最近一次 target/host/AttemptOutcome 决定是否阻止相同拒绝目标的普通自动重试。
- Chimera 旧差异：CoreLifecycleWorkflow 在 reconcile_with_trigger 和 Service-to-Local fallback 过程中，只凭 Core 提交返回的 Result<()> 就更新 AttemptPolicy，Actor 的 Completed 消息随后才检查 source mailbox、Profiles/Clash/Application 源目标变化、真实 Running receipt 及 lower-outcome-uncertain。可能提前记录 Applied 或把已被新源覆盖的拒绝保存为最新目标的 refusal；同一轮 Attempt/Conclusion 没有共享最终事实边界。
- 本轮迁移：Reconciler::RuntimeProgress 新增仅包含 target/host 的 PendingAttempt，由已获准的生产 Core worker stage；Service-to-Local fallback 证明 release 后重新 stage Local，取代旧的 Service 请求目标。所有历史终态由唯一 CoreLifecycle Actor 的 Completed 消息调用 settle_observed_attempt，使用最终 Result、源推进、Running receipt、实际 Prepared ApplyOutcome、以及 workflow panic/lower uncertainty 合并后的 final_uncertain 来决定 AttemptOutcome。没有 Core 实际申请的计划 Idle/Waiting、被缓存的 PreviousRefusal 和其他命令都不新增尝试。
- 统一语义：只有成功终态、未被推进的源、无 uncertain 和已证明的 Running/active receipt 才能记录 AttemptOutcome::Applied；实际 Prepared RolledBack 保留其 ref typed refusal，Unknown 保留其 operation-id-bound cause；S2 Service owner wait/源推进/未证实的成功结算为 Waiting 并清除短期拒绝，不污染下一次最新目标重试。现有明确 Retry 仍允许强制覆盖已拒绝的目标。Actor 同时使用 final_uncertain 阻止恢复 applied_selection 或执行旧 Core 连接中断。
- 兼容边界：AttemptPolicy::record / record_prepared 旧直接记录接口只保留测试使用；真实 CoreLifecycle 仍唯一执行 Core，Actor 继续独占 Conclusion、source notice、reconcile scheduling 和一次性 effects 完成；RuntimeProgress 的锁仅保护同步共享状态，不跨 await。PendingAttempt 不复制实际 Core receipt 或 RuntimeProduct writer。
- 新增回归用例代码验证 stage 不等于成功、未证实 Running、源推进覆盖拒绝、已确认 Refused、operation-Unknown、Service-to-Local 最终 Host 和不同命令不继承 pending Attempt。按用户要求不运行测试、编译、Clippy 或 E2E；仅运行 Rustfmt、Prettier 和静态差异检查。完整 ref actor 收敛与旧 Core apply 路径后续继续迁移。

### DIFF-COMPLETED-SINGLE-OBSERVED-VERDICT：Actor Completed 只分类一次并同时结算 Attempt 与 Conclusion

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 RuntimeReconciler 是统一的结果所有者，最终状态和 Attempt 均由同一轮观测到的 ApplyOutcome 推进，而不是两个互不相干的 Result<()> 分类器。
- Chimera 旧差异：Completed 调用 CoreLifecycleWorkflow::settle_observed_attempt 记录 Attempt，再调用 ReconcilerStatus::finish_observed 发布 Conclusion；两个入口都独立使用 observed_apply_verdict 解释相同的 Result、Core uncertainty 和 Prepared ApplyOutcome。即便现阶段的输入相同，增加新类型化终态时也可能出现两个分类器分叉。另外当已经返回 Core 错误、但新的 SourceCommitted 通知已进入 mailbox 时，旧逻辑只在成功 Result 上标记 superseded_in_mailbox，早到的拒绝可能覆盖更新源目标。
- 本轮对齐：由 CoreLifecycle Actor 的 Completed 消息在 source/mailbox 和 final_uncertain 栅栏之后，只调用一次 reconciler::attempt::observed_apply_verdict，生成同一份 ref-shaped LegacyApplyVerdict。此后 RuntimeProgress::settle_with_verdict 与 ReconcilerStatus::finish_with_verdict 只消费此 verdict，保留原 Arc<RuntimeError> 的身份以正确传递 CoreRolledBack、SubmissionUnobserved(operation_id) 等真实终态。Attempt 的 Applied 仍必须另外满足 Running/receipt 验证，Conclusion 的 Converged 仍独立验证 reconciled、Core 和最新源证据；Stop/ServiceRelease 的特殊流程不被通用 Apply 结果覆盖。
- 修正 mailbox 的源推进判断为 superseded_in_mailbox 或 typed CommittedTargetAdvanced，两者任一成立即优先于旧 Core 错误；Actor 按当前源重新规划，旧失败只能得到 Waiting，不会把被覆盖目标的 Refused 记为下一次的自动重试禁止条件。没有新增 Core writer、source actor 或复用已过期的 PreparedRuntime。
- 旧 ReconcilerStatus::finish/finish_observed 与 RuntimeProgress::settle(Result,...) 只保留给原有单元测试，以避免测试辅助接口重新进入生产分类链。新增未执行的共享 verdict 回归用例：同一 CoreRolledBack 引用被 Attempt 和 Conclusion 保存、同一 Unknown 保留 operation ID、源推进覆盖先前 Core 拒绝且允许最新源重新规划。
- 本轮只执行 Rustfmt、Prettier、源码审阅及 git diff --check；按用户要求不执行编译、测试、Clippy 或 E2E。完整上游 RuntimeReconciler Actor 和剩余 legacy Apply/Owner 流程仍需要继续迁移。

### DIFF-EXHAUSTIVE-S3-S4-PLAN-ADMISSION：明确区分 ref Step::Apply、已结算计划与 legacy recovery

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/startup.rs::converge 消费 plan::plan(Facts) 的每一个 Step；只有 Step::Apply 可以准备候选并提交 Core，Step::Stop 需要确认终止后才能继续，Idle/Owner/FallBackToLocal 各自具有独立状态/动作语义。
- Chimera 旧差异：planned_committed_round 将 Step::Apply、无可证明计划、S3 Stop 完成后的继续执行统一编码成 None，且旧 match 的通配分支遗漏 Step::Idle(NotInEffect/Unknown/Inactive/Closed) 和 Stop { intent: true }，可能将已有明确决策错误地解释成旧提交路径的执行许可。
- 本轮迁移：在 reconciler/decision.rs::classify_committed_step 穷尽映射 ref 的所有 Step 到 CommittedStepAction：Apply、Converged、Wait、Refused、Owner、StopForReplacement、FallBackToLocal、Blocked。CoreLifecycleWorkflow 的 planned_committed_round 现在返回显式 CommittedRound::{Apply,LegacyRecovery,Settled}；生产 Profiles 和无 Profiles 两条路径均对三个结果做穷尽式消费，而不是依赖 Option<Result<()>> 的隐式含义。仅在 Core/Facts 无法可靠采样或 ref planner 未产生可证实方案时保留 legacy recovery，不允许具体 Refused/Owner/Waiting 意外进入 Core Apply。
- 对上游 Step::Idle(NotInEffect) 使用 PlannedTargetRefused 保留原 Arc<RuntimeError>，Actor 最终 observed_apply_verdict 直接继承 ref 已确认的 typed cause；源推进和未知 Core 终态优先于旧拒绝。对运行中目标的异常 Stop intent / Inactive / Closed / Unknown plan 返回依赖等待，不创建新 Core writer。
- 上游 Step::Apply 进入生产提交前，再重新验证 startup_target_identity 与 committed target 一致，并阻止 lower Core outcome_uncertain 绕开 actor 的 owner recovery；S3 Stop 完成仍必须经过稳定 Stopped 状态、receipt/owner 与最新源检查才返回 Apply。保留 Service→Local 仅在已证明退役并持有既有 transition lease 的情况下执行。
- 新增未执行回归用例代码覆盖 Step 枚举全分支、同一 source/host 已拒绝目标不能自动重放、显式 Retry 可以重新申请、原始 Refused Arc 身份、源推进与 Unknown 优先级。未运行编译、测试、Clippy 或 E2E；仅执行 rustfmt、Prettier、静态源码与 git diff --check。后续继续逐步迁移 ref 的 RuntimeReconciler::converge 和剩余 legacy apply 兼容逻辑。

### DIFF-CONVERGE-S3-STOP-REPLAN-BEFORE-S4：S3 Stop 后重新采样并再次执行 ref converge 规划

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/startup.rs::converge 在 Step::Stop 被 Core terminal 确认后返回循环顶部，重新收集 owner、Service evidence、Core status、accepted runtime receipt 和目标信息，再调用 plan::plan(Facts)。之前的 Running Core 的 Step::Stop 决策不能直接授权下一步 Apply。
- Chimera 旧差异：planned_committed_round 在 Step::StopForReplacement 后调用 stop_planned_unverified_core，成功后直接返回 CommittedRound::Apply。虽然已确认旧 Core 停止和 source target，仍没有在执行 S4 前按 ref 重新验证最新 S2/S3 Facts。停止期间 Core 可能换 Host、Service 可能重启，或出现新的 active receipt，不能只复用旧计划。
- 本轮迁移：原有唯一 CoreLifecycle worker 的 planned_committed_round 引入最多两轮的 bounded converge。第一轮按既有独立 Service 与 Core 证据做 ref Step 分类；Step::StopForReplacement 调用原 S3 严格终态停止接口后不再返回 Apply，而是重新读取 status、accepted receipt、Service readiness generation 和完整 owner Facts，在第二轮重新调用 plan_committed_target 与 classify_committed_step。只有新计划明确是 Step::Apply 并通过 latest committed source fence，才进入后续 S4；若重规划得到 Owner、Waiting、Refused 等则按相应类型结算，不会误入 Apply。
- 新增 decision::s3_replanned_stopped_owner_ready 纯证明：第二轮要求 Core 仍是选中的 Stopped host、state generation 已初始化、原 accepted receipt 已清除。第二轮的原始 receipt read 如果不可读、async Service 证据探测期间又出现新 receipt、状态不一致或重新规划无结果，均 fail closed 为 OwnershipTransitionPending，绝不回退至无证明的 legacy Core submit。已确认 Stop 后最多重规划一次，避免重复调用 Stop/无限循环。
- 保留 S2 Service transition lease 和 LocalRuntimeHost 的现有唯一所有权，未新增 Core actor、RuntimeProduct writer、revision allocator。尚未把所有 S2 Owner 交接、所有候选 Apply 或旧 compatibility fallback 迁移到完整上游单 Actor converge；本轮只实质性替换 S3 Stop → S4 Apply 的旧短路。补充未运行的单元回归用例代码验证 stale receipt、Running Core、错误 Host、未初始化 status 不会在 S3 后放行。
- 按用户要求不执行编译、测试、Clippy 或 E2E；仅执行 Rustfmt、Prettier、静态代码与 git diff --check。构建与运行正确性仍待正式验证。

### DIFF-REF-S4-STRICT-PREPARED-ADMISSION-AND-RECEIPT-TARGET：明确的 Step::Apply 不允许回退旧 Core Submit

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。上游 client/reconciler/startup.rs::converge 的 Step::Apply 由 prepare_candidate → apply_prepared 构成一次候选创建与 Core submit，失败必须以明确的 Refused/Waiting/Unknown 返回；未证明准备条件不能成为通过其他 Runtime builder 再提交一次的许可。
- Chimera 旧差异：planned_committed_round 已能明确返回 CommittedRound::Apply，但生产 Profiles 分支仍将它与 LegacyRecovery 合并，try_reconcile_prepared_local/service 返回 None（例如无法取得稳定 Core receipt、当前 source authority 无法证明 committed target、缺少已应用 selection 或 gate 未准备好）时，自动调用 reconcile_profiles。这会绕开明确的 ref S4 准入和候选源版本校验。与此同时，decision::plan_committed_target 将 verified Running 的期望 target 当作 applied target，而不是采用 accepted receipt 记录的实际目标，可能在源更新竞态下误报 Converged。
- 本轮迁移：新增 reconciler/apply.rs::route_committed_prepared 区分 ref 显式 Apply 和未形成计划的 legacy 恢复：对 Step::Apply，已准备的候选结果直接成为本轮终态；若没有可证明的 PreparedCandidate，则只返回 typed OwnershipTransitionPending Waiting，绝不通过 reconcile_profiles 提交另一套构建内容。只有 CommittedRound::LegacyRecovery 且 prepared 也不可用时允许旧 fallback；任何已得到的 Prepared Result（包括 Err）均不能重新提交 Core。
- 将 decision::plan_committed_target 的 applied 绑定到 active RuntimeApplyReceipt.target，只有真实 receipt 身份与期望 committed target 一致才可以报告 Converged；不再用目标参数直接制造 applied。Workflow 对 Converged 也在最终出站前重新读取最新 committed target 并检查 lower Core outcome_uncertain，确保本轮源已被更新时不能错误结算。已有 SourceCommitted/Retry 强制语义保持不变。
- 生产 ChimeraClient 初始化会安装独立源权威和唯一 CandidateCheck，因此可在完整 Profiles 场景执行严格 ref S4 admission；无 Profiles 的测试/兼容路径继续使用旧 Core port，当前尚未整体移除 legacy execution。新回归用例代码覆盖 explicit Apply 缺少候选不能 fallback、未完成规划可以 legacy、已返回错误不能二次提交，以及 verified Running receipt 绑定不同目标不等于 Converged。
- 保留 CoreLifecycle 唯一 command mailbox、原有 RuntimeProduct writer、Core revision allocator、SourceActor 发布与最终 Actor Completed 栅栏。本轮仅执行 rustfmt、Prettier、源码差异静态审阅和 git diff --check；按用户约定不运行编译、测试、Clippy 或 E2E，实际构建与运行仍需验证。

### DIFF-REF-S3-UNCONFIRMED-RUNNING：未确认 Running 的 Stop 与不可读 Facts 的 Waiting

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。对照 `client/reconciler/startup.rs::converge` 和 `plan.rs::plan`：有 Running 观测但没有匹配 confirmed receipt 时，Facts 仍为 `Running { verified: false }`，先执行 S2，再执行 S3 Stop，重新采样后才允许 S4；不可读的 Core 不允许 Apply。
- Chimera `reconciler/decision.rs::plan_committed_target` 采用相同 CoreFact 映射，移除将无 receipt 的 Running 降格为 Unreadable 并返回无计划的分支。`core_lifecycle/workflow.rs::planned_committed_round` 在完整 Profiles/CandidateCheck 会话的 SourceCommitted/Retry 路径遇到不可读状态、不可读 receipt 或未初始化 Facts 时直接返回 typed Waiting，不调用旧 builder。未安装 canonical gate 的兼容端口仍保留旧 recovery；其它 trigger 的迁移另行处理。
- `same_active_receipt` 在 S2/S3/S4 采样与 Stop 前同时检查已接受 receipt 的身份和缺失状态；采样期间新 receipt 出现、消失、被替换或不可读均不得使用旧 Facts 停止或提交 Core。保留原有唯一 CoreLifecycle Actor、S2 Service ownership、S3 Stop 后重采样和 source fence；不更改持久化格式、IPC、前端或宿主网络配置。
- 风险 L2；验收为纯规划同 ref、无 receipt 的 S2 优先级及 receipt presence/absence 竞态证明，新增/更新未执行的回归用例。回滚仅撤销本切片，不改配置或 ref pin。按用户约定不运行编译、测试、Clippy 或 E2E；本轮只做源码差异审阅与格式检查，运行正确性待验证。

### DIFF-REF-PROFILE-IMPORT：整块复制 Profile 模型和 URL 导入识别

- ref 基线：4607a7379f0aa060daf55b369dc8103dda929d82。本切片以 `ref/backend/nyanpasu-config/src/profile/` 完整目录与 `ref/backend/nyanpasu-core/src/profile_import.rs` 为直接复制来源；现有同内容文件不制造无意义 diff。复用上游 ProfileImportKind、ImportFacts、ImportSignal、ImportClassification、classify 及全部原始分类用例。
- 将 `ref/backend/nyanpasu-core/src/client/profiles.rs::ProfileUrlProbe/probe` 和 `state/profiles/actor.rs::remote_import_definition` 迁入 `backend/chimera-core/src/profile_import.rs`，因为本地 ProfilesActor 尚位于 Tauri crate。探测直接消费已有 SubscriptionFetcher port，使用相同 remote definition 与 typed content validator；真实的 actor 导入及 refresh 路径也使用该 validator，原 URL 命名策略从桌面 facade 移入共享模块。没有第二套 source owner、下载实现或写入事务。
- 必要适配：nyanpasu_config → chimera_config；Profile 模型测试仍使用 serde_yaml_ng，分类及 typed content validation 使用现有 serde_yaml 以保留 ProfileContentError 的错误类型；现有 strum 0.28.0 仅增加依赖边。保留 NewProfileRequest.folder 的 legacy Specta optional 字段；保留全部原有额外 Proxy Group 测试、folder/interval/validation 断言。新 probe 是 core 库只读 API，不向远程访问或 agent 暴露；当前桌面 IPC、生成 TS bindings、远程 transform 提交入口仍未接入，不宣称 UI 导入流程完整迁移。
- L2：验收目标是共享模型/分类/构造与校验直接采用上游代码，同时保持现有 Config 导入事务、源文档与用户数据兼容。未执行编译、测试、Clippy 或 E2E；只做 rustfmt、静态差异审阅和 git diff --check。回滚仅撤销本 Profile 切片，保留此前 Runtime 工作，不改 ref pin 或持久化数据。

### DIFF-REF-REMOTE-TRANSFORM-IMPORT：Profiles 导入探测及 Transform actor 路径

- ref 基线仍为 4607a7379f0aa060daf55b369dc8103dda929d82。参考上游 `client/profiles.rs::probe/import`、`client/mod.rs::probe_profile_url/import_profile` 和 `state/profiles/actor.rs::ImportRemote/CommitImported/spawn_download`，让同一个源 actor 携带用户确认的 `Option<TransformKind>`，从请求校验、下载校验、pending 状态到最终持久化，复用上一切片的上游 definition builder。
- 桌面 ProfilesClient 与 actor 共享同一 fetcher。Core 的 probe_fetched 提供唯一下载后分类、可接受类型、名称及订阅信息投影，兼容现有 anyhow 下载端口并保留可识别的 SubscriptionFetchError；探测不经过 actor、无文件写入和自动激活。Core 的 typed fetch probe 仍调用同一投影函数。
- 将上游下载/校验/结算逻辑统一为 actor 的 download helper，import 与 refresh 共用；保留 Chimera 既有 import panic supervision、refresh job instrumentation、stale-download fence、持久化补偿与错误类型。Config 仍自动激活（若无当前配置），Transform 不自动激活，且不能进入 Config folder。既有 IPC 签名不变，仍默认 Config；新的 probe_profile_url / import_profile_with_transform 是后端入口，暂未新增 IPC 或 TS bindings。
- L2；直接迁移上游三个 probe 回归用例（兼容现有测试构造器及 mock 错误），补充三种 Transform 规范扩展名、非法 Overlay 无持久化、Transform 禁止加入 Config folder 的用例。未执行测试或编译；格式化和静态 diff 检查。回滚此切片不修改 ref pin，且保留此前未提交的 Runtime/Profile 工作；若已创建 Transform，禁止通过删除用户文件进行回滚。

### DIFF-REF-RUNTIME-RECOVERY：完整迁移已确认运行态证明

- ref 基线仍为 4607a7379f0aa060daf55b369dc8103dda929d82。完整迁移 `ref/backend/nyanpasu-core/src/client/runtime_recovery.rs` 的 ObservedRuntime、matches_confirmed_apply、confirmed_apply_kind 及三组上游回归用例到 Tauri `client/runtime_recovery.rs`，不创建第二个运行时 owner。保留 Chimera 原回执的 config_text、core_spec、artifact、ports、run_intent 与旧事务恢复能力；将 ref receipt.binding.revision 对应到现有 applied_revision，增加 instance_id 和可选 controller_generation（独立于配置 revision generation）。
- Canonical CoreActor projection 使用已观测到的真实 owner generation，连接代数变化时必须证明同一 process instance；host、epoch、revision generation、effective/source hash 与 Running 均必须匹配，保留 Chimera 已发布 applied_kind 不一致的拒绝条件。断线缓存和无状态不得算作证据。直接旧 endpoint 不虚构 router generation；仅无绑定的兼容 receipt 可走旧 revision-only 判断，生产 projection 不使用该退路。
- 将 shared proof 接入 active_runtime_receipt、observe_runtime_baseline、PreparedCoreSettlement/ReconcileReport、runtime_inspection 和 application effect ports；restore 在 routed host 上使用真实 owner token 和 RoutedEndpoint admission，保留旧测试/直接端口的 None 边界。回执记录实际应用确认时的 instance_id 与提交绑定的 owner generation（无 token 的历史路径保持 None，不能冒充同一连接）。不新增 IPC、持久化字段或前端变更。
- 风险 L2：验收目标是连接代数变化且 instance 不同/缺失时拒绝复用，断线缓存、已知 kind 冲突、旧 revision 或 source 不一致均拒绝；保留上游恢复判定测试，补充缓存及兼容端口用例。仅 rustfmt 与 git diff --check、静态审阅，未执行编译、测试、Clippy 或 E2E。未完成：整个 RuntimeReconciler actor/生命周期 workflow 的物理迁移，以及所有旧 reconcile 提交路径的统一 admission。回滚仅撤销此恢复证明切片，不改 ref pin 或先前 Profile 工作。

### DIFF-REF-CONVERGE-TRIGGERS：Start/CoreChanged 共用 S2/S3/S4 决策

- ref 基线 4607a7379f0aa060daf55b369dc8103dda929d82。对齐 `client/reconciler/startup.rs::converge` 的 `confirmed -> CoreFact::Running.verified / Facts.applied` 两个独立事实，以及 `client/reconciler/workflow.rs::execute` 的 Start/Retry 收敛入口、`plan.rs::Trigger::forced` 行为。生产 Profiles + CandidateCheck 会话的 Start/CoreChanged 进入同一个已有 S2/S3/S4 bounded round，不允许缺失 prepared candidate 后退回 inline Core submit；现有 owner lease 由唯一 CoreLifecycle actor 执行，S2 动作保留待交接续轮。
- 真实 live receipt 已由上轮 runtime_recovery proof 验证；S3 仅检查 receipt 的 host/run intent 并在异步采样之后重验同一 Arc，旧目标仍是 verified Running，目标不一致由 planner 决定 Apply 而不是误触发 Stop。Completed 的 exact-current-target 验证不变。已确认的无操作收敛不清空 selection；未知 previous selection 使用 ref 的 Option 语义，不推断连接中断，也不阻塞合法 prepared Apply。selection 恢复要求同一 accepted Arc。
- 仍保留无 CandidateCheck 的旧测试/兼容 session 路由和原来的 stale-target stop 语义；Startup/FollowService 的实际 handoff 与 ServiceOperation 尚未并入同一 convergence actor。L2，验收条件：Start 强制 Apply、CoreChanged 同目标不重复提交、有效旧回执更新不做额外 S3 Stop、无确认回执仍需 Stop、已有外部 Service 命令未结算时不提交。新增 trigger、owner proof 和 planner 回归用例；未执行编译、测试、Clippy 或 E2E。仅格式化、静态审阅及 git diff --check。回滚仅撤销该切片，保留前序 Runtime/Profile 更改和 ref pin。

### DIFF-REF-KV-READ-RECOVERY：更新 ref 后同步 KV 读取修复

- ref 从 4607a7379f0aa060daf55b369dc8103dda929d82 快进至 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769（本次 fetch 的 origin/main）；22 个新增提交、23 个变更文件。此次唯一实际业务源码差异为 `frontend/query/src/provider/kv-storage-provider.tsx`，其余主要是工具链、依赖、工作流、文档及回归用例。
- 将 5d2c0b3ba 与 173436374 的完整 read/ensureRead 改动迁入 `frontend/interface/src/provider/kv-storage-provider.tsx`，保留 Chimera Tauri RPC、写入 revision fence、listener registration retry、local cache 和 resync revision merge。两个上游浏览器回归场景适配现有 node:test harness：旧 store 的未完成读取不阻塞新 store，已完成 key 在 resync 等待时保持 loaded。
- 本轮不盲目覆盖本地依赖清单、锁文件或工具链；尤其保留 rust-toolchain.toml 的已知编译问题 pin，OXC/MLua/Zip 等依赖升级另行验证。风险 L1；验收目标为两处读取逻辑与 ref 一致、原写入与监听语义不变。仅 Prettier、git diff --check 和静态检查，未执行测试或编译。回滚仅恢复旧 ref gitlink 与本切片。

### DIFF-REF-GEO-OWNER：共享 GeoIP 索引与 IO 分类

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。目标是将 country index 文件适配器、actor、client、ports 和测试整体移至非 Tauri 的 `chimera-core/src/geo/`。直接复制 ref `nyanpasu-core/src/geo/adapters.rs` 全文件（包括 watch-home error source 回归用例），将 observer port import 映射至同目录 ports；WatchHome 保留 upstream BoxedCause。原 actor/client/ports 中 Chimera 的 instance_id 绑定、同步 SetMode、失败重试与低频刷新保留，不用 ref 当前未携带实例身份的 observer actor 覆盖。桌面模块只做 re-export，原 traffic caller、mock 和 fixtures 仍共用同一实现。
- 直接复制 `nyanpasu-core/src/corrupt.rs`，复用本地 redb_ext 分类；桌面 `core/corrupt.rs` 只 re-export 共享 error_io 并保留 config_node_exists 与 persistence 兼容入口。无新的数据库删除、目录重建、IPC、网络或持久化策略。Cargo 仅迁移已有依赖边及 test-support feature，保留已锁定的依赖版本。
- L2。验收：GeoIP 文件读取、mmap 缓存、文件监听、错误原因与 ref 一致，原实例隔离和 traffic consumer 调用不变，无重复 owner。回滚：恢复原桌面 geo 文件与 error_io，撤销共享模块和 manifest/lock 依赖边，保留此次 ref pin 与 KV 修复。仅 rustfmt、git diff --check、Cargo metadata 离线解析及静态差异审阅；按约定未执行编译、Clippy 或测试。后续仍需对齐 observers 的完整 lifecycle 与流量存储，不宣称已完成上游 observer actor 的整体替换。

### DIFF-REF-PROFILE-IO：完整提取 Profile 文件与物化事务

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。直接复制 `ref/backend/nyanpasu-profile-io/` 的 Cargo manifest、lib、content_source、materialization、完整 fs_service 与 error_contract_tests 至新共享 crate `backend/chimera-profile-io/`；workspace 与桌面生产 composition 接入新 crate。共享 typed ProfileFsPort / ProfileMaterializationPort 已由 chimera-contracts 提供，桌面现有 anyhow trait 仅机械转发并保留 typed cause，不再维护第二套事务实现。订阅下载、设备标识、代理选择及 legacy YAML normalization 仍由现有适配层处理，未变更 actor 调度、IPC 或 TS bindings。
- 必要兼容：沿用现有锁定的 sha2 0.10 与 serde_yaml 0.9，保持 journal 序列化与错误源类型；新增 from_profiles_dir(PathBuf) 适配原生桌面路径，保留上游 PathResolver 构造器及受检查的 resolve_path。所有原 journal 目录、文件名、hash tags、revision fence、operation ID 和持久化 schema 不变。采用上游 retry_io、typed Snafu、private derived-directory evidence-preserve/rebuild；私有根符号链接行为由旧拒绝切换为上游保留链接本身并重建真实目录，不跟随外部目标。普通 managed parent、symlink leaf 与 journal/stage/backup 的安全拒绝保持。
- L2。验收：生产实际使用唯一 typed owner，完整上游文件事务与错误契约用例均迁入；保留本地独有 read/atomic-write typed cause 及目标不被覆盖断言，旧 private-root 拒绝用例由上游 evidence-preserve 用例替代，其余本地订阅与 normalization 用例仍在桌面，额外增加真实 desktop bridge 类型与事务用例。非目标：迁移 ProfilesActor 自身、HTTP port 或更改用户配置。回滚仅撤销新 crate 和桌面转发，禁止通过删除用户文件或已保留证据回滚。执行 rustfmt、git diff --check、Cargo metadata offline/locked 与 ref 静态映射；按约定未执行编译、Clippy 或测试，仍需独立 review 和实际运行验证。

### DIFF-REF-PROFILE-TRANSFORM：独立配置转换 crate

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。新增 `backend/chimera-profile-transform/`，以 ref `nyanpasu-profile-transform/src/runtime_builder.rs` 的纯构建流程、DNS/Hosts 派生、TUN 派生、变换诊断与最终校验为共享 owner，迁入 chain、utils、test_support、builtin 资源及适用纯构建测试。Core 与 Tauri 生产调用使用同一 RuntimeBuilder，标准 build 签名与 ref 一致，旧显式表注入改为 build_with_builtins，现有 blocking/capture/投影调用不另造 executor。
- 保留 ChimeraClient/Alpha 的 TunFlavor、MMDB 字段、Premium 与不支持 DNS/Hosts 接管的拒绝规则、四个现有 builtin 的顺序及脚本语义；verge_hy_alpn 直接采用 ref 文件，clash_rs_comp 的旧 IPv6/allow-lan 兼容前缀、meta_guard/config_fixer 暂保留。PostProcessingOutput/LogSpan 仅一个定义，保持原 IPC 字段（不引入 ref advice）；保留当前 RuntimeBuildError variants 及 serde_yaml 版本，不假称上游结构化 wire error 已对齐。
- L2。验收：共享 crate 执行完整 profile -> RuntimeArtifact，核心与桌面不重复维护纯 builder 或 builtin，保持现有 receipt 与 staged content fence、strict/lenient policy。保留已有 Core/Tauri 回归用例，迁入 ref 纯构建测试并适配本地 core；上游两项 wire error 用例与 real-engine golden/script fixture 尚待 scripts/typed errors 迁移，原桌面 real-engine 回归仍保留。非目标：更换实际 JS/Lua engine、修改 IPC bindings、发布或应用运行配置。回滚撤销本 crate 与 re-export/调用变更，不回滚独立 profile-io、Geo、KV 或 ref 更新。只执行 rustfmt、git diff --check、Cargo metadata offline/locked 及静态检查，未执行编译、Clippy 或测试；待独立 review。

### DIFF-REF-SCRIPTS：独立脚本执行能力与结构化引擎准入

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。新增 `backend/chimera-scripts/`，对齐 ref scripts 的 crate、adapter、runner、extensions、js 与 lua 模块边界、javascript/lua feature 及 lazy engine admission。将现有 QuickJS、Lua 实现及全部原引擎回归用例整体搬迁；桌面 legacy ScriptType 仅映射到 config ScriptRuntime，原 request UID、mapping、logs、调用入口和 test mock 保持兼容。现有同步 ScriptRunner 实际使用共享 crate，当前线程 runtime 与 blocking worker 生命周期不变。
- ref 使用 Boa/HTTP module loader，Chimera 当前 QuickJS 0.13/OXC 0.152 与 Lua 保持原沙箱及 transform 的 32 MiB 内存、750 ms 墙钟、指令/中断及日志限制；不引入 HTTP/文件 import、Boa 依赖或额外 Lua 权限。现有 item predicate/expression 的 JSON 转换不变，补齐 ref 的 named chunk、禁用/拒绝引擎时 typed StartEngine（先于 config 转换）、feature gate，保留非准入错误分类。迁入 ref adapter 可直接适配的引擎启用/禁用与拒绝回归用例；上游 ordered-map、失败前日志、真实 Boa loader、ScriptDirs/ConsoleSink 完整接口仍有差异，待单独迁移。现有 bounded console 逻辑与用例移动到共享模块。
- L2。验收：生产 runtime builder 与 legacy transform chain 均调用唯一共享 engine owner，默认功能与锁定引擎版本不变，禁用 engine fail closed，原 IPC ScriptType 不改名。非目标：更换引擎、放宽沙箱、引入网络加载器、修改配置格式。回滚撤销 scripts crate 和转发，保留独立 profile-io、profile-transform 及其他先前迁移。仅 rustfmt、git diff --check、Cargo metadata offline/locked 及静态检查；未执行编译、Clippy 或测试，仍需独立 review。

### DIFF-REF-STARTUP-CONVERGENCE：共享 Selection 与 Local 恢复 S3/S4

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将 ref reconciler/committed.rs 的 Selection/SelectionChange/interruption 与 core_lifecycle/workflow.rs 的 effective_host 提取到 chimera-core/client/reconciler；旧 Tauri Selection 只转发共享纯策略，保留其 RuntimeInputs 构造签名。Service daemon release 仍由单一 CoreLifecycle owner 及其既有租约执行，不重复实现。
- 已完成 S2 Local owner handoff、状态已初始化、允许 receipt reuse 的 canonical startup recovery 使用与 SourceCommitted/Retry/Start/CoreChanged 相同的 planned_committed_round(Startup)。真实旧 receipt 的 applied target 与 desired target 分开：相同目标 Converged；不同目标 Apply；未验证 Running 才 Stop 后重新采样。S3 已确认停止立即失效旧 selection；S4 失去 owner/authority 必须 Waiting，不降级到 legacy submit。依赖等待与不确定结果不宣称 owner proven，沿用既有 startup recovery/重试分类。保留 fresh startup、未初始化状态、Elevated、Service handoff 特有的 expected-owner retirement 与 release lease 路径。
- L2。非目标：完整搬迁 RuntimeReconciler mailbox、改变 Service release/ownership、覆盖用户配置、执行构建或测试。验收为共享 selection 和 recovery planner 实际进入生产调用链，现有 receipt、source fence、唯一提交 owner 不变；增加 Startup trigger 的 planner 回归断言。回滚仅撤销本轮共享决策与 Local recovery 接入。执行 rustfmt、git diff --check、静态审阅；未执行编译/Clippy/测试，仍需独立 review 与实际运行验证。

### DIFF-REF-CONVERGENCE-OWNER-EXECUTOR：迁移 S3/S4 执行与 Service 恢复

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将已生产接入的 planned_committed_round / stop_planned_unverified_core / CommittedRound 从旧 core_lifecycle/workflow.rs 移入 ref 对应 reconciler/startup.rs；通过借用原有 CoreLifecyclePort、RuntimeProgress、canonical session 访问器保留唯一 mutation mailbox，不新增状态副本。共享 service_prepared_owner_proven 移到 decision.rs，Startup 与 S4 复用同一判断。
- Local 和 Service 的已初始化 canonical recovery 均复用 Startup trigger 的统一 S3/S4；重新计算最新 Service readiness/failed generation/foreign host 偏好，拒绝已失效的 S2 证明。Service S3 使用 Startup 持有的 transition lease 和现有 expected-owner 条件停止；停止前再次比较 Service generation/evidence、Core identity、receipt、source target；不把 S3 失败解释为 S4 拒绝。Service fallback 不重复获取 lease，保留 ApplicationWorkflow 唯一 handback 分支，增加 fresh stopped owner / generation / target 检查，只有 release 成功才抑制失败 generation。Local fallback 重新 stage 实际 Local host。显式首次启动、状态未初始化、Elevated 和旧 port 继续走原兼容路径。
- L2。验收：实际收敛执行从 legacy workflow 移入 reconciler，原调用链与单 owner 不变；Service recovery 同目标不重复 apply、旧已验证目标可 S4，未验证实例条件 S3 后重采样；等待/不确定结果不得触发 fallback。非目标：全量替换 mailbox、修改 Service 协议、执行构建/测试或发布。回滚仅撤销本轮执行器搬迁与 Service recovery 扩展。执行 rustfmt、git diff --check、静态审阅；未执行编译/Clippy/测试，待独立 review。

### DIFF-REF-CONVERGENCE-APPLY-WORKFLOW：S4 执行与 committed round 调度归位

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将实际 apply_checked_runtime / submit_prepared_verdict / try_reconcile_prepared_selected 及 Local/Service 入口由 core_lifecycle/workflow.rs 移入 reconciler/apply.rs；将 reconcile_with_trigger / committed_run_type / ProfilesSourceAdvanced 移入 reconciler/workflow.rs。保留原 inherent 方法调用及旧错误 re-export，借用唯一 CoreLifecyclePort、RuntimeProgress、CandidateCheck 与 RuntimeSourceAuthority，结果和一次性连接中断仍交原 Actor Completed 最终确认，未另建 actor/receipt/store。ports.rs 原已与 ref 基本同构，RuntimeBuildError 改为直接依赖共享 profile-transform。
- 对齐 ref effective_host：canonical committed round 读取 ServiceActor typed readiness + 当前选中 owner，复用 Startup/FollowService 同一 desired_host；保留 failed-generation 与 foreign-instance 保护，稳定采样再次计算 host，已变化则 Waiting，保留 AdoptService 计划，未知 readiness 不盲选另一 host。非 canonical 兼容路仍用旧 IPC 分类。Checked Apply 捕获旧 API 后再核对 source authority/uncertain，普通 S4 再核对实际 owner、receipt 和完整 target/policy 后提交唯一 candidate；共享 same_active_receipt，原终端分类和产品发布路径不变。
- L2。验收：主要生产收敛入口进入 ref 对应模块，单 owner/租约及原 source fences 不变；补充 typed readiness 与 failed-generation/foreign owner 决策用例。非目标：整套 RuntimeReconciler mailbox 切换、改变 Core IPC、实际运行用户配置、发布。回滚仅撤销本轮搬迁、host 策略与附加 fence，保留先前 startup/共享 crate 迁移。执行 rustfmt、git diff --check、Cargo metadata locked/offline 及静态检查；未执行编译、Clippy 或测试，待独立 review。

### DIFF-REF-CONVERGENCE-ROUND-STATUS：统一生产 Round 与状态模型

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。删除重复 legacy_status.rs / legacy_mutation.rs，真实 actor watch 改用 reconciler/status.rs 与 mutation.rs 的唯一 ReconcilerStatus / RoundRecord；保留本地 mailbox u64 operation ID、SourceVersions 可缺省版本语义、32 条历史及产品发布债务。EngineVersions 与实际 SourceVersions 统一，Closed 历史使用 ref ShuttingDown cause。RoundDegradation 保留原共享 typed reason；产品发布债务仅有 target 证据，不伪造 PublishRuntimeError；桌面 MutationOutcome 保持现有 flat wire projection。ref outcome 函数使用同一状态模型，现有外部 mutation 路径仍不在本轮切换。
- 将真实 Completed 的 Core status/receipt 证明、selection recovery、一次性连接效果与 typed Attempt/Conclusion 结算提取到 reconciler/workflow.rs::Round，删除未接入的 startup Round 占位。保留原有单 mailbox、worker join、source fence、E2E trace stages、最终状态发布和 response waiter 顺序。Round::settle 使用同一个 verdict；status.settled_for 统一 waiter version 条件。维护 debt 不另建 writer 或 receipt owner。
- L2。验收：生产状态和历史不再存在平行类型，Round 实际进入 Actor Completed，原状态观测/确认及 IPC 兼容行为保留；保留并适配原回归用例。非目标：改变 actor 调度/租约、重写所有 mutation DTO 或运行用户配置、执行编译/测试/发布。回滚仅撤销本轮状态/round 合并。执行 rustfmt、git diff --check、Cargo metadata locked/offline 及静态检查；未执行编译、Clippy、测试，待独立 review。

### DIFF-REF-RECONCILER-MAILBOX-STATE：Runtime 消息准入与 Checked 缓存归位

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。生产原 ApplicationWorkflowActor/State/Args 更名 RuntimeReconcilerActor/Mailbox/Args，仍唯一原 actor 实例和 CoreLifecycleClient 兼容 API。将 CheckedRuntimeCandidate/Inbox/Selection 的完整实现迁入 reconciler/checked.rs，保留 32 条上限、domain/request/target 精确匹配、CAS abort、ambiguous fallback 及 executable spec；缓存队列私有。reconciler/workflow.rs 新 RuntimeReconcilerState 实际持有原 RunIntent、dirty/source_dirty/retained handoff、checked、seen_sources 及 service/recovery/interruption maintenance；不复制 mailbox/状态。
- 迁移生产 command admit、checked source notice 分类与过期/重复 receipt 撤销、runtime trigger、source supersession 与 mode-effect debt 策略到 reconciler/workflow.rs；Actor 仅调用原有队列和调度动作，RunIntent 的 Stop latch、只有显式 Start 恢复、queued Checked 再检查版本语义保留。维护聚合复用原 ServiceReleaseDebt，产品债务仍读取实际 receipt owner；保留原 source-only observation 与所有外部消息契约。尚未把整个 mailbox 实现物理移动到 reconciler/mod.rs，未切换 source actor 等待契约。
- L2。验收：状态与准入逻辑实际进入生产路径、Checked queue 不再由旧 lifecycle 文件维护、无第二 owner 或新任务调度；保留现有缓存/actor 回归用例并增加 Stop latch 与单调版本通知用例。非目标：替换 IPC、改变租约顺序、运行配置、提交发布。回滚仅撤销本轮 mailbox state/cache 提取。执行 rustfmt、git diff --check、Cargo metadata locked/offline 及静态检查；未执行编译、Clippy、测试，待独立 review。

### DIFF-REF-RECONCILER-ACTOR-BOUNDARY：生产 mailbox 与 client 整体迁移

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将原 core_lifecycle/mod.rs 的生产 RuntimeReconcilerActor、Message、Request、ActiveOperation、状态初始化、调度、Completed、shutdown、watch、RPC client 与 ChimeraClient forwarding impl 整体移动到 reconciler/actor.rs；原全部 tests 移入 reconciler/tests.rs，仍是 actor 的测试子模块，保留 fixture/私有访问及原用例逻辑。实际 client 更名为 ref RuntimeReconcilerClient，ChimeraClient composition 与 CheckedCandidatePort 直接使用它；core_lifecycle 保留 CoreLifecycleClient alias 及旧 DTO/port exports，IPC 类型及字段不改名。CoreLifecycleWorkflow 的必要跨模块方法调整为 client scope；无第二 actor、线程、队列或订阅。保留原 Service transition lease 顺序、operation id、queue/backpressure、source fences、E2E tracing。
- L2。验收：生产 spawn 唯一 RuntimeReconcilerActor 在 reconciler 下，旧 core_lifecycle/mod.rs 仅保留底层模块及兼容导出，所有历史回归测试保留；当前测试契约命令路径随迁移更新。非目标：复制 ref 新 mailbox 同步调度导致阻塞、取消本地扩展、修改 IPC、运行配置、提交发布。回滚将 actor/tests 原样移回 core_lifecycle 并撤销 alias/composition 改名。执行 rustfmt、git diff --check、Cargo metadata locked/offline 及静态检查；未执行编译、Clippy、测试，待独立 review。

### DIFF-REF-LOWER-LIFECYCLE-BOUNDARY：批量迁移 Service、Ownership、Receipt 与 Source

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。本轮按完整生产职责移动约 1,800 行原 CoreLifecycleWorkflow：core_lifecycle/service.rs 持有 daemon 操作、endpoint recovery 与 evidence probe；ownership.rs 持有选中 host、conditional retirement、quarantine stop 与 failed generation；binary.rs 持有二进制安装与 restart intent；reconciler/proof.rs 持有真实 receipt verification；release.rs 持有 lease-fenced daemon release；startup.rs 持有 Startup S4 入口与 fallback；legacy_source.rs 持有旧 Application CAS/check、frozen input capture、Profile 兼容 reconcile。保留同一 CoreLifecycleWorkflow 实例、原 actor 和 ports，未新增 owner；为实际模块间调用开放 client scope，旧来源 API 仍保留。
- 从 ref core_lifecycle/workflow.rs 直接抽取 ServiceRecovery/RECOVERY_BUDGET/rearm/pause/suppress、service_usable、RestartIntent 与 replace_binary 决策，迁入 chimera-core/client/core_lifecycle，保留 Chimera 的 owner observation、显式 rearm 与原三次预算。唯一 RECOVERY_INTERVAL 共享；Unknown readiness 不伪造成 false，failed generation 仅过滤已知 Ready。Endpoint recovery 保留 CoreError typed cause；binary 使用现有共享 core_kind，接受非 uncertain NotStarted 后仍须执行 ref Recover；生产 adapter 通过 selected RoutedEndpoint 提交并等待 Recovered，拒绝未知 terminal/host change 后写入可执行文件，避免 Stopped 投影掩盖 quarantine。旧注入 mock port 不含 manager epoch，保留兼容默认；实际 router 未接入则生产拒绝安装。保持 Chimera 无 companion 目录的现行打包契约。未改 Service lease 获取顺序和 IPC wire。
- L2。验收：旧 workflow 中上述职责消失且实际 production call path 唯一；共享 recovery budget/host preference/restart decision 接入实际执行；安装前 recover 走同一 router 及 admission ticket；保留旧回归用例并添加纯策略断言。非目标：完整替换 CoreFacade、将所有兼容 source writer 移到新 actor、构建/测试/发布。回滚仅撤销本轮模块拆分和共享 policy/Recover 接线。执行 rustfmt、Prettier、git diff --check、Cargo metadata locked/offline 及静态检查；未编译、未执行 Clippy/测试，待独立 review 和运行验证。

### DIFF-REF-SHARED-RUNTIME-STORE：RuntimeSnapshotStore、Receipt 与端口 owner 整体迁移

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将真实 RuntimeRevision/Allocator、Snapshot/Data、RuntimeApplyReceipt、LifecycleState/Store、RebuildGate、TransformFailure、TransactionSnapshot、inspection view 方法及原 runtime_recovery 完整证明和 SessionPortResolver/CandidatePortBindings/PortsFingerprint/PreparationSession 迁入 chimera-core/client/{runtime,runtime_recovery,ports}.rs；端口 resolver 仍读取唯一 receipt，原测试/fixture 随 resolver 与 recovery 一起迁移，desktop runtime 文件仅保留产品/候选文件路径、持久化、回滚与 wire DTO。原 desktop 类型均 re-export，CoreRunIntent/ScriptType 统一共享类型，state/mutation/runtime_apply.rs 不再定义平行 receipt。clash_info.rs 共享 YAML controller guard/secret projection，desktop IClashTemp 委托同一实现。nanoid 使用现有锁定版本，不升级依赖。
- 对齐 ref watch::Sender 的单状态发布边界：RuntimeSnapshotStore clone 共享 revision allocator、accepted receipt、promoted/applied 与 transform failure；生产 LocalRuntimeHost 直接构造共享 store。原 exact bytes/digest/instance/controller generation/target/ports/rollback receipt 语义保留，pending product 仍要求同一 Arc receipt 与 digest，watch send_if_modified 原子核对后发布。旧 RuntimeLifecycle 仅 alias，失败回滚或 receipt invalidation 不再允许重新标记 Applied。暂不复制 ref config_text-only snapshot、applied_binding / InspectionState 的不同数据模型：Chimera 仍需本地 Mapping、完整 CoreSpec/LocalIpc、原始提交字节与事务恢复契约。
- CoreFacade 普通 routed reconcile 在选中 host 后读取 Connected owner/generation，将原 token 一直传递给 LocalRuntimeHost 的 build/submit/receipt，而非构建后由 RoutedEndpoint 重新选取 generation；TCC prepared Try 同样要求 Connected。保留无 router 的旧注入路径与原 operation registry、lease、TUN、Service IPC，不复制第二 CoreFacade。
- L2。验收：实际 production store/resolver/receipt 类型只有一份，shared owner 不依赖 Tauri；旧测试和兼容 wire 保留，增加 cloned store 与 invalidated receipt 断言；真实 routed reconcile 携带源 generation。非目标：完整替换 desktop CoreFacade、改变 runtime 产品文件格式、升级工具链、编译/测试/发布。回滚仅撤销本轮共享迁移与路由 token 接线。执行 rustfmt、Prettier、git diff --check、Cargo metadata locked/offline 及静态引用检查；未执行编译、Clippy、测试，待独立 review。

### DIFF-REF-CORE-LOG-OBSERVER：日志观察者与持久化整体迁入

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。完整复制 ref nyanpasu-observers/src/logs 与 nyanpasu-core/src/logs（含 preset.zdict、model/ports/actor、codec、redb、原测试和 fixture）、contracts/logs.rs 与 application/core_logs.rs，进行品牌及依赖路径替换。新 chimera-observers crate 先承载 logs；geo/traffic 尚未迁移。保留现有 Redb 4、Specta 和 Zstd 锁定版本，不升级工具链；owner-support drain helper 原行为内联，diagnose 的导出时长/大小常量保留在 logs。导入 ref redact_text 及测试，保留 Chimera 更严格的 config redaction 并扩展 text sensitive keys。
- 实际生产 ChimeraClient 创建独立 logs/core 分片目录，注入唯一 CoreLogsActor；现有 StreamsActor 的 instance-bound、epoch-fenced 日志交付写入同一 observer，复用原 websocket 而非建立第二采集器。按原 recording 开关采集；实例切换清理旧代记录，Clear Logs 同步清理持久化，shutdown 不额外清空并走 tracked drain。存储初始化失败降级为 UnavailableCoreLogStore；日志存储失败不阻断 live UI。旧 websocket snapshot/events 和 stdout Logger 保留为兼容投影与不同日志来源，不改 IPC/生成 bindings。生产目前采用 ref 默认 16 MiB shard/64 MiB budget/Preset，尚未接入 Application core_logs settings 或 frontend 分页查询；CoreLogsClient 的完整 query/detail/export/configure/status 已接入内部 API，不对外宣称 UI 支持。日志仅保存在本地；导出走原有 redaction，不自动上传。
- L2。验收：共享日志模块实际接入生产捕获点，保留 upstream bounded batches/cursors/compression/retention/shutdown 及原测试，旧 main/legacy UI 保持现有契约；非目标：App/Service text log RPC、crash archive、Traffic/TSDB、所有 observer crate 一次性依赖升级、运行用户配置、编译/测试/发布。回滚撤销新 observer/store 与 streams 注入，旧日志显示无需数据迁移。执行 rustfmt、Prettier、git diff --check、Cargo metadata locked/offline 与静态接线检查；未编译、未执行 Clippy/测试，Redb 4 与 ref 5 API 兼容性待编译验证，待独立 review。

### DIFF-REF-TRAFFIC-TSDB：完整上游时序库、流量统计与生产观察者

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769；runtime 62ebdf467f7419b9f8565617979dc984c2c2f186。整体复制 nyanpasu-tsdb（包含 storage/codec/schema/catalog/chunk/query/write/maintenance/recovery、基准、模型检验及回归 fixtures）、nyanpasu-traffic（完整 accounting/key/model/query/topology/store 和 v3 import fixtures）、nyanpasu-observers/src/traffic。品牌/依赖路径替换；Redb 保持 4.4，使用其无参数 compact API；Specta 保持原锁定版本。新增 postcard/proptest 等依赖，第一次离线解析缺少 proptest，联网解析下载后锁定，最终 locked/offline metadata 通过。
- 生产 setup 使用 TrafficFileStorage 指向 traffic/traffic.tsdb，真实 TrafficActor 使用 ref TrafficDb/Session/TrafficFeed，持久化批次、部分提交重试、恢复与查询由唯一 shared actor 负责。旧 v3 写入器和旧 Tauri actor 移除；原桌面 Geo/traffic 回归改为注入新文件工厂，保留实例绑定、模式重试、用户授权的 LocalSourceIps，移至薄 InstanceBoundNormalizer（ref normalizer 增加默认 async 和 disconnect hook）。退出使用 tracked drain。旧 UI 和 agent 继续同一 IPC 方法；上游 ClosedCursor 从 id 改为 incarnation/seq，生成的 TS bindings 尚未重新生成，属于待验证兼容边界，不手工修改。
- 数据保护：沿用 ref v3 importer 的原子导入进度，不删除旧 traffic.redb，不在 TSDB 打开/导入失败时删除任何文件或启动空统计 session；启动失败仅禁用可选流量统计，后续启动可继续导入。导入完成后旧文件仅作回滚保留，不参与双写；回滚到旧版本后新旧库不会自动合并。实际用户数据库未被打开或迁移。原 ref fixture 与 TSDB 测试保留；旧 v3 writer 专属测试由 importer 与上游存储回归替代，不等同于全部旧测试逐项迁移。
- L2。验收：生产统计只使用共享 TrafficDb 和单一 observer，ref 的 TSDB 及导入器整块保留；既有地理实例隔离及数据保护策略保留。非目标：运行真实迁移、编译/测试、手工修改 bindings、更新 ref/工具链、提交发布。回滚撤销代码接线并恢复原 v3 writer；旧文件仍在，但 TSDB 新增统计不会自动回写。验证 Rustfmt（改动的适配文件）、Prettier、Git diff --check、Cargo locked/offline metadata 和静态调用检查；未编译、未执行 Clippy/测试，待独立 review、Redb 4 API/target 检查、v3 导入测试及 bindings regeneration。TSDB 上游仅支持 64 位小端目标，其他目标未验证。

#### 提交前兼容性修复与验证

- TSDB 使用上游 Redb 5.0.0 的完整接口（含 fault backend、文件锁与 compaction）；旧 v3 导入器及其余现有数据库仍使用 Redb 4.4，不整体升级存储依赖。修复 Zstd 压缩界限 API、v3 range 类型参数、Geo hex 依赖、CoreFacade change_core owner generation 与日志目录 PathBuf 接线。
- 已通过整个 workspace 的 Clippy（all-targets/all-features）；使用项目 Specta 生成器重新生成 TypeScript bindings，包含 ClosedCursor 的 incarnation/seq。未运行常规测试套件或真实用户数据库导入。此前各迁移记录中的未编译和待生成状态以本次提交验证为准。

### DIFF-REF-OBSERVER-GEO-LOG-QUERY：共享 Geo 与 Core 日志分页

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。Geo actor/client/ports/fixtures/tests 完整移入 chimera-observers/src/geo，保留 instance_id fence、同步 SetMode、watch fallback、mode retry；Core 只持有 FsCountryIndexSource 及兼容 exports，Snafu selector 改为同型显式构造，不产生第二个 observer。
- 复制 ref Core 日志分页、详情、状态、summary 与 status event 的调用边界，desktop command 使用已有唯一 CoreLogsClient；公开状态/错误固定化，不泄露后端路径与错误消息。Clear 经现有 StreamsActor，同时清空持久化与 legacy snapshot。仅状态元数据广播，完整 payload 按页或详情读取。主 UI 使用从 ref 复制的 bounded merge（500 行/2 MiB）、cursor sync、older/tail、expired recovery、clear epoch、focus resync、detail、scroll anchor 与虚拟列表，适配现有直接 Tauri RPC，legacy useClashLogs/WS 接口仍保留。补齐 ref i18n（俄语使用英文 fallback）。未迁移 file logs/Meow 专属页面、CoreLogSettings 配置持久化及 ref RPC capability framework。
- L2。验收：共享 Geo 单 owner；主日志页面实际从持久化读取而非 WS 内存列表；生成 bindings、原有 UI 兼容入口保留。Rustfmt、全 workspace Clippy all-targets/all-features、Specta 生成、interface/chimera TypeScript 检查与 git diff --check 通过。未执行常规测试套件或真实日志数据库验证；保留旧 WS 事件及历史内存投影，后续可将 StreamsActor 与日志状态/设置效果进一步对齐。回滚仅撤销本轮 commit。

### DIFF-REF-STREAM-RATES-LOG-SETTINGS：上游速率算法与日志存储效果

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。复制 ref clash/connection_rates.rs 完整派生算法（含 counter scratch、member rates、借用序列化和测试）；实际 StreamsActor 直接使用 typed snapshot、真实采样间隔和复用计数器，首次样本速率为零，断开时重置。保留现有 owned connection detail DTO、legacy full JSON/history/event、capture/core instance 双身份与 join/epoch fencing；暂不替换整个 WS wire contract，也不宣称已经消除所有 JSON 序列化。旧纯函数回归保留在测试模块。
- CoreLogSettings 接入 typed Application 持久化、全量及条件提交验证、启动配置及 ref 风格 ApplicationDesired/ApplicationEffect::CoreLogStorage。现有 EffectsActor 负责版本、失败与重试；生产 executor 在启动前绑定同一个 StreamsClient，不另起日志 owner。保存配置后通过 postcommit effect 生效，不进入 runtime rebuild；保留 legacy App 日志字段和原 Clash log level 提交路径。主设置页复制 ref 日志分片大小、容量限制及压缩选择，适配本地 slider/setting API 和五种 locale，bindings 由 Specta 生成。日志设置的 effect 异步收敛，失败通过 effect health/status 报告。
- L2。验收：ref rate derivation 接入实际 stream worker，配置从持久化到同一 CoreLogsClient，保留现有连接和 legacy UI 接口。非目标：全量替换 StreamsActor、App/Service 文件日志及 Diagnose、运行常规测试或实际存储旋转。Rustfmt、workspace Clippy all-targets/all-features、Specta 生成、interface/chimera typecheck、git diff --check 通过。为生成 bindings 清理项目 target 下旧的可重建测试构建产物；未触及用户配置、历史数据库和源码。回滚撤销本次 commit。

### DIFF-REF-STREAMS-NATIVE-CONTRACT：删除旧后端兼容投影

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。完整替换 ref clash/ws.rs、connection_rates.rs、traffic_frames.rs（包含 upstream 测试），生产使用 CoreClient、ClashWsPacket、ClashWsUpdate、ClashConnectionsSummary、ConnectionDetails 与 Arc<ConnectionsSnapshot>。删除旧 ClashWsConnectionSnapshot、ClashWsLog、DetailCounters、JSON 全量历史及二次反序列化、旧 streams source trait；日志直接由 CoreLogsClient 单独保存。保留宿主必须的可选 API instance ID 校验、明确退出停止操作、shutdown drain 与实例绑定 Geo normalizer。复用现有 window-owned native Channel，传递 Arc 而非复制完整 detail frame。ref 的 status 生命周期识别真实 Core 停止；明确启动不再人为重启 socket。旧测试由 ref 测试替换并使用真实 CoreClient endpoint fixture；原 Geo 流量测试适配 typed snapshot。Agent diagnostics 改用摘要和最近一页 error 日志（最多 200 条，非完整日志总计）。
- 前端适配方向：复制 ref sequence reducer，provider 先订阅再同步、检测 gap 并重新获取 snapshot、保留有限事件 buffer；Dashboard/Widget/计数使用 connectionCount，legacy connection table 使用 on-demand detail，legacy logs 使用持久化 query hook，仅 UI 层转换展示字段。移除旧 WS log/history 契约，重新生成 Specta bindings，更新 dashboard fixture 并迁入 ref sequence reducer 测试。当前前端 provider 保留 Chimera 的 context 入口，而非完整移入 ref RPC framework；仍不保留旧后端投影。
- L2。验收：生产后端无旧 WS 全量 JSON/log 数组，按原生 ref wire 接线；前端负责兼容。Rustfmt、全 workspace Clippy all-targets/all-features、Specta 生成、interface/chimera typecheck、diff --check；未运行常规测试套件或真实 GUI/E2E，待 CI。回滚需同时撤销前后端接口变更。

### DIFF-REF-DIAGNOSE-OWNER-SUPPORT：共享诊断与 Owner 生命周期

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。完整迁入 ref nyanpasu-diagnose 和 nyanpasu-owner-support（含设计说明、测试、CPU signature/microcode、GPU host info、report、archive、blocking join、shutdown tracking）。原样复用固定 gpu-info git revision；保留本地 Specta 和 ZIP 8.6 API，现有 ZIP 9.1.3 锁定版本未升级。Archive 的文件名过滤适配真实 clash-chimera.*.app.log；归档报告额外经过同一 redact_text。Core 日志导出大小/年龄常量由 diagnose 唯一持有。
- 生产 WS、AppUpdate、Service、CoreLogs、Traffic 的 actor drain 统一调用 owner-support；CoreLogs 的 blocking store operations、Traffic 的 blocking initialization 使用上游 panic propagation。删除重复 helper，不新增旧接口兼容层。
- 桌面原 EnvInfo/System::new_all 与当日未脱敏 ZIP collector 删除。真实 collect_envs/collect_logs IPC 使用 shared diagnosis steps：Host、Webview、Core，显式归档时再追加 AppLogs 与 CoreLogs。归档通过已有 CoreLogsClient 导出，App 与 Core 分别按 3 天/16 MiB 上限，临时文件原子发布、保留最近五个归档；报告与日志均经过脱敏。前端直接消费 report 字符串；显式收集日志后打开本地归档，不自动上传。诊断报告不采集任意 WebView 环境变量；页面能力当前以默认空值传入。GPU 探测失败不阻止其他步骤。保留旧 issue 入口，但旧 EnvInfo DTO 及前端 formatter 已删除。
- L2。验收：完整 upstream crate 进入生产调用，现有后端重复归档/owner helper 退出路径，重新生成 bindings；Rustfmt、全 workspace Clippy all-targets/all-features、Specta 生成、interface/chimera typecheck、locked/offline metadata、diff --check 通过。未运行常规测试套件、实际用户归档或 GUI E2E；用户需检查脱敏后的诊断文件再分享。App/Service 文件日志分页需要 runtime 子模块新增 logging crate 与 log-query IPC/service endpoints；本轮不改 runtime pin，不实现临时协议。ref 的独立 reporter/crash-child UI 与 PageCapabilities 采集尚未迁入。回滚撤销本轮代码提交，不删除本地已有归档。

### DIFF-REF-FILE-LOGS：Runtime 与桌面文件日志联合迁移

- 主仓基线 96aaab05896ebd862b1950bbd7eab6386c98dd7b，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769，ref runtime 0bf12e53530847d429d6bc98670162626806a41c。先同步 origin/master 及递归 submodules，保留其他设备的 scripts/platform 提交。Runtime main 基线 62ebdf467f7419b9f8565617979dc984c2c2f186，完整复制上游 logging crate（1,647 行 Rust，含测试），迁移 v1/logs/files/open/query/close 服务端路由、IPC client 与 optional log_query_version。使用现有 IPC transport，实际 Service 日志前缀仍为 nyanpasu-service，避免修改既有文件路径。索引采用标准 HashMap/RandomState 代替 gxhash，不引入全局 AES CPU 编译要求；其他索引/游标/session 算法和限制沿用 ref。Service 启动单 owner、退出 shutdown；保留既有 service retrieve/inspect endpoints，因其为现有独立 IPC 消费者，并非桌面 UI 兼容投影。
- 桌面 shared FileLogsClient 使用实际 App 日志目录，owner 由原生 window label 确定，Service owner 由 UUID/client-window namespace 确定，打开前检测协商版本；旧 Service 明确 Unsupported。不另建索引、兼容数据结构或协议。复制 ref summary DTO/ServiceLogsPort/分页逻辑，实际应用启动后单 owner，随 root token 跟踪关闭。前端复制 ref use-file-logs、merge cursor/reducer、文件菜单与虚拟列表/时间/target/文本过滤页面；使用现有 Tauri binding、modal/DateTimeField 和 source selector，Core/App/Service 各自挂载和关闭。生成 Specta bindings。未发布新 Service 二进制：旧安装包仍可能缺少 log-query 能力，需要随后发布并升级 Service。
- L2。Runtime 已签名推送 main e362ff7d564f371d0d7e4a55699374d08b080d9e；主仓更新 gitlink。Rustfmt、Runtime service/ipc/logging Clippy all-targets/all-features、App workspace Clippy all-targets/all-features、Specta 生成、interface/chimera typecheck、diff--check 通过。未运行常规测试、真实 Service IPC/日志读写或 GUI E2E。Runtime 新依赖解析包含必要 transitive lock changes；保持原 release version，实际 Service 发布另行处理。回滚主仓及 Runtime 关联提交，旧 Service 仍可正常连接，日志分页会显示 Unsupported。

### DIFF-REF-SHARED-SESSION-FRONTEND-EVENTS：共享会话状态与前端错误采集

- 基线 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。完整复制 ref frontend_events.rs 的数据类型、bounded sanitizer、fingerprint、tracing sink、report 及 Rust 测试，生产 IPC 按真实 WebviewWindow label 记录事件。日志 target 采用 clash_chimera::frontend，额外在写入前调用现有 redact_text 并移除 route query/fragment；删除 debug-only frontend-console bridge 及 .tmp/ 日志写入。前端整体复制 ref error-reporting 五个文件（归一化、fingerprint、去重限流、batch、React root error hooks），适配现有生成 Tauri IPC；两组纯前端测试迁入 Node test runner，浏览器专用 Vitest 测试未移入，新增 test:error-reporting 入口。
- SessionStateClient 与 SessionStateActor 单 owner 移入 chimera-core/client 与 state，恢复读取器及证据保留测试移入 chimera-migration/session_state；原子写入路径不变。采用 ref snapshot read、任意 WindowLabel 保存、最大化/全屏保留 normal geometry；保留实际多域 CAS/prepared replacement、已有 legacy config mirror transaction 与明确 shutdown mailbox fence，Tauri 只负责 main-window label 和宿主 legacy 配置落盘，未另建 owner。共享 mirror/conditional ports 供现有 Application/Clash 配置事务复用，保留跨 migration/Tauri 集成测试。UI state type 与 Core status/binding 变化判定及测试移入共享 Core；原生通知、tray 与实际 runtime effect 仍在 Tauri，避免重复触发 OS writes。桌面 blocking helper 复用已迁入的 owner-support。
- L2。验收：实际 SessionState/FrontendEvent 路径已接线，旧 debug 文件桥删除，generated bindings 重生成；workspace Clippy all-targets/all-features、Rustfmt、Specta generator、interface/chimera typecheck、diff--check 通过。未运行普通测试套件或 GUI/E2E；尚未全面替换 shared application facade、配置 mirror、独立 Slint crash reporter、生命周期与 host adapters。回滚撤销本轮代码提交，不删除用户会话状态或日志。

### DIFF-REF-SHARED-CONFIG-OWNERS：Application、ClashConfig、Profiles

- 基线 master 7bbcf7af230818b189a1df0676303e5178ae0453，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移实际生产 ApplicationActor/ApplicationClient、ClashConfigActor/ClashConfigClient、ProfilesActor/ProfilesClient、Profiles jobs/scheduler/ports 到 chimera-core/client 与 state；约 6,844 行 Rust（含迁移原有测试）。桌面 Application 与 Profiles 直接使用 shared client，Clash 只在桌面持有 runtime orchestration wrapper，内部唯一 shared client，无独立 state/mailbox。保留实际旧配置落盘/事务 port、CAS 版本、现有 typed 错误及故障恢复，不新建旧前端数据契约。
- 采用 ref 的 source snapshot 读取，Application/ClashConfig 的读不经过 mailbox。共享 source impact/target identity、ProfileContentLock/Reader、RuntimeCandidate/RuntimeGate/DomainChange，复用 runtime core-manager payload_digest，Profiles 的实际 checked-write/文件 journal/补偿/发布通知保持原顺序。Profiles source owner 通过 ProfileMutationPort 使用实际桌面 coordinator，移除对旧 workflow hints 的直接依赖，保留 requested runtime、touched paths、staged content 三项真正影响 source admission 的数据；共享 profile_inputs_changed 复用原 closure comparison。JobsClient、profile sync status/history/log 查询随 source client 迁入，保留 E2E tracing feature 传递。Profiles startup 检查存在但不可读的文件节点，不再使用 Path::exists 的静默 fallback。
- L2。验收：三域唯一实际 source owner 在 shared Core；桌面不再独立实现其 Actor；原有测试保留并适配 shared test fixtures，RuntimeGate 仍由真实 Core candidate checker 提供。Rustfmt、workspace Clippy all-targets/all-features、generated bindings 重新生成及提交检查。未执行常规测试套件或实际用户配置/文件迁移验证。当前不是 ref 三域文件逐字复制：Application/Clash 仍由桌面 workflow 执行预检查和 commit notification，shared source owners 保留本地 legacy mirror/CAS、Profiles legacy anyhow ports 与 typed gate adapter，尚待完整上游 MutationCoordinator、CommitNotifications、Effects 和 shutdown admission 生命周期联合迁移。回滚仅撤销本轮代码提交，不改用户配置。

### DIFF-REF-EFFECTS-MUTATION-CONNECTION：共享 Effects 与生产 MutationCoordinator

- 基线 73efaafe152cbad1408b62effc0c0ce869d5be26，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。迁移实际 EffectsClient/Actor、ApplicationEffectPlan/Fields、EffectStatus、ApplicationEffectsPort、CommitNotifications 到 chimera-core/client/effects，桌面不再持有独立实现。采用 ref 的单次最近 retry deadline timer（过期 Tick 不能清除替代 timer），按 ref 将 CoreLogStorage 与 TransparentProxy 归入 Core group；保留显式 Agent reconcile 等待、错误健康状态、全部活跃任务 drain 与 panic 处理、退出时 hotkeys 释放。迁入 ref From<&Config> domain slice 投影，保留 Chimera 现有字段与类型。
- 采用 ref watch Connection Pending/Ready/Isolated 的生产 MutationCoordinator；实际 Profile source 提交通知由共享 owner 发出，旧桌面 ApplicationWorkflow connection 及 participant/outcome fixtures 仅在 cfg(test) 保留。保留 source-only version observation（不触发运行时 apply），由实际 CheckedCandidatePort/ReconcilerCheckedInbox 传递；不伪造已检查的候选。App/Clash 仍有独立的旧事务入口，尚未直接在 shared Actor 中采用同一 coordinator 完成所有 check/commit/effect steps。
- 按 ref CoreRuntimeEffects 分层，真实 Core 日志 storage 与透明捕获执行迁入共享 executor，使用实际 StreamsClient/CoreClient 及已有透明捕获状态/owner generation fence，保留未观测 OS 提交的 retryability；存储失败采用 ref non-retryable 策略。删除 bind_streams 二次注入接口，Tauri executor 只承担宿主侧效果（旧直接 Core executor 回归 fixture 为 cfg(test)）。Logger/system proxy/hotkey/tray 等宿主 executor 及退出顺序尚未完全替换为 ref。
- L2。验收：共享 actor/plan/ports/coordinator/executor 接入生产路径，Rustfmt、workspace Clippy all-targets/all-features、diff--check；新增 stale retry callback regression 并保留原有 tests，未执行普通测试套件/GUI E2E。未主动变更 IPC wire shape，未重新生成 bindings。回滚撤销本轮代码，不改用户配置。

### DIFF-REF-NATIVE-SOURCE-COMMIT：Application 与 Clash 原生提交路径

- 基线 master b8693b1a3a0cde29bdaa43a2ff7bcd197bfa779a，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。实际 Application/Clash source clients 和 actor 接入与 Profiles 同一生产 MutationCoordinator；按 ref 采用检查、持久化 CAS、提交后的 effect/runtime 通知顺序。共享 CommitNative settlement：只对持久化成功的精确版本发送通知，source-only 通知不产生未检查 runtime apply，shutdown 拒绝未开始的 native commit，配置版本冲突不写盘。Application settings watch 随已提交版本更新，显式 requested owners 进入 effects actor；不再在 facade 重复发布原生提交的 effects/UI 通知。
- 本地 checked admission 必须携带 exact request receipt（包括 Clash mode interruption）；共享 CheckedReceipt 在已获得 ACK 后由 RAII 持有，未提交即 drop/abort，成功只 settle 一次。保留两套 engine Version 的显式转换。与 ref 的差异：检查放在 shared source client、CAS 与通知放在串行 source actor；旧 RuntimeReconciler 的内核/Service 切换命令仍可能 await source actor，若 actor 内反向 await RuntimeGate 会有 mailbox 循环等待。此轮不引入第二 source owner 或前端兼容 DTO。检查 ACK 前的取消/不确定状态仍遵循现有 gate 协议。
- 实际路由：普通非 Runtime Application patch、既有本地 enhancement checked patch；Clash 文档型 patch、既有 DNS/Hosts 与 builder flags checked patch、允许 checked 的 raw/override 字段（保留 mode intent）。保留当前授权、TUN/Service/端口/内核切换及明确等待运行中 Core apply 的调用路径；这些真实宿主工作尚未完成 ref lifecycle cutover。Legacy mirror 仍为实际未迁移的宿主调用者所需，不能宣称已删除。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check 通过；新增 exact receipt drop/settle regression，保留旧测试，未运行普通测试套件、真实 runtime/GUI E2E；未主动改变 IPC wire DTO。回滚撤销代码提交，不改用户配置。

### DIFF-REF-SHARED-RECONCILER-PREPARATION：候选准备、源证明与轮次结算

- 基线 9fab78ffe188668b0bc69e22a2b4f5eaaa437f28，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移实际 RuntimeInputs、RuntimePreparation、RuntimeBuildPort、RuntimeValidatorPort、LiveSourceAuthority、CheckedRuntimeInbox、AttemptPolicy、ApplyOutcome、RoundRecord、ReconcilerStatus 及相关测试到 chimera-core/client/reconciler；上游 core_lifecycle/ports 的候选与 publication boundary 同步移入共享层，保留本地实际 Core terminal receipt 与 exact product identity 校验。SourceDomain 直接使用 ref ConfigDomain，Selection 删除桌面 wrapper，SourceVersions 共享；MutationOutcome 移入 shared mutation_outcome，保持原有序列化字段。实际生产 RuntimeReconciler 复用这些类型，不复制状态或引入新 actor。
- 本轮是 ref 模块边界对齐，不是全部逐字替换。保留已实现的 exact checked request、跨域版本 fence、冻结 managed file closure、typed refusal/unknown 和 publication debt。宿主 CoreLifecyclePort、真实命令、Service transition、权限处理、legacy source transaction 仍在 Tauri；完整上游 actor/executor 和 legacy config mirror 删除尚未完成。现有桌面测试使用 shared test-support，无新增前端兼容协议。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；未运行常规测试或真实 Core/Service/E2E，未主动改变 IPC wire schema，未重新生成 bindings。回滚撤销本轮代码提交，不改用户配置。

### DIFF-REF-SHARED-S4-EXECUTION：共享运行时 Apply 与已应用选择

- 基线 00a8ab69954f10ce0b0f650693ca9750c761650c，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移 ref-shaped reconciler/apply 的实际 PreparedCandidate 构建、checked reuse、S4 submit/settlement、pending product retry、typed connection effect 与 runtime progress/selection recovery 到 chimera-core；core_lifecycle/apply 的 upstream pre-apply API capture/finish 同步迁移，保留原有测试。实际 checked Apply 在 shared apply_checked 执行源身份验证、捕获实例绑定 API、最后提交前 fence、exact Core terminal 与提交后 source verification；原唯一 actor 仍负责记录观察结果、最终轮次结算、selection promotion/invalidation 和 one-shot effect。共享 RuntimeApplyPort 使用实际现有 CoreLifecyclePort 实现，不新增执行 owner；API 从同一个 instance-bound lease 取得 shared client，直接调用 shared interrupt_connections(All)。
- 本轮对齐执行边界但不声称整个 ref lifecycle 逐字复制。保留精确 checked request 回执、publication debt、host generation fence 和本地实际 Core/Service control transport；剩余桌面 S2/S3 owner、startup executor、legacy transaction、CoreLifecycle actor 本体尚需联合迁移。无新增前端兼容 DTO。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check 通过；未运行常规测试或真实 Core/Service/GUI E2E，未改变 wire schema。回滚撤销本轮代码提交，不改用户配置。清理仅限可重建的旧 target 产物与增量缓存，不涉及用户数据。

### DIFF-REF-SHARED-S23-OWNERSHIP：共享 Service 证据与运行时所有权证明

- 基线 f0ad715326bf56b4a5bf62fdb3f063f47c217fe2，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移实际 S2/S3 Facts assembly/decision、ServiceEvidence、release debt、startup report/trigger、receipt proof 与相关测试到 chimera-core/client/reconciler。保留本地 quarantine、foreign daemon、readiness generation、exact CoreOwner token 与相同实例/receipt fence；ref planner 继续接收保守的证据投影。RunType 与 CoreStatusSnapshot 单一共享类型，classify 接收显式连接布尔值，移除 RunType 隐式 Default；仍需要旧配置的宿主调用点显式读取原配置。
- 共享 RuntimeObservationPort 承接实际 CoreLifecyclePort 的同一实例绑定观察；verified_applied_runtime、recovery_receipt_matches 和 startup_service_refusal_stopped 在共享模块执行，恢复回执验证新增最终 committed target 重读。未新增实际 Core owner 或前端兼容契约；保留原有旧宿主 transport、S2/S3 stop/handoff executor、Service transition lease 与 startup workflow 编排。此轮不是 ref 完整 actor 的逐字替换。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；迁移保留原有测试，未运行常规测试套件、真实 Core/Service 或 GUI E2E。RunType 迁移维持原 serde 形状，未主动变更 IPC DTO，未重新生成 bindings。回滚撤销本轮提交，不改用户配置。

### DIFF-REF-SHARED-OWNER-EXECUTION：Service 恢复与 S3 退休执行

- 基线 0e6f7889（先同步远程退出流程、root shutdown token 与 Windows tray E2E 提交），ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将实际 Local Core stop/baseline confirmation、conditional Service retirement、quarantine recovery、bounded endpoint recovery、S3 Stop 与独立 Service daemon release executor 迁入 chimera-core/client/core_lifecycle/{ownership,service} 与 reconciler/release。复用已有唯一 lifecycle actor 和 ServiceRecovery；共享 ServiceTransitionLease / RuntimeOwnershipPort 连接真实宿主 Core control，未创建新 owner。维持先观察、条件停止、再证明的 ref 所有权边界，保留 exact receipt、target identity、ready generation、非阻塞 Service lease 与 shutdown token。
- 安全差异：需要 Ready stopped owner 的退休确认同时要求 runtime_owned=true，避免将其他实例 daemon 的 stopped 状态作为本实例接管证明。Local stopped baseline 仍由真实 Core port 提供；历史 timestamp-zero 分支与 legacy S3 确认条件未改变。宿主显式命令、完整 Startup/Reconciler Actor 和旧配置事务尚未全部逐字替换。保留当前真实宿主桥接与错误分类，不新增前端兼容 DTO。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；未运行常规测试、实际 Core/Service 或 GUI E2E。未主动改变 wire schema。回滚撤销本轮提交，不改用户配置。

### DIFF-REF-SHARED-CANDIDATE-MAILBOX：共享候选检查与 Reconciler 状态

- 基线 8fbcabe01eae1d239f1977033b898b4086f671bf，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移实际 CandidateCheck、CheckedCandidatePort、严格候选构建/校验、精确请求回执及测试至 chimera-core/client/reconciler/check.rs；保留生产 Core-owned revision/port session 和 ACK 后 source CAS。消除旧 Profile workflow 重复 RuntimeCheckOutcome/Unavailable 定义，使用共享结果及 contracts 类型，CheckRecord/RuntimeCheckFailure/TryCauseKind 随之共享。保留 stopped TCC 对 unsupported check 的拒绝与 retry 分类，不将未运行的检查记作通过。
- 实际 Reconciler mailbox 持有 shared RuntimeReconcilerState，统一 source version notice、checked admission、run intent、dirty 标记、maintenance debt、source advanced 分类与 effective host。宿主命令映射只传递准入意图及精确版本，未新增 actor 或前端兼容协议；本轮不声称完整 ref Actor 逐字替换，旧宿主命令集、完整 round dispatcher 与 legacy transaction 仍待迁移。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；保留并迁移原有测试，未运行常规测试或真实 Core/Service/GUI E2E。无主动 IPC wire 改动。回滚撤销本轮代码，不改用户配置。

### DIFF-REF-SHARED-ROUND-RETIRE-TCC：共享轮次结算与退休旧 Profile 事务入口

- 基线 f243240afaae2227432c808d6c7122e56835b019，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将实际 Completed round 的运行回执验证、应用选择恢复、一次性连接中断、attempt/Conclusion 原子结算迁入 chimera-core/client/reconciler/round.rs；原唯一 Actor 直接使用共享 RuntimeObservationPort、RuntimeProgress 与 source authority。保持 worker join、source-version fence、E2E tracing 与已有状态发布顺序。
- 原 ProfileMutation、ConnectApplicationWorkflow、动态事务接线、ApplicationWorkflowClient/participant 与 TCC prepare/cancel 入口仅保留在 cfg(test)，正式构建不再接受该旧事务。生产 Profiles 继续走共享 source owner 和 RuntimeGate；旧启动恢复依赖的 TCC recovery 辅助逻辑仍在，未删除现有恢复路径。重试错误分类进入 shared check_record。未新增前端兼容 DTO 或第二 actor。完整 ref Actor、CoreLifecycleWorkflow、legacy config mirror 仍待迁移。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；保留旧回归测试，未运行常规测试或真实 Core/Service/GUI E2E。未主动改变 wire schema。回滚撤销本轮代码，不改用户配置。

### DIFF-REF-NATIVE-REESTABLISH：启动重试与原生恢复

- 基线 e7e3818cd0b712371d381eefea06e19b53255799，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将实际 committed target、retry budget、LiveAttempt 与 startup observation/report 迁入 chimera-core/client/reconciler/reestablish.rs；统一 StartupOutcome 为现有 ref 共享类型（保留本地 Unsettled），ConfigDomain 使用 contracts。启动重试实际调度使用共享 update_reestablish_target；保留 committed identity 变化重置预算、dependency wait、下层 owner 不确定时禁止清除 attempt。报告仍使用本地 operation sequence/evidence，尚未完全统一为 ref report。
- 正式构建的 recover_after_reconcile 只清除已结算的 native lifecycle/reestablish attempt，拒绝旧 Profile source-decision/rollback evidence；旧 TCC replay、confirm、restore 与 source decision 恢复逻辑仅在 cfg(test) 编译，保留原有回归测试。生产恢复不再重放旧 Profile 事务。完整上游 Actor、宿主 lifecycle 命令、旧配置镜像与尚存的旧 Profile runtime 端口仍待迁移。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；未运行常规测试、真实 Core/Service/GUI E2E。未主动改变 wire schema。回滚撤销本轮代码，不改用户配置。

### DIFF-REF-SHARED-STARTUP-POLICY：共享启动策略

- 基线 05fcd9bfaebfe228dfe05363b3983eca98bc9b58，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。在独立 ref-alignment/runtime-convergence 分支迁移，不直接修改 master。将生产启动失败分类、Service fallback refusal、S2 owner plan/followup、RuntimeEvidence 与 readiness owner signal 迁入共享 Core startup_policy.rs，直接复用已有 ref planner、receipt host proof 及 startup outcome。原启动决策和 retry fixtures 随代码迁移；旧 StartupRetryTarget 仅为 test-support。保留单一 lifecycle owner，不增加前端兼容 DTO。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check；未运行常规测试及真实 Core/Service/GUI E2E。完整 startup executor、Actor 与 legacy config mirror 尚未逐字替换。回滚撤销本轮提交，不改用户配置。

### DIFF-REF-SHARED-STARTUP-EXECUTOR：共享启动执行器

- 分支 ref-alignment/runtime-convergence，基线 252068b684941c2fa50a79b3bd3d7df2f93fe79f，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。完整迁移已有 S2/S3 startup executor 与 readiness 双向复核至 chimera-core/client/reconciler/startup_executor.rs，单一生产 reconciler worker 通过 StartupHost 连接真实宿主操作。直接复用共享 ownership/proof 函数，移除旧桌面重复转发。保留原有 owner 证据、generation/target fence、Service fallback 正向拒绝条件、root shutdown 生命周期和报告语义。
- 本轮迁移是 ref 执行边界对齐，不是逐字复制完整 ref Actor。宿主仍负责实际 Service lease、host selection、Core apply 与 startup retry attempt；StartupHost 是目前的临时宿主边界，后续完整 lifecycle composition 迁移时应收敛，不增加前端兼容协议。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check 与正常签名提交钩子；未运行常规测试、真实 Core/Service 或 GUI E2E，未主动修改 IPC schema。仅推送迁移分支；回滚撤销本轮代码，不改用户配置。

### DIFF-REF-SHARED-REESTABLISH-TRANSITIONS：共享启动尝试状态转换

- 分支 ref-alignment/runtime-convergence，基线 fecfcbfda2e79e82ef26639dee4a95c3b0d34f10，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。将实际 begin/update/conclude attempt 与 committed source change retry re-arm 迁入已有 chimera-core/client/reconciler/reestablish.rs；沿用同一 live/deferred 状态，保持原有 retry identity、budget 与下层 owner 不确定性处理。宿主只提供当前源身份观察、下层状态和恢复信息发布，未增加第二 owner 或前端兼容协议。
- 此为执行状态边界迁移，非完整 ref Actor 逐字替换。保留既有未结算尝试、旧测试事务以及原有 StartupOutcome 行为；完整 native actor 与宿主 lifecycle composition 仍待对齐。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check 与签名提交钩子；未运行常规测试或真实 Core/Service/GUI E2E。未主动修改 IPC schema。仅推送迁移分支。

### DIFF-REF-UNIFIED-LIFECYCLE-PORTS：统一实际 Core/Service 生命周期端口

- 分支 ref-alignment/runtime-convergence，基线 4c25f4024f1800306c7ebd66f69c9701d9f483fa，ref d95b44c05516a3e2e4eb24e2285a7eeaa70e1769。整体迁移现有 CoreLifecyclePort、ServiceLifecyclePort、RunningConfigPort、PreparedCoreBinary、BinaryInstallProgress、RuntimeTransformDiagnostics 至 chimera-core/client/core_lifecycle/ports.rs。删除之前迁移中引入的 RuntimeApplyPort/RuntimeObservationPort/RuntimeOwnershipPort 三个过渡端口及其桌面转发实现；实际 S2/S3/S4、startup executor、round、release 与 proof 直接调用原唯一 lifecycle capability。Core API 直接使用 shared instance-bound client，仅 Tauri 边界保留现有 API wrapper。Service instance ownership 和 effective ClashInfo 由真实宿主/现有测试 fixture 显式提供，不在共享层读取桌面全局配置。保持原有 test adapter 行为、runtime receipt / source identity / Service generation fence 与 serialized owner。
- 本轮移除跨 crate 依赖障碍，不声称完整 ref Actor 已逐字替换。原 RuntimeReconciler mailbox、CoreLifecycleWorkflow、旧 config transaction 与宿主 adapters 仍存在。保留现有 shared wire 字段及旧测试，无新增前端兼容 DTO。
- L2。Rustfmt、workspace Clippy all-targets/all-features offline、diff--check 与正常签名提交钩子。未运行常规测试、真实 Core/Service 或 GUI E2E；未主动修改 IPC schema，未重新生成 bindings。仅推送迁移分支；回滚撤销本轮代码提交，不改用户配置。

### DIFF-REF-SHARED-RUNTIME-BUILD-VALIDATION：共享实际候选构建与检查

- 分支 ref-alignment/runtime-convergence-next-20261124，主仓基线 5b4e6a88798e79631fe96a6a8d07371c391dfd7c。ref 从 d95b44c05516a3e2e4eb24e2285a7eeaa70e1769 更新至 375af2770f7928395c9db18998c17e3880d432ea（SSH main，新增 6 个提交）；此次涉及的 upstream reconciler/runtime_build/runtime 源文件未改变，递归子模块未变。
- 对齐 ref client/reconciler/adapters.rs、profiles.rs、client/runtime_build.rs、artifact_snapshot.rs、runtime.rs：迁移生产 CoreCheckValidator、RuntimeBuildService、map_profile_degradation 与 write_product 到 chimera-core 对应路径，CoreCheckValidator 直接持有 upstream initial EndpointHandle，不再保留 CoreClient.check_config 桌面转发。保留 E2E tracing 和两种 check verdict 的现有测试。共享候选构建使用 ref 的 blocking::join，runtime_snapshot_data_from_artifact 统一封装 artifact projection；保留 Chimera 当前 Mapping 和一次序列化的精确字节、inspection graph 验证与现有原子写错误契约。Tauri composition 仅注入 Core spec resolver、script runner、product path 和同一 Core-owned session。原 enhance 执行器明确重命名为 RuntimeBuildExecutor 并保留既有调用，未新增兼容层。
- L2。macOS 完整 workspace all-features/all-targets offline cargo check、runtime build 13/13、CoreCheckValidator 2/2、rustfmt、diff--check。未重跑全 workspace 测试、真实 Core/Service、GUI E2E；未修改 IPC schema。现有 legacy enhance executor 与 RuntimeSnapshot Mapping 差异、Tauri RuntimeReconciler Actor 和 lifecycle composition 仍待迁移，非完整 ref 对齐。回滚撤销本切片提交，不改用户配置。

### DIFF-REF-SHARED-RUNTIME-PATHS：共享 Runtime 文件生命周期与目录恢复

- 分支 ref-alignment/runtime-convergence-next-20261124，基线 4ff4ab077986baf60354e04084be238032653fdd，ref 375af2770f7928395c9db18998c17e3880d432ea。将实际 RuntimePaths、candidate allocation/hash verification/promotion、transaction capture/restore 与现有回归测试迁入 chimera-core/client/runtime/paths.rs，由 shared runtime 导出；Tauri setup 仅传入 config root，RuntimeBuildService 与 LocalRuntime 使用同一 RuntimePaths 契约。Tauri runtime 模块仅余共享导出与宿主 IPC。
- 直接复制 ref client/runtime.rs 的 cleanup_candidates、typed Error、prepare_private_dir_with、reparse 判断和 persistence 重试/保留证据逻辑，品牌映射 nyanpasu_persistence/owner_support → chimera；异常候选目录节点先保留到 sibling .corrupt-*，再重建私有目录；失败保留 fatal，不写入链接目标。清理单个被占用的候选文件失败只记录并继续。移入 ref 的异常文件、符号链接、重试次数、Windows sharing violation 回归；保留现有 exclusive create、精确字节/哈希、篡改拒绝及回滚测试。
- L2。macOS shared runtime filter 43/43 通过，workspace all-features/all-targets offline cargo check、Rustfmt、diff--check；Windows sharing violation test 未在 macOS 执行，未运行全 workspace 测试或真实 Core/Service/GUI E2E。保留 Chimera 的本地 candidate file、原有 rollback 及 RuntimeSnapshot Mapping/receipt 模型，不主动更改 IPC。回滚撤销本切片，不删除用户文件或 evidence。

### DIFF-REF-SINGLE-RUNTIME-BUILDER：移除独立 RuntimeBuildExecutor

- 分支 ref-alignment/runtime-convergence-next-20261124，基线 f0b25dd029e5a9180c45c213c12924f7b66b27ca，ref 375af2770f7928395c9db18998c17e3880d432ea。删除第二套 RuntimeBuildExecutor 及其 anyhow ScriptRunnerFactory；生产 typed-source enhance 路径直接调用 shared RuntimeBuildService::build_from_source，与 ref-style RuntimeBuildPort::build 共用 builtin selection、RuntimeBuilder、strict transform validation 和 artifact projection。host 注入同一 ScriptRunnerFactory，捕获活动 Profile 文件和 staged bytes 后在同一 blocking worker 创建/销毁脚本运行器；采用 owner-support blocking::join。保留 typed error 传播和原有不提前发布产品的边界。
- 原 executor regression suite 迁入 cfg(test) build_tests；自定义 builtin table 仅用于 fixture，生产两入口均由实际 app/core 决定 builtin table。原 Tauri staged-content 和 source deletion fixture 直接调用实际 shared service，不保留旧 builder 类型。旧 Config::generate_runtime_output_from_profiles 仍消费 ArtifactSnapshotData，未移除其生产调用，下一步收敛 LocalRuntime prepared-only admission 与 RuntimeSnapshot 精确字节模型；不新增 IPC 或兼容 DTO。
- L2。macOS workspace all-features/all-targets offline cargo check、shared runtime_build filter 13/13、rustfmt、diff--check。未运行完整 workspace、真实 Core/Service、GUI E2E；Tauri staged-content fixture 已参与 all-targets 编译但未单独运行。回滚撤销本切片，不修改用户文件。

### DIFF-REF-RUNTIME-SNAPSHOT-PRODUCT：统一实际快照构建与产品序列化

- 分支 ref-alignment/runtime-convergence-next-20261124，基线 0b00b32167cb0cdb327f038c9453fcb68864bb59，ref 375af2770f7928395c9db18998c17e3880d432ea。对齐 ref runtime_build/artifact_snapshot/runtime 的共享构建边界：RuntimeBuildService 的冻结源入口和 RuntimeBuildPort::build 统一调用 build_snapshot，返回同一 RuntimeSnapshot。LocalRuntime 的遗留 typed Profile 检查与 reconcile 直接消费该快照的精确产品字节；删除 Config::generate_runtime_output_from_profiles、RuntimeInputOutput、Enhance build_from_profiles_with_inspection 以及无 typed Profile 且始终拒绝的 Config 生成 API，删除旧 manager 无调用的未完成 promote_and_start_locked。未增加后端兼容类型。
- 将产品 header/序列化移入共享 runtime/artifact_snapshot，采用 ref 风格的品牌 header；check/intent/receipt/hash/product 均使用同一精确字节。保留本地现有 Mapping 与 receipt 模型、严格变换错误及 staged source freeze。新增同输入两条构建入口字节与 SHA-256 完全相同的断言，并验证准备阶段不发布文件。旧 LocalRuntime inline fallback 仍存在，但不再自行生成 Mapping 或重新序列化；后续迁移 native prepared-only admission，不能将本切片视为完整 ref Actor 对齐。
- L2。macOS shared runtime_build filter 13/13、workspace all-features/all-targets offline cargo check、rustfmt、diff--check。未运行完整 workspace 测试、真实 Core/Service 或 GUI E2E；未主动改变 IPC schema。回滚撤销本切片，不修改用户配置。

### DIFF-REF-PREPARED-LOCAL-ADMISSION：统一 LocalRuntime 提交边界

- 分支 ref-alignment/runtime-convergence-next-20261124，基线 6e80390ddea3f698565f829e78e164b6ffac8e71，ref 375af2770f7928395c9db18998c17e3880d432ea。共享 preparation 的 runtime_local_ipc 与 prepared_runtime_from_snapshot 统一构造精确字节 intent、真实 Clash 控制通道策略、候选端口与目标身份。旧 typed source 普通 reconcile 也必须得到已证实的 routing owner 与 source target，使用同一 PreparedRuntime 进行 advisory check、reconcile_prepared_settlement、receipt 确认和 derived product publication；不再由旧 TCC 混合函数执行普通提交。无 typed source 的路径保持拒绝。
- Profile Try 改为独立 try_prepared_profile，仅接收 PreparedRuntime、CoreSpec、operation identity 与 expected owner；删除 optional prepared/source 混合分支。保留原 TCC held-file / Confirm / Cancel、typed refusal、uncertain outcome、exact bytes / digest 检查，并补充 CoreSpec kind fence。旧 host-transition 调度与 deferred retry 上层入口尚在，仍可发起独立 advisory build，后续迁移其 source/owner admission 至 ref Reconciler；本切片不声称所有 legacy 调度已移除。
- L2。macOS workspace all-features/all-targets offline cargo check、LocalRuntime 21/21（新增实际 ordinary prepared check/reconcile 端口、拒绝时零提交、相同 check/submit/product 字节回归）、Reconciler Actor 161/161、rustfmt、diff--check。未运行全 workspace 测试、真实 Core/Service 或 GUI E2E。保留已有明确 HostUnsupported advisory 行为；不更改 IPC schema。回滚撤销本切片。

### DIFF-REF-DEFERRED-PREPARED-RETRY：延迟 Profile 收敛统一候选

- 基线 505231afaa25535f0a546c99db3e780e00e19fc8，ref 375af2770f7928395c9db18998c17e3880d432ea。生产 deferred retry 使用已提交目标的 S2/S3/S4 plan、同一 CandidateCheck 的严格 preparation 和 advisory validator、已准备候选的 S4 提交及前后 source/owner fence；不再先检查再重建另一份产品。旧独立 Profile validator 从 Tauri adapter、Facade、LocalRuntime 删除；legacy fixtures 的 port fallback 仅测试路径使用。
- 目标已过期不再执行旧目标；未观测到 Running Core 或 ownership proof 时等待，不绕过 planner。保留原有 deferred retry budget、typed check rejection/unavailable 和事务边界。Application host switch/Service release 及旧 TCC 调度尚未完全替换；本切片未修改其主机切换逻辑。
- L2。macOS shared CandidateCheck 16/16（新增严格一次构建/一次检查、过期目标不构建、拒绝保留 typed failure），workspace all-features/all-targets offline cargo check、rustfmt、diff--check；未重跑全 workspace、Tauri actor suite 或 GUI E2E。未更改 IPC schema。

### DIFF-REF-APPLICATION-SOURCE-CONVERGENCE：Application 提交与主机收敛分离

- 基线 076684bef718e365d840cc71b92e76429fa9b969，ref 375af2770f7928395c9db18998c17e3880d432ea。生产 Application host/core/runtime 配置变更严格预检、CAS 后直接通知唯一 Reconciler 的 SourceCommitted，按最新 source/owner 进入 S2/S3/S4，不在配置提交函数中调用旧 inline reconcile。新 source_requires_reconcile 通知与已 ACK 的 Checked receipt 明确区分，避免误用另一请求的候选；保留 isolated enhancement 的既有 checked path。Service mode 请求即使 daemon 未连接也由 canonical planner 决定，缺失 canonical session 在写入前拒绝。旧 inline postcommit apply 仅保留给测试 fixture。
- 保存与生效分离，已通知的 durable CAS version 不再由 UI 重发，mutation receipt 对尚待收敛的 runtime 返回 Pending，保留既有 OS-effect/legacy mirror 逻辑。原 Service release、startup fallback 与显式 Profile 调度仍需进一步迁移；不声明完整 ref actor 替换。
- L2。macOS CandidateCheck 17/17（新增预检不通知、已提交版本仅触发 fresh reconcile 的回归）、Reconciler Actor 161/161、workspace all-features/all-targets offline cargo check、rustfmt、diff--check。未运行完整 workspace、真实 Service/Local 主机切换及 GUI E2E；新增测试验证通知边界而非完整真实主机切换。

### DIFF-REF-STARTUP-RELEASE-OWNER：共享 Service 释放与失败目标身份

- 基线 master 7c5a2599d7109a2ba33b06267e718dc1c183a3d7（v0.29.1）；迁移分支原 1dee373569098a6f032f165e266d9e7758c3d01f 的八个提交已签名 rebase 至该基线，无冲突，ref 仍为 375af2770f7928395c9db18998c17e3880d432ea。
- Service 实例条件退役、结果不确定检查和释放后重新观察迁入 shared core_lifecycle::ownership::release_service_owner；Tauri 仅提供 Core port 和 Service evidence。Startup/Service fallback 不再允许生产环境回到 inline runtime 构建提交；缺少 canonical preparation/authority 时返回等待。Service fallback 跨 lease 再验证 stopped-owner 与原提交目标；首次启动和恢复均在释放前保留原失败目标、readiness generation，Local 接管前拒绝已过期目标。仍保留已有 leased host transition 与 test-only fixture 入口，尚未完全替换 StartupExecutor/Actor 调度。
- L2。macOS workspace all-features/all-targets offline cargo check、Reconciler Actor 161/161（包含过期失败目标时零 Local 接管/零 Core 提交断言）、rustfmt、diff--check；未执行完整 workspace、真实 Service/Local 切换或 GUI E2E。

### DIFF-REF-SHARED-COMMITTED-ROUND：共享 S2/S3/S4 执行模块

- 基线 65a6c52003228ab3d86768f9a55a56abb0c4155e，master 7c5a2599，ref 375af277。将 Tauri startup.rs 中完整 planned_committed_round 及 S3 停止执行装配迁入 chimera-core/client/reconciler/committed_round.rs，复用已有 StartupHost、RuntimeProgress、CoreLifecyclePort、共享 decision/proof/ownership；原 Tauri 方法仅委托，不新增状态、revision session 或 actor。保持原 S2 Facts 样本、S3 bounded replan、S4 source fence 与 Service fallback 语义。本轮为既有 ref-shaped 执行器的归属迁移，并非逐字复制 ref 的整个 Actor。
- L2。macOS workspace all-features/all-targets offline cargo check，Reconciler Actor 161/161，rustfmt、diff--check。初次编译缺少 CoreLifecyclePort trait import，补齐后通过。未运行完整 workspace、真实主机切换或 GUI E2E。剩余 Tauri startup adapter/fallback、prepared apply 和 Actor 调度继续迁移。

### DIFF-REF-SHARED-APPLY-EXECUTOR：共享 Prepared Apply 执行模块

- 基线 609d5a0c708d2ea61f2e7f1a681aa87a42be9a5d，master 7c5a2599，ref 375af277。将 Checked runtime apply、普通/严格 deferred prepared apply、实际 Core 提交 terminal 映射及 connection effect staging 的执行体迁入 chimera-core/client/reconciler/apply_executor.rs。ApplyHost 继承同一 StartupHost，使用现有 CoreLifecyclePort、RuntimeProgress、CandidateCheck 和 RuntimeSourceAuthority；Tauri 保留 host adapter、调用入口和测试，未新增 actor 或产品发布者。保留既有 source/receipt/Service generation fences 和 rollback/unknown 语义。本切片为既有 ref-shaped 执行体归属迁移，不等同于整套 ref Actor 源码替换。
- L2。macOS workspace all-features/all-targets offline cargo check，Tauri client::reconciler:: 全部 196/196，rustfmt、diff--check；未执行完整 workspace、真实 Service 切换或 GUI E2E。

### DIFF-REF-SHARED-SOURCE-DISPATCH：共享 Reconciler 源消息处理

- 基线 48a7c54d813bac36c2ec8dc4359afcc2004f0c84，master 7c5a2599，ref 375af277。Checked admission/abort、SourceCommitted、CheckedSourceCommitted、显式 checked target dispatch 的生产状态决策迁入 chimera-core/client/reconciler/source_dispatch.rs。复用同一 RuntimeReconcilerState 与 bounded checked inbox；Tauri mailbox 仅同步采集事实、执行返回的调度动作、入队、RPC reply 和 watch publication。保留 source version 单调性、停止意图、owner handoff continuation、queue backpressure、单次 mode effect degradation；没有第二 actor 或额外 runtime state。本轮不宣称完整复制 ref Actor，Completed 归并、timer/command 调度仍在 Tauri。
- L2。macOS workspace all-features/all-targets offline cargo check，共享 source_dispatch 3/3，Tauri reconciler 196/196，rustfmt、diff--check。初次 all-targets 检查发现测试通过父模块间接导入旧 helper，改为直接导入共享 helper 后通过。未执行全 workspace 测试、真实主机切换或 GUI E2E。

### DIFF-REF-SHARED-COMPLETION：共享 Reconciler 完成状态决策

- 基线 b16fa030444cd8ea2bae8552dd82a97d0b693f7c，master 7c5a2599，ref 375af277。将 Completed 的源版本 fence、Core outcome uncertainty、连接中断维护状态、Service handoff/release/recovery debt、最新目标重试和 handoff continuation 决策迁入 chimera-core/client/reconciler/completion.rs。复用现有 Round::observe/settle、RuntimeReconcilerState 和 attempt/status；Tauri 保留 task join、RPC、实际 timer、Service watch 与宿主通知。无第二 actor、无 IPC schema 修改。本轮为 ref-shaped 完成状态执行的归属迁移，不是完整逐字替换 ref Actor。
- L2。macOS workspace all-features/all-targets offline cargo check，共享 completion 3/3，Tauri reconciler 196/196，rustfmt、diff--check。未执行全 workspace 测试、真实 Service/Local 切换或 GUI E2E。

### DIFF-REF-PROXY-GRAPH-AND-ACTOR：完整复制代理图并归位唯一缓存 Actor

- 基线 4795d99a94181745c184211b452de4a0430a1b09，master 7c5a2599，SSH 核对上游 main 仍为 375af2770f7928395c9db18998c17e3880d432ea；未变更 submodule。完整复制 ref/backend/nyanpasu-core/src/clash/proxies.rs 的 1294 行（含上游测试，逐行比较 1294/1294 匹配），仅追加现有 fetch 入口与 Chimera 回归测试。规范类型采用 ref 的 Proxies；Tauri legacy 投影明确为 LegacyProxies，生成 TS binding 并同步前端调用者，保持 IPC 命令名和 JSON shape 不变。
- 实际 ProxiesActor/ProxiesClient 归位到 ref 的 chimera-core/src/proxies.rs，删除 proxies_actor.rs；生产 spawn 直接接收共享 CoreClient，不再套自定义 CoreClient/trait object，测试源通过 test-support 独立注入。既有延迟测试扩展移入 proxies/delay.rs；未被生产调用的旧 ProxySnapshotState 仅保留测试回归，不是第二套生产缓存。保留现有比 ref 更严格的 source-instance/shutdown fences、显式 invalidation、join/panic propagation 和既有 fingerprint 实现；并非逐字替换完整 Proxy Actor。
- L2。macOS workspace all-features/all-targets offline cargo check；代理 core 77/77（含既有回归及完整 ref graph 测试），追加 ref CoreActor 真实端口集成测试 4/4；Tauri proxy 8/8；Specta regeneration/freshness 通过；interface TypeScript build、UI proxy 25/25、interface proxy 19/19。保留已有主/legacy UI 和 agent 命令 wire contract。非目标：更改代理协议、实际网络环境、发布版本、master 或 ref tag。未运行全 workspace、真实 GUI E2E/跨平台发布构建。回滚：在迁移分支 revert 此提交。

### DIFF-REF-SHARED-ENDPOINT-AND-API：统一 Local/Service 控制端点与 Core API

- 基线 916f2737d3345852d036e2b11f64c13a7cc15258，master 7c5a2599，ref 375af2770f7928395c9db18998c17e3880d432ea（SSH 核对 main 无更新）；submodule 未改。直接复制 ref LocalEndpoint 实现至 chimera-core/actor_v2/endpoint.rs（按 pinned runtime 调整 Host/YAML/futures 路径）；合并原 Tauri ServiceEndpoint 及 wire/错误/所有权回归至同文件，删除桌面 control_endpoint.rs/service_endpoint.rs。保留现有静态 IPC shortcut 客户端、StopIfOwner、RecoverIfServiceOwner、ChimeraClient alpha、transparent capture 及 credential redaction；桌面工厂仅注入原本的按需应用目录解析器，恢复仍以同一规范化目录双重 fence 为准，新增 resolver fail-closed 回归。
- 采用 ref API InterruptFailure::from、delay_deadline、typed connection read/close 方法及单组 interruption 执行，复用唯一 API capability，不另设客户端；迁入 ref 的实际 CoreActor HTTP/revocation/deadline/group-list 测试，适配现有 binding 表示。保留现有更严格的实例/transport/命令防护。ServiceActor 仍使用现有原生 restart 扩展；其完整替换、API narrow port 与 ref Meow 独有 API 并非本轮目标。未改变 IPC schema、OS 配置或 runtime pin。
- L2。macOS shared actor_v2 162/162、Tauri core::actor_v2 48/48、setup::actor_tests 7/7、shared proxies 81/81、connections 10/10（筛选可能重叠，不合计为独立用例）；workspace --locked --all-features --all-targets offline cargo check，rustfmt、diff--check。初次检查发现桌面两个 CoreStatusSnapshot 导入冲突与旧测试引用已删除 helper，已修正；首次 setup 筛选名错误、重新以正确模块执行。未运行全 workspace、真实 privileged Service 或 GUI E2E/跨平台打包，未获得独立评审。回滚：revert 此迁移提交。

### DIFF-REF-CANONICAL-RUNTIME-TEXT-INSPECTION：共享快照文本与检查读取器

- 基线 e1c579035d49e161e2f3708867d4ce2a58b53b00，master 7c5a2599，ref 375af2770f7928395c9db18998c17e3880d432ea；远程与 ref 核对无更新。RuntimeSnapshotData/from_data 收敛为单一 Arc<str> 配置文本，生成 header 仍包含在精确提交/发布字节中，digest 与上轮保持一致；PreparedRuntime intent 直接共享该分配。保留 Mapping 供旧配置发布和 ClashInfo，未完整替换为 ref 的 graph-only model；无第二份产品字节缓存。仅测试构造器可以单独传入 Mapping/bytes。get_runtime_config 从 graph final node 恢复，get_runtime_yaml 直接读取原始生成文本（不含产品头）。
- 将 Tauri RuntimeInspection 的完整读取、实例和 owner generation 双重校验、过期节点拒绝、blocking 内容恢复与原回归迁至共享 runtime_inspection.rs 的 RuntimeInspectionReader；保留既有有效视图的动态 receipt 认证而非 ref 的持久化 applied_binding（尚未迁移）。类型化 RuntimeSnapshotChanged/RuntimeNodeNotFound，保留双侧脱敏。ArtifactSnapshot 保留 finalizing node 与内容一致性验证，采用 ref advice 投影，旧持久化输出缺失该字段时默认空，Specta 重新生成；目前没有新增独立 advice UI。
- L2。macOS workspace --locked --all-features --all-targets offline check；Runtime 50/50、ArtifactSnapshot 4/4（初次旧断言要求不输出 advice，已改为验证保留完整诊断及旧数据解析）、Tauri reconciler 196/196、Tauri core::actor_v2 48/48，新增共享文本 Arc identity、YAML/graph 一致性及类型化错误断言。Specta regeneration/freshness，interface/UI typecheck 通过。未执行全 workspace、真实特权 Service/GUI E2E 或跨平台构建，未获得独立评审。清理未运行的 Rust 增量缓存，不触及源码或用户配置；构建使用 CARGO_INCREMENTAL=0。回滚：revert 本提交。

### DIFF-REF-PROFILE-SOURCES-AND-SHUTDOWN：上游源回执与 Profile Actor 生命周期

- 基线 b2591a00a14d11a8207c55b0e0a4b4c971819097，master 7c5a2599，ref 375af2770f7928395c9db18998c17e3880d432ea；远程与 ref 均无更新。完整采用 ref state/profiles/sources.rs 的 SourceLedger、SourcesSnapshot 及测试，source origin/outcome/status 采用 ref 结构，复用既有 ConvergenceHealth。ProfilesActor 是唯一写者，watch 接收端公开只读快照/订阅；ProfileSyncStatus 增加 latest source，Dashboard summary 保持不含详情。删除 Profile 后清除 ledger，rename 更新名称。
- 采用 ref conclude_refresh 与 sync_mirror 的处理顺序：下载成功或失败均先验证当前 definition/url，再决定是否提交；旧请求结果记 Superseded 而非当前 source failure；保留原文件/文档 CAS、runtime gate、materialization recovery 与 typed caller error。源回执仅采用不含 URL、凭据、文件路径和配置内容的稳定诊断，原调用错误仍保留。外部文件 watcher 记录实际 commit/refusal，未增加独立外部源 UI。
- 采用 ref shutdown admission gate，所有进入 mailbox 的新写请求拒绝，producer completion 不再触发提交；已开始的事务仍按原事务边界结束。补齐原 ImportRemote task owner，用 catch_unwind 保留 fetch panic 的明确失败，post_stop 取消并回收实际 refresh/import task，回复 ShuttingDown；不增加第二个 Actor。
- L2。macOS workspace --locked --all-features --all-targets offline check；shared profiles 22/22，Tauri profiles 102/102，Specta regeneration、interface/UI typecheck 通过。初次 standalone core test 缺少 profile-transform test-support feature，补 dev-dependency；初次新增外部文件测试在 watch publication 前读取，改为等待实际 watch 通知后通过，未降低断言。未执行全 workspace、实际 GUI E2E 或跨平台构建，未获得独立评审。现有 ProfilesActor 尚非逐字 ref 替换；完整 ConfigurationStatus 聚合和 UI 状态仍待后续迁移。

### DIFF-REF-CONFIGURATION-STATUS：配置聚合、脱敏观察与调试 UI

- 基线 fc7be9a7e9327fc3a1e1bf98a46e07916612ce4e，master 7c5a2599，ref 375af2770f7928395c9db18998c17e3880d432ea；远程及 ref main 核对无更新。迁入 ref ConfigurationStatus 及 configuration_observation 的完整配置摘要转换和 hostile-data 测试，复用现有 EffectKind/ConvergenceHealth；现有 owner string code 转为闭集 EffectFailureCode，未知值只输出 Other。共享纯聚合读取真实 Reconciler/Effects/SourceLedger 与已提交域版本，保留独立 maintenance、recent operations、source outcome 和 active operation，不产生额外 writer。
- Tauri 注册只读完整/摘要查询、tracked watch observer 及明确用户触发的 runtime/effect retry，沿用原 Reconciler/Effects owners；observer 随 client shutdown 取消。不同于 ref RPC event bus，仅广播不带诊断、路径或 Profile 名称的 summary，详细查询留在受信任本地 IPC。session 版本等无独立 watch 的变化由查询/10s polling 刷新，不宣称每个域都有即时事件。未改变全局 client startup/shutdown 执行顺序。
- 迁入 ref configuration-status helpers、summary provider 与实际 debug/settings 页面，适配现有 Tauri IPC、query descriptors、i18n、retry result unwrap。debug 页监听安全摘要作为详细查询的失效通知，不广播详细诊断；结构共享在 cache 写入时校验 event_seq，避免迟到查询覆盖较新状态。summary provider 已导出但未在全局窗口挂载，避免无消费者的轮询。部分 upstream locale 缺键按英文回退；未改现有翻译。
- L2。macOS workspace --locked --all-features --all-targets offline check；shared configuration 5/5（含 ref hostile-data tests），Tauri reconciler 196/196，frontend configuration 2/2，Specta regeneration/freshness 与 interface/UI typecheck 通过。初次复制 locale 发现 ref 个别语言缺键，补英文 fallback 后类型检查通过。未执行全 workspace、真实 UI E2E、实际 privileged Service 或跨平台构建；未获得独立评审。尚未迁入 ref 完整 Client Lifecycle / RuntimeReconcilerActor。回滚：revert 本提交。

### DIFF-REF-MUTATION-IMPACT：统一源变更投影与工作流分类

- 基线 9a93e0afc73bcc826534a6c86e9139a0b8ca5ce1，master cd164f83c65d3b132543029d09a2fabf512f6b85，ref 375af2770f7928395c9db18998c17e3880d432ea；远程与 ref main 核对无更新。将既有 ref-derived state/impact 与桌面 application_workflow/impact 收敛到 ref 的 state/mutation/impact.rs 路径；源 Actor、RuntimeInputs、桌面 application workflow/legacy_source 直接消费统一的 runtime target、reaches_runtime 及 request intent。删除独立桌面 state/mutation/impact 和旧 projection；保留 Chimera host/core switch 与 effects/request intent 所需类型，而非完整替换 application workflow。
- 管理中的 DNS/Hosts 进入同一 Clash 目标指纹及判定；未接管的草稿不触发运行时变更。MIPS 仍仅在选中时影响目标。Profiles 使用 ref 的 ordered closure target + touched content 判定，替代重复的结构比较；保留 materialization 及 mutation gate。迁入 ref Profile closure/layout 测试并添加 request/source verdict 一致性及 managed DNS/Hosts 测试。现有 target digest 算法与共享 projection 不改；原桌面 Clash 指纹变更为该共享 projection。
- L2。macOS pinned nightly-2026-11-23：workspace --locked --all-features --all-targets offline check；mutation impact 25/25、Profiles State 22/22、Tauri client 500/500。补安装新工具链 rustfmt/clippy（首次 fmt 因组件缺失而停止）。未执行全 workspace tests、真实 GUI E2E、特权 Service 或跨平台构建，未获得独立评审；未改 master。回滚：revert 本提交。

### DIFF-REF-WORKFLOW-PROGRESS：共享工作流状态、事务策略与回执

- 基线 b38909e3e268d831ea110c8d4beaad7f5b9ed60d，master cd164f83c65d3b132543029d09a2fabf512f6b85，ref 375af2770f7928395c9db18998c17e3880d432ea；远程及 ref main 核对无更新。将 ApplicationWorkflow 的实际 live/deferred/startup/recovery progress、状态推进、重试准入与结算迁入共享 reconciler/workflow_progress.rs。桌面保留 host command 分类、Service 租约及唯一执行 mailbox；无新增 Actor 或状态镜像。未完整替换为 ref 的 Actor。
- 事务 policy、request/receipt 和相关测试整体迁入 shared mutation_policy.rs/workflow_mutation.rs，旧桌面文件删除；仍保留 Chimera Profile TCC 事务证据及既有安全边界。明确未知结果已证明时，保留原始 unresolved reason，绝不放行后续无关命令。新增共享回归覆盖 stale target、dependency wait budget、unknown transaction evidence、startup vs runtime recovery。
- L2。macOS nightly-2026-11-23：workspace all-features/all-targets offline check；shared reconciler 122/122、Tauri client 484/484（16 项原桌面策略/回执测试已迁到 shared）。首次 fmt 遇到移动时遗漏 impl 边界，已修复；未执行完整 workspace tests、真实 Service/GUI E2E 或跨平台构建，未获得独立评审。用 cargo clean -p chimera -p chimera-core 释放 24 GiB 可重建产物。回滚：revert 本提交。

### DIFF-REF-RECONCILER-MAILBOX：统一请求调度与定时器所有权

- 基线 master 67d717c3f14b683bd8a291d0ec5d70d40b6ca7b1，ref 375af2770f7928395c9db18998c17e3880d432ea；远端主线、迁移分支与 ref main 核对无更新。沿用原迁移分支。本轮对应 ref/backend/nyanpasu-core/src/client/reconciler/mod.rs 的单一 actor 状态、request/publish_round、拒绝和 Tick 生命周期；采用 ref Ractor send_interval/send_after。实际 pending/active/next_id/status/uncertainty/startup-owner/shutdown waiters 与结果由共享 reconciler/mailbox.rs 持有，统一入队及出队复检，保持 FIFO、三阶段拒绝回执顺序、32 项容量及历史、shutdown 单次执行和精确失败重放。Tauri 只做平台命令分类、实际 Core/Service/事务调用、progress 回调与租约适配。
- 脏状态合并与 Tick 消费迁入 shared RuntimeReconcilerState；owner handoff/满队列时保留 durable source intent。定时器由 mailbox 持有 JoinHandle，关闭或 drop 取消通知，关闭后不能重新创建；不取消正在执行的 Core worker，保留 post_stop join 和未知结果防线。保留 Chimera 原异步 worker/Checked 非阻塞准入边界，并非完整逐字复制 ref 同步 actor。没有新增状态镜像、actor 或 IPC 契约变更。
- L2。macOS nightly-2026-11-23：workspace all-features/all-targets offline check 通过；shared reconciler 127/127（新增 5 项队列/关闭/dirty/history 测试），Tauri client 484/484，Specta bindings freshness 通过。初次 all-targets check 发现旧测试需要显式导入共享 MAX_PENDING，已修正原引用，不改变断言。未执行完整 workspace tests、真实 GUI E2E、特权 Service、跨平台构建或独立 Review。下一步迁移完整 worker 执行与 CoreLifecycleWorkflow；回滚 revert 本提交。

### DIFF-REF-RECONCILER-WORKER：共享执行及终态证据

- 基线 master ab74bc27ad1c6bcc20b5fb86a429c363006d0887，ref 375af2770f7928395c9db18998c17e3880d432ea。远程新增 3 个启动/DevTools/Vite 提交，与 mailbox 改动无文件重叠，签名 rebase 至 55a9e278357d24eeed5bf7d72b364cb79afbe487；核对变更路径，保留 rebase 前恢复引用。对应 ref/backend/nyanpasu-core/src/client/reconciler/mod.rs 的单一 request/execute/round publication 责任。将现有 Worker 的执行、panic 恢复、进度回调、Service 探测、completion payload 与 startup recovery lease worker 迁入共享 worker.rs；Tauri 保留实际命令和宿主适配，test-only Profile TCC 特殊分支仍在桌面。复用共享 ServiceEvidence，WorkflowProgress 持有 lifecycle attempt 创建/结算、未知结果及 startup report 缓存。无新增 Actor，无 IPC/schema 改动。
- 保留 Checked 非阻塞准入、lease-before-mailbox 顺序、源版本 fence、真实 Core 回执与单一 owner。仅有无 ServiceActor 的旧端口才重复探测。回调 panic 不覆盖执行结果，未知结果保留恢复证据；startup recovery admission guard 在退出时清除。不是完整逐字复制 ref 同步 Actor。下一步迁移 CoreLifecycleWorkflow 与命令分类。
- L2，macOS nightly-2026-11-23：workspace all-features/all-targets offline check；shared reconciler 132/132（新增 5 项 worker 测试）、Tauri client 484/484、Specta bindings freshness 通过。未执行全 workspace tests、真实 GUI E2E、特权 Service、跨平台构建或独立 Review。回滚：revert 本提交。master 不修改。
