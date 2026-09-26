import { useState } from "react";
import { BookOpen, ChevronRight, ExternalLink, Lock, LogOut } from "lucide-react";
import { toast } from "sonner";

import { LocalityBadge } from "@/components/locality-badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api } from "@/lib/api";
import { useProviders, useSettings } from "@/lib/queries";
import { isTauri, openInBrowser, request } from "@/lib/transport";

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
    if (!settings || name === null) return;
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
      <DialogContent className="gap-0 p-0 sm:max-w-lg">
        <DialogHeader className="border-b px-6 pt-6 pb-4">
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Your assistant and your privacy.</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-6 px-6 py-5">
          <form
            className="flex flex-col gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              saveName();
            }}
          >
            <Label htmlFor="assistant-name">Assistant's name</Label>
            <div className="flex gap-2">
              <Input id="assistant-name" value={value} onChange={(e) => setName(e.target.value)} />
              <Button type="submit" disabled={name === null || !value.trim()}>
                Save
              </Button>
            </div>
          </form>

          <section className="flex flex-col gap-2">
            <h3 className="text-sm font-medium">Memory</h3>
            <button
              onClick={onOpenMemory}
              className="flex items-center gap-3 rounded-xl border px-3.5 py-3 text-left transition-colors hover:bg-subtle"
            >
              <BookOpen className="size-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 text-[13.5px]">
                See and change what your assistant remembers about you
              </span>
              <ChevronRight className="size-4 text-faint" />
            </button>
          </section>

          <section className="flex flex-col gap-2">
            <h3 className="text-sm font-medium">Your data</h3>
            <div className="flex gap-3 rounded-xl bg-subtle p-3.5 text-[13px] leading-relaxed">
              <Lock className="mt-0.5 size-4 shrink-0 text-private" />
              <div className="flex flex-col gap-1">
                <span>
                  Your conversations, settings and passwords are stored encrypted on this
                  computer, and nowhere else.
                </span>

              </div>
            </div>
          </section>

          {providers.length > 0 && (
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-medium">Where your messages go</h3>
              <div className="flex flex-col divide-y rounded-xl border">
                {providers.map((p) => (
                  <div key={p.id} className="flex items-center gap-3 px-3.5 py-2.5 text-[13px]">
                    <span className="flex-1 truncate font-medium">{p.name}</span>
                    <LocalityBadge locality={p.locality} />
                  </div>
                ))}
              </div>
            </section>
          )}
        </div>

        <div className="flex justify-between gap-2 border-t bg-subtle/60 px-6 py-3.5">
          {isTauri ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => openInBrowser().catch((e) => toast.error(e?.message ?? String(e)))}
            >
              <ExternalLink /> Open in your browser
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
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
