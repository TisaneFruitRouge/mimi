import { createContext, useCallback, useContext } from "react";

/**
 * Lets whichever view is scrolling tell the shell whether content is under the top
 * bar, so the bar can fade in its material and hairline (like macOS toolbars).
 */
export const ScrollEdgeContext = createContext<(scrolled: boolean) => void>(() => {});

/** An `onScroll` handler that reports the scroll edge to the top bar. */
export function useScrollEdge() {
  const report = useContext(ScrollEdgeContext);
  return useCallback(
    (e: React.UIEvent<HTMLElement>) => report(e.currentTarget.scrollTop > 4),
    [report],
  );
}
