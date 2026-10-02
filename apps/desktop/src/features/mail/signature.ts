import type { MailDraft } from "@/bindings/MailDraft";
import type { MailSignature } from "@/bindings/MailSignature";
import type { MailSignatures } from "@/bindings/MailSignatures";
import type { NewMailAttachment } from "@/bindings/NewMailAttachment";

/**
 * The user's signature in the emails they write (Settings › General › Signature; the
 * daemon keeps it, `mail::signature`). Pure functions, kept apart so they can be tried
 * without a page; `use-signature.ts` gives them to the editors.
 *
 * A signature is just part of the draft: below what the user writes, after a `-- ` line
 * (a `<div>-- </div>` in the HTML), above anything forwarded. New messages, replies and
 * forwards start with the signature of their From address; picking another From swaps
 * it, unless the user changed it; they may change or delete it in any one email.
 * The plain text and the HTML are changed together, line for line, so the editor reads
 * them as the same message (`body-editor.tsx`: when they differ, the text wins).
 */

/** The line before a signature, as mail apps write it. */
export const SEPARATOR = "-- ";
const SEPARATOR_HTML = "<div>-- </div>";
const BLANK_HTML = "<div><br></div>";
/** The first line of a forwarded message (`forwardOf` in `mail-view.tsx`): the signature goes above it. */
export const FORWARD_HEADER = "---------- Forwarded message ----------";
const FORWARD_HTML = `<div>${FORWARD_HEADER}</div>`;

export type DraftKind = "new" | "reply" | "forward";

/** A reply, a forward, or a new message. */
export function draftKind(d: MailDraft): DraftKind {
  if (d.reply_to !== null) return "reply";
  if (d.forward_of !== null || lines(d.body).includes(FORWARD_HEADER)) return "forward";
  return "new";
}

export function isEmpty(sig: MailSignature): boolean {
  return !sig.text.trim() && sig.pictures.length === 0;
}

/**
 * The signature for an email from `from`: the address's own, else the one for all. None
 * in replies and forwards when the user leaves it out of them, and none when empty.
 * The same choice as the daemon's `signature::pick`.
 */
export function signatureFor(
  sigs: MailSignatures | undefined,
  from: string | null,
  kind: DraftKind,
): MailSignature | null {
  if (!sigs || (kind !== "new" && !sigs.in_replies)) return null;
  const address = (from ?? "").trim().toLowerCase();
  const sig = sigs.same_for_all
    ? sigs.all
    : (sigs.addresses.find((a) => a.address === address)?.signature ?? sigs.all);
  return isEmpty(sig) ? null : sig;
}

/** The address a draft goes from: the one chosen, else its account's, else the first account's. */
export function fromOf(d: MailDraft, accounts: { connection_id: string; email: string }[]): string | null {
  if (d.from) return d.from.toLowerCase();
  const account = accounts.find((a) => a.connection_id === d.connection_id) ?? accounts[0];
  return account ? account.email.toLowerCase() : null;
}

/** A draft with the signature added at the end of what's written (above anything forwarded). */
export function addSignature(d: MailDraft, sig: MailSignature | null): MailDraft {
  if (!sig || isEmpty(sig)) return d;
  const text = splitText(d.body);
  if (text.sig !== null) return d;
  // The assistant's drafts may already end with it.
  const before = stripDuplicate(text.before, sig.text);
  const html = d.html ? splitHtml(d.html) : null;
  return build(d, { ...text, before }, html && before !== text.before ? { ...html, before: textHtml(before) } : html, sig);
}

/**
 * The draft once its From changed from `was` to `now`: the signature swapped when it's
 * still the old address's as it was put in; left alone when the user changed it or took
 * it out. One is added when the old address had none.
 */
export function swapSignature(d: MailDraft, was: MailSignature | null, now: MailSignature | null): MailDraft {
  const text = splitText(d.body);
  // Taken out by the user; or there was none.
  if (text.sig === null) return was ? d : addSignature(d, now);
  // Changed by the user, or written by them.
  if (!was || !same(text.sig, was.text)) return d;
  return build(d, text, d.html ? splitHtml(d.html) : null, now);
}

/**
 * The draft with new text above its signature ("Draft it for me"): the signature stays
 * as it is in this email, and a copy of it at the end of the new text is taken out.
 */
export function replaceText(d: MailDraft, newText: string): MailDraft {
  const text = splitText(d.body);
  const html = d.html ? splitHtml(d.html) : null;
  const sig: MailSignature | null =
    text.sig === null
      ? null
      : {
          text: text.sig,
          html: html?.sig ?? undefined,
          pictures: d.attachments.filter((a) => a.content_id && html?.sig?.includes(`cid:${a.content_id}`)),
        };
  const before = sig ? stripDuplicate(newText, sig.text) : newText.replace(/\s+$/, "");
  return build(d, { ...text, before }, html ? { ...html, before: textHtml(before) } : null, sig);
}

/** The text without the signature (anything forwarded stays): to tell whether anything was written. */
export function withoutSignature(body: string): string {
  const t = splitText(body);
  return t.after === null ? t.before : `${t.before}\n\n${t.after}`;
}

