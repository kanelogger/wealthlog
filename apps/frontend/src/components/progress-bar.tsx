import { cn } from "@/lib/utils";

export function ProgressBar({ progress, fillClass }: { progress: number; fillClass: string }) {
  const pct = Math.max(0, Math.min(1, progress));
  return (
    <div className="bg-muted/60 relative h-[5px] w-full overflow-hidden">
      <div className={cn("h-full", fillClass)} style={{ width: `${pct * 100}%` }} />
      <div className="pointer-events-none absolute inset-0 flex">
        {Array.from({ length: 20 }).map((_, i) => (
          <div key={i} className="border-card flex-1 border-r last:border-r-0" />
        ))}
      </div>
    </div>
  );
}
