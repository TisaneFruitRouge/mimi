import { useLayoutEffect, useRef, useState } from "react";
import type { Editor } from "@tiptap/core";
import Image from "@tiptap/extension-image";
import { Placeholder } from "@tiptap/extensions";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { Bold, Italic, Link, List, ListOrdered, TextQuote, Underline } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailDraft } from "@/bindings/MailDraft";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Popover, PopoverAnchor, PopoverContent } from "@/components/ui/popover";
import { readAttachments } from "@/features/mail/draft-attachments";
import {
  INLINE_PICTURES,
  docOfText,
  editorHtml,
  emailHtml,
  isPlain,
  picturesOf,
  textOfDoc,
} from "@/features/mail/mail-text";
import { hasMod, mod } from "@/lib/platform";
import "./body-editor.css";

/** Our own HTML keeps its spaces as typed. */
const PARSE = { preserveWhitespace: "full" } as const;

/** Pictures shown no wider than this when they're put in (the recipient's app may shrink them). */
const PICTURE_WIDTH = 600;

/**
 * A picture in the text. Only pictures put in here (with a content id) are kept: a
 * pasted web page's pictures would load from elsewhere, so they're left out.
 */
const Picture = Image.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      cid: {
        default: null,
        parseHTML: (el) => el.getAttribute("data-cid"),
        renderHTML: (attrs) => (attrs.cid ? { "data-cid": attrs.cid } : {}),
      },
    };
  },
  parseHTML() {
    return [{ tag: "img[data-cid]" }];
  },
});

/** Links go to the web or to an email address, nowhere else. */
const LINKABLE = /^(https?:\/\/|mailto:)/i;

function extensions() {
  return [
    StarterKit.configure({
      code: false,
      codeBlock: false,
      heading: false,
      horizontalRule: false,
      strike: false,
      link: {
        openOnClick: false,
        autolink: true,
        linkOnPaste: true,
        defaultProtocol: "https",
        isAllowedUri: (url) => LINKABLE.test(url) || !/^[a-z][a-z0-9+.-]*:/i.test(url),
      },
    }),
    Picture.configure({ inline: true, allowBase64: true }),
    Placeholder.configure({ placeholder: "Write your message" }),
  ];
}

/**
 * The text of an email being written, with formatting (bold, italic, underline, links,
 * lists, quotes) and pictures where they were pasted or dropped. It keeps the draft's
 * plain-text `body` up to date, and its `html` once there's any formatting: a message
 * without any goes as plain text, as before. Other files dropped or pasted here go to
 * `onFiles`, to be attached.
 */
