import { Cloud, House, ShieldCheck } from "lucide-react";
import { cn } from "cn";

import type { Locality } from "@/bindings/Locality";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { localityExplanation, localityLabel } from "@/lib/format";

const styles: Record<Locality, string> = {
  device: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
  network: "bg-sky-500/10 text-sky-700 dark:text-sky-400",
  cloud: "bg-amber-500/15 text-amber-800 dark:text-amber-400",
};

const icons: Record<Locality, typeof Cloud> = {
  device: ShieldCheck,
  network: House,
  cloud: Cloud,
};

export function LocalityBadge({
  locality,
  className,
  compact = false,
}: {
  locality: Locality;
  className?: string;
  compact?: boolean;
}) {
  const Icon = icons[locality];
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium",
            styles[locality],
            className,
          )}
        >
          <Icon className="size-3" />
          {!compact && localityLabel[locality]}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-64">{localityExplanation[locality]}</TooltipContent>
    </Tooltip>
  );
}
