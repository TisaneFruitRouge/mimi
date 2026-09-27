import { useEffect, useState } from "react";
import { BookOpen, ChevronRight, ExternalLink, Loader2, Lock, LogOut, Power, RotateCcw } from "lucide-react";
import { toast } from "sonner";

import { LocalityBadge } from "@/components/locality-badge";
import { Grouped, IconTile, Row } from "@/components/page";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/api";
import { mod } from "@/lib/platform";
import { useProviders, useSettings } from "@/lib/queries";
import { Switch } from "@/components/ui/switch";
import {
  type BackgroundStatus,
  backgroundStatus,
  isTauri,
  openInBrowser,
  request,
  setBackground,
} from "@/lib/transport";

const shortcuts = [
  ["New chat", `${mod}N`],
  ["Search conversations", `${mod}K`],
  ["Chat, Connections, Models", `${mod}1 – 3`],
  ["Settings", `${mod},`],
];

export function SettingsDialog({
  open,
  onOpenChange,
  onOpenMemory,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenMemory: () => void;
}) {
  const settings = useSettings().data;
  const providers = useProviders().data ?? [];
  const [name, setName] = useState<string | null>(null);
  const value = name ?? settings?.assistant_name ?? "";

  const saveName = async () => {
    if (!settings || name === null || !value.trim()) return;
    try {
      await api.putSettings({ ...settings, assistant_name: value });
      setName(null);
      toast.success("Saved");
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[85vh] gap-0 overflow-hidden bg-canvas p-0 sm:max-w-[520px]"
        onOpenAutoFocus={(e) => e.preventDefault()}
      >
        <DialogHeader className="px-6 pt-6 pb-4">
          <DialogTitle className="type-title">Settings</DialogTitle>
          <DialogDescription>Your assistant and your privacy.</DialogDescription>
        </DialogHeader>

        <div className="flex max-h-[calc(85vh-96px)] flex-col gap-6 overflow-y-auto px-6 pb-6">
          <Group title="Assistant">
            <form
              className="flex items-center gap-3 px-4 py-3"
              onSubmit={(e) => {
                e.preventDefault();
                saveName();
              }}
            >
              <label htmlFor="assistant-name" className="w-20 shrink-0 type-body">
                Name
              </label>
              <Input
                id="assistant-name"
                value={value}
                onChange={(e) => setName(e.target.value)}
                onBlur={saveName}
                className="h-9"
              />
              {name !== null && (
                <Button type="submit" size="sm" disabled={!value.trim()}>
                  Save
                </Button>
              )}
            </form>
            <Row
              icon={
                <IconTile size="sm" className="bg-fill text-muted-foreground">
                  <RotateCcw />
                </IconTile>
              }
              title="Show the welcome again"
              detail="Choose a model, connect apps and introduce yourself"
              trailing={<ChevronRight className="size-4 text-faint" />}
              onClick={() => {
                if (!settings) return;
                onOpenChange(false);
                api
                  .putSettings({ ...settings, onboarding_done: false, onboarding_step: 0 })
                  .catch((e) => toast.error((e as Error).message));
              }}
            />
          </Group>

          <Group title="Memory">
            <Row
              icon={
                <IconTile size="sm" className="bg-lime-soft text-lime-deep">
                  <BookOpen />
                </IconTile>
              }
              title="What your assistant remembers"
              detail="See, change or forget it"
              trailing={<ChevronRight className="size-4 text-faint" />}
              onClick={onOpenMemory}
            />
          </Group>

          {isTauri && <BackgroundGroup open={open} />}

          <Group title="Privacy">
            <Row
              icon={
                <IconTile size="sm" className="bg-private-soft text-private">
                  <Lock />
                </IconTile>
              }
              title="Stored on this computer"
              detail="Conversations, settings and passwords are encrypted, and nowhere else."
              className="[&_.truncate]:whitespace-normal"
            />
          </Group>

          {providers.length > 0 && (
            <Group title="Where your messages go">
              {providers.map((p) => (
                <Row key={p.id} title={p.name} trailing={<LocalityBadge locality={p.locality} />} className="min-h-[52px]" />
              ))}
            </Group>
          )}

          <Group title="Keyboard shortcuts">
            {shortcuts.map(([label, keys]) => (
              <div key={label} className="flex h-11 items-center justify-between px-4 type-callout">
                <span>{label}</span>
                <kbd className="font-sans type-subhead text-muted-foreground">{keys}</kbd>
              </div>
            ))}
          </Group>

          {isTauri ? (
            <Button
              variant="outline"
              className="self-start"
              onClick={() => openInBrowser().catch((e) => toast.error(e?.message ?? String(e)))}
            >
              <ExternalLink /> Open in your browser
            </Button>
          ) : (
            <Button
              variant="outline"
              className="self-start"
              onClick={() => request("POST", "/web/logout").finally(() => location.reload())}
            >
              <LogOut /> Sign out of this browser
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** Desktop only: keep the assistant running (and starting at login) with no window. */
function BackgroundGroup({ open }: { open: boolean }) {
  const [status, setStatus] = useState<BackgroundStatus | null>(null);
  const [busy, setBusy] = useState(false);

  // Re-read each time Settings opens: the service can change outside the app.
  useEffect(() => {
    if (open) backgroundStatus().then(setStatus).catch(() => setStatus(null));
  }, [open]);

  const toggle = async (enabled: boolean) => {
    setBusy(true);
    try {
      setStatus(await setBackground(enabled));
      toast.success(enabled ? "Mimi will keep running in the background" : "Mimi now runs only while it's open");
    } catch (e) {
      toast.error((e as Error).message);
      backgroundStatus().then(setStatus).catch(() => {});
    } finally {
      setBusy(false);
    }
  };

  const detail = !status
    ? "Checking…"
    : !status.available
      ? (status.unavailable_reason ?? "Not available on this computer.")
      : status.enabled
        ? status.no_restart
          ? "Starts when you log in, so Telegram and reminders work with this window closed."
          : "Starts when you log in and keeps going with this window closed, so Telegram and reminders always work."
        : "Mimi only works while this window is open.";

  return (
    <Group title="Background">
      <Row
        icon={
          <IconTile size="sm" className="bg-private-soft text-private">
            {busy ? <Loader2 className="animate-spin" /> : <Power />}
          </IconTile>
        }
        title="Keep Mimi running in the background"
        detail={detail}
        className="[&_.truncate]:whitespace-normal"
        trailing={
          <Switch
            checked={!!status?.enabled}
            disabled={!status?.available || busy}
            onCheckedChange={toggle}
            aria-label="Keep Mimi running in the background"
          />
        }
      />
    </Group>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="section-label">{title}</h3>
      <Grouped>{children}</Grouped>
    </section>
  );
}
