import { useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";

import { LogoMark } from "@/components/brand";
import { ChatView } from "@/features/chat/chat-view";
import { ConnectionsView } from "@/features/connections/connections-view";
import { ModelsView } from "@/features/models/models-view";
import { ConversationPalette } from "@/features/shell/conversation-palette";
import { SettingsDialog } from "@/features/shell/settings-dialog";
import { type Section, TopBar } from "@/features/shell/top-bar";
import type { DaemonError } from "@/lib/api";
import { useConnected } from "@/lib/events";
import { useSettings } from "@/lib/queries";

export default function App() {
  const connected = useConnected();
  const settings = useSettings();

  if ((settings.error as DaemonError | null)?.kind === "unauthorized") return <SignedOut />;
  // Wait for the first settings answer, so a browser that isn't signed in (whose event
  // socket is refused) doesn't flash the offline screen first.
  if (connected === false && !settings.isPending) return <Offline />;
  if (!settings.data) return <Splash />;
  if (!settings.data.default_model) return <Setup />;
  return <Shell />;
}

/** Where the app is, kept in the URL hash so the browser's back button and bookmarks work. */
type Route = { section: Section; conversationId: string | null };

function parseHash(): Route {
  const [first, second] = location.hash.replace(/^#\/?/, "").split("/");
  if (first === "models" || first === "connections") return { section: first, conversationId: null };
  return { section: "chat", conversationId: first === "chat" && second ? second : null };
}

function hashOf(r: Route) {
  if (r.section !== "chat") return `#/${r.section}`;
  return r.conversationId ? `#/chat/${r.conversationId}` : "#/";
}

function useRoute() {
  const [route, setRoute] = useState(parseHash);
  useEffect(() => {
    const onHash = () => setRoute(parseHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  const navigate = (next: Route) => {
    if (hashOf(next) !== location.hash) location.hash = hashOf(next);
    setRoute(next);
  };
  return [route, navigate] as const;
}

function Shell() {
  const [{ section, conversationId }, navigate] = useRoute();
  // Coming back to Chat returns to the conversation that was open.
  const lastConversation = useRef(conversationId);
  if (section === "chat") lastConversation.current = conversationId;
  const setSection = (s: Section) =>
    navigate({ section: s, conversationId: s === "chat" ? lastConversation.current : null });
  const setConversationId = (id: string | null) => navigate({ section: "chat", conversationId: id });
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const openConversation = setConversationId;

  return (
    <div className="flex h-screen flex-col">
      <TopBar
        section={section}
        onSection={setSection}
        onPalette={() => setPaletteOpen(true)}
        onSettings={() => setSettingsOpen(true)}
      />
      {section === "chat" && (
        <ChatView
          // Remount per conversation so scroll and draft state don't leak between them.
          key={conversationId ?? "new"}
          conversationId={conversationId}
          onCreated={setConversationId}
          onNewConversation={() => openConversation(null)}
          onSection={setSection}
        />
      )}
      {section === "connections" && <ConnectionsView />}
      {section === "models" && <ModelsView onChat={() => setSection("chat")} />}

      <ConversationPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onOpenConversation={openConversation}
        onNewConversation={() => openConversation(null)}
        onSection={setSection}
      />
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </div>
  );
}

/** First run: the Models page, until a model is chosen. */
function Setup() {
  return (
    <div className="flex h-screen flex-col">
      <header className="flex h-16 shrink-0 items-center gap-2.5 px-5">
        <LogoMark />
        <span className="text-[15px] font-semibold tracking-tight">hearth</span>
      </header>
      <ModelsView setup />
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 p-6 text-center">
      {children}
    </div>
  );
}

function Splash() {
  return (
    <Centered>
      <LogoMark className="size-11 rounded-[14px]" />
      <Loader2 className="size-4 animate-spin text-faint" />
    </Centered>
  );
}

/** Browser only: this browser has no valid session. */
function SignedOut() {
  return (
    <Centered>
      <LogoMark className="size-11 rounded-[14px]" />
      <h1 className="text-xl font-semibold tracking-tight">Open Hearth from your computer</h1>
      <p className="max-w-sm text-[14px] leading-relaxed text-muted-foreground">
        For your privacy, this browser needs a sign-in link. Use “Open in your browser” in the
        Hearth app's settings, or run <code className="font-mono text-[13px]">hearth open</code>{" "}
        in a terminal.
      </p>
    </Centered>
  );
}

function Offline() {
  return (
    <Centered>
      <LogoMark className="size-11 rounded-[14px] opacity-50 grayscale" />
      <h1 className="text-xl font-semibold tracking-tight">Your assistant isn't running</h1>
      <p className="max-w-sm text-[14px] leading-relaxed text-muted-foreground">
        Hearth reconnects on its own as soon as it starts again.
      </p>
      <Loader2 className="size-4 animate-spin text-faint" />
    </Centered>
  );
}
