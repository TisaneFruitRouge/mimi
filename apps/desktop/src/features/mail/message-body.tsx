import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { ImageOff, Loader2 } from "lucide-react";
import { cn } from "cn";
import { motion } from "motion/react";
import { toast } from "sonner";

import type { MailMessage } from "@/bindings/MailMessage";
import { Markdown } from "@/components/markdown";
import { Skeleton } from "@/components/ui/skeleton";
import { api, keys } from "@/lib/api";
import { openExternal } from "@/lib/transport";

// --- How mail is shown, per viewer ------------------------------------------------------

/**
 * Text: the plain text the assistant reads. Formatted: tidy text with headings, lists and
 * links, made by the daemon. Original: the email's own layout and pictures, made safe by
 * the daemon and shown in a sandboxed frame.
 */
export type MailMode = "text" | "formatted" | "original";

const modes: { id: MailMode; label: string; hint: string }[] = [
  { id: "text", label: "Text", hint: "Just the words" },
  { id: "formatted", label: "Formatted", hint: "Tidy text with headings, lists and links" },
  { id: "original", label: "Original", hint: "As it was sent, with its layout and pictures" },
];

/**
 * A small store shared by every message card, so switching the mode switches them all.
 * Remembered per viewer (a convenience: everything works without it).
 */
function localStore<T>(key: string, read: (raw: string | null) => T, write: (v: T) => string) {
  let value: T;
  try {
    value = read(localStorage.getItem(key));
  } catch {
    value = read(null);
  }
  const listeners = new Set<() => void>();
  return {
    subscribe: (l: () => void) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    get: () => value,
    set: (v: T) => {
      value = v;
      try {
        localStorage.setItem(key, write(v));
      } catch {
        // Not important.
      }
      listeners.forEach((l) => l());
    },
  };
}

// Original by default: the daemon strips anything that could run or track, and the
// frame can't run scripts.
const modeStore = localStore<MailMode>(
  "mimi.mail.mode",
  (raw) => (modes.some((m) => m.id === raw) ? (raw as MailMode) : "original"),
  (v) => v,
);

// Senders whose pictures load without asking ("Always for this sender").
const trustedStore = localStore<string[]>(
  "mimi.mail.images",
  (raw) => {
    try {
      const v = JSON.parse(raw ?? "[]");
      return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
    } catch {
      return [];
    }
  },
  (v) => JSON.stringify(v),
);

export function useMailMode(): [MailMode, (m: MailMode) => void] {
  return [useSyncExternalStore(modeStore.subscribe, modeStore.get), modeStore.set];
}

/** Text · Formatted · Original, for the reader's header. */
export function MailModeSwitch({ original }: { original: boolean }) {
  const [mode, setMode] = useMailMode();
  const shown = original ? modes : modes.filter((m) => m.id !== "original");
  const current = !original && mode === "original" ? "formatted" : mode;
  return (
    <div role="radiogroup" aria-label="Show emails as" className="flex rounded-[9px] bg-fill p-[2px]">
      {shown.map((m) => (
        <button
          key={m.id}
          role="radio"
          aria-checked={current === m.id}
          title={m.hint}
          onClick={() => setMode(m.id)}
          className={cn(
            "relative h-7 rounded-[7px] px-3 text-[13px] font-medium transition-colors",
            current === m.id ? "text-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {current === m.id && (
            <motion.span
              layoutId="mail-mode"
              className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
              transition={{ type: "spring", stiffness: 520, damping: 38 }}
            />
          )}
          <span className="relative">{m.label}</span>
        </button>
      ))}
    </div>
  );
}

// --- A message's body -------------------------------------------------------------------

/** Where quoted history starts in a plain-text (or formatted) body, as the daemon finds it. */
function splitQuoted(body: string): [string, string | null] {
  const lines = body.split("\n");
  const at = lines.findIndex((line, i) => {
    const t = line.trim().toLowerCase();
    return (
      (t.startsWith(">") && lines.slice(i).every((l) => !l.trim() || l.trim().startsWith(">"))) ||
      (t.startsWith("on ") && t.endsWith("wrote:")) ||
      (t.startsWith("le ") && /a écrit ?:$/.test(t)) ||
      (t.startsWith("am ") && t.endsWith("schrieb:")) ||
      t.startsWith("-----original message-----")
    );
  });
  if (at <= 0) return [body, null];
  return [lines.slice(0, at).join("\n").trimEnd(), lines.slice(at).join("\n")];
}

/** A message's body in the chosen mode, with quoted history folded (Text and Formatted). */
/** What a right-click inside the rendered email was on, for the message's menu. */
export interface FrameContext {
  link: string | null;
  selection: string;
}

