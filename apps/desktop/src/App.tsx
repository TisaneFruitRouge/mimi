import { useEffect, useRef, useState } from "react";
import { AnimatePresence, MotionConfig, motion } from "motion/react";
import { Loader2 } from "lucide-react";
import { cn } from "cn";

import { AssistantAvatar } from "@/components/assistant-avatar";
import { LogoMark } from "@/components/brand";
import { CalendarView } from "@/features/calendar/calendar-view";
import { ChatView } from "@/features/chat/chat-view";
import { MailView } from "@/features/mail/mail-view";
import { ModelsView } from "@/features/models/models-view";
import { Onboarding } from "@/features/onboarding/onboarding";
import { PeopleView } from "@/features/people/people-view";
import { GuestApprovals } from "@/features/people/guest-approvals";
import { ConversationPalette } from "@/features/shell/conversation-palette";
import { SettingsView, settingsPages } from "@/features/shell/settings-view";
import { AppContextMenu } from "@/components/app-context-menu";
import { BarMaterial, type Section, type SettingsPage, TopBar, isTab, tabs } from "@/features/shell/top-bar";
import { useUpdateNotice } from "@/features/shell/updates";
import type { DaemonError } from "@/lib/api";
import { type Draft, setDraft } from "@/lib/draft";
import { useConnected } from "@/lib/events";
import { hasMod, macOverlayTitleBar } from "@/lib/platform";
import { useSettings } from "@/lib/queries";
import { createScrollEdge, ScrollEdgeContext } from "@/lib/scroll-edge";

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

/**
 * Where the app is, kept in the URL hash so the browser's back button and bookmarks
 * work: `#/chat/<id>`, `#/calendar`, `#/mail`, `#/people/<id>`, `#/settings/<page>`.
 */
type Route = { section: Section; conversationId: string | null; personId: string | null };

const home: Route = { section: "chat", conversationId: null, personId: null };

