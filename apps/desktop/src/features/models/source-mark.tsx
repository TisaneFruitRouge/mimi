import { House, Monitor, Server } from "lucide-react";
import { cn } from "cn";

import type { Provider } from "@/bindings/Provider";
import type { ProviderPreset } from "@/bindings/ProviderPreset";
import { IconTile } from "@/components/page";

/**
 * Cloud services get a lettered tile in a tint of their own colour, so they can be told
 * apart at a glance; things that run on the user's machines keep the locality icon.
 */
const marks: Record<string, { letter: string; tone: string }> = {
  anthropic: { letter: "A", tone: "bg-[#f6e7de] text-[#b4532f]" },
  openai: { letter: "O", tone: "bg-[#e8ece9] text-[#1f2a24]" },
  mistral: { letter: "M", tone: "bg-[#fdebd9] text-[#d4610f]" },
  openrouter: { letter: "R", tone: "bg-[#e6e8fb] text-[#4b52c8]" },
};

/** The preset a stored source was made from: by kind for Anthropic (there's one), else by address. */
export function presetOf(provider: Provider, presets: ProviderPreset[]): ProviderPreset | undefined {
  const bare = (url: string) => url.replace(/\/$/, "");
  return presets.find(
    (p) =>
      p.kind === provider.kind &&
      (p.kind === "anthropic" || bare(p.base_url) === bare(provider.base_url)),
  );
}

export function SourceMark({
  presetId,
  locality,
  size = "md",
  className,
}: {
  presetId?: string | null;
  locality: "device" | "network" | "cloud";
  size?: "sm" | "md" | "lg";
  className?: string;
}) {
  const mark = presetId ? marks[presetId] : undefined;
  if (mark) {
    return (
      <IconTile size={size} className={cn(mark.tone, "font-semibold tracking-[-0.02em]", className)}>
        <span className={size === "lg" ? "text-[22px]" : size === "sm" ? "text-[13px]" : "text-[16px]"}>
          {mark.letter}
        </span>
      </IconTile>
    );
  }
  const Icon = locality === "device" ? Monitor : locality === "network" ? House : Server;
  const tone =
    locality === "device"
      ? "bg-private-soft text-private"
      : locality === "network"
        ? "bg-network-soft text-network"
        : "bg-cloud-soft text-cloud";
  return (
    <IconTile size={size} className={cn(tone, className)}>
      <Icon />
    </IconTile>
  );
}
