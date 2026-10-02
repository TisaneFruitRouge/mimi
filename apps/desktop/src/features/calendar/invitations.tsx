import { useState } from "react";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { Check, Loader2, Send } from "lucide-react";
import { toast } from "sonner";

import type { InvitationOffer } from "@/bindings/InvitationOffer";
import { Button } from "@/components/ui/button";
import { api, keys } from "@/lib/api";

/**
 * Invitations Mimi can email for an event. Calendars email nobody: after an event with
 * guests is saved, changed or removed, these are offered, and only the user's click (or
 * a request they approved) sends them, from their own email account.
 */

/** "Sam", "Sam and Léa", "Sam, Léa and Tom". */
export function joinNames(names: string[]) {
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

export function guestNames(offer: InvitationOffer) {
  return joinNames(offer.guests.map((g) => g.name ?? g.email));
}

/** What the button says it will do. */
export function offerLabel(offer: InvitationOffer) {
  const who = guestNames(offer);
  switch (offer.kind) {
    case "invite":
      return offer.guests.length === 1 ? `Send the invitation to ${who}` : `Send invitations to ${who}`;
    case "update":
      return `Let ${who} know about the change`;
    case "cancel":
      return `Tell ${who} it's cancelled`;
    case "uninvite":
      return `Withdraw the invitation for ${who}`;
  }
}

/** What happened once it was sent. */
export function sentLabel(offer: InvitationOffer) {
  const who = guestNames(offer);
  switch (offer.kind) {
    case "invite":
      return offer.guests.length === 1 ? `Invitation sent to ${who}` : `Invitations sent to ${who}`;
    case "update":
      return `${who} got the new details`;
    case "cancel":
      return `Told ${who} it's cancelled`;
    case "uninvite":
      return `Invitation withdrawn for ${who}`;
  }
}

/** Sends an offer (the user's own click) and keeps every view of it up to date. */
export function useSendOffer() {
  const qc = useQueryClient();
  return async (offer: InvitationOffer) => {
    const sent = await api.sendInvitations(offer.id);
    qc.setQueryData(keys.invitation(offer.id), sent);
    return sent;
  };
}

/** One offer: a quiet button that sends it, or what happened. */
export function InvitationRow({ offer: initial }: { offer: InvitationOffer }) {
  const [offer, setOffer] = useState(initial);
  const [busy, setBusy] = useState(false);
  const send = useSendOffer();
  const current = initial.sent_at !== null ? initial : offer;

  if (current.sent_at !== null)
    return (
      <p className="flex items-center gap-1.5 type-subhead text-muted-foreground">
        <Check className="size-3.5 shrink-0 text-private" strokeWidth={2.6} />
        <span className="min-w-0">
          {sentLabel(current)}
          {current.from && <span className="text-faint"> · from {current.from}</span>}
        </span>
      </p>
    );

  if (current.from === null)
    return (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 type-subhead text-muted-foreground">
        <span className="min-w-0">
          Nothing was emailed to {guestNames(current)}. To send invitations from here, connect your email.
        </span>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            location.hash = "#/settings/connections";
          }}
        >
          Connect email
        </Button>
      </div>
    );

  const go = async () => {
    setBusy(true);
    try {
      setOffer(await send(current));
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex flex-col items-start gap-1">
      <Button variant="secondary" size="sm" onClick={go} disabled={busy} className="max-w-full">
        {busy ? <Loader2 className="animate-spin" /> : <Send />}
        <span className="truncate">{offerLabel(current)}</span>
      </Button>
      <span className="type-footnote text-faint">{current.from_note ?? `From ${current.from}.`}</span>
    </div>
  );
}

/** Offers by id (from a tool's result in the chat), each read fresh from the daemon. */
export function InvitationOffers({ ids }: { ids: string[] }) {
  const offers = useQueries({
    queries: ids.map((id) => ({ queryKey: keys.invitation(id), queryFn: () => api.invitation(id) })),
  });
  const loaded = offers.flatMap((q) => (q.data ? [q.data] : []));
  if (loaded.length === 0) return null;
  return (
    <div className="flex flex-col gap-2.5">
      {loaded.map((o) => (
        <InvitationRow key={o.id} offer={o} />
      ))}
    </div>
  );
}

/** The question a toast asks about an offer, the guests already named above it. */
const question: Record<InvitationOffer["kind"], string> = {
  invite: "Send them the invitation?",
  update: "Let them know about the change?",
  cancel: "Tell them it's cancelled?",
  uninvite: "Tell them they're no longer invited?",
};

/**
 * After something the user did in the panel, when the sheet or dialog is gone: a toast
 * per offer (`headline` says what happened), whose Send button sends it. The first
 * replaces the toast `replacing`, if one already said what happened.
 */
export function toastOffers(
  headline: string,
  offers: InvitationOffer[],
  send: (o: InvitationOffer) => Promise<InvitationOffer>,
  replacing?: string | number,
) {
  for (const [i, offer] of offers.entries()) {
    const id = i === 0 ? replacing : undefined;
    const nothing = `Nothing was emailed to ${guestNames(offer)}.`;
    if (offer.from === null) {
      toast(headline, {
        id,
        description: `${nothing} To send invitations from here, connect your email in Settings › Connections.`,
      });
      continue;
    }
    toast(headline, {
      id,
      description: `${nothing} ${question[offer.kind]}`,
      duration: 20_000,
      action: {
        label: "Send",
        onClick: () =>
          send(offer)
            .then((sent) => toast.success(sentLabel(sent)))
            .catch((e) => toast.error((e as Error).message)),
      },
    });
  }
}