export function MessageBody({
  message: m,
  onFrameContext,
}: {
  message: MailMessage;
  onFrameContext?: (c: FrameContext) => void;
}) {
  const [mode] = useMailMode();
  const trusted = useSyncExternalStore(trustedStore.subscribe, trustedStore.get);
  const sender = m.from.email.toLowerCase();
  const [askedImages, setAskedImages] = useState(false);
  // Pictures never load by themselves in suspicious mail, whoever it claims to be from.
  const wantsImages = askedImages || (!m.suspicious && trusted.includes(sender));
  const rich = mode !== "text";

  const content = useQuery({
    queryKey: keys.mailContent(m.id, false),
    queryFn: () => api.mailContent(m.id),
    enabled: rich,
    staleTime: Infinity,
    retry: false,
  });
  const remote = content.data?.remote_images ?? 0;
  const loaded = useQuery({
    queryKey: keys.mailContent(m.id, true),
    queryFn: () => api.mailContentWithImages(m.id),
    enabled: mode === "original" && wantsImages && remote > 0,
    staleTime: Infinity,
    retry: false,
  });
  useEffect(() => {
    if (loaded.error) toast.error((loaded.error as Error).message);
  }, [loaded.error]);

  if (!rich || content.isError) return <Folded body={m.body} render={(t, quoted) => <PlainText text={t} quoted={quoted} />} />;
  if (!content.data) return <BodySkeleton />;
  const c = loaded.data ?? content.data;
  if (mode === "original" && c.html !== null) {
    return (
      <div className="flex flex-col gap-3">
        {remote > 0 && !c.images_loaded && (
          <ImagesHidden
            loading={loaded.isFetching}
            onLoad={() => setAskedImages(true)}
            onAlways={
              m.suspicious || trusted.includes(sender)
                ? null
                : () => {
                    trustedStore.set([...trusted, sender]);
                    setAskedImages(true);
                  }
            }
          />
        )}
        <EmailFrame html={c.html} onContext={onFrameContext} />
      </div>
    );
  }
  return <Folded body={c.formatted} render={(t, quoted) => <Formatted text={t} quoted={quoted} />} />;
}

function PlainText({ text, quoted }: { text: string; quoted: boolean }) {
  // Plain text only: nothing in an email is turned into a link or formatting.
  return (
    <div
      className={cn(
        "break-words whitespace-pre-wrap",
        quoted ? "type-subhead text-muted-foreground" : "type-body leading-[1.55]",
      )}
    >
      {text || " "}
    </div>
  );
}

function Formatted({ text, quoted }: { text: string; quoted: boolean }) {
  return (
    <div className={cn("min-w-0 break-words", quoted && "opacity-75")}>
      <Markdown>{text || " "}</Markdown>
    </div>
  );
}

/** A body with its quoted history behind a ••• button. */
function Folded({ body, render }: { body: string; render: (text: string, quoted: boolean) => React.ReactNode }) {
  const [text, quoted] = splitQuoted(body);
  const [showQuoted, setShowQuoted] = useState(false);
  return (
    <>
      {render(text, false)}
      {quoted && (
        <div>
          <button
            onClick={() => setShowQuoted((s) => !s)}
            aria-label={showQuoted ? "Hide quoted text" : "Show quoted text"}
            className="rounded-full bg-fill px-2.5 py-0.5 type-footnote font-semibold text-muted-foreground hover:text-foreground"
          >
            •••
          </button>
          {showQuoted && (
            <div className="mt-2 border-l-2 border-separator pl-3">{render(quoted, true)}</div>
          )}
        </div>
      )}
    </>
  );
}

function BodySkeleton() {
  return (
    <div className="flex flex-col gap-2 py-1" aria-label="Loading the email">
      <Skeleton className="h-3.5 w-11/12 rounded-[6px]" />
      <Skeleton className="h-3.5 w-4/5 rounded-[6px]" />
      <Skeleton className="h-3.5 w-2/3 rounded-[6px]" />
    </div>
  );
}

function ImagesHidden({
  loading,
  onLoad,
  onAlways,
}: {
  loading: boolean;
  onLoad: () => void;
  onAlways: (() => void) | null;
}) {
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-[12px] bg-subtle px-3 py-2">
      <ImageOff className="size-4 shrink-0 text-muted-foreground" />
      <p className="min-w-0 flex-1 type-subhead text-muted-foreground">Images are hidden to protect your privacy.</p>
      <div className="flex items-center gap-1">
        <button
          onClick={onLoad}
          disabled={loading}
          className="pressable inline-flex h-7 items-center gap-1.5 rounded-[8px] bg-background px-2.5 text-[13px] font-medium shadow-[0_0_0_0.5px_rgb(0_0_0/0.08),0_1px_2px_rgb(0_0_0/0.08)] disabled:opacity-70"
        >
          {loading && <Loader2 className="size-3.5 animate-spin" />} Load images
        </button>
        {onAlways && (
          <button
            onClick={onAlways}
            disabled={loading}
            className="pressable h-7 rounded-[8px] px-2.5 text-[13px] font-medium text-muted-foreground hover:bg-fill hover:text-foreground disabled:opacity-70"
          >
            Always for this sender
          </button>
        )}
      </div>
    </div>
  );
}

// --- The email as it was sent ----------------------------------------------------------

