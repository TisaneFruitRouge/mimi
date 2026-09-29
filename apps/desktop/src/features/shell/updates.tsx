import { useEffect, useState } from "react";
import { Download, Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";

import { Grouped, IconTile, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { api, keys } from "@/lib/api";
import { useSettings, useUpdates } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

/** Settings › General › Updates: the opt-in check and what it found. */
export function UpdatesGroup() {
  const qc = useQueryClient();
  const settings = useSettings().data;
  const status = useUpdates().data;
  const [checking, setChecking] = useState(false);
  const on = !!settings?.update_check;

  const toggle = (update_check: boolean) => {
    if (!settings) return;
    api.putSettings({ ...settings, update_check }).catch((e) => toast.error((e as Error).message));
  };
  const checkNow = async () => {
    setChecking(true);
    try {
      qc.setQueryData(keys.updates, await api.checkForUpdates());
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setChecking(false);
    }
  };

  const available = status?.available;
  const detail = !status
    ? "Checking…"
    : available
      ? `Version ${available.version} is out. You have ${status.current}.`
      : status.error
        ? status.error
        : status.checked_at
          ? `You have the latest version (${status.current}). Checked ${since(status.checked_at)}.`
          : `You have version ${status.current}.`;

  return (
    <Section title="Updates">
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="bg-network-soft text-network">
              <RefreshCw />
            </IconTile>
          }
          title="Check for new versions"
          detail="Once a day, this computer asks GitHub, where Mimi is published, whether there's a newer version. Nothing about you is sent."
          className="[&_.truncate]:whitespace-normal"
          trailing={<Switch checked={on} disabled={!settings} onCheckedChange={toggle} aria-label="Check for new versions" />}
        />
        <Row
          icon={
            <IconTile size="sm" className={available ? "bg-lime-soft text-lime-deep" : "bg-fill text-muted-foreground"}>
              <Download />
            </IconTile>
          }
          title={available ? "A new version is available" : "Version"}
          detail={detail}
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <>
              {available && (
                <Button variant="lime" size="sm" onClick={() => openExternal(available.url)}>
                  See what's new
                </Button>
              )}
              <Button variant="ghost" size="sm" disabled={checking} onClick={checkNow}>
                {checking && <Loader2 className="animate-spin" />} Check now
              </Button>
            </>
          }
        />
      </Grouped>
    </Section>
  );
}

const SEEN = "mimi.update-noticed";

/** Says once per new version, anywhere in the app, that it's out. */
export function useUpdateNotice() {
  const available = useUpdates().data?.available;
  useEffect(() => {
    if (!available) return;
    try {
      if (localStorage.getItem(SEEN) === available.version) return;
      localStorage.setItem(SEEN, available.version);
    } catch {
      // Without storage the notice may come back next launch; that's fine.
    }
    toast(`Mimi ${available.version} is available`, {
      description: "See what's new and download it from its page.",
      duration: 12_000,
      action: { label: "See what's new", onClick: () => openExternal(available.url) },
    });
  }, [available]);
}

function since(ms: number) {
  const minutes = Math.round((Date.now() - ms) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.round(hours / 24);
  return `${days} day${days === 1 ? "" : "s"} ago`;
}