export function MailBodyEditor({
  draft,
  onChange,
  onFiles,
  autoFocus,
}: {
  draft: MailDraft;
  onChange: (d: MailDraft) => void;
  onFiles: (files: File[]) => void;
  autoFocus?: boolean;
}) {
  const latest = useRef({ draft, onChange, onFiles });
  latest.current = { draft, onChange, onFiles };
  // What the editor shows, as text and HTML: anything else in the draft was changed
  // from outside (the assistant wrote it again), and is loaded.
  const shown = useRef<{ html?: string; body: string } | null>(null);
  const [linking, setLinking] = useState(false);

  const emit = (editor: Editor) => {
    const doc = editor.getJSON();
    const body = textOfDoc(doc);
    const html = isPlain(doc) ? undefined : emailHtml(editor.getHTML());
    shown.current = { html, body };
    const d = latest.current.draft;
    latest.current.onChange({
      ...d,
      body,
      html,
      attachments: [...d.attachments.filter((a) => !a.content_id), ...picturesOf(doc)],
    });
  };

  // Pictures are read in the background, then put where they were pasted or dropped.
  const place = async (files: File[], at?: number) => {
    try {
      const read = await readAttachments(files, latest.current.draft.attachments);
      const nodes = await Promise.all(
        read.map(async (a, i) => ({
          type: "image",
          attrs: {
            src: `data:${a.mime};base64,${a.data}`,
            alt: a.name,
            cid: `${randomId()}@inline`,
            width: await widthOf(files[i]),
          },
        })),
      );
      if (!editor || editor.isDestroyed) return;
      const chain = editor.chain().focus();
      if (at === undefined) chain.insertContent(nodes).run();
      else chain.insertContentAt(Math.min(at, editor.state.doc.content.size), nodes).run();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  // Read once: the editor is made with them.
  const [initial] = useState(() => ({
    extensions: extensions(),
    content: draft.html ? editorHtml(draft.html, draft.attachments) : docOfText(draft.body),
  }));
  const editor = useEditor({
    ...initial,
    parseOptions: PARSE,
    autofocus: autoFocus ? "start" : false,
    shouldRerenderOnTransaction: false,
    editorProps: {
      attributes: {
        "aria-label": "Message",
        class: "min-h-40 w-full px-4 py-3 type-body leading-[1.5] outline-none",
      },
      handleKeyDown: (_view, event) => {
        if (hasMod(event) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "k") {
          // A link, not the app's search.
          event.stopPropagation();
          setLinking(true);
          return true;
        }
        return false;
      },
      handlePaste: (_view, event) => {
        const data = event.clipboardData;
        const files = Array.from(data?.files ?? []);
        if (!data || files.length === 0) return false;
        const pictures = files.filter((f) => INLINE_PICTURES.test(f.type));
        const others = files.filter((f) => !INLINE_PICTURES.test(f.type));
        const html = data.getData("text/html");
        const words = html ? (new DOMParser().parseFromString(html, "text/html").body.textContent ?? "") : data.getData("text/plain");
        if (words.trim()) {
          // Text that comes with files (copied from a document): the text pastes, and
          // files that aren't a picture of it are attached.
          if (others.length > 0) latest.current.onFiles(others);
          return false;
        }
        if (pictures.length > 0) void place(pictures);
        if (others.length > 0) latest.current.onFiles(others);
        return true;
      },
      handleDrop: (view, event, _slice, moved) => {
        const files = Array.from(event.dataTransfer?.files ?? []);
        if (moved || files.length === 0) return false;
        const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
        const pictures = files.filter((f) => INLINE_PICTURES.test(f.type));
        const others = files.filter((f) => !INLINE_PICTURES.test(f.type));
        if (pictures.length > 0) void place(pictures, at);
        if (others.length > 0) latest.current.onFiles(others);
        return true;
      },
    },
    onUpdate: ({ editor }) => emit(editor),
  });

  // Loads the draft when it changed from outside, and checks the formatting still says
  // what the text does: when only the text was replaced, the text wins.
  useLayoutEffect(() => {
    if (!editor) return;
    const d = latest.current.draft;
    const now = shown.current;
    if (now && now.html === d.html && now.body === d.body) return;
    if (now) {
      const content = d.html ? editorHtml(d.html, d.attachments) : docOfText(d.body);
      editor.commands.setContent(content, { emitUpdate: false, parseOptions: PARSE });
    }
    shown.current = { html: d.html, body: d.body };
    if (d.html && textOfDoc(editor.getJSON()) !== d.body.replace(/\r\n?/g, "\n")) {
      editor.commands.setContent(docOfText(d.body), { emitUpdate: false });
      emit(editor);
    }
  }, [editor, draft.html, draft.body]);

  return (
    <div className="mail-body">
      <Toolbar editor={editor} linking={linking} setLinking={setLinking} />
      <EditorContent editor={editor} />
    </div>
  );
}

function randomId() {
  // Not `crypto.randomUUID`: a browser reaching the daemon over plain http has none.
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** How wide a picture is put in: its own width, up to `PICTURE_WIDTH`. */
async function widthOf(file: File): Promise<number | null> {
  try {
    const bitmap = await createImageBitmap(file);
    const width = Math.min(bitmap.width, PICTURE_WIDTH);
    bitmap.close();
    return width;
  } catch {
    return null;
  }
}

/** The formatting buttons, quiet until used. */
function Toolbar({
  editor,
  linking,
  setLinking,
}: {
  editor: Editor;
  linking: boolean;
  setLinking: (open: boolean) => void;
}) {
  const on = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      underline: e.isActive("underline"),
      link: e.isActive("link"),
      bullets: e.isActive("bulletList"),
      numbers: e.isActive("orderedList"),
      quote: e.isActive("blockquote"),
    }),
  });
  const chain = () => editor.chain().focus();
  return (
    <div className="flex h-9 items-center gap-0.5 px-3 shadow-[inset_0_-0.5px_0_var(--separator)]" role="toolbar" aria-label="Formatting">
      <Tool label={`Bold (${mod}B)`} active={on.bold} onClick={() => chain().toggleBold().run()}>
        <Bold />
      </Tool>
      <Tool label={`Italic (${mod}I)`} active={on.italic} onClick={() => chain().toggleItalic().run()}>
        <Italic />
      </Tool>
      <Tool label={`Underline (${mod}U)`} active={on.underline} onClick={() => chain().toggleUnderline().run()}>
        <Underline />
      </Tool>
      <LinkTool editor={editor} active={on.link} open={linking} setOpen={setLinking} />
      <span className="mx-1 h-4 w-px bg-separator" />
      <Tool label="Bulleted list" active={on.bullets} onClick={() => chain().toggleBulletList().run()}>
        <List />
      </Tool>
      <Tool label="Numbered list" active={on.numbers} onClick={() => chain().toggleOrderedList().run()}>
        <ListOrdered />
      </Tool>
      <Tool label="Quote" active={on.quote} onClick={() => chain().toggleBlockquote().run()}>
        <TextQuote />
      </Tool>
    </div>
  );
}