// A second policy inside the frame, on top of the app's own: nothing may load but the
// pictures the daemon put in, and nothing may run.
const FRAME_CSP = "default-src 'none'; img-src data:; style-src 'unsafe-inline'";

// Dark text on the white card, long lines wrapped, pictures never wider than the card.
// The frame is as tall as its content, so only a too-wide layout scrolls (sideways).
const FRAME_STYLE = `
html,body{margin:0;padding:0;background:transparent;overflow:hidden}
body{color:#1d1d1f;font:15px/1.55 -apple-system,BlinkMacSystemFont,system-ui,"Segoe UI",sans-serif;-webkit-font-smoothing:antialiased;overflow-wrap:break-word}
.mimi-mail{overflow-x:auto;overflow-y:hidden}
img{max-width:100%}
img[src]{height:auto}
table{max-width:100%}
pre{white-space:pre-wrap}
a{color:#0b63ce}
blockquote{margin:0 0 0 .6em;padding-left:.8em;border-left:2px solid #e3e3e8;color:#6e6e73}
`;

function frameDocument(body: string) {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${FRAME_CSP}"><meta name="referrer" content="no-referrer"><meta name="color-scheme" content="light"><style>${FRAME_STYLE}</style></head><body><div class="mimi-mail">${body}</div></body></html>`;
}

/**
 * The email's own HTML, already made safe by the daemon (no scripts, event handlers,
 * forms, frames or remote content), in a sandboxed frame.
 *
 * The sandbox leaves out `allow-scripts`, so nothing in the email can ever run, whatever
 * got past the daemon. It allows only `allow-same-origin`, so that this page (not the
 * email) can measure the content to size the frame and catch link clicks to open them
 * in the browser instead of inside the app. Same origin is harmless without scripts:
 * the email has no way to use it. (Scripts plus same origin together would let an email
 * lift its own sandbox; never add `allow-scripts`.) No `allow-popups`,
 * `allow-top-navigation` or `allow-forms` either.
 */
function EmailFrame({ html, onContext }: { html: string; onContext?: (c: FrameContext) => void }) {
  const ref = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(0);
  const [hover, setHover] = useState<string | null>(null);
  const doc = useMemo(() => frameDocument(html), [html]);
  const detach = useRef<() => void>(() => {});
  useEffect(() => () => detach.current(), []);

  const attach = () => {
    detach.current();
    const frame = ref.current;
    const d = frame?.contentDocument;
    if (!frame || !d?.documentElement) return;
    const measure = () => setHeight(Math.ceil(d.documentElement.getBoundingClientRect().height));
    const linkOf = (e: Event) => {
      const t = e.target as Element | null;
      return typeof t?.closest === "function" ? (t.closest("a[href]") as HTMLAnchorElement | null) : null;
    };
    const click = (e: MouseEvent) => {
      const a = linkOf(e);
      if (!a) return;
      e.preventDefault();
      openExternal(a.href);
    };
    const over = (e: MouseEvent) => setHover(linkOf(e)?.href ?? null);
    // A right-click in the frame opens the message's menu in the page, where it is:
    // note what was under it, then hand the page an equivalent right-click.
    const menu = (e: MouseEvent) => {
      e.preventDefault();
      onContext?.({ link: linkOf(e)?.href ?? null, selection: d.getSelection()?.toString().trim() ?? "" });
      const r = frame.getBoundingClientRect();
      frame.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: r.left + e.clientX,
          clientY: r.top + e.clientY,
        }),
      );
    };
    d.addEventListener("contextmenu", menu);
    const leave = () => setHover(null);
    d.addEventListener("click", click);
    d.addEventListener("auxclick", click);
    d.addEventListener("mouseover", over);
    d.addEventListener("mouseleave", leave);
    // Pictures change the height as they decode; the card's width changes the layout.
    d.addEventListener("load", measure, true);
    const observer = new ResizeObserver(measure);
    observer.observe(d.documentElement);
    observer.observe(frame);
    measure();
    detach.current = () => {
      observer.disconnect();
      d.removeEventListener("click", click);
      d.removeEventListener("auxclick", click);
      d.removeEventListener("mouseover", over);
      d.removeEventListener("mouseleave", leave);
      d.removeEventListener("contextmenu", menu);
      d.removeEventListener("load", measure, true);
    };
  };

  return (
    <div className="relative">
      <iframe
        ref={ref}
        title="Email"
        sandbox="allow-same-origin"
        referrerPolicy="no-referrer"
        srcDoc={doc}
        onLoad={attach}
        className={cn("block w-full border-0 transition-opacity", height === 0 && "opacity-0")}
        // Capped, so an email can't stretch the reader without end.
        style={{ height: Math.min(Math.max(height, 24), 30_000) }}
      />
      {hover && (
        <div className="pointer-events-none absolute bottom-1 left-1 max-w-[85%] truncate rounded-[7px] bg-background/95 px-2 py-0.5 type-footnote text-muted-foreground shadow-[0_0_0_0.5px_rgb(0_0_0/0.08),0_1px_3px_rgb(0_0_0/0.1)]">
          {hover}
        </div>
      )}
    </div>
  );
}
