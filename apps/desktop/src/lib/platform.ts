import { isTauri } from "@/lib/transport";

export const isMac = /mac/i.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);

/** The desktop app on macOS draws its own title bar area (traffic lights over our bar). */
export const macOverlayTitleBar = isTauri && isMac;

/** Label for the primary modifier key: ⌘ on macOS, Ctrl elsewhere. */
export const mod = isMac ? "⌘" : "Ctrl ";

/** Whether an event has the primary modifier pressed (⌘ on macOS, Ctrl elsewhere). */
export const hasMod = (e: KeyboardEvent | React.KeyboardEvent) => (isMac ? e.metaKey : e.ctrlKey);
