import { Cloud, House, ShieldCheck } from "lucide-react";
import { cn } from "cn";

import type { Locality } from "@/bindings/Locality";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { localityExplanation, localityLabel } from "@/lib/format";

export const localityStyles: Record<Locality, string> = {
  device: "bg-private-soft text-private",
  network: "bg-network-soft text-network",
  cloud: "bg-cloud-soft text-cloud",
};

export const LocalityIcon = ({ locality, className }: { locality: Locality; className?: string }) => {
  const Icon = { device: ShieldCheck, network: House, cloud: Cloud }[locality];
  return <Icon className={className} />;
};

export function LocalityBadge({
  locality,
  label,
  className,
}: {
  locality: Locality;
  /** Overrides the default label. */
  label?: string;
  className?: string;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className={cn(
            "inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full px-2.5 text-xs font-medium",
            localityStyles[locality],
            className,
          )}
        >
          <LocalityIcon locality={locality} className="size-3.5" />
          {label ?? localityLabel[locality]}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-64">{localityExplanation[locality]}</TooltipContent>
    </Tooltip>
  );
}
