import { useQuery } from "@tanstack/react-query";

import type { MailDraft } from "@/bindings/MailDraft";
import { addSignature, draftKind, fromOf, signatureFor, swapSignature } from "@/features/mail/signature";
import { api, keys } from "@/lib/api";

/**
 * The user's signatures for the editors (`signature.ts` does the work). `sign` puts the
 * From address's signature into a draft that's just been started (a new message, a
 * reply, a forward, a draft the assistant wrote); `swap` changes it with the From.
 * Until the signatures have loaded (`ready`), drafts stay as they are.
 */
export function useSignature() {
  const sigs = useQuery({ queryKey: keys.mailSignatures, queryFn: api.mailSignatures, staleTime: 60_000 });
  const accounts = useQuery({ queryKey: keys.mailOverview(), queryFn: () => api.mailOverview() }).data?.accounts ?? [];
  const pick = (d: MailDraft) => signatureFor(sigs.data, fromOf(d, accounts), draftKind(d));
  return {
    ready: sigs.data !== undefined && accounts.length > 0,
    sign: (d: MailDraft) => addSignature(d, pick(d)),
    swap: (was: MailDraft, now: MailDraft) => swapSignature(now, pick(was), pick(now)),
  };
}
