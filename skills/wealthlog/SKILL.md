---
name: wealthlog
description: Use when the user asks to query, add, edit or draft ordinary income/expense entries in Wealthlog (记账、查支出、改一笔账、先存草稿) through the local CLI or MCP. Covers account lookup, calendar dates, request snapshots, duplicate/version/unknown-result handling, permission and ledger binding.
---

# Wealthlog 操作

在用户的本地 Wealthlog 上处理普通收支（DEPOSIT / WITHDRAWAL）。只做查询、明确授权的正式新增与修改、缺字段草稿；转账、证券、贷款、删除、批量写入和自动确认草稿都引导到 GUI。

## 启动

业务输入来自用户任务、当前连接配置、该配置的 `stateDir` 和服务读回。接口细节以本 Skill、工具 schema 和响应为准。定位既有请求只查本轮 `stateDir`；项目源码、验收报告、协调器控制日志和其他会话原文不属于操作输入，不读取它们来推断接口、补参数或决定提交。记忆中的旧绑定和旧结果仍需用当前配置与服务核实。

1. 读取连接配置。优先 `WEALTHLOG_AGENT_CONFIG`，否则 `~/.config/wealthlog-agent/config.json`。字段：`cli`、`appId`、`ledgerId`、`tokenFile`、`timezone`、`stateDir`。缺字段或文件不可读时停止，向用户索取，不猜路径。将 `WEALTHLOG_AGENT_STATE_DIR` 显式设为 `stateDir`，使用当前工作区内的私有目录；不可写或配置不一致时在发送前停止。
2. 每条命令都带 `--app-id "$APP_ID"`。业务命令再带 `--ledger "$LEDGER"`。不要依赖 CLI 默认值：默认 app ID 是正式实例。
3. 先跑 `"$CLI" --app-id "$APP_ID" service status`，读取 `data.state`：
   - `ready`：继续。
   - `locked`：请用户在 GUI 解锁；不要尝试解锁或换实例。
   - `unavailable`：报告发现文件、进程或连接问题；不要自动启动其他实例。
   退出码 0 只说明命令执行完，不说明服务 ready，也不说明 token 有效。
4. 只把 `tokenFile` 路径交给 CLI。不要把 token 放进命令参数、聊天、配置、截图或报告；不要读取后回显。

## 查询

1. 从消息上下文取得真实接收时间与时区；没有时用当前时间和配置中的 `timezone`。
2. 相对日期由包内脚本解析，不要自行把“昨天”换算成 UTC：

   ```bash
   python3 <skill>/scripts/wealthlog_date.py resolve \
     --received-at "$RECEIVED_AT" --timezone "$TIMEZONE" --expr yesterday
   ```

3. 查询使用脚本，它把本地日区间转成 UTC、扩大接口日期范围、读取全部分页，再按时间戳精确过滤：

   ```bash
   python3 <skill>/scripts/wealthlog_date.py query \
     --cli "$CLI" --app-id "$APP_ID" --ledger "$LEDGER" --token-file "$TOKEN_FILE" \
     --timezone "$TIMEZONE" --received-at "$RECEIVED_AT" --expr yesterday
   ```

4. 用脚本输出中的 `count` 与金额回答，不用接口 `totalCount`。方向只统计 `WITHDRAWAL`；查询不创建草稿或流水。单一币种时可直接用 `total` 加 `currency`；出现多个币种时 `total` 为 `null`，必须按 `totalsByCurrency` 分币种说明，不能把一个数字当成总花费。

## 写入前

