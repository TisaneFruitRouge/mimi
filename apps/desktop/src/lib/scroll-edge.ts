import { createContext, useCallback, useContext, useSyncExternalStore } from "react";

/**
 * Whether content is under the top bar, so the bar can fade in its material and
 * hairline (like macOS toolbars). A tiny store rather than React state in the shell:
 * scrolling past the edge must re-render only the bar's material, not every screen.
 */
export function createScrollEdge() {
  let scrolled = false;
  const listeners = new Set<() => void>();
  return {
    get: () => scrolled,
    /** Notifies only on a change, so it's cheap to call on every scroll event. */
    set(next: boolean) {
      if (next === scrolled) return;
      scrolled = next;
      for (const l of listeners) l();
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
export type ScrollEdge = ReturnType<typeof createScrollEdge>;

export const ScrollEdgeContext = createContext<ScrollEdge>(createScrollEdge());

/** An `onScroll` handler that reports the scroll edge to the top bar. */
export function useScrollEdge() {
  const edge = useContext(ScrollEdgeContext);
  return useCallback((e: React.UIEvent<HTMLElement>) => edge.set(e.currentTarget.scrollTop > 4), [edge]);
}

/** Whether the view under the bar is scrolled off its top edge. */
export function useScrolled() {
  const edge = useContext(ScrollEdgeContext);
  return useSyncExternalStore(edge.subscribe, edge.get);
}
