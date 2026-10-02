import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ImagePlus } from "lucide-react";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import type { MailSignature } from "@/bindings/MailSignature";
import type { MailSignatures } from "@/bindings/MailSignatures";
import type { NewMailAttachment } from "@/bindings/NewMailAttachment";
import { Grouped, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { MailBodyEditor } from "@/features/mail/body-editor";
import { readAttachments } from "@/features/mail/draft-attachments";
import { INLINE_PICTURES } from "@/features/mail/mail-text";
import { textHtml } from "@/features/mail/signature";
import { api, keys } from "@/lib/api";
import { useAssistantName } from "@/lib/queries";

/** How wide a signature's pictures show, and how many pixels they keep (sharp on fine screens). */
const PICTURE_WIDTH = 240;
const PICTURE_PIXELS = 480;

type SaveState = "idle" | "saving" | "saved" | "error";

/**
 * Settings › General › Signature: what goes at the end of the emails the user writes,
 * and of those their assistant writes for them. One for every address, or one per
 * address; written with formatting and a small picture (a logo). Saved shortly after
 * typing stops, like the Personality page.
 */
export function SignatureSettings() {
  const assistant = useAssistantName();
  const qc = useQueryClient();
  const saved = useQuery({ queryKey: keys.mailSignatures, queryFn: api.mailSignatures }).data;
  const overview = useQuery({ queryKey: keys.mailOverview(), queryFn: () => api.mailOverview() }).data;
  // What's being edited, ahead of what's saved.
  const [edit, setEdit] = useState<MailSignatures | null>(null);
  const [state, setState] = useState<SaveState>("idle");
  const timer = useRef<number | undefined>(undefined);
  const pending = useRef<MailSignatures | null>(null);

  const save = async (next: MailSignatures) => {
    window.clearTimeout(timer.current);
    pending.current = null;
    setState("saving");
    try {
      const out = await api.saveMailSignatures(await smaller(next));
      qc.setQueryData(keys.mailSignatures, out);
      setState("saved");
    } catch (e) {
      setState("error");
      toast.error((e as Error).message);
    }
  };
  const saveRef = useRef(save);
  saveRef.current = save;
  // Leaving the page mid-sentence still saves it.
  useEffect(
    () => () => {
      if (pending.current) void saveRef.current(pending.current);
    },
    [],
  );
  useEffect(() => {
    if (state !== "saved") return;
    const t = window.setTimeout(() => setState("idle"), 2000);
    return () => window.clearTimeout(t);
  }, [state]);

  if (!saved || !overview) return null;
  const s = edit ?? saved;
  const addresses = [...new Set(overview.accounts.flatMap((a) => a.addresses.map((x) => x.email.toLowerCase())))];
  const one = s.same_for_all || addresses.length < 2;

  const change = (next: MailSignatures, now = false) => {
    setEdit(next);
    pending.current = next;
    window.clearTimeout(timer.current);
    if (now) void save(next);
    else timer.current = window.setTimeout(() => void save(next), 800);
  };
  const sigOf = (address: string | null) =>
    address === null ? s.all : (s.addresses.find((a) => a.address === address)?.signature ?? s.all);
  const setSig = (address: string | null, sig: MailSignature, now = false) =>
    change(
      address === null
        ? { ...s, all: sig }
        : { ...s, addresses: [...s.addresses.filter((a) => a.address !== address), { address, signature: sig }] },
      now,
    );

  return (
    <Section
      title="Signature"
      action={
        state !== "idle" && (
          <span className={state === "error" ? "type-footnote text-destructive" : "type-footnote text-muted-foreground"}>
            {state === "saving" ? "Saving…" : state === "saved" ? "Saved" : "Couldn't save"}
          </span>
        )
      }
    >
      <p className="type-subhead text-muted-foreground">
        Added at the end of the emails you write, and of those {assistant} writes for you. You can still change or
        remove it in any email before you send it.
      </p>
      {one ? (
        <SignatureEditor key="all" sig={s.all} onChange={(sig, now) => setSig(null, sig, now)} />
      ) : (
        addresses.map((address) => (
          <SignatureEditor
            key={address}
            label={address}
            sig={sigOf(address)}
            onChange={(sig, now) => setSig(address, sig, now)}
          />
        ))
      )}
      <Grouped>
        {addresses.length > 1 && (
          <Row
            title="The same for every address"
            detail={one ? "One signature for all your addresses" : "Each address signs its own way"}
            className="[&_.truncate]:whitespace-normal"
            trailing={
              <Switch
                checked={s.same_for_all}
                onCheckedChange={(on) => change({ ...s, same_for_all: on }, true)}
                aria-label="The same signature for every address"
              />
            }
          />
        )}
        <Row
          title="Also in replies and forwards"
          detail="Under what you write, above the message you forward."
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <Switch
              checked={s.in_replies}
              onCheckedChange={(on) => change({ ...s, in_replies: on }, true)}
              aria-label="Also in replies and forwards"
            />
          }
        />
      </Grouped>
    </Section>
  );
}