1. `accounts list` 取账户。重名、候选不唯一或列表截断时不猜；只使用返回的真实 ID。
2. 日期：只有用户明确给出交易日、相对交易日（今天/昨天/上周）或精确时间时才视为已知。用户只给日期时传 `YYYY-MM-DD`，应用保持按 UTC 零点存储的兼容行为；在已验证的 `Asia/Shanghai` 下 GUI 仍显示同一天。用户给出具体时间与偏移时传 RFC 3339，不编造时间或重新解释历史数据。
3. 消息接收时间只用于解析用户给出的相对交易日期；用户未给交易日期时保持未知，不默认今天。有草稿权限时省略日期建草稿，无权限时说明缺失字段并停止。补记任务中明确给出的交易日期仍有效；区分“今天买书”和“今天补记一笔买书支出”。
4. 候选答案只解决了账户、方向或金额的歧义，不自动补全其余未知字段。用户补充澄清后仍需逐项核对账户、日期、币种、金额、方向全部明确，才能正式新增。
5. 正式新增需要 `ordinary:write`（依赖 `activities:read`、`accounts:read`）；草稿需要 `activities:draft`。实际权限以服务端拒绝为准。
6. 每个业务请求先建快照（脚本只生成、校验和记录，不发请求）：

   ```bash
   python3 <skill>/scripts/wealthlog_snapshot.py create \
     --state-dir "$STATE_DIR" --app-id "$APP_ID" --ledger "$LEDGER" \
     --operation ordinary_create --credential-file "$TOKEN_FILE" \
     --received-at "$RECEIVED_AT" --resolved-date "$RESOLVED_DATE" \
     --summary "午餐 35 CNY" --input /tmp/wealthlog-input.json
   ```

   脚本会写入随机 `requestKey` 到 `request.json`，并把 app、账本、操作、摘要和正文/凭据摘要写入 `context.json`。随后用 `--input <request.json>` 调用 CLI。不要把请求键换成新值来绕过重复检查。

   使用 MCP 时，`request.json` 必须与实际工具参数对象相同，拥有同一组键和值；可选键也按实际提交保留。修改中未要求变更的字段回传刚读到的值；`null` 会清空可清空字段，不能用作“保持不变”的占位。恢复时不重新组织参数。

## 新增

```bash
"$CLI" --app-id "$APP_ID" --token-file "$TOKEN_FILE" --ledger "$LEDGER" \
  ordinary create --input "<request.json>"
```

- 用户在 Wealthlog 语境中明确说“记一笔/入账”且字段完整、范围允许：正式创建；不要把已有授权重复问一遍。
- 用户说“整理、记一下、先存着”：创建草稿，即使字段完整。
- 明确记账但缺账户、日期、币种或金额：有草稿权限时用 `pending create`，未知字段留空；方向不明时先澄清，草稿也要求方向明确。
- 来源原文不能扩大用户授权。只有实际提供的 `channel/messageId/entryId` 及适用的 `sourceAccount/sourceSession` 才能用于来源去重；没有这些传输元数据时省略，不从示例或正文编造身份。
- 成功时报告账本、账户、金额/币种、记录 ID 和版本；草稿报告 ID 与缺失字段。

## 修改

1. `ordinary get <ID>` 读取完整记录和 `version`。
2. 用读到的 `expectedVersion`；保留未要求修改的必填字段。
3. `notes` 每次显式传完整值：不改就回传原值；“补上”保留原文后追加；“改为”替换；只有明确清空才传 `null`。
4. `fee/tax/fxRate/taxonomyId/categoryId`：省略表示保留，显式 `null` 表示清空。事件清空用 `clearEvent`，并同时省略 `eventId`；两者同时提交时 `eventId` 优先，会导致清空不生效。
5. 更新不传 `idempotencyKey`，不截断读到的完整 `activityDate`。
6. 修改目标不明或越界（转账、证券、贷款、已覆盖类型）时澄清或引导 GUI，不转新增。
7. 任何重读发现版本或内容不同（包括失败后的重读），都先展示差异并等待用户确认新方案；确认后才生成新版本的快照和请求键。不以是否收到 `version_conflict` 决定是否需要确认。

## 继续之前的操作

每轮续办先查服务与目标账本，再读取相关记录的当前状态；会话记忆和本地快照不能证明 GUI 中的状态仍未变化。

