# Agent 操作验收运行手册

用例见 [cases.md](cases.md)，接入说明见 [docs/agent-integration.md](../../docs/agent-integration.md)。本目录只放验收程序和脱敏证据；token、快照和账本数据不提交。

## 0. 准备

```bash
cargo build -p wealthlog-cli
/usr/bin/python3 -m unittest discover -s e2e/agent/tests -v
```

启动开发实例并保持它运行：

```bash
pnpm tauri dev
```

在 GUI 中创建合成 A/B 账本、启用 Agent Access，并按 [接入说明](../../docs/agent-integration.md) 创建凭据矩阵。令牌用 `credential_store.py` 保存：

```bash
/usr/bin/python3 e2e/agent/credential_store.py save \
  --path "$HOME/.config/wealthlog-agent/tokens/write"
```

配置文件只保存路径：

```json
{
  "cli": "/absolute/path/to/target/debug/wealthlog",
  "appId": "local.kanelogger.wealthlog.dev",
  "ledgerId": "<A ledger UUID>",
  "tokenFile": "/Users/you/.config/wealthlog-agent/tokens/write",
  "timezone": "Asia/Shanghai",
  "stateDir": "/absolute/path/to/workspace/.wealthlog-agent-state"
}
```

## 1. 预检

```bash
/usr/bin/python3 e2e/agent/preflight.py --config <config> --workspace <temp-git-workspace> --out <evidence>/preflight.json
```

只有 `state=ready`、账本匹配、token 可读、MCP 工具列表完整时才进入下一步。失败时记录具体层，不换账本或关闭身份检查。

## 2. 资源清单与预置数据

```bash
/usr/bin/python3 e2e/agent/resource_inventory.py init \
  --path <runtime>/inventory.json --run-id <runId> \
  --app-id local.kanelogger.wealthlog.dev \
  --ledger-a <A> --ledger-b <B>

/usr/bin/python3 e2e/agent/seed.py --config <preseed-config> \
  --account-id <cash-account-id> --day <D-1> --out <runtime>/seed.json
```

`seed.py` 只调用真实的 `ordinary create`，不写数据库。登记生成的 ID 和请求键。

## 3. 运行一个用例

首轮：

```bash
/usr/bin/python3 e2e/agent/session_driver.py run \
  --workspace <temp-git-workspace> \
  --agent-config <config> \
  --prompt-file <prompt.txt> \
  --out <evidence>/A01.jsonl \
  --sandbox workspace-write \
  -c 'mcp_servers.wealthlog.url="http://127.0.0.1:<port>/mcp"' \
  -c 'mcp_servers.wealthlog.bearer_token_env_var="WEALTHLOG_MCP_TOKEN"' \
  -c 'mcp_servers.wealthlog.http_headers."X-WF-Profile-Id"="<ledger>"'
```

续跑使用明确会话 ID，不使用 `--last`：

```bash
/usr/bin/python3 e2e/agent/session_driver.py run \
  --workspace <temp-git-workspace> --session <session-id> \
  --agent-config <config> --prompt-file <next-prompt.txt> \
  --out <evidence>/A05-round2.jsonl
```

每轮重新应用同配置。MCP 端口变化后重新读取 `mcp.lock` 并初始化。

## 4. 读写核对

```bash
# 用例前
/usr/bin/python3 e2e/agent/verify.py snapshot --config <config> --out <runtime>/A02-before.json

# 用例后
/usr/bin/python3 e2e/agent/verify.py snapshot --config <config> --out <runtime>/A02-after.json
/usr/bin/python3 e2e/agent/verify.py check --before <before> --after <after> \
  --expect-new-activities 1 --expect-new-pending 0

# 请求与审计（只读打开 profile 数据库）
/usr/bin/python3 e2e/agent/verify.py audit --db <profile.db> \
  --since <receivedAt> --request-key <key> --out <evidence>/A02-audit.json
```

`verify.py` 只读；不要用它写入或修复数据。

## 5. 故障适配

A12：

```bash
/usr/bin/python3 e2e/agent/fault_cli.py \
  --real-cli target/debug/wealthlog --state <runtime>/fault \
  --mode unknown --match-request-key <key> -- "$@"
```

A10：

