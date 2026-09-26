import { useEffect, useState } from "react";
import { Flame, RefreshCw } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { type DaemonError, type Status, daemonStatus } from "@/lib/daemon";

type State =
  | { phase: "loading" }
  | { phase: "ok"; status: Status }
  | { phase: "error"; error: DaemonError };

export default function App() {
  const [state, setState] = useState<State>({ phase: "loading" });

  const refresh = () =>
    daemonStatus()
      .then((status) => setState({ phase: "ok", status }))
      .catch((error: DaemonError) => setState({ phase: "error", error }));

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 5000);
    return () => clearInterval(id);
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center bg-background p-6 text-foreground">
      <Card className="w-full max-w-md">
        <CardHeader>
          <div className="flex items-center gap-2">
            <Flame className="size-5 text-orange-500" />
            <CardTitle>Hearth</CardTitle>
            <StatusBadge state={state} />
          </div>
          <CardDescription>Your private assistant, running on this computer.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 text-sm">
          <StatusBody state={state} />
          <Button variant="outline" size="sm" onClick={refresh}>
            <RefreshCw /> Check again
          </Button>
        </CardContent>
      </Card>
    </main>
  );
}

function StatusBadge({ state }: { state: State }) {
  if (state.phase === "ok") return <Badge className="ml-auto">Running</Badge>;
  if (state.phase === "error") return <Badge variant="destructive" className="ml-auto">Offline</Badge>;
  return <Badge variant="secondary" className="ml-auto">Checking…</Badge>;
}

function StatusBody({ state }: { state: State }) {
  switch (state.phase) {
    case "loading":
      return <p className="text-muted-foreground">Looking for the assistant…</p>;
    case "error":
      return state.error.kind === "not_running" ? (
        <p className="text-muted-foreground">
          The assistant isn't running. Start it with <code className="font-mono">hearthd</code>.
        </p>
      ) : (
        <p className="text-destructive">{state.error.message}</p>
      );
    case "ok":
      return (
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
          <dt className="text-muted-foreground">Version</dt>
          <dd>{state.status.version}</dd>
          <dt className="text-muted-foreground">Uptime</dt>
          <dd>{formatUptime(state.status.uptime_secs)}</dd>
          <dt className="text-muted-foreground">Data</dt>
          <dd className="truncate font-mono text-xs leading-5">{state.status.data_dir}</dd>
        </dl>
      );
  }
}

function formatUptime(secs: number) {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}
