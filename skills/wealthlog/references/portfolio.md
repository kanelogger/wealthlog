# 投资账户：脚本 / CLI / MCP

这些接口在本地账本记录**已经发生**的事件，不连接银行执行支付，也不向券商下单。首先按 SKILL.md 读取当前连接配置并检查服务 ready。使用服务返回的真实账户/资产 ID；这类查询和写入都不需要 Computer Use。

## 能力与授权

| 操作 | CLI | MCP 工具 | 权限 |
|---|---|---|---|
| 账户列表 | `accounts list` | `get_accounts` | `accounts:read` |
| 今日持仓、分币种现金和版本 | `holdings get --account-id ID` | `get_investment_account_state` | `accounts:read`、`holdings:read`、`activities:read` |
| 现金内部转账 | `transfers create --input FILE` | `create_internal_cash_transfer` | `transfers:write`，依赖账户与活动读取 |
| 手动持仓账户的已成交买卖 | `holdings trade --input FILE` | `record_manual_trade` | `holdings:write`，依赖账户、持仓与活动读取 |
| 手动持仓/现金快照校准 | `holdings patch --input FILE` | `patch_holdings_snapshot` | 同上 |

上述写入口仅在桌面宿主开放。读写命令都显式传 `--app-id`、`--ledger`、`--token-file`；MCP 写调用提供 `X-WF-Profile-Id`。已有 `ordinary:write` 令牌不自动获得新的权限；缺权限时请用户在应用 Agent Access 中授权，不自动改令牌或换凭据绕过拒绝。

## 查询和准备

随包脚本读取同一连接配置，不读取数据库：

```bash
python3 <skill>/scripts/wealthlog_portfolio.py accounts
python3 <skill>/scripts/wealthlog_portfolio.py state --account-id ACCOUNT_ID
```

`stateVersion` 是持仓与现金写入的并发前提。`positions` 含 assetId、symbol、quantity、averageCost、totalCostBasis、currency；`cashBalances` 按币种返回十进制字符串。记录用户提供的数据，不能把折合总额当成某一币种余额。

### 已完成的现金转账

准备输入 JSON（不含 requestKey）：fromAccountId、toAccountId、activityDate（用户明确的 YYYY-MM-DD）、sourceCurrency、destinationCurrency、sourceAmount。跨币种必填 destinationAmount；同币种两端金额一致。notes 可选。两条正式活动和请求/审计一次提交，失败全部回滚。

默认防重复身份来自同一内容。明确是两笔独立但完全相同的转账时才给不同的 idempotencyKey；不能因超时或 duplicate_activity 换 key。

### 手动证券账户买卖

先读 state；没有历史快照（hasSnapshot=false）时先用已知持仓/现金初始化快照，不从未知余额猜出零。准备 accountId、activityDate、expectedStateVersion、assetId、side（BUY/SELL）、quantity、price、currency。写日期只支持今天。仅支持已经存在的股票/ETF/基金资产，以及与账户同币种的交收；不支持卖空或处理既有空头、债券、衍生品。费用和税费可分别提供 fee、tax；未提供时保持未知，计算暂按零处理，结果用 feesProvided / taxProvided 明示。

BUY 将成交额及已知费用计入成本并更新加权平均成本，SELL 按现有平均成本减少持仓。该入口返回 `recordingMode=MANUAL_SNAPSHOT`，成交输入保存在请求与业务审计中；它不会伪造交易模式 BUY/SELL 流水。其他证券和币种保持原值。

用户提供实际现金时传 observedCashBalance，以它为准。返回 calculatedCashBalance、cashBalance、cashAdjustment，向用户解释差额；不把差额自动归为收入、手续费或交易损益。快照按当日最终余额处理，仅快照日期之后的现金转账自动累计。同日操作先记转账，再保存最终持仓/现金；对已有同日或更晚快照的转账，结果 cashReconciliationAccounts 会列出需要核对的手动账户。读取并核实实际余额后再局部校准，不把转账重复加到已包含它的快照中。

### 快照校准

准备 accountId、snapshotDate（今天）、expectedStateVersion、notes（观测依据），以及 positions 和/或 cashBalances。

- positions 是局部补丁，每项包含 assetId、quantity、averageCost、currency；quantity=0 明确移除该持仓。
- cashBalances 仅修改给出的币种。未给出的证券、现金币种原样保留。
- 未提供确切平均成本时先问清；不要拿今日市价覆盖历史成本。

## 冻结、提交和读回

```bash
python3 <skill>/scripts/wealthlog_portfolio.py prepare transfer_create --input transfer.json --summary "已完成的现金转账"
python3 <skill>/scripts/wealthlog_portfolio.py prepare manual_trade_record --input trade.json --summary "已成交加仓"
python3 <skill>/scripts/wealthlog_portfolio.py prepare holdings_snapshot_patch --input patch.json --summary "按实际余额校准"
```

每次只执行本次需要的 prepare；它只生成 `stateDir` 下的 request.json / context.json，不发送请求。随后对返回的 requestDir 提交：

```bash
python3 <skill>/scripts/wealthlog_portfolio.py submit --request-dir REQUEST_DIR
```

脚本核对账本、凭据摘要和冻结内容，显式绑定目标，再调用 CLI；不会自动重试写入。使用 MCP 时，发送同一份 request.json 的完整参数，不另编 requestKey。保存响应后，再读双方账户/目标持仓，报告保存结果与差额；交易模式账户的派生余额可能在后台重算后才刷新。

- 成功：报告账本、账户、转账的 activityIds 或持仓的 snapshotId，以及 requestId、分币种现金、数量与差额；不要要求快照结果提供普通流水版本。result 中有 requestId、replayed；同一请求重试返回相同结果，无额外入账。
- `result_unknown`：保留原请求，先核对结果；必要时原样重试。
- `request_conflict` / `duplicate_activity`：停止，不换 key 绕过。
- `state_conflict`：旧状态未被覆盖。重新读状态、核对用户目标后准备新的请求；不盲目重放旧数量增量。
- 已经通过界面录入的历史事件，不再用这些新接口重复录入。
