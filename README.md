# Wealthlog

Wealthlog 是独立维护的本地优先个人财务工作区，使用 React、Rust、Tauri 和 SQLite。
保留手工记账、账户和投资核算、收支预算、行情与导出，并提供 CLI 和 MCP 接入。

项目基于 [Wealthfolio](https://github.com/wealthfolio/wealthfolio) 的
`bfcd6d6202ceb4922d0be85aa5666d1802eda456` 提交开发。修改始于 2026-10-13；
删除了云同步、自动更新、内置聊天、Addon、目标工具及导入入口，新增普通收支 CLI、
桌面服务生命周期与 Agent 接入。详见[来源和修改](docs/customization.md)。
整体代码按 [AGPL-3.0](LICENSE) 发布；原有版权声明及第三方依赖许可继续有效。
Wealthfolio is a trademark of Teymz Inc. Wealthlog 与 Wealthfolio / Teymz Inc.
没有隶属关系，也未获得其背书。

## 开发

需要 Node 24、pnpm 10.33.4 和 `rust-toolchain.toml` 指定的 Rust 工具链。

```sh
pnpm install --frozen-lockfile
pnpm build:types
pnpm tauri dev
```

```sh
pnpm test
pnpm type-check
pnpm build
pnpm build:tauri
cargo check --locked -p wealthfolio-app -p wealthfolio-server
```

开发应用标识为 `local.kanelogger.wealthlog.dev`。真实账本应保存在应用数据目录，
验证时使用隔离合成数据；可参照 `.env.example` 设置 `WF_DATA_DIR`。

## Agent 消息记账

向加载了 `wealthlog` Skill 的 Agent 发送消费信息，例如“记账：10 月 28 日 GOOGLE 消费 29.19 USD，记在中国银行（0407）”。Agent 读取真实账户、分类规则与近期历史，选择分类后通过普通收支接口一次入账及归类；账户或分类不明确时会说明待确认项。

CLI 的 `ordinary context`（MCP：`get_bookkeeping_context`）提供分类依据，需账户、流水、分类读取权限；正式记账需 `ordinary:write`。启用的现金和信用卡账户均可记账，支出统计中的账户选择只影响报表。来源短信保留原币，不自动编造结算汇率。

## 许可与来源

完整许可证见 [LICENSE](LICENSE)；界面“设置 → 关于与许可证”可离线阅读。
[第三方声明](apps/frontend/public/legal/THIRD_PARTY_NOTICES.txt)列出已识别的字体，
发布二进制前仍需完成全部依赖许可审查。
源码与问题反馈：https://github.com/kanelogger/wealthlog
