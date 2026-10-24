# 字段、错误与恢复

## 普通收支

查询返回：

```json
{"page":{"rows":[{"id":"...","accountId":"...","activityType":"WITHDRAWAL",
  "activityDate":"2026-10-20","currency":"CNY","amount":"35.00","fee":null,
  "tax":null,"notes":"午餐","status":"POSTED","version":1}],
  "totalCount":1,"page":0,"pageSize":200}}
```

单笔读取额外返回 `fxRate`、`taxonomyId`、`categoryId`、`eventId`。修改时把读到的字段原样带回，除非用户要求改动。

新增字段：

| 字段 | 规则 |
|---|---|
| `accountId` | 真实账户 ID；必须是启用中的支出账户 |
| `activityType` | `DEPOSIT` 或 `WITHDRAWAL` |
| `activityDate` | `YYYY-MM-DD` 或 RFC 3339；只给日期时不要编造时间 |
| `currency` | ISO 币种 |
| `amount` | 正数小数字符串 |
| `fee`、`tax` | 小数字符串；允许 0；修改时省略保留，显式 null 清空 |
| `fxRate` | 小数字符串 |
| `notes` | 完整备注 |
| `requestKey` | 冻结的 UUID；由快照脚本生成 |
| `idempotencyKey` | 可选；缺省用内容指纹。空白字符串无效 |

修改另外要求 `activityId`、`expectedVersion`；不接受 `idempotencyKey`，不接受未知字段。

事件清空使用 `clearEvent: true`，并省略 `eventId`。同时提交 `eventId` 和 `clearEvent` 时 `eventId` 优先，记录会继续关联该事件。

## 日期

- 裸 `YYYY-MM-DD` 按 UTC 零点存储；已验证的 `Asia/Shanghai` 下 GUI 显示同一天，不承诺其他时区日期边界。
- 用户给出具体时间与偏移时使用 RFC 3339，保持其瞬间；不自行重新解释或重编码历史记录。
- 接口的 `dateFrom`/`dateTo` 是包含式日历日边界，按存储字符串比较。需要“当地日”精确语义时，先扩大范围再用 `wealthlog_date.py` 按瞬间过滤。
- 修改时把读到的 `activityDate` 原样传回，不截断带偏移的时间。

## 草稿

`create_pending_activity` 只需要 `activityType` 和 `requestKey`。已知字段才传：

```json
{"activityType":"WITHDRAWAL","amount":"35","currency":"CNY",
 "activityDate":"2026-10-20","notes":"午餐",
 "requestKey":"..."}
```

不知道的账户、日期、币种、金额就省略。不要猜，也不要为草稿编造时间。方向不明时先问用户。草稿不会改变正式余额和统计；只有 GUI 确认后才生成一笔正式流水。

日期只有在用户明确给出时才视为已知。消息接收时间、快照生成时间、当前时间都不是交易日期。即使用户在后续澄清中回答了账户或方向，也不代表日期已被补全。正式新增缺日期不得直接提交；有草稿权限时留空建草稿。

草稿创建响应中的 `requestId` 关联实际业务请求；同请求重放仍返回该 ID，按来源去重的新请求则有自己的请求 ID，但草稿 ID 不变。普通草稿列表不承诺返回 `requestId`；它不是草稿 ID，也不是 JSON-RPC 调用编号。

有真实来源封套时，`source` 保留其中的 `channel`、`messageId`、`entryId`，以及适用的 `sourceAccount`、`sourceSession`；`excerpt` 仅放原文。来源去重需要完整身份，缺少身份时省略相应字段，不用正文或示例制造 ID。重投相同来源应核对原草稿 ID 与状态，明确是否重复，而不是把相同来源当成新的消费。

## 错误码

| `error.code` | 退出码 | 行为 |
|---|---|---|
| `unauthorized`、`scope_denied`、`forbidden`、`write_blocked` | 3 | 停止，说明凭据或权限限制 |
| `profile_mismatch`、`profile_required` | 2 | 修正目标/绑定；不跟随当前账本 |
| `profile_locked` | 6 | 等 GUI 解锁，再核对目标 |
| `session_revoked`（MCP） | 3 或 6 | 重建连接，不生成新业务请求 |
| `version_conflict` | 4 | 重读、展示差异，确认后换新版本和新请求键 |
| `duplicate_activity` | 4 | 报告 `data.existingActivityId`；新增数为零 |
| `request_conflict` | 4 | 请求键被不同内容用过；停止，检查快照 |
| `activity_not_found`、`activity_out_of_scope` | 2 | 重新定位或引导 GUI |
| `invalid_amount`、`invalid_input` | 2 | 按原意修正；缺依据时澄清 |
| `request_in_progress`、可重试的 `storage_error` | 4 或 1 | 按预算重放同一快照 |
| `result_unknown`、发送后断连 | 5 | 保留待核实，先只读核对 |

CLI 会把部分 MCP 会话撤销呈现为 `profile_locked`；按实际 `error.code` 处理，退出码只做粗分类。

## 快照状态

`wealthlog_snapshot.py` 的 `status` 文件取值：

- `pending`：已生成，尚未确认结果。
- `unknown`：请求已发出，但响应不可见或传输中断。
- `resolved`：已通过成功响应或只读核对确认结果。
- `abandoned`：确认没有执行，且不再重放。

`request.json` 创建后不覆盖；`context.json` 记录正文与凭据摘要。恢复时先 `verify`：

```bash
python3 <skill>/scripts/wealthlog_snapshot.py verify --request-dir "$REQUEST_DIR" \
  --app-id "$APP_ID" --ledger "$LEDGER" --operation "$OPERATION" \
  --credential-file "$TOKEN_FILE"
```

恢复时传本轮配置实际选用的 app、账本、操作和凭据文件；不传这些参数的 `verify` 只核对已保存的上下文，不能证明本轮配置相同。摘要或上下文不匹配时停止重放，只读核对。

## 自动重试预算

自动重试先遵守 Skill 的发送状态与 `retryable` 判定；只有 `request_in_progress` 和明确可重试的存储错误进入预算，最多两次，间隔 1 秒、2 秒。必须同时满足：

- 同一账本、同一操作、同一凭据主体、同一请求键、同一正文；
- 快照 `verify` 通过；
- 没有跨过原来的相对日期解释（恢复时不重新计算“昨天”）。

权限、重复、版本、内容冲突、未知结果都不自动重试。`result_unknown` 先只读核对候选；不能确认时报告待核实，不生成新请求。

## 结果表达示例

- 正式新增：“已在账本 <id> 的账户 <account> 记一笔支出 35.00 CNY（2026-10-20），记录 <activityId>，版本 1。”
- 已存草稿：“已存为待确认草稿 <id>；缺少账户，未正式入账。”
- 重复：“已存在记录 <id>，本次未新增。”
- 结果待核实：“请求键 <key> 的响应不可见；已按只读核对找到/未找到候选，仍需确认。”