function Tool({
  label,
  active,
  onClick,
  children,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      // The text keeps its selection.
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        "flex size-7 items-center justify-center rounded-[7px] text-faint transition-colors hover:bg-fill hover:text-foreground [&_svg]:size-4",
        active && "bg-fill text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** "example.com" → https://example.com, "sam@example.com" → mailto:; null for anything else. */
export function linkOf(raw: string): string | null {
  const v = raw.trim();
  if (!v || /\s/.test(v)) return null;
  if (LINKABLE.test(v)) return v;
  if (/^[^@/:]+@[^@/:]+\.[^@/:]+$/.test(v)) return `mailto:${v}`;
  if (/^[a-z][a-z0-9+.-]*:/i.test(v)) return null;
  return /^[^/]+\.[a-z]{2,}([/?#:].*)?$/i.test(v) ? `https://${v}` : null;
}

/** The link button, and the little sheet to type or change the address. */
function LinkTool({
  editor,
  active,
  open,
  setOpen,
}: {
  editor: Editor;
  active: boolean;
  open: boolean;
  setOpen: (open: boolean) => void;
}) {
  const [value, setValue] = useState("");
  const [wrong, setWrong] = useState(false);
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) {
    // Opening starts from the link under the cursor, if any.
    setWasOpen(open);
    if (open) {
      const href = (editor.getAttributes("link").href as string | undefined) ?? "";
      setValue(href.replace(/^mailto:/i, ""));
      setWrong(false);
    }
  }
  const apply = () => {
    if (!value.trim()) {
      editor.chain().focus().extendMarkRange("link").unsetLink().run();
      setOpen(false);
      return;
    }
    const href = linkOf(value);
    if (!href) {
      setWrong(true);
      return;
    }
    const chain = editor.chain().focus().extendMarkRange("link");
    if (editor.state.selection.empty && !editor.isActive("link")) {
      // What's typed next isn't part of the link.
      chain
        .insertContent({ type: "text", text: value.trim(), marks: [{ type: "link", attrs: { href } }] })
        .unsetMark("link")
        .run();
    } else {
      chain.setLink({ href }).run();
    }
    setOpen(false);
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <span className="flex">
          <Tool label={`Link (${mod}K)`} active={active} onClick={() => setOpen(!open)}>
            <Link />
          </Tool>
        </span>
      </PopoverAnchor>
      <PopoverContent
        align="start"
        className="w-80 p-3"
        onCloseAutoFocus={(e) => {
          e.preventDefault();
          editor.commands.focus();
        }}
      >
        <form
          className="flex flex-col gap-2.5"
          onSubmit={(e) => {
            e.preventDefault();
            apply();
          }}
        >
          <Input
            autoFocus
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              setWrong(false);
            }}
            placeholder="Web or email address"
            aria-label="Link address"
            aria-invalid={wrong}
          />
          {wrong && <p className="type-footnote text-destructive">That isn't a web or email address.</p>}
          <div className="flex justify-end gap-2">
            {active && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  editor.chain().focus().extendMarkRange("link").unsetLink().run();
                  setOpen(false);
                }}
              >
                Remove link
              </Button>
            )}
            <Button type="submit" size="sm" variant="secondary">
              {active ? "Change link" : "Add link"}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}
