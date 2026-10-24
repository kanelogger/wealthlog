import { UserIcon } from "@phosphor-icons/react/dist/csr/User";
import { cn } from "@wealthfolio/ui/lib/utils";

export { DEFAULT_PROFILE_AVATAR, PROFILE_AVATARS } from "./avatar-catalog";

export function ProfileAvatar({
  className,
}: {
  id: string;
  className?: string;
  animated?: boolean;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "bg-muted text-muted-foreground relative mx-auto flex aspect-square w-20 max-w-full items-center justify-center overflow-hidden rounded-full",
        className,
      )}
    >
      <UserIcon weight="duotone" className="size-3/5" />
    </span>
  );
}
