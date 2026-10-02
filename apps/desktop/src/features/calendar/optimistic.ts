import type { QueryClient } from "@tanstack/react-query";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { CalendarEvents } from "@/bindings/CalendarEvents";
import { keys } from "@/lib/api";

/**
 * Changes and deletions from the panel show at once: every loaded week is updated
 * before the daemon (and Google, or the CalDAV server) answers. When it has answered,
 * the weeks are read again for the real thing (an event that moved gets a new id); when
 * it fails, they go back to how they were.
 */

const EVENTS = ["calendar", "events"] as const;

// Writes still waiting for an answer: the weeks are read again only once all are done,
// so an early answer doesn't bring back what a later write already hid.
let pending = 0;

/** Every occurrence of a series shares everything in its id but the start. */
export function seriesOf(id: string) {
  return id.replace(/^ev:\d+:/, "");
}

export async function optimistic<T>(
  qc: QueryClient,
  change: (events: CalendarEvent[], from: number, to: number) => CalendarEvent[],
  write: () => Promise<T>,
): Promise<T> {
  pending += 1;
  // A read already on its way would overwrite the change with what was there before.
  await qc.cancelQueries({ queryKey: EVENTS });
  const before = qc.getQueriesData<CalendarEvents>({ queryKey: EVENTS });
  for (const [key, data] of before) {
    if (!data) continue;
    const [, , from, to] = key as ReturnType<typeof keys.events>;
    qc.setQueryData<CalendarEvents>(key, { ...data, events: change(data.events, from, to) });
  }
  try {
    return await write();
  } catch (e) {
    if (pending === 1) for (const [key, data] of before) qc.setQueryData(key, data);
    throw e;
  } finally {
    pending -= 1;
    if (pending === 0) qc.invalidateQueries({ queryKey: keys.calendar });
  }
}
