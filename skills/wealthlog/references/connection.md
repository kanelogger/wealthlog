# 连接与权限

## 配置

`WEALTHLOG_AGENT_CONFIG` 指向一个 JSON 文件；未设置时使用 `~/.config/wealthlog-agent/config.json`。

```json
{
  "cli": "/Users/your-user/.local/bin/wealthlog",
  "appId": "local.kanelogger.wealthlog",
  "ledgerId": "00000000-0000-0000-0000-000000000000",
  "tokenFile": "/private/path/to/token",
  "timezone": "Asia/Shanghai",
  "stateDir": "/absolute/path/to/workspace/.wealthlog-agent-state"
}
```

- `appId` 决定发现文件：正式实例与开发实例使用不同标识。CLI 的默认值是正式实例，所以每次调用都显式传 `--app-id`。
- `ledgerId` 是当前目标账本。写入必须显式传 `--ledger`；查询也传，避免跟随 GUI 当前账本漂移。
- `tokenFile` 只保存一个 PAT。CLI 拒绝符号链接、非当前用户文件、非 `0400/0600` 权限、空文件、超过 4096 字节和多行内容；允许一个结尾换行。
- `stateDir` 是当前工作区内的可写私有目录。把 `WEALTHLOG_AGENT_STATE_DIR` 显式设为该路径；快照脚本核对两者一致，按 `0700`/`0600` 创建，工作区外目录会被拒绝。
- `timezone` 是 IANA 名称。首版只承诺 `Asia/Shanghai` 的端到端日期行为。

## 随应用交付

正式 macOS 应用首次从 `/Applications` 或 `~/Applications` 启动时，默认通过 `~/.local/share/wealthlog/current` 提供 CLI 和完整技能。用户移除入口后保持停用，可在应用的「AI 代理访问」页分别重新安装；移除入口不撤销令牌。安装器不创建连接配置，也不授权账本。配置字段仍须按上节核对，不从工具已安装推断账本已连接。

包内脚本需要 Python 3.9+ 和 IANA 时区数据。macOS 的 `python3` 可能触发安装提示，缺依赖时由用户准备后继续。包内技能只读使用；请求和快照仅写配置指定的 `stateDir`。

## 服务状态

```bash
"$CLI" --app-id "$APP_ID" service status
```

输出 `data.state` 为 `ready`、`locked` 或 `unavailable`。`ready` 仍不证明 token 有效；首次业务调用才能证明认证与权限。

服务未启动、发现文件过期、PID/UID 不匹配或 socket 不可用时，报告问题并请用户启动或重启 GUI。不要自动启动另一个实例，不要切换到正式实例。

## 凭据与权限

PAT 由 Wealthlog GUI 的 Agent Access 页面创建，秘密只显示一次。保存到 `tokenFile` 时使用无回显输入，不经过命令参数、聊天或截图。

常见权限：

- 查询：`accounts:read`、`activities:read`。
- 草稿：`activities:draft`。
- 普通收支正式写入：`ordinary:write`，并依赖 `activities:read`、`accounts:read`。
- `activities:write` 是另一类草稿提交权限，不能替代 `ordinary:write`。

实际权限以服务端返回为准；不要通过额外的写入探测权限。

## MCP

MCP 使用目标应用实例发布的回环地址、PAT 和固定的 `X-WF-Profile-Id` 头：

- URL 从发现文件或 GUI 的 Agent Access 页面取得，不假设端口固定。
- 每个请求都带 `Authorization: Bearer <PAT>` 与 `X-WF-Profile-Id: <ledgerId>`。
- 正式写入必须带头；缺失返回 `profile_required`，不匹配返回 `profile_mismatch`。
- 会话撤销后重建连接，不生成新的业务请求。
- 不要把 PAT 写入 MCP 配置文件；用仅注入被测进程的环境变量或客户端凭据存储。

## 账本切换

GUI 切换账本后，旧连接和旧目标都可能失效。CLI 的本地预检在目标账本不是活动账本时返回 `profile_mismatch`（退出码 2），不会跟随 GUI 切换。MCP 由服务端按 `X-WF-Profile-Id` 校验。两种情况都停止并请用户确认目标，不换账本重试。

受密码保护的账本锁定后，CLI 返回 `profile_locked`（退出码 6），MCP 会话可能被撤销。请用户在 GUI 解锁，再重新核对账本。