/** One signature, written in the mail editor, with a button to add a picture. */
function SignatureEditor({
  sig,
  label,
  onChange,
}: {
  sig: MailSignature;
  label?: string;
  onChange: (sig: MailSignature, now?: boolean) => void;
}) {
  const picker = useRef<HTMLInputElement>(null);
  const draft: MailDraft = {
    connection_id: null,
    from: null,
    to: [],
    cc: [],
    bcc: [],
    subject: "",
    body: sig.text,
    html: sig.html,
    reply_to: null,
    forward_of: null,
    attachments: sig.pictures,
  };
  const addPicture = async (file: File) => {
    if (!INLINE_PICTURES.test(file.type)) {
      toast.error("Choose a picture: PNG, JPEG, GIF or WebP.");
      return;
    }
    try {
      const [read] = await readAttachments([file], []);
      const { picture, width } = await shrink({ ...read, content_id: `${randomId()}@signature` });
      const alt = picture.name.replace(/[&<>"]/g, "");
      const img = `<div><img src="cid:${picture.content_id}" alt="${alt}" width="${width}"></div>`;
      const empty = !sig.text.trim();
      onChange(
        {
          text: empty ? `[image: ${alt}]` : `${sig.text}\n[image: ${alt}]`,
          html: (empty ? "" : (sig.html ?? textHtml(sig.text))) + img,
          pictures: [...sig.pictures, picture],
        },
        true,
      );
    } catch (e) {
      toast.error((e as Error).message);
    }
  };
  return (
    <div className="surface overflow-hidden">
      {label && <div className="px-4 pt-3 type-subhead font-medium text-muted-foreground">{label}</div>}
      <div className="[&_.ProseMirror]:min-h-24">
        <MailBodyEditor
          draft={draft}
          onChange={(d) =>
            onChange({ text: d.body, html: d.html, pictures: d.attachments.filter((a) => a.content_id) })
          }
          onFiles={() => toast.error("Only pictures can go in a signature.")}
          placeholder="Your name, and how people can reach you"
        />
      </div>
      <div className="flex items-center justify-between gap-3 border-t-[0.5px] border-separator px-4 py-2">
        <span className="type-footnote text-faint">A logo or small picture is kept small, to fit in every email.</span>
        <Button variant="ghost" size="sm" onClick={() => picker.current?.click()}>
          <ImagePlus /> Add a picture
        </Button>
        <input
          ref={picker}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) void addPicture(file);
          }}
        />
      </div>
    </div>
  );
}

/** Every picture of every signature made small, and shown no wider than `PICTURE_WIDTH`. */
async function smaller(s: MailSignatures): Promise<MailSignatures> {
  const one = async (sig: MailSignature): Promise<MailSignature> => {
    if (sig.pictures.length === 0) return sig;
    const pictures = await Promise.all(sig.pictures.map(async (p) => (await shrink(p)).picture));
    let html = sig.html;
    if (html) {
      const doc = new DOMParser().parseFromString(html, "text/html");
      for (const img of Array.from(doc.body.querySelectorAll("img"))) {
        const w = Number(img.getAttribute("width")) || PICTURE_WIDTH;
        img.setAttribute("width", String(Math.min(w, PICTURE_WIDTH)));
      }
      html = doc.body.innerHTML;
    }
    return { ...sig, html, pictures };
  };
  return {
    ...s,
    all: await one(s.all),
    addresses: await Promise.all(s.addresses.map(async (a) => ({ ...a, signature: await one(a.signature) }))),
  };
}

/**
 * A picture at most `PICTURE_PIXELS` wide (a phone photo would be megabytes in every
 * email), and how wide it shows. Small pictures stay as they are.
 */
async function shrink(p: NewMailAttachment): Promise<{ picture: NewMailAttachment; width: number }> {
  const bytes = Uint8Array.from(atob(p.data), (c) => c.charCodeAt(0));
  const bitmap = await createImageBitmap(new Blob([bytes], { type: p.mime ?? "image/png" }));
  try {
    const width = Math.min(bitmap.width, PICTURE_WIDTH);
    if (bitmap.width <= PICTURE_PIXELS && bytes.length <= 100 * 1024) return { picture: p, width };
    const scale = Math.min(1, PICTURE_PIXELS / bitmap.width);
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    canvas.getContext("2d")!.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const mime = p.mime === "image/jpeg" ? "image/jpeg" : "image/png";
    const data = canvas.toDataURL(mime, 0.85).replace(/^data:[^,]*,/, "");
    return { picture: { ...p, mime, data }, width };
  } finally {
    bitmap.close();
  }
}

function randomId() {
  return Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, "0")).join("");
}