/** The signature's text in a draft as it is now, or null when there's none. */
export function signatureIn(body: string): string | null {
  return splitText(body).sig;
}

/**
 * A text without a signature its writer (a model) already ended it with: a short block
 * under its own `--` line, or the signature's own lines. A sign-off ("Best, Vincent")
 * stays. The same as the daemon's `signature::strip_duplicate`.
 */
export function stripDuplicate(body: string, signature: string): string {
  const ls = lines(body.replace(/\s+$/, ""));
  for (let i = ls.length - 1; i >= 0; i--) {
    if (ls[i].trimEnd() === "--") {
      if (ls.length - i - 1 <= 6) return ls.slice(0, i).join("\n").replace(/\s+$/, "");
      break;
    }
  }
  const sig = lines(signature)
    .map(norm)
    .filter((l) => l && !l.startsWith("[image:"));
  if (sig.length === 0) return ls.join("\n");
  let j = ls.length;
  let k = sig.length;
  while (k > 0 && j > 0) {
    const l = norm(ls[j - 1]);
    if (!l) {
      j--;
      continue;
    }
    if (l !== sig[k - 1]) break;
    k--;
    j--;
  }
  return k === 0 ? ls.slice(0, j).join("\n").replace(/\s+$/, "") : ls.join("\n");
}

/** Plain text as the mail editor writes it in HTML: a `<div>` per line. */
export function textHtml(text: string): string {
  return lines(text)
    .map((l) => (l ? `<div>${escape(l)}</div>` : BLANK_HTML))
    .join("");
}

// --- Inside ------------------------------------------------------------------------------

/** A message cut in three: what's written, the signature (`null`: none), what's forwarded (`null`: nothing). */
interface Parts {
  before: string;
  sig: string | null;
  after: string | null;
}

function lines(text: string): string[] {
  return text.replace(/\r\n?/g, "\n").split("\n");
}

function norm(line: string): string {
  return line.split(/\s+/).filter(Boolean).join(" ").toLowerCase();
}

function same(a: string, b: string): boolean {
  const flat = (s: string) => lines(s).map(norm).filter(Boolean).join("\n");
  return flat(a) === flat(b);
}

/** Blank lines at the end left out; at least one line. */
function trimEnd(ls: string[]): string[] {
  let n = ls.length;
  while (n > 1 && ls[n - 1].trim() === "") n--;
  return ls.slice(0, n);
}

function splitText(body: string): Parts {
  const ls = lines(body);
  const at = ls.indexOf(FORWARD_HEADER);
  const limit = at < 0 ? ls.length : at;
  const start = limit > 0 ? ls.lastIndexOf(SEPARATOR, limit - 1) : -1;
  return {
    before: trimEnd(ls.slice(0, start < 0 ? limit : start)).join("\n"),
    sig: start < 0 ? null : trimEnd(ls.slice(start + 1, limit)).join("\n"),
    after: at < 0 ? null : ls.slice(at).join("\n"),
  };
}

function trimHtml(html: string): string {
  let h = html;
  while (h.endsWith(BLANK_HTML)) h = h.slice(0, -BLANK_HTML.length);
  return h || BLANK_HTML;
}

function splitHtml(html: string): Parts {
  const at = html.indexOf(FORWARD_HTML);
  const limit = at < 0 ? html.length : at;
  const start = html.lastIndexOf(SEPARATOR_HTML, limit - SEPARATOR_HTML.length);
  return {
    before: trimHtml(html.slice(0, start < 0 ? limit : start)),
    sig: start < 0 ? null : trimHtml(html.slice(start + SEPARATOR_HTML.length, limit)),
    after: at < 0 ? null : html.slice(at),
  };
}

/**
 * The draft put back together with `sig` as its signature: the text, and the HTML when
 * it has one or the signature brings formatting. Pictures in the text are those the
 * HTML shows; other files stay.
 */
function build(d: MailDraft, text: Parts, html: Parts | null, sig: MailSignature | null): MailDraft {
  const body =
    text.before + (sig ? `\n\n${SEPARATOR}\n${sig.text}` : "") + (text.after !== null ? `\n\n${text.after}` : "");
  let out: string | undefined;
  if (html || sig?.html) {
    const h = html ?? {
      before: textHtml(text.before),
      sig: null,
      after: text.after === null ? null : textHtml(text.after),
    };
    out =
      h.before +
      (sig ? BLANK_HTML + SEPARATOR_HTML + (sig.html ?? textHtml(sig.text)) : "") +
      (h.after !== null ? BLANK_HTML + h.after : "");
  }
  const seen = new Set<string>();
  const pictures: NewMailAttachment[] = [];
  for (const a of [...d.attachments.filter((a) => a.content_id), ...(sig?.pictures ?? [])]) {
    const id = a.content_id!;
    if (!seen.has(id) && out?.includes(`cid:${id}`)) {
      seen.add(id);
      pictures.push(a);
    }
  }
  return { ...d, body, html: out, attachments: [...d.attachments.filter((a) => !a.content_id), ...pictures] };
}

function escape(line: string): string {
  return line
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/ {2,}/g, (run) => run.replace(/ /g, (_, i: number) => (i % 2 ? " " : "&nbsp;")));
}
