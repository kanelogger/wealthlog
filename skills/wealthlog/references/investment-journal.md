# 投资工作台

按 [连接与权限](connection.md) 核实目标桌面实例、账本和凭据。用 CLI 的 `investments commands` 与 `investments schema COMMAND` 读取当前服务的命令和字段。桌面 MCP 用同名工具及其参数 schema，正式写入绑定同一个 `X-WF-Profile-Id`。读取需要 `investments:read` 及账户、持仓、流水读取权限，保存与关联另需 `investments:write`。确认、提交复盘和发布原则还需独立的 `investments:confirm`。

## 读回和起草

- 从 `get_investment_decision`、`get_investment_review` 或 `get_investment_principle` 获取目标及当前版本。复盘关联具体确认版，保持观察截止日。原则可引用确切的复盘版本，来源状态以读回为准。
- 原文标记为 `USER_PROVIDED`，有整理或代拟的正文标记为 `ASSISTED_DRAFT`，来源不清楚时用 `UNSPECIFIED`。把用户陈述、可核实事实、假设和未知分别保存。只在用户提供依据时填概率。保留未知金额及缺失来源。
- 按当前 schema 和读回的版本调用对应的 `save_investment_decision_draft`、`save_investment_review_draft` 或 `save_investment_principle_draft`。草稿写入后读取其 ID、版本和状态。用户仅授权起草时在草稿读回后结束。

## 预览后正式写入

展示预览返回的准确版本与正文、待冻结的背景或证据及缺失项。按用户对该动作与目标的具体授权执行。已给出的持续授权仅适用于其明确涵盖的范围，权限令牌只用于通过服务端鉴权。使用预览提供的 `ledgerId`（应与连接匹配）、`target` 和 `expectedContentHash`，以及表中的配套字段。

| 动作 | 只读预览 | 正式命令 | 从预览原样传回 |
| --- | --- | --- | --- |
| 确认决策 | `preview_investment_decision_confirmation` | `confirm_investment_decision` | `context`、`expectedContextHash` |
| 提交复盘 | `preview_investment_review_submission` | `submit_investment_review` | `facts`、`expectedFactsHash` |
| 发布原则 | `preview_investment_principle_publication` | `publish_investment_principle` | `sourceContext`、`expectedSourceContextHash` |

关联已入账成交时，从待办读取真实成交和来源版本，用 `preview_investment_execution_link` 核对成交与决策确认版，再用 `link_investment_execution` 保存这条证据。通过 `list_investment_execution_links` 读回该关联。交易模式证券账户的新成交通过桌面录入。手动持仓账户补记成交见 [投资账户操作](portfolio.md)。

## 冻结、提交与恢复

按 SKILL.md 的快照步骤，用完整命令名作为 `--operation`。传入当前 schema 要求的参数，省略由脚本生成的根级 `requestKey`。正文变更先保存草稿，再读预览。对 CLI 发送冻结文件，对 MCP 发送其中完整的 JSON 对象。

```bash
"$CLI" --app-id "$APP_ID" --ledger "$LEDGER" --token-file "$TOKEN_FILE" \
  investments call "$OPERATION" --input "$REQUEST_PATH"
```

收到成功回执后读回目标版本和状态，报告账本、版本、程序调用身份及是否重放。若预览或版本已冲突，重新读取并向用户展示差异，依据新的预览和授权生成新快照。结果未知时按 SKILL.md 先将快照标为 `unknown`，再读回目标版本、状态及对应内容或证据。读回不足以确定结果时，先用原凭据、账本和操作核验快照，再在原授权范围内重放原请求。换凭据或无法核对时保持待核实状态。
