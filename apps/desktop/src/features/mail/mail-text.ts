import type { JSONContent } from "@tiptap/core";

import type { NewMailAttachment } from "@/bindings/NewMailAttachment";

/**
 * Turning what's written in the mail editor into what's sent, and back.
 *
 * A message is lines, as in a mail app: Enter starts a new line and an empty line is a
 * blank one. The plain-text version (`MailDraft.body`, what models, approval cards and
 * checks read) has one line per paragraph, "- " and "1. " for lists, "> " for quotes and
 * "[image: name]" for a picture. The HTML (`MailDraft.html`) has one `<div>` per line,
 * like Gmail and Apple Mail write, and pictures as `cid:` addresses of the draft's inline
 * attachments; the daemon cleans it again before it goes.
 */

/** Picture types that go in the text; anything else is attached. */
export const INLINE_PICTURES = /^image\/(png|jpeg|gif|webp)$/;

/** Plain text as the editor's document: one paragraph per line, exactly as written. */
export function docOfText(text: string): JSONContent {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  return {
    type: "doc",
    content: lines.map((line) =>
      line ? { type: "paragraph", content: [{ type: "text", text: line }] } : { type: "paragraph" },
    ),
  };
}

/** Whether a document has no formatting at all, so it can go as plain text. */
export function isPlain(doc: JSONContent): boolean {
  return (doc.content ?? []).every(
    (block) =>
      block.type === "paragraph" &&
      (block.content ?? []).every(
        (n) => n.type === "hardBreak" || (n.type === "text" && (n.marks ?? []).length === 0),
      ),
  );
}

/** The plain-text version of a document. */
export function textOfDoc(doc: JSONContent): string {
  // Typed runs of spaces come as no-break spaces: they're spaces in the text.
  return blocks(doc.content ?? []).join("\n").replace(/\u00a0/g, " ");
}

function blocks(nodes: JSONContent[]): string[] {
  return nodes.flatMap(block);
}

function block(node: JSONContent): string[] {
  switch (node.type) {
    case "paragraph":
      return inline(node.content ?? []).split("\n");
    case "bulletList":
    case "orderedList": {
      let n = Number(node.attrs?.start ?? 1) || 1;
      return (node.content ?? []).flatMap((item) => {
        const marker = node.type === "bulletList" ? "- " : `${n++}. `;
        const lines = blocks(item.content ?? []);
        if (lines.length === 0) return [marker.trimEnd()];
        return lines.map((l, i) => (i === 0 ? marker : " ".repeat(marker.length)) + l);
      });
    }
    case "blockquote":
      return blocks(node.content ?? []).map((l) => (l ? `> ${l}` : ">"));
    default:
      return blocks(node.content ?? []);
  }
}

/** A paragraph's text; a link reads "words (address)" unless the words are the address. */
function inline(nodes: JSONContent[]): string {
  let out = "";
  let link: { href: string; text: string } | null = null;
  const close = () => {
    if (!link) return;
    const bare = link.href.replace(/^(https?:\/\/|mailto:)/i, "").replace(/\/$/, "");
    const same = [link.href, bare].includes(link.text.trim().replace(/\/$/, ""));
    out += same ? link.text : `${link.text} (${link.href})`;
    link = null;
  };
  for (const n of nodes) {
    const href = n.marks?.find((m) => m.type === "link")?.attrs?.href as string | undefined;
    if (link && link.href !== href) close();
    if (n.type === "text" && href) {
      link ??= { href, text: "" };
      link.text += n.text ?? "";
      continue;
    }
    close();
    if (n.type === "text") out += n.text ?? "";
    else if (n.type === "hardBreak") out += "\n";
    else if (n.type === "image") out += `[image: ${n.attrs?.alt || "picture"}]`;
  }
  close();
  return out;
}

/** The pictures in a document, once each, as the draft's inline attachments. */
export function picturesOf(doc: JSONContent): NewMailAttachment[] {
  const seen = new Map<string, NewMailAttachment>();
  const walk = (n: JSONContent) => {
    const cid = n.attrs?.cid as string | undefined;
    const src = n.attrs?.src as string | undefined;
    const data = src?.match(/^data:([^;,]+);base64,(.*)$/s);
    if (n.type === "image" && cid && data && !seen.has(cid)) {
      seen.set(cid, {
        name: (n.attrs?.alt as string) || "Picture",
        mime: data[1],
        data: data[2],
        content_id: cid,
        source: null,
      });
    }
    n.content?.forEach(walk);
  };
  walk(doc);
  return [...seen.values()];
}

/**
 * The editor's HTML as it's sent: a `<div>` per line (an empty one keeps its height),
 * pictures as `cid:` addresses, and nothing the editor adds for itself.
 */
export function emailHtml(editorHtml: string): string {
  const doc = new DOMParser().parseFromString(`<body>${editorHtml}</body>`, "text/html");
  for (const p of Array.from(doc.body.querySelectorAll("p"))) {
    const div = doc.createElement("div");
    div.append(...Array.from(p.childNodes));
    if (!div.hasChildNodes()) div.append(doc.createElement("br"));
    p.replaceWith(div);
  }
  for (const img of Array.from(doc.body.querySelectorAll("img"))) {
    const cid = img.getAttribute("data-cid");
    if (!cid) {
      img.remove();
      continue;
    }
    img.setAttribute("src", `cid:${cid}`);
    img.removeAttribute("data-cid");
  }
  for (const a of Array.from(doc.body.querySelectorAll("a"))) {
    for (const name of ["target", "rel", "class"]) a.removeAttribute(name);
  }
  // Runs of spaces would shrink to one in the recipient's mail app.
  for (const t of textNodes(doc.body)) {
    t.data = t.data
      .replace(/\u00a0/g, " ")
      .replace(/ {2,}/g, (run) => run.replace(/ /g, (_, i: number) => (i % 2 ? " " : "\u00a0")));
  }
  return doc.body.innerHTML;
}

/** A draft's HTML as the editor reads it: lines as paragraphs, pictures shown. */
export function editorHtml(html: string, attachments: NewMailAttachment[]): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const pictures = new Map(attachments.filter((a) => a.content_id).map((a) => [a.content_id!, a]));
  for (const img of Array.from(doc.body.querySelectorAll("img"))) {
    const cid = img.getAttribute("src")?.replace(/^cid:/i, "");
    const a = cid ? pictures.get(cid) : undefined;
    if (!a || !a.mime || !INLINE_PICTURES.test(a.mime)) {
      img.remove();
      continue;
    }
    img.setAttribute("src", `data:${a.mime};base64,${a.data}`);
    img.setAttribute("data-cid", a.content_id!);
  }
  for (const div of Array.from(doc.body.querySelectorAll("div"))) {
    const p = doc.createElement("p");
    const only = div.childNodes.length === 1 ? div.firstChild : null;
    if (!(only instanceof HTMLBRElement)) p.append(...Array.from(div.childNodes));
    div.replaceWith(p);
  }
  for (const t of textNodes(doc.body)) t.data = t.data.replace(/\u00a0/g, " ");
  return doc.body.innerHTML;
}

function textNodes(root: Node): Text[] {
  const walker = root.ownerDocument!.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const out: Text[] = [];
  while (walker.nextNode()) out.push(walker.currentNode as Text);
  return out;
}
