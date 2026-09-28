import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { Minus, Search, Settings, Square, SquarePen, X } from "lucide-react";
import { cn } from "cn";

import { LogoMark } from "@/components/brand";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { macOverlayTitleBar, mod } from "@/lib/platform";
import { useSettings } from "@/lib/queries";
import { useScrolled } from "@/lib/scroll-edge";
import { windowAction, windowChrome } from "@/lib/transport";

/** The panels used every day, in the top bar. */
export type Tab = "chat" | "calendar" | "mail" | "people";
/** Everything set up once, in the Settings window's sidebar. */
export type SettingsPage =
  | "general"
  | "personality"
  | "connections"
  | "models"
  | "memory"
  | "notifications"
  | "permissions"
  | "privacy";
/** Anywhere the app can go. */
export type Section = Tab | SettingsPage;

export const tabs: { id: Tab; label: string; key: string }[] = [
  { id: "chat", label: "Chat", key: "1" },
  { id: "calendar", label: "Calendar", key: "2" },
  { id: "mail", label: "Mail", key: "3" },
  { id: "people", label: "People", key: "4" },
];

export const isTab = (s: Section): s is Tab => tabs.some((t) => t.id === s);

export const paletteShortcut = `${mod}K`;

/**
 * The translucent bar over every screen. Content scrolls under it; it gains its
 * material and hairline only once something is underneath (see `BarMaterial`).
 */
export function TopBar({
  section,
  onSection,
  onNewChat,
  onPalette,
  onSettings,
}: {
  section: Section;
  onSection: (s: Section) => void;
  onNewChat: () => void;
  onPalette: () => void;
  onSettings: () => void;
}) {
  const name = useSettings().data?.assistant_name ?? "Mimi";
  return (
    <header
      data-tauri-drag-region
      className={cn(
        "absolute inset-x-0 top-0 z-30 grid h-[60px] grid-cols-[1fr_auto_1fr] items-center gap-4 px-4",
        macOverlayTitleBar && "pl-[84px]",
      )}
    >
      <BarMaterial />
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-2">
        <LogoMark />
        <span data-tauri-drag-region className="truncate text-[15px] font-semibold tracking-[-0.016em]">
          {name}
        </span>
      </div>

      <SegmentedControl value={isTab(section) ? section : null} onChange={onSection} />

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
        <IconButton label="Settings" shortcut={`${mod},`} onClick={onSettings} active={!isTab(section)}>
          <Settings />
        </IconButton>
        <WindowControls />
      </div>
    </header>
  );
}

/**
 * The bar's material and hairline, behind its content, shown once the view under the
 * bar is scrolled. The blur stays set and only the layer's opacity fades (see
 * `bar-material` in index.css): transitioning `backdrop-filter`, background and shadow
 * restyled and repainted the bar every frame, and adding the filter made a new layer
 * just as scrolling started. Its own component, reading the scroll edge from a store,
 * so crossing the edge re-renders this element and not the whole shell.
 */
export function BarMaterial() {
  const scrolled = useScrolled();
  return <div aria-hidden data-scrolled={scrolled || undefined} className="bar-material" />;
}

/**
 * Minimize, maximize and close, for Linux desktops where the app draws no system
 * title bar and the window manager doesn't provide them (not under tiling WMs).
 */
function WindowControls() {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    windowChrome().then((c) => setShown(c.controls));
  }, []);
  if (!shown) return null;
  const button = (action: "minimize" | "maximize" | "close", label: string, icon: React.ReactNode) => (
    <button
      onClick={() => windowAction(action)}
      aria-label={label}
      className="pressable flex size-6 items-center justify-center rounded-full bg-fill text-muted-foreground hover:bg-[rgb(118_118_128/0.2)] hover:text-foreground [&_svg]:size-3"
    >
      {icon}
    </button>
  );
  return (
    <div className="ml-2 flex items-center gap-2">
      {button("minimize", "Minimize", <Minus strokeWidth={2.4} />)}
      {button("maximize", "Maximize", <Square strokeWidth={2.4} />)}
      {button("close", "Close", <X strokeWidth={2.4} />)}
    </div>
  );
}

/** macOS-style segmented control with a sliding selection. */
function SegmentedControl({ value, onChange }: { value: Tab | null; onChange: (s: Tab) => void }) {
  return (
    <nav aria-label="Sections" className="flex rounded-[10px] bg-fill p-[3px]">
      {tabs.map((s) => {
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
  active,
  children,
}: {
  label: string;
  shortcut?: string;
  onClick: () => void;
  active?: boolean;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          onClick={onClick}
          aria-label={label}
          aria-current={active ? "page" : undefined}
          className={cn(
            "pressable flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-fill hover:text-foreground [&_svg]:size-[17px]",
            active && "bg-fill text-foreground",
          )}
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
