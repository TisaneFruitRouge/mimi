import { useState } from "react";
import { Flame, Loader2 } from "lucide-react";

import { ChatView } from "@/features/chat/chat-view";
import { Sidebar } from "@/features/chat/sidebar";
import { SettingsView } from "@/features/settings/settings-view";
import { Setup } from "@/features/setup/setup";
import { useConnected } from "@/lib/events";
import { useSettings } from "@/lib/queries";

type View = { kind: "chat"; conversationId: string | null } | { kind: "settings" };

export default function App() {
  const connected = useConnected();
  const settings = useSettings();
  const [view, setView] = useState<View>({ kind: "chat", conversationId: null });

  if (connected === false) return <Offline />;
  if (!settings.data) return <Splash />;
  if (!settings.data.default_model) return <Setup />;

  return (
    <div className="flex h-screen">
      <Sidebar
        activeId={view.kind === "chat" ? view.conversationId : null}
        settingsOpen={view.kind === "settings"}
        onSelect={(id) => setView({ kind: "chat", conversationId: id })}
        onNew={() => setView({ kind: "chat", conversationId: null })}
        onSettings={() => setView({ kind: "settings" })}
      />
      {view.kind === "chat" ? (
        <ChatView
          // Remount per conversation so scroll and draft state don't leak between them.
          key={view.conversationId ?? "new"}
          conversationId={view.conversationId}
          onCreated={(id) => setView({ kind: "chat", conversationId: id })}
          onManageModels={() => setView({ kind: "settings" })}
        />
      ) : (
        <SettingsView />
      )}
    </div>
  );
}

function Splash() {
  return (
    <div className="flex h-screen items-center justify-center">
      <Loader2 className="size-5 animate-spin text-muted-foreground" />
    </div>
  );
}

function Offline() {
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-3 p-6 text-center">
      <div className="rounded-2xl bg-muted p-3">
        <Flame className="size-7 text-muted-foreground" />
      </div>
      <h1 className="text-lg font-semibold">The assistant isn't running</h1>
      <p className="max-w-sm text-sm text-muted-foreground">
        Hearth will reconnect as soon as it starts. During development, run{" "}
        <code className="font-mono">pnpm dev</code> from the repository.
      </p>
      <Loader2 className="size-4 animate-spin text-muted-foreground" />
    </div>
  );
}
