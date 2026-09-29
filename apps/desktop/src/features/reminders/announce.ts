import { toast } from "sonner";

import type { Delivery } from "@/bindings/Delivery";
import { clock } from "@/features/reminders/time";
import { api } from "@/lib/api";

const fail = (e: Error) => toast.error(e.message);

/**
 * A reminder going off, or a routine finishing, while the app is open. The same thing
 * also reaches the user's messaging apps and the desktop's notifications; this is the in-app copy.
 */
export function announceDelivery(d: Delivery) {
  if (d.kind === "reminder" && (d.status === "delivered" || d.status === "late")) {
    toast(d.title, {
      id: d.id,
      description: d.status === "late" ? `Late: it was due at ${clock(d.due_at)}` : "Reminder",
      duration: 5 * 60_000,
      action: { label: "Done", onClick: () => api.reminderDone(d.id).catch(fail) },
      cancel: { label: "In 10 min", onClick: () => api.snoozeReminder(d.id, 10).catch(fail) },
    });
  } else if (d.kind === "routine" && (d.status === "delivered" || d.status === "late")) {
    toast.success(`${d.title} is ready`, {
      id: d.id,
      action: d.conversation_id
        ? { label: "Open", onClick: () => (location.hash = `#/chat/${d.conversation_id}`) }
        : undefined,
    });
  } else if (d.kind === "routine" && d.status === "failed") {
    toast.error(`${d.title} didn't finish`, { id: d.id, description: d.detail ?? undefined });
  }
}
