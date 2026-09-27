import { useEffect, useRef, useState } from "react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { Loader2, Unplug } from "lucide-react";
import { cn } from "cn";

import { LogoMark } from "@/components/brand";
import { ChatView } from "@/features/chat/chat-view";
import { ConnectionsView } from "@/features/connections/connections-view";
import { MemoryView } from "@/features/memory/memory-view";
import { ModelsView } from "@/features/models/models-view";
import { Onboarding } from "@/features/onboarding/onboarding";
import { PeopleView } from "@/features/people/people-view";
import { ConversationPalette } from "@/features/shell/conversation-palette";
import { SettingsDialog } from "@/features/shell/settings-dialog";
import { type Section, TopBar } from "@/features/shell/top-bar";
import type { DaemonError } from "@/lib/api";
import { useConnected } from "@/lib/events";
import { hasMod, macOverlayTitleBar } from "@/lib/platform";
import { useSettings } from "@/lib/queries";
import { ScrollEdgeContext } from "@/lib/scroll-edge";

export default function App() {
  const connected = useConnected();
  const settings = useSettings();

  let screen: React.ReactNode;
  let key: string;
  if ((settings.error as DaemonError | null)?.kind === "unauthorized") {
    [screen, key] = [<SignedOut />, "signed-out"];
  } else if (connected === false && !settings.isPending) {
    // Wait for the first settings answer, so a browser that isn't signed in (whose event
    // socket is refused) doesn't flash the offline screen first.
    [screen, key] = [<Offline />, "offline"];
  } else if (!settings.data) {
    [screen, key] = [<Splash />, "splash"];
  } else if (!settings.data.onboarding_done) {
    [screen, key] = [<Onboarding />, "onboarding"];
  } else if (!settings.data.default_model) {
    [screen, key] = [<Setup />, "setup"];
  } else {
    [screen, key] = [<Shell />, "shell"];
  }

  return (
    <MotionConfig reducedMotion="user">
      <AnimatePresence mode="wait" initial={false}>
        <motion.div
          key={key}
          className="h-screen"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.25 }}
        >
          {screen}
        </motion.div>
      </AnimatePresence>
    </MotionConfig>
  );
}

/** Where the app is, kept in the URL hash so the browser's back button and bookmarks work. */
type Route = { section: Section; conversationId: string | null };

