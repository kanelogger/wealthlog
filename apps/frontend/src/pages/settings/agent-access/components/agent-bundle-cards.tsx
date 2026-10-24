import type { AgentBundleAction, AgentBundleTool, AgentBundleToolState } from "@/adapters";
import { useState } from "react";
import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@wealthfolio/ui/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { useAgentBundle } from "../hooks/use-agent-bundle";

const labels: Record<AgentBundleToolState, string> = {
  provided_here: "当前副本提供",
  provided_elsewhere: "其他副本提供",
  user_managed: "用户管理",
  removed: "已移除",
  installing: "安装中",
  pending_validation: "待校验",
  needs_repair: "安装记录需修复",
  state_conflict: "状态冲突",
  conflict: "归属冲突",
  failed: "失败",
  unsupported_location: "当前位置不支持",
  unsupported: "当前环境不支持",
};

const tokenInstructions = `python3 - <<'PY'
import getpass
import os
from pathlib import Path
directory = Path.home() / ".config/wealthlog-agent"
directory.mkdir(mode=0o700, parents=True, exist_ok=True)
target = directory / "token"
token = getpass.getpass("Wealthlog token (hidden): ")
if not token or any(c.isspace() for c in token) or len(token.encode()) > 4096:
    raise SystemExit("Invalid token")
fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, "w", encoding="utf-8") as handle:
    handle.write(token + "\\n")
print("Saved:", target)
PY
pbcopy < /dev/null
mkdir -p "$HOME/Wealthlog-Agent/.wealthlog-agent-state"
chmod 700 "$HOME/Wealthlog-Agent" "$HOME/Wealthlog-Agent/.wealthlog-agent-state"`;

function InstallError({ error }: { error: unknown }) {
  return (
    <div role="alert" className="text-destructive space-y-1 text-sm">
      <p>工具操作未完成。请按卡片状态检查目录权限、已有入口或安装记录，再重试。</p>
      <details>
        <summary className="cursor-pointer">查看错误详情</summary>
        <pre className="whitespace-pre-wrap break-all text-xs">{String(error)}</pre>
      </details>
    </div>
  );
}

