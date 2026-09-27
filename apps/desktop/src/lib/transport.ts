/**
 * How the UI reaches the daemon. The same app runs in two places:
 *
 * - The desktop app (Tauri): requests go through the `api` command and events arrive
 *   via Tauri events. The Rust side holds the daemon token; the webview never sees it.
 * - A browser, served by the daemon itself: same-origin `fetch` and a WebSocket,
 *   authenticated by the session cookie set when a login link was opened.
 */
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

import type { Event } from "@/bindings/Event";

export const isTauri = "__TAURI_INTERNALS__" in window;

/** Mirrors CommandError in src-tauri/src/lib.rs, plus the browser-only `unauthorized`. */
export type TransportError =
  | { kind: "not_running" }
  | { kind: "unauthorized" }
  | { kind: "api"; code: string; message: string }
  | { kind: "other"; message: string };

/** Calls a `/v1` route. Rejects with a `TransportError`. */
export async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  if (isTauri) return invoke<T>("api", { method, path, body: body ?? null });

  let res: Response;
  try {
    res = await fetch(`/v1${path}`, {
      method,
      credentials: "same-origin",
      headers: body !== undefined ? { "content-type": "application/json" } : {},
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  } catch {
    throw { kind: "not_running" } satisfies TransportError;
  }
  if (res.status === 401) throw { kind: "unauthorized" } satisfies TransportError;
  const text = await res.text();
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    if (res.ok) throw { kind: "other", message: "The assistant sent an unreadable reply." } satisfies TransportError;
  }
  if (!res.ok) {
    // Requests the daemon couldn't even read come back without the usual error body,
    // as they do through the desktop app's proxy.
    throw (json?.code
      ? { kind: "api", code: json.code, message: json.message }
      : { kind: "api", code: "unknown", message: `The assistant returned ${res.status}.` }) satisfies TransportError;
  }
  return json as T;
}

/**
 * Subscribes to daemon events and connection changes. `onConnection` fires on every
 * change, starting with the initial state. Returns an unsubscribe function.
 */
export function subscribe(
  onEvent: (event: Event) => void,
  onConnection: (connected: boolean) => void,
): () => void {
  return isTauri ? subscribeTauri(onEvent, onConnection) : subscribeBrowser(onEvent, onConnection);
}

function subscribeTauri(onEvent: (e: Event) => void, onConnection: (c: boolean) => void) {
  let disposed = false;
  const unlisteners = [
    listen<Event>("daemon-event", ({ payload }) => onEvent(payload)),
    listen<boolean>("daemon-connection", ({ payload }) => onConnection(payload)),
  ];
  invoke<boolean>("daemon_connected").then((c) => !disposed && onConnection(c));
  return () => {
    disposed = true;
    unlisteners.forEach((p) => p.then((un) => un()));
  };
}

function subscribeBrowser(onEvent: (e: Event) => void, onConnection: (c: boolean) => void) {
  let socket: WebSocket | null = null;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let delay = 1000;
  let disposed = false;

  const connect = () => {
    const scheme = location.protocol === "https:" ? "wss" : "ws";
    socket = new WebSocket(`${scheme}://${location.host}/v1/events`);
    socket.onopen = () => {
      delay = 1000;
      onConnection(true);
    };
    socket.onmessage = (msg) => {
      try {
        onEvent(JSON.parse(msg.data) as Event);
      } catch {
        // Ignore events this version doesn't understand.
      }
    };
    socket.onclose = () => {
      if (disposed) return;
      onConnection(false);
      retry = setTimeout(connect, delay);
      delay = Math.min(delay * 2, 10_000);
    };
  };
  connect();
  return () => {
    disposed = true;
    clearTimeout(retry);
    socket?.close();
  };
}

/** Opens an external link in the user's browser, never inside the app's own view. */
/** Whether the top bar must draw window buttons (Linux desktops without a tiling WM). */
export async function windowChrome(): Promise<{ controls: boolean }> {
  if (!isTauri) return { controls: false };
  try {
    return await invoke<{ controls: boolean }>("window_chrome");
  } catch {
    return { controls: false };
  }
}

/** Desktop only: the window buttons the top bar draws where the system doesn't. */
export function windowAction(action: "minimize" | "maximize" | "close") {
  if (!isTauri) return;
  const w = getCurrentWindow();
  void (action === "minimize" ? w.minimize() : action === "maximize" ? w.toggleMaximize() : w.close());
}

export function openExternal(url: string) {
  if (!/^(https?|mailto):/i.test(url)) return;
  if (isTauri) openUrl(url);
  else window.open(url, "_blank", "noopener,noreferrer");
}

/** Desktop only: opens the web interface in the default browser, already signed in. */
export async function openInBrowser(): Promise<void> {
  if (!isTauri) return;
  await invoke("open_in_browser");
}

/** Mirrors BackgroundStatus in src-tauri/src/daemon_process.rs. */
export interface BackgroundStatus {
  /** Whether the switch can be used here. */
  available: boolean;
  unavailable_reason: string | null;
  /** Starts at login and keeps running with no window open. */
  enabled: boolean;
  /** Starts at login, but nothing restarts it after a crash (no systemd). */
  no_restart: boolean;
}

/** Desktop only: whether Mimi keeps running in the background. Null in a browser. */
export async function backgroundStatus(): Promise<BackgroundStatus | null> {
  if (!isTauri) return null;
  return invoke<BackgroundStatus>("background_status");
}

/** Desktop only: turns background mode on or off. Rejects with a readable message. */
export async function setBackground(enabled: boolean): Promise<BackgroundStatus> {
  try {
    return await invoke<BackgroundStatus>("set_background", { enabled });
  } catch (err) {
    throw new Error((err as { message?: string })?.message ?? "Couldn't change this setting.");
  }
}
