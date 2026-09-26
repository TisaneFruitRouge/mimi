import { motion } from "motion/react";
import { Search, Settings, SquarePen } from "lucide-react";
import { cn } from "cn";

import { LogoMark } from "@/components/brand";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { macOverlayTitleBar, mod } from "@/lib/platform";
import { useSettings } from "@/lib/queries";

export type Section = "chat" | "connections" | "models";

const sections: { id: Section; label: string; key: string }[] = [
  { id: "chat", label: "Chat", key: "1" },
  { id: "connections", label: "Connections", key: "2" },
  { id: "models", label: "Models", key: "3" },
];

export const paletteShortcut = `${mod}K`;

/**
 * The translucent bar over every screen. Content scrolls under it; it gains its
 * material and hairline only once something is underneath (`scrolled`).
 */
export function TopBar({
  section,
  scrolled,
  onSection,
  onNewChat,
  onPalette,
  onSettings,
}: {
  section: Section;
  scrolled: boolean;
  onSection: (s: Section) => void;
  onNewChat: () => void;
  onPalette: () => void;
  onSettings: () => void;
}) {
  const name = useSettings().data?.assistant_name ?? "Hearth";
  return (
    <header
      data-tauri-drag-region
      className={cn(
        "absolute inset-x-0 top-0 z-30 grid h-[60px] grid-cols-[1fr_auto_1fr] items-center gap-4 px-4 transition-[background-color,box-shadow,backdrop-filter] duration-300",
        scrolled ? "material shadow-[inset_0_-0.5px_0_var(--separator)]" : "bg-transparent",
        macOverlayTitleBar && "pl-[84px]",
      )}
    >
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-2">
        <LogoMark />
        <span data-tauri-drag-region className="truncate text-[15px] font-semibold tracking-[-0.016em]">
          {name}
        </span>
      </div>

      <SegmentedControl value={section} onChange={onSection} />

      <div data-tauri-drag-region className="flex items-center justify-end gap-1">
        <IconButton label="New chat" shortcut={`${mod}N`} onClick={onNewChat}>
          <SquarePen />
        </IconButton>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              onClick={onPalette}
              className="pressable flex h-8 items-center gap-1.5 rounded-full bg-fill pr-3.5 pl-3 text-[13px] text-muted-foreground hover:bg-[rgb(118_118_128/0.18)] hover:text-foreground"
            >
              <Search className="size-3.5" strokeWidth={2.2} />
              Search
            </button>
          </TooltipTrigger>
          <TooltipContent>Find a conversation · {paletteShortcut}</TooltipContent>
        </Tooltip>
        <IconButton label="Settings" shortcut={`${mod},`} onClick={onSettings}>
          <Settings />
        </IconButton>
      </div>
    </header>
  );
}

/** macOS-style segmented control with a sliding selection. */
function SegmentedControl({ value, onChange }: { value: Section; onChange: (s: Section) => void }) {
  return (
    <nav aria-label="Sections" className="flex rounded-[10px] bg-fill p-[3px]">
      {sections.map((s) => {
        const active = value === s.id;
        return (
          <Tooltip key={s.id}>
            <TooltipTrigger asChild>
              <button
                onClick={() => onChange(s.id)}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "relative h-[26px] rounded-[7px] px-4 text-[13px] font-medium transition-colors duration-200",
                  active ? "text-foreground" : "text-muted-foreground hover:text-foreground",
                )}
              >
                {active && (
                  <motion.span
                    layoutId="segment"
                    className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                    transition={{ type: "spring", stiffness: 520, damping: 38 }}
                  />
                )}
                <span className="relative">{s.label}</span>
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {mod}
              {s.key}
            </TooltipContent>
          </Tooltip>
        );
      })}
    </nav>
  );
}

function IconButton({
  label,
  shortcut,
  onClick,
  children,
}: {
  label: string;
  shortcut?: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          onClick={onClick}
          aria-label={label}
          className="pressable flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-fill hover:text-foreground [&_svg]:size-[17px]"
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent>
        {label}
        {shortcut && <span className="ml-1.5 opacity-60">{shortcut}</span>}
      </TooltipContent>
    </Tooltip>
  );
}