export function AgentBundleCards() {
  const { data, error, mutation, isLoading } = useAgentBundle();
  const [guideOpen, setGuideOpen] = useState(false);
  const [preview, setPreview] = useState<AgentBundleAction | null>(null);
  const [copyResult, setCopyResult] = useState("");
  const config = data?.config ? JSON.stringify(data.config, null, 2) : null;
  const allowed = data?.supported && data.locationSupported;
  const act = (action: AgentBundleAction, tool?: AgentBundleTool) =>
    mutation.mutate({ action, tool });
  const copyConfig = async () => {
    if (!config) return;
    try {
      await navigator.clipboard.writeText(config);
      setCopyResult("配置已复制，不含令牌");
    } catch {
      setCopyResult("复制失败，请手工选择配置");
    }
  };

  return (
    <section className="space-y-4" aria-label="Agent 工具与接入指南">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">Agent 工具</h2>
          <p className="text-muted-foreground text-sm">
            工具安装与账本连接分别管理。连接账本还需启用服务、创建令牌并核对配置。
          </p>
        </div>
        <Button variant="outline" onClick={() => setGuideOpen(true)}>
          接入指南（离线）
        </Button>
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        {(["cli", "skill"] as const).map((tool) => {
          const entry = data?.[tool];
          return (
            <Card key={tool}>
              <CardHeader>
                <CardTitle className="text-base">
                  {tool === "cli" ? "Wealthlog CLI" : "Wealthlog 技能"}
                </CardTitle>
                <CardDescription role="status">
                  {entry ? labels[entry.state] : isLoading ? "读取工具状态…" : "状态读取失败"}
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                {entry?.path && <p className="break-all font-mono text-xs">{entry.path}</p>}
                {entry?.error && <InstallError error={entry.error} />}
                {allowed && entry && (
                  <div className="flex flex-wrap gap-2">
                    {entry.state === "state_conflict" ? (
                      <>
                        <Button
                          size="sm"
                          disabled={mutation.isPending}
                          onClick={() => act("restore", tool)}
                        >
                          恢复管理
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={mutation.isPending}
                          onClick={() => act("keep_removed", tool)}
                        >
                          保持移除
                        </Button>
                      </>
                    ) : (
                      <>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={
                            mutation.isPending ||
                            entry.state === "conflict" ||
                            entry.state === "needs_repair" ||
                            entry.state === "user_managed"
                          }
                          onClick={() => act("reinstall", tool)}
                        >
                          重新安装
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={
                            mutation.isPending ||
                            entry.state === "removed" ||
                            entry.state === "user_managed"
                          }
                          onClick={() => act("remove", tool)}
                        >
                          移除
                        </Button>
                      </>
                    )}
                  </div>
                )}
              </CardContent>
            </Card>
          );
        })}
      </div>
      {(data?.cli.state === "user_managed" || data?.skill.state === "user_managed") && (
        <p role="alert" className="text-sm">
          安装记录需修复。已有文件、目录或外部链接由用户管理；请先核对并自行移走冲突对象，再修复记录。应用不会覆盖或删除这些对象。
        </p>
      )}
      {data?.providerApp && (
        <p className="text-muted-foreground break-all text-xs">
          工具来源：{data.providerApp} · {data.version}
        </p>
      )}
      {(error || data?.error || mutation.error) && (
        <InstallError error={mutation.error ?? error ?? data?.error} />
      )}
      {allowed && (
        <div className="flex flex-wrap gap-2">
          {data.canRepair && (
            <Button
              variant="outline"
              disabled={mutation.isPending}
              onClick={() => setPreview("repair")}
            >
              修复安装记录
            </Button>
          )}
          {data.canUseCopy && (
            <Button
              variant="outline"
              disabled={mutation.isPending}
              onClick={() => setPreview("use_copy")}
            >
              使用此副本
            </Button>
          )}
        </div>
      )}
      <Dialog
        open={preview !== null}
        onOpenChange={(open) => {
          if (!open) setPreview(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{preview === "use_copy" ? "使用此副本" : "修复安装记录"}</DialogTitle>
            <DialogDescription>
              {preview === "use_copy"
                ? "将当前运行的应用设为工具来源。跨路径的低版本副本不能接管。"
                : "重新核对并认领有效的受管链接。缺失入口保持停用；current 缺失时使用当前符合安装条件的副本。"}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2 break-all text-xs">
            <p>已有工具来源：{data?.providerApp ?? "无有效记录，请以实际受管链接为准"}</p>
            <p>将使用的应用：{preview === "use_copy" ? data?.currentApp : data?.repairApp}</p>
            <p>CLI：{data?.cli.path}</p>
            <p>技能：{data?.skill.path}</p>
          </div>
          <Button
            disabled={mutation.isPending}
            onClick={() => {
              if (preview)
                mutation.mutate({ action: preview }, { onSuccess: () => setPreview(null) });
            }}
          >
            确认{preview === "use_copy" ? "使用此副本" : "修复"}
          </Button>
        </DialogContent>
      </Dialog>
      <Dialog open={guideOpen} onOpenChange={setGuideOpen}>
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>连接 Wealthlog 与 Agent</DialogTitle>
            <DialogDescription>指南离线可读；工具已安装仍需完成账本授权。</DialogDescription>
          </DialogHeader>
          <ol className="list-decimal space-y-3 pl-5 text-sm">
            <li>
              将正式 macOS 应用放到 /Applications 或
              ~/Applications，首次启动后自动安装工具。开发实例、下载目录和磁盘映像不会安装。
            </li>
            <li>
              准备 Python 3.9+ 和 IANA 时区数据。macOS 的 python3
              可能提示安装开发者工具；先自行完成安装。
            </li>
            <li>
              打开目标账本，在本页启用并启动本地服务。新建专用令牌：查询需要
              accounts:read、activities:read；普通收支写入另需 ordinary:write；草稿另需
              activities:draft。
            </li>
            <li>
              令牌只显示一次。用终端的无回显输入保存到 tokenFile；文件必须属于当前用户、权限为 0400
              或 0600，且不能是符号链接。只将文件路径交给 Agent。
            </li>
            <li>
              在终端执行完整 CLI 路径加{" "}
              <code>--app-id local.kanelogger.wealthlog service status</code>。确认 data.state 为
              ready，核对当前账本名称和 ledgerId。
            </li>
            <li>
              将配置保存为 ~/.config/wealthlog-agent/config.json。ledgerId
              必须手工核对填写；已有配置先核对绑定。创建配置中的 stateDir 并设为 0700，在 Agent
              中打开对应的 Wealthlog-Agent 工作文件夹。
            </li>
            <li>
              重启
              Agent，在新会话询问“查一下昨天花了多少钱，按币种分别汇总”。首次只做查询，核对账本、账户和日期后再启用写入。
            </li>
          </ol>
          <details className="space-y-2 text-sm">
            <summary className="cursor-pointer">在自己的终端保存令牌和准备工作文件夹</summary>
            <p>输入不会显示。令牌文件已存在时命令停止，请先核对已有绑定。保存后清空剪贴板。</p>
            <pre className="bg-muted overflow-x-auto rounded-md p-3 text-xs">
              {tokenInstructions}
            </pre>
          </details>
          {config ? (
            <div className="space-y-2">
              <pre className="bg-muted overflow-x-auto rounded-md p-3 text-xs">{config}</pre>
              <Button variant="outline" onClick={() => void copyConfig()}>
                复制配置（不含令牌）
              </Button>
              <p role="status" className="text-muted-foreground text-xs">
                {copyResult}
              </p>
            </div>
          ) : (
            <p className="text-sm">请在 macOS 桌面应用中查看填有完整路径的配置示例。</p>
          )}
          <p className="text-muted-foreground text-sm">
            第三方工具删除入口也会视为移除，可分别重新安装。移除技能或 CLI
            不会撤销令牌；停用访问还需在本页移除对应令牌。已有外部文件或目录不会被覆盖。
          </p>
        </DialogContent>
      </Dialog>
    </section>
  );
}
