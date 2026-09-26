import { AtSign, Hash, Mail, MessageCircle, Phone, Send, ShieldCheck } from "lucide-react";
import { cn } from "cn";

import type { Channel } from "@/bindings/Channel";

/** How each way to reach someone is named and drawn. */
export const channelInfo: Record<Channel, { label: string; icon: typeof Phone; tone: string }> = {
  phone: { label: "Phone", icon: Phone, tone: "text-[#2f7a4f]" },
  email: { label: "Email", icon: Mail, tone: "text-[#6146ad]" },
  telegram: { label: "Telegram", icon: Send, tone: "text-[#136c86]" },
  signal: { label: "Signal", icon: ShieldCheck, tone: "text-[#3353a8]" },
  whatsapp: { label: "WhatsApp", icon: MessageCircle, tone: "text-[#1d7a3a]" },
  matrix: { label: "Matrix", icon: Hash, tone: "text-foreground" },
  other: { label: "Other", icon: AtSign, tone: "text-muted-foreground" },
};

export function ChannelIcon({ channel, className }: { channel: Channel; className?: string }) {
  const { icon: Icon, tone, label } = channelInfo[channel];
  return <Icon className={cn("size-3.5", tone, className)} aria-label={label} />;
}

/** A row of small channel icons, e.g. in a picker. */
export function ChannelIcons({ channels, className }: { channels: Channel[]; className?: string }) {
  if (channels.length === 0) return null;
  return (
    <span className={cn("inline-flex items-center gap-1", className)} title={channels.map((c) => channelInfo[c].label).join(", ")}>
      {channels.map((c) => (
        <ChannelIcon key={c} channel={c} className="size-3" />
      ))}
    </span>
  );
}

const avatarTones = [
  "bg-[#e5eefc] text-[#1f64c7]",
  "bg-[#e1f4ec] text-[#0b7a5c]",
  "bg-[#fdf0dc] text-[#9a5600]",
  "bg-[#efe9fb] text-[#6146ad]",
  "bg-[#fbe8ec] text-[#a83250]",
  "bg-[#def3f7] text-[#136c86]",
];

function hash(s: string) {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) | 0;
  return Math.abs(h);
}

export function initials(name: string) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const letters = words.length > 1 ? [words[0], words[words.length - 1]] : words;
  return letters.map((w) => [...w][0]?.toUpperCase() ?? "").join("") || "?";
}

/** Initials in a colour that stays the same for the same person. */
export function PersonAvatar({ id, name, size = "md" }: { id: string; name: string; size?: "sm" | "md" | "lg" }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex shrink-0 items-center justify-center rounded-full font-medium select-none",
        avatarTones[hash(id) % avatarTones.length],
        size === "sm" && "size-6 text-[10.5px]",
        size === "md" && "size-8 text-[12px]",
        size === "lg" && "size-12 text-[16px]",
      )}
    >
      {initials(name)}
    </span>
  );
}
