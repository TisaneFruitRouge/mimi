import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Send } from "lucide-react";
import { toast } from "sonner";

import type { CalendarEvent } from "@/bindings/CalendarEvent";
import type { InvitationOffer } from "@/bindings/InvitationOffer";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { whenLong } from "@/features/calendar/dates";
import { canInvite } from "@/features/calendar/event-sheet";
import { sentLabel, toastOffers, useSendOffer } from "@/features/calendar/invitations";
import { optimistic, seriesOf } from "@/features/calendar/optimistic";
import { api, keys } from "@/lib/api";

/**
 * "Send invitations…" on an event: the invitation for its guests as it is now, shown
 * before anything is emailed. Sending is the user's click on Send.
 */
export function SendInvitationsDialog({ event, onClose }: { event: CalendarEvent | null; onClose: () => void }) {
  return (
    <Dialog open={!!event} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-5 bg-canvas sm:max-w-[440px]">
        {event && <SendInvitations key={event.id} event={event} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function SendInvitations({ event, onDone }: { event: CalendarEvent; onDone: () => void }) {
  const [offer, setOffer] = useState<InvitationOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const send = useSendOffer();
  useEffect(() => {
    api
      .offerInvitations(event.id)
      .then(setOffer)
      .catch((e) => setError((e as Error).message));
  }, [event.id]);

  const go = async () => {
    if (!offer) return;
    setBusy(true);
    try {
      const sent = await send(offer);
      toast.success(sentLabel(sent));
      onDone();
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle className="type-title">Send invitations</DialogTitle>
        <DialogDescription>
          For “{event.title}”, {whenLong(event)}. Their calendar app lets them answer.
        </DialogDescription>
      </DialogHeader>
      {!offer && !error && (
        <p className="flex items-center gap-2 type-subhead text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" /> Getting them ready…
        </p>
      )}
      {offer && (
        <div className="flex flex-col gap-2">
          <div className="grouped flex flex-col">
            {offer.guests.map((g) => (
              <div key={g.email} className="flex min-h-[44px] flex-col justify-center px-4 py-1.5">
                <span className="truncate type-callout">{g.name ?? g.email}</span>
                {g.name && <span className="truncate type-footnote text-muted-foreground">{g.email}</span>}
              </div>
            ))}
          </div>
          <p className="type-footnote text-faint">
            {offer.from === null
              ? "To send invitations from here, connect your email in Settings › Connections."
              : (offer.from_note ?? `From ${offer.from}.`)}
          </p>
        </div>
      )}
      {error && <p className="type-subhead text-destructive">{error}</p>}
      <DialogFooter>
        <Button variant="ghost" onClick={onDone}>
          Not now
        </Button>
        {offer?.from === null ? (
          <Button
            onClick={() => {
              onDone();
              location.hash = "#/settings/connections";
            }}
          >
            Connect email
          </Button>
        ) : (
          <Button variant="lime" onClick={go} disabled={!offer || busy} className="min-w-[96px]">
            {busy ? <Loader2 className="animate-spin" /> : <Send />} Send
          </Button>
        )}
      </DialogFooter>
    </>
  );
}

/**
 * Deleting an event, confirmed first. A repeating one asks which: this time or every
 * time. It leaves the calendar at once; if the calendar refuses, it comes back. On
 * Google calendars Google tells the guests; elsewhere they aren't emailed, and the toast
 * offers to tell them once the calendar has answered.
 */
export function DeleteEventDialog({ event, onClose }: { event: CalendarEvent | null; onClose: () => void }) {
  const qc = useQueryClient();
  const send = useSendOffer();
  const remove = (all: boolean) => {
    if (!event) return;
    const series = seriesOf(event.id);
    const gone = (e: CalendarEvent) => (all ? seriesOf(e.id) === series : e.id === event.id);
    const headline = `“${event.title}” was deleted`;
    const shown = toast.success(headline);
    optimistic(qc, (events) => events.filter((e) => !gone(e)), () => api.removeEvent(event.id, all))
      .then((done) => {
        if (done.invitations.length > 0) toastOffers(headline, done.invitations, send, shown);
      })
      .catch((e) => toast.error(`“${event.title}” couldn't be deleted. ${(e as Error).message}`, { id: shown }));
  };
  const guests = event ? canInvite(event) : false;
  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars }).data ?? [];
  const calendar = calendars.find((c) => c.id === event?.calendar_id);
  // Signed in to Google: Google tells the guests itself.
  const google = !!calendar?.google && calendar.writable;
  return (
    <AlertDialog open={!!event} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete “{event?.title}”?</AlertDialogTitle>
          <AlertDialogDescription>
            {event?.repeats
              ? "It repeats. Delete only this time, or every time?"
              : `It's removed from ${event?.calendar}.`}
            {guests &&
              (google ? " Google tells your guests it's cancelled." : " Your guests aren't emailed: you can tell them next.")}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          {event?.repeats ? (
            <>
              <AlertDialogAction variant="secondary" onClick={() => remove(false)}>
                Only this time
              </AlertDialogAction>
              <AlertDialogAction variant="destructive" onClick={() => remove(true)}>
                Every time
              </AlertDialogAction>
            </>
          ) : (
            <AlertDialogAction variant="destructive" onClick={() => remove(false)}>
              Delete
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