function parseHash(): Route {
  const [first, second] = location.hash.replace(/^#\/?/, "").split("/");
  const at = (section: Section): Route => ({ ...home, section });
  if (first === "calendar" || first === "mail") return at(first);
  if (first === "people") return { ...home, section: "people", personId: second || null };
  if (first === "settings") {
    const page = settingsPages.find((p) => p.id === second);
    return at(page ? page.id : "general");
  }
  // Addresses from before Settings had its own window.
  if (first === "models" || first === "connections" || first === "memory") return at(first);
  if (first === "reminders") return at("calendar");
  return { ...home, conversationId: first === "chat" && second ? second : null };
}

function hashOf(r: Route) {
  if (r.section === "chat") return r.conversationId ? `#/chat/${r.conversationId}` : "#/";
  if (r.section === "people") return r.personId ? `#/people/${r.personId}` : "#/people";
  if (isTab(r.section)) return `#/${r.section}`;
  return `#/settings/${r.section}`;
}

function useRoute() {
  const [route, setRoute] = useState(parseHash);
  useEffect(() => {
    const onHash = () => {
      const next = parseHash();
      // Old addresses (`#/models`, `#/reminders`) show their new one.
      if (location.hash && hashOf(next) !== location.hash) history.replaceState(null, "", hashOf(next));
      setRoute(next);
    };
    onHash();
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  /** `replace`: the address was an old name for the same place (no new history entry). */
  const navigate = (next: Route, replace = false) => {
    if (hashOf(next) !== location.hash) {
      if (replace) history.replaceState(null, "", hashOf(next));
      else location.hash = hashOf(next);
    }
    setRoute(next);
  };
  return [route, navigate] as const;
}

const sectionKeys: Record<string, Section> = Object.fromEntries(tabs.map((t) => [t.key, t.id]));

function Shell() {
  const [{ section, conversationId, personId }, navigate] = useRoute();
  useUpdateNotice();
  // Coming back to Chat, People or Settings returns to where it was.
  const last = useRef({ conversationId, personId, settings: "general" as SettingsPage });
  if (section === "chat") last.current.conversationId = conversationId;
  if (section === "people") last.current.personId = personId;
  if (!isTab(section)) last.current.settings = section;
  const setSection = (s: Section) =>
    navigate({
      section: s,
      conversationId: s === "chat" ? last.current.conversationId : null,
      personId: s === "people" ? last.current.personId : null,
    });
  const setConversationId = (id: string | null) => navigate({ ...home, conversationId: id });
  const openPerson = (id: string | null, replace = false) =>
    navigate({ ...home, section: "people", personId: id }, replace);
  const openSettings = () => setSection(last.current.settings);
  /** "Ask Mimi about this": a new chat with the thing already mentioned. */
  const askMimi = (draft: Draft) => {
    setDraft(draft);
    setConversationId(null);
  };
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [scrollEdge] = useState(createScrollEdge);

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
  useEffect(() => scrollEdge.set(false), [scrollEdge, section, conversationId]);

  // Keep the latest navigation in a ref so the key handler is registered once.
  const actions = useRef({ setSection, setConversationId, openSettings });
  actions.current = { setSection, setConversationId, openSettings };
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
        actions.current.openSettings();
      } else if (sectionKeys[key]) {
        e.preventDefault();
        actions.current.setSection(sectionKeys[key]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <ScrollEdgeContext.Provider value={scrollEdge}>
      <AppContextMenu
        onNewChat={() => setConversationId(null)}
        onSearch={() => setPaletteOpen(true)}
        onSettings={openSettings}
        onAsk={askMimi}
      >
        <div className="relative h-screen overflow-hidden">
          <TopBar
            section={section}
            onSection={setSection}
            onNewChat={() => setConversationId(null)}
            onPalette={() => setPaletteOpen(true)}
            onSettings={openSettings}
          />
          <AnimatePresence mode="wait" initial={false}>
            <motion.main
              // Settings pages animate inside the Settings window, not as whole screens.
              key={section === "chat" ? `chat:${chatKey}` : isTab(section) ? section : "settings"}
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
              {section === "calendar" && (
                <CalendarView
                  onAsk={askMimi}
                  onOpenPerson={openPerson}
                  onOpenConversation={setConversationId}
                  onSection={setSection}
                />
              )}
              {section === "mail" && <MailView onSection={setSection} onAsk={askMimi} onOpenPerson={openPerson} />}
              {section === "people" && (
                <PeopleView
                  personId={personId}
                  onOpenPerson={openPerson}
                  onAsk={askMimi}
                  onOpenConversation={setConversationId}
                  onSection={setSection}
                />
              )}
              {!isTab(section) && (
                <SettingsView page={section} onSection={setSection} onOpenConversation={setConversationId} />
              )}
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
          <GuestApprovals />
        </div>
      </AppContextMenu>
    </ScrollEdgeContext.Provider>
  );
}

/** First run: the Models page, until a model is chosen. */
function Setup() {
  const [scrollEdge] = useState(createScrollEdge);
  return (
    <ScrollEdgeContext.Provider value={scrollEdge}>
      <div className="relative h-screen overflow-hidden">
        <header
          data-tauri-drag-region
          className={cn(
            "absolute inset-x-0 top-0 z-30 flex h-[60px] items-center gap-2 px-4",
            macOverlayTitleBar && "pl-[84px]",
          )}
        >
          <BarMaterial />
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
        <LogoMark className="size-14 rounded-[16px]" />
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
      {/* Asleep while the daemon is away. */}
      <AssistantAvatar size={76} mood="sleepy" decorative className="mb-1" />
      <h1 className="type-title">Your assistant isn't running</h1>
      <p className="max-w-[360px] type-body text-muted-foreground">
        Mimi reconnects on its own as soon as it's back.
      </p>
      <Loader2 className="mt-2 size-4 animate-spin text-faint" />
    </Centered>
  );
}
