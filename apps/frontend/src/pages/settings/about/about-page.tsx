import { Separator } from "@wealthfolio/ui/components/ui/separator";
import { SettingsHeader } from "../settings-header";
import { version } from "../../../../../../package.json";

const sourceUrl = "https://github.com/kanelogger/wealthlog";
const upstreamUrl = "https://github.com/wealthfolio/wealthfolio";

export default function AboutPage() {
  return (
    <div className="space-y-6">
      <SettingsHeader heading="关于与许可证" text="Wealthlog 的来源、版本和开源许可。" />
      <Separator />
      <div className="space-y-4 text-sm leading-6">
        <p>Wealthlog {version} · 独立维护的本地个人财务工作区。</p>
        <p>
          基于{" "}
          <a className="underline" href={upstreamUrl} target="_blank" rel="noreferrer">
            Wealthfolio
          </a>
          （基准提交 bfcd6d6202ceb4922d0be85aa5666d1802eda456）开发。Wealthfolio 是 Teymz Inc.
          的商标。Wealthlog 与上游没有隶属关系，也未获得上游背书。
        </p>
        <p>
          本项目整体按 GNU Affero General Public License version 3（AGPL-3.0）发布。
          许可不提供任何保证。你可以取得、修改和再分发相应源码，须遵守许可证条件。
        </p>
        <div className="flex flex-wrap gap-x-5 gap-y-2">
          <a className="underline" href="/legal/LICENSE.txt" target="_blank" rel="noreferrer">
            阅读完整许可证
          </a>
          <a
            className="underline"
            href="/legal/THIRD_PARTY_NOTICES.txt"
            target="_blank"
            rel="noreferrer"
          >
            第三方声明
          </a>
          <a className="underline" href={sourceUrl} target="_blank" rel="noreferrer">
            获取源码
          </a>
        </div>
      </div>
    </div>
  );
}