function parseHash(): Route {
  const [first, second] = location.hash.replace(/^#\/?/, "").split("/");
  if (first === "models" || first === "connections" || first === "memory" || first === "people")
    return { section: first, conversationId: null };
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

const sectionKeys: Record<string, Section> = { "1": "chat", "2": "connections", "3": "models" };

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
  const [scrolled, setScrolled] = useState(false);

  // The chat view is remounted (and animated) when the user opens another conversation,
  // but not when a new chat gets its id from its first message.
  const created = useRef<string | null>(null);
  const [chatKey, setChatKey] = useState(() => conversationId ?? "new");
  const shown = useRef(conversationId);
  useEffect(() => {
    if (shown.current === conversationId) return;
    shown.current = conversationId;
    if (conversationId && conversationId === created.current) return;
    setChatKey(conversationId ?? `new-${Date.now()}`);
  }, [conversationId]);
  const onCreated = (id: string) => {
    created.current = id;
    setConversationId(id);
  };

  // Each view starts at the top: the bar starts clear.
  useEffect(() => setScrolled(false), [section, conversationId]);

  // Keep the latest navigation in a ref so the key handler is registered once.
  const actions = useRef({ setSection, setConversationId });
  actions.current = { setSection, setConversationId };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!hasMod(e) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (key === "n") {
        e.preventDefault();
        actions.current.setConversationId(null);
      } else if (key === ",") {
        e.preventDefault();
        setSettingsOpen(true);
      } else if (sectionKeys[key]) {
        e.preventDefault();
        actions.current.setSection(sectionKeys[key]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <ScrollEdgeContext.Provider value={setScrolled}>
      <div className="relative h-screen overflow-hidden">
        <TopBar
          section={section}
          scrolled={scrolled}
          onSection={setSection}
          onNewChat={() => setConversationId(null)}
          onPalette={() => setPaletteOpen(true)}
          onSettings={() => setSettingsOpen(true)}
        />
        <AnimatePresence mode="wait" initial={false}>
          <motion.main
            key={section === "chat" ? `chat:${chatKey}` : section}
            className="h-full"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ type: "spring", stiffness: 420, damping: 36, mass: 0.8 }}
          >
            {section === "chat" && (
              <ChatView
                conversationId={conversationId}
                onCreated={onCreated}
                onNewConversation={() => setConversationId(null)}
                onSection={setSection}
              />
            )}
            {section === "connections" && <ConnectionsView onPeople={() => setSection("people")} />}
            {section === "people" && <PeopleView onConnections={() => setSection("connections")} />}
            {section === "models" && <ModelsView onChat={() => setSection("chat")} />}
            {section === "memory" && <MemoryView />}
          </motion.main>
        </AnimatePresence>

        <ConversationPalette
          open={paletteOpen}
          onOpenChange={(open) => {
            setPaletteOpen(open);
            // Back to typing: return focus to the message box, if there is one.
            if (!open)
              requestAnimationFrame(() =>
                document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')?.focus(),
              );
          }}
          onOpenConversation={setConversationId}
          onNewConversation={() => setConversationId(null)}
          onSection={setSection}
        />
        <SettingsDialog
          open={settingsOpen}
          onOpenChange={setSettingsOpen}
          onOpenMemory={() => {
            setSettingsOpen(false);
            setSection("memory");
          }}
        />
      </div>
    </ScrollEdgeContext.Provider>
  );
}

/** First run: the Models page, until a model is chosen. */
function Setup() {
  const [scrolled, setScrolled] = useState(false);
  return (
    <ScrollEdgeContext.Provider value={setScrolled}>
      <div className="relative h-screen overflow-hidden">
        <header
          data-tauri-drag-region
          className={cn(
            "absolute inset-x-0 top-0 z-30 flex h-[60px] items-center gap-2 px-4 transition-[background-color,box-shadow] duration-300",
            scrolled ? "material shadow-[inset_0_-0.5px_0_var(--separator)]" : "bg-transparent",
            macOverlayTitleBar && "pl-[84px]",
          )}
        >
          <LogoMark />
          <span className="text-[15px] font-semibold tracking-[-0.016em]">Mimi</span>
        </header>
        <ModelsView setup />
      </div>
    </ScrollEdgeContext.Provider>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div
      data-tauri-drag-region
      className="flex h-screen flex-col items-center justify-center gap-3 p-6 text-center"
    >
      {children}
    </div>
  );
}

function Splash() {
  return (
    <Centered>
      <motion.div
        initial={{ scale: 0.9, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 300, damping: 22 }}
      >
        <LogoMark className="size-14 rounded-[16px] [&_svg]:size-7" />
      </motion.div>
    </Centered>
  );
}

/** Browser only: this browser has no valid session. */
function SignedOut() {
  return (
    <Centered>
      <LogoMark className="mb-2 size-14 rounded-[16px]" />
      <h1 className="type-title">Open Mimi from your computer</h1>
      <p className="max-w-[380px] type-body text-muted-foreground">
        For your privacy, this browser needs a sign-in link. In the Mimi app, open Settings
        and choose “Open in your browser”.
      </p>
    </Centered>
  );
}

function Offline() {
  return (
    <Centered>
      <div className="mb-2 flex size-14 items-center justify-center rounded-[16px] bg-fill text-muted-foreground">
        <Unplug className="size-6" />
      </div>
      <h1 className="type-title">Your assistant isn't running</h1>
      <p className="max-w-[360px] type-body text-muted-foreground">
        Mimi reconnects on its own as soon as it's back.
      </p>
      <Loader2 className="mt-2 size-4 animate-spin text-faint" />
    </Centered>
  );
}
