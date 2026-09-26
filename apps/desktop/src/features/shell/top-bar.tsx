import { Search, Settings2 } from "lucide-react";
import { cn } from "cn";

import { LogoMark } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { useSettings } from "@/lib/queries";

/** Top-bar sections, plus pages reached from elsewhere (Memory, from Settings). */
export type Section = "chat" | "connections" | "models" | "memory";

const sections: { id: Section; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "connections", label: "Connections" },
  { id: "models", label: "Models" },
];

const isMac = navigator.platform.toLowerCase().includes("mac");
export const paletteShortcut = isMac ? "⌘K" : "Ctrl K";

export function TopBar({
  section,
  onSection,
  onPalette,
  onSettings,
}: {
  section: Section;
  onSection: (s: Section) => void;
  onPalette: () => void;
  onSettings: () => void;
}) {
  const name = useSettings().data?.assistant_name ?? "Hearth";
  return (
    <header className="grid h-16 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-4 px-5">
      <div className="flex items-center gap-2.5">
        <LogoMark />
        <span className="text-[15px] font-semibold tracking-tight lowercase">{name}</span>
      </div>

      <nav
        aria-label="Sections"
        className="flex gap-0.5 rounded-xl border bg-background p-1 shadow-xs"
      >
        {sections.map((s) => (
          <button
            key={s.id}
            onClick={() => onSection(s.id)}
            aria-current={section === s.id ? "page" : undefined}
            className={cn(
              "h-8 rounded-lg px-4 text-[13.5px] font-medium transition-colors",
              section === s.id
                ? "bg-foreground text-background"
                : "text-muted-foreground hover:bg-subtle hover:text-foreground",
            )}
          >
            {s.label}
          </button>
        ))}
      </nav>

      <div className="flex items-center justify-end gap-2">
        <button
          onClick={onPalette}
          className="flex h-9 items-center gap-2.5 rounded-lg border bg-background pr-2 pl-3 text-[13px] text-muted-foreground shadow-xs transition-colors hover:text-foreground"
        >
          <Search className="size-4" />
          Conversations
          <kbd className="rounded-md bg-subtle px-1.5 py-0.5 font-mono text-[11px] text-faint">
            {paletteShortcut}
          </kbd>
        </button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onSettings}
          aria-label="Settings"
          className="text-muted-foreground"
        >
          <Settings2 />
        </Button>
      </div>
    </header>
  );
}