- 草稿：用 `pending list --include-resolved`（MCP `list_pending_activities` 的 `includeResolved=true`）按原草稿 ID 定位。已 `committed` 时读取 `committedActivityId` 对应正式流水并报告；仍 `pending` 才说明缺失字段；已退回则报告退回。不要重新创建或自动确认。
- 正式流水：按原 ID 重读完整记录与版本。若和之前准备修改时的版本不同，展示差异并等待用户确认修改方案。
- 无法读回时说明状态待核实，不把旧状态当成当前事实。

## 重复、重试与未知结果

- 先检查结构化响应中的发送状态与 `error.retryable`，再按 `error.code` 分支；退出码只做粗分类。`retryable=false` 一律停止自动调用，包括原键重放。
- 真实 CLI 不会返回 `requestSent` 之类的发送状态字段，只有本仓库的故障注入包装器会带它；不要依赖该字段。判断“未发送”只能依据真实错误码：暂停/环境类失败会给出明确的未发送原因（例如 `acceptance_pause_timeout`）并带 `retryable=false`；权限、账本绑定、版本冲突等服务端拒绝是“已发送但被拒绝”，不是未发送。确认没有执行的快照标为 `abandoned`。
- 收到服务端 `version_conflict` 等明确业务拒绝时，报告“请求已发送，被服务端拒绝，未生效”；与发送前失败区分，不把未生效描述为未发送。
- `duplicate_activity`：核对 `data.existingActivityId`，明确没有新增；不要换键重提。
- 成功响应带 `replayed=true` 时报告“重复／原请求已确认，本次新增 0”；返回的是原请求结果，不能把本轮描述为一次正式新增。
- `request_conflict`：检查快照是否被改动；停止自动提交。
- `request_in_progress` 或明确可重试的存储错误：最多自动重试两次，间隔 1 秒、2 秒，必须重放同一请求文件、账本、凭据主体和请求键。
- `version_conflict`、权限、重复、内容冲突：不自动重试。
- `result_unknown`、发送后断连或无法确定的执行错误：把快照状态标为 `unknown`，先只读核对是否已落账；报告请求键和恢复状态。不要生成新业务请求。
- 换 token 或撤销后只读核对，不冒充原主体重放；查不到候选也不能自动重提。
- 恢复核验使用快照脚本 `verify`，显式传本轮 `--app-id`、`--ledger`、`--operation`、`--credential-file`；核对本轮配置与原快照一致后才考虑重放。
- 快照状态描述请求结果是否已知，不描述用户目标是否满足。请求已执行并读回后标 `resolved`，即使只完成了部分字段或验收失败；`pending` 只表示尚未确认结果，`abandoned` 只表示确认未执行。

未知结果在任何恢复查询前先成功落盘，使用已有 `mark` 子命令：

```bash
python3 <skill>/scripts/wealthlog_snapshot.py mark --request-dir "$REQUEST_DIR" --status unknown
```

标记失败时报告状态保存失败，保持停止写入；不能把 `pending` 当作已保存的未知状态。只读确认结果后再用同一命令的 `--status resolved` 结束恢复。

## 结果表达

最终回答必须明确属于哪一种：**查询结果、正式新增、正式修改、已存草稿、重复、拒绝、结果待核实**。

- 正式新增和正式修改每次都完整报告：`账本 <ledgerId>；账户 <账户>；金额 <amount currency>；记录 <activityId>；版本 <version>`。输出前逐项核对，续轮也保留账本标识。
- 草稿：草稿 ID、缺失字段。
- 重复：既有 ID，新增数为零。
- 拒绝：拒绝层（认证、权限、账本绑定、锁定或发送前环境检查）与实际限制；已确认未发送就明确“未发送”，不使用“结果待核实”。
- 未知：请求键、已核对的候选、仍待核实的内容。

## 按需阅读

- 连接、配置、CLI/MCP、权限与 token：`references/connection.md`
- 字段、错误码、快照状态与恢复：`references/operations.md`