```bash
/usr/bin/python3 e2e/agent/fault_cli.py \
  --real-cli target/debug/wealthlog --state <runtime>/fault \
  --mode pause --match-request-key <key> --timeout 10 -- "$@"
```

A10 仅在同配置超时预检及自动 GUI 预演通过后运行。暂停最多 10 秒；存在最短硬超时时还需小于其三分之一。等到 `pause.waiting` 后，自动 GUI 完成修改并核对版本，再写 `release` 放行；人工操作放在两轮之间。包装器只用于验收，报告必须披露。

MCP A12 使用回环代理，保持原认证、账本和会话头：

```bash
/usr/bin/python3 e2e/agent/fault_mcp.py \
  --upstream http://127.0.0.1:<discovered-port>/mcp \
  --state <private-runtime>/mcp-fault --match-request-key <key>
```

将输出 URL 传给 `session_driver.py run --mcp-url <proxy-url>`。驱动从私有文件读取 PAT，仅注入 Codex 主进程；每轮使用 `shell_environment_policy.filters` 排除两个 token 变量，MCP 验收关闭本轮 shell 环境快照，并仅为七个已授权测试工具设置审批。固定账本头以完整 TOML 映射传入，续跑时同样应用。驱动同时从配置设置 `WEALTHLOG_AGENT_STATE_DIR`，指向工作区内私有目录。

省略 `--match-request-key` 仅适用于专属、单次写入会话。代理完整接收成功响应后写入私有 `hidden-response.json` 和 `unknown.fired`，随后断开客户端；业务错误和其余请求透传。实测加 `--verify-db <synthetic-profile.db>`，由独立只读程序核对提交后才隐藏响应；控制目录不提供给被测 Agent。CLI A12 同样支持 `--verify-db`；`--match-next-create` 仅用于专属单次写入会话。

## 6. 会话读取检查

```bash
/usr/bin/python3 e2e/agent/session_driver.py inspect \
  --events <evidence>/A01.jsonl \
  --sentinel "$PWD/e2e/agent/cases.md" --sentinel "$PWD/crates" --sentinel "$PWD/db"
```

输出 `observed` / `not_observed` / `indeterminate`。`not_observed` 仅覆盖完整工具事件中的字面路径；缺少轮次结束、工具未完成、事件畸形或失败时记为 `indeterminate`。间接路径、工具外读取和丢失的会话页仍无法判定，不能据此宣称隔离。

## 7. 收尾

1. 结束被测会话和会写入的进程。
2. 在 GUI 撤销全部测试凭据并验证失效。
3. 先自测扫描器，再扫描全部交付物。下列 `--secret` 参数须按本轮资源清单逐项提供，包含预置、负例和重跑新建凭据；原始文件必须保留到最终扫描通过。

```bash
/usr/bin/python3 e2e/agent/deliverable_scan.py --self-test
/usr/bin/python3 e2e/agent/deliverable_scan.py \
  --root skills/wealthlog --root docs/agent-integration.md --root e2e/agent \
  --root docs/verification --root docs/agent-operation-skill-plan.md \
  --root tasks \
  --secret "<credential-label>=<private-token-file>" --out <evidence>/scan.json
```

扫描包含命名为 `dist`、`target` 等目录中的附件，也检查 JSON/JSONL 及其嵌套文本字段。缺失根目录、不可读或过大文件、符号链接和特殊文件不能判通过；仅资源清单内显式注册的凭据文件可用 `--allow-file` 排除。输出记录扫描根目录、数量和命中位置，不包含秘密正文。

4. 检索并清理测试记忆中的账本 UUID、包装器路径、开发 app ID、快照目录；只删除会误导正式使用的测试内容。
5. 失败样本与成功样本都保留到脱敏归档完成；快照状态反映实际请求结果，验收失败单独记入报告。已确认落账的请求保留 `resolved`，不能改为表示确定未执行的 `abandoned`。
6. 按清单删除本轮合成账本、包装器、快照、日志和临时文件；保留脱敏报告。失败样本随其所属合成账本统一清理，不为复验提前删除。

## 退出条件

- 阶段 0–4 的退出条件全部有证据；
- CLI A01–A15 有结果，MCP 必测子集有结果；
- 阻断项关闭；未覆盖项写清原因；
- 全部测试凭据撤销并验证；
- 交付物扫描通过，报告脱敏。
