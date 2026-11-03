import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, ExternalLink, Loader2, MailX } from "lucide-react";
import { toast } from "sonner";

import type { MailUnsubscribe } from "@/bindings/MailUnsubscribe";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api, keys } from "@/lib/api";
import { useAssistantName } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

/**
 * "Unsubscribe" in the reader's toolbar, for a conversation whose latest email comes
 * from a mailing list. A sheet says what will happen; nothing happens without the
 * user's click. Afterwards it shows "Unsubscribed" and offers to archive.
 */
export function UnsubscribeButton({ thread, onArchived }: { thread: number; onArchived: () => void }) {
  const qc = useQueryClient();
  const offer = useQuery({
    queryKey: keys.mailUnsubscribe(thread),
    queryFn: () => api.mailUnsubscribe(thread),
    retry: false,
    staleTime: 60_000,
  });
  const [open, setOpen] = useState(false);
  const o = offer.data;
  if (!o) return null;
  return (
    <>
      {o.done ? (
        <Button variant="ghost" size="sm" className="text-muted-foreground" onClick={() => setOpen(true)}>
          <Check /> Unsubscribed
        </Button>
      ) : (
        <Button variant="secondary" size="sm" onClick={() => setOpen(true)}>
          <MailX /> Unsubscribe
        </Button>
      )}
      <AlertDialog open={open} onOpenChange={setOpen}>
        <AlertDialogContent>
          {o.done ? (
            <Done
              thread={thread}
              offer={o}
              onArchived={() => {
                setOpen(false);
                onArchived();
              }}
            />
          ) : (
            <Confirm thread={thread} offer={o} onDone={(next) => qc.setQueryData(keys.mailUnsubscribe(thread), next)} />
          )}
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function Confirm({
  thread,
  offer,
  onDone,
}: {
  thread: number;
  offer: MailUnsubscribe;
  onDone: (next: MailUnsubscribe) => void;
}) {
  const assistant = useAssistantName();
  const [busy, setBusy] = useState(false);
  const what = {
    one_click: `${assistant} will ask ${offer.domain} to stop sending you these emails. Nothing else about you is shared.`,
    email: `${assistant} will send a short email from your address to ${offer.domain}, asking them to stop sending you these emails. A copy goes to Sent.`,
    website: `${offer.domain} asks you to unsubscribe on its website. It opens in your browser, where you may need to confirm.`,
  }[offer.method];
  const go = async () => {
    setBusy(true);
    try {
      const next = await api.unsubscribeMail(thread);
      if (next.method === "website" && next.url) openExternal(next.url);
      onDone(next);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>Unsubscribe from {offer.sender}?</AlertDialogTitle>
        <AlertDialogDescription>{what}</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel disabled={busy}>Cancel</AlertDialogCancel>
        <Button onClick={go} disabled={busy}>
          {busy ? <Loader2 className="animate-spin" /> : offer.method === "website" && <ExternalLink />}
          {offer.method === "website" ? "Open website" : "Unsubscribe"}
        </Button>
      </AlertDialogFooter>
    </>
  );
}

function Done({ thread, offer, onArchived }: { thread: number; offer: MailUnsubscribe; onArchived: () => void }) {
  const assistant = useAssistantName();
  const [archiving, setArchiving] = useState<"one" | "all" | null>(null);
  const method = offer.done?.method ?? offer.method;
  const title = method === "website" ? "Finish in your browser" : "Unsubscribed";
  const said = {
    one_click: `${offer.domain} was asked to stop sending you these emails.`,
    email: `${assistant} emailed ${offer.domain} to ask them to stop. You'll find it in Sent.`,
    website: `If ${offer.domain}'s page asks you to confirm, do it there.`,
  }[method];
  const archive = async (all: boolean) => {
    setArchiving(all ? "all" : "one");
    try {
      if (all) {
        const { archived } = await api.archiveMailList(thread);
        toast.success(archived === 1 ? "Archived 1 conversation" : `Archived ${archived} conversations`);
      } else {
        await api.archiveMail(thread);
        toast.success("Archived");
      }
      onArchived();
    } catch (e) {
      toast.error((e as Error).message);
      setArchiving(null);
    }
  };
  const many = offer.in_inbox > 1;
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>{title}</AlertDialogTitle>
        <AlertDialogDescription>
          {said} Some senders take a few days to stop.
          {offer.in_inbox > 0 && " You can archive what they already sent."}
        </AlertDialogDescription>
      </AlertDialogHeader>
      <div className="flex flex-col gap-2">
        {many && (
          <Button onClick={() => archive(true)} disabled={!!archiving}>
            {archiving === "all" && <Loader2 className="animate-spin" />}
            Archive all {offer.in_inbox} from {offer.sender}
          </Button>
        )}
        {offer.in_inbox > 0 && (
          <Button variant={many ? "secondary" : "default"} onClick={() => archive(false)} disabled={!!archiving}>
            {archiving === "one" && <Loader2 className="animate-spin" />}
            Archive this conversation
          </Button>
        )}
        {method === "website" && offer.url && (
          <Button variant="secondary" onClick={() => openExternal(offer.url!)}>
            <ExternalLink /> Open their website again
          </Button>
        )}
        <AlertDialogCancel disabled={!!archiving}>Done</AlertDialogCancel>
      </div>
    </>
  );
}
