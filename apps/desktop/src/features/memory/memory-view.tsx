import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { BookOpen, Cloud, Loader2, Plus, ShieldCheck, Trash2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MemoryNoteSummary } from "@/bindings/MemoryNoteSummary";
import type { MemorySource } from "@/bindings/MemorySource";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { api, keys } from "@/lib/api";
import { useActiveModel } from "@/lib/queries";

/** The library's folders, as the user sees them. Mirrors `memory::FOLDERS` in the daemon. */
const FOLDERS: { id: string; label: string }[] = [
  { id: "people", label: "People" },
  { id: "habits", label: "Habits and routines" },
  { id: "preferences", label: "Likes and preferences" },
  { id: "places", label: "Places" },
  { id: "work", label: "Work" },
  { id: "interests", label: "Interests" },
  { id: "health", label: "Health" },
  { id: "notes", label: "Other" },
];

const folderOf = (path: string) => path.split("/")[0] ?? "notes";

const sourceLabel: Record<MemorySource, string> = {
  you: "Written by you",
  assistant: "Noted during a conversation",
  learned: "Learned from your conversations",
};

/** What the assistant knows about the user, all of it editable. */
export function MemoryView() {
  const memory = useQuery({ queryKey: keys.memory, queryFn: api.memory });
  const active = useActiveModel();
  const [selected, setSelected] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [forgetting, setForgetting] = useState(false);

  const notes = memory.data?.notes ?? [];
  const groups = useMemo(
    () =>
      FOLDERS.map((f) => ({ ...f, notes: notes.filter((n) => folderOf(n.path) === f.id) })).filter(
        (g) => g.notes.length > 0,
      ),
    [notes],
  );
  // Keep a selection while notes come and go.
  useEffect(() => {
    if (selected && !notes.some((n) => n.path === selected)) setSelected(null);
    if (!selected && notes.length > 0) setSelected(notes[0].path);
  }, [notes, selected]);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[960px] flex-col gap-8 px-6 pt-6 pb-12">
        <div className="flex flex-col gap-1.5">
          <h1 className="text-[30px] font-semibold tracking-[-0.03em]">Memory</h1>
          <p className="max-w-xl text-[15px] text-muted-foreground">
            What your assistant knows about you and the people in your life. It's kept on this
            computer, and you can change or delete any of it.
          </p>
        </div>

        {active?.provider.locality === "cloud" && (
          <div className="flex items-start gap-3 rounded-2xl bg-cloud-soft px-5 py-4 text-[13.5px] leading-relaxed text-cloud">
            <Cloud className="mt-0.5 size-4 shrink-0" />
            You're using {active.provider.name}, a cloud service. The parts of your memory that
            matter to a message are sent to it along with the message.
          </div>
        )}

        {memory.isLoading ? (
          <Skeleton className="h-40 rounded-2xl" />
        ) : (
          memory.data && (
            <>
              <LearningCard learning={memory.data.learning} />
              <ProfileCard profile={memory.data.profile} limit={memory.data.profile_limit} />
            </>
          )
        )}

        <section className="flex flex-col gap-3">
          <div className="flex items-end justify-between">
            <SectionLabel>Notes</SectionLabel>
            <Button variant="ghost" size="sm" onClick={() => setAdding(true)} className="-mr-2 text-muted-foreground">
              <Plus /> Add a note
            </Button>
          </div>
          {!memory.isLoading && notes.length === 0 ? (
            <div className="flex items-center gap-4 rounded-2xl border border-dashed bg-background/60 px-5 py-5">
              <div className="flex size-10 items-center justify-center rounded-xl bg-subtle text-faint">
                <BookOpen className="size-5" />
              </div>
              <div>
                <p className="text-[14.5px] font-medium">Nothing remembered yet</p>
                <p className="text-[13px] text-muted-foreground">
                  Tell your assistant about the people in your life, your routines and what you
                  like. It keeps notes here.
                </p>
              </div>
            </div>
          ) : (
            <div className="grid min-h-80 grid-cols-[260px_1fr] overflow-hidden rounded-2xl border bg-background shadow-xs">
              <nav aria-label="Notes" className="flex flex-col gap-4 overflow-y-auto border-r bg-subtle/50 p-3">
                {groups.map((g) => (
                  <div key={g.id} className="flex flex-col gap-0.5">
                    <span className="px-2 pb-1 text-[12px] font-medium text-faint">{g.label}</span>
                    {g.notes.map((n) => (
                      <NoteButton key={n.path} note={n} active={n.path === selected} onClick={() => setSelected(n.path)} />
                    ))}
                  </div>
                ))}
              </nav>
              <div className="min-w-0">
                {selected ? (
                  <NoteEditor key={selected} path={selected} />
                ) : (
                  <p className="p-6 text-[13.5px] text-muted-foreground">Choose a note.</p>
                )}
              </div>
            </div>
          )}
        </section>

        <section className="flex items-center justify-between gap-4 rounded-2xl border px-5 py-4">
          <div>
            <p className="text-[14.5px] font-medium">Forget everything</p>
            <p className="text-[13px] text-muted-foreground">
              Deletes all memories. Your conversations are kept.
            </p>
          </div>
          <Button
            variant="outline"
            className="text-destructive hover:text-destructive"
            onClick={() => setForgetting(true)}
            disabled={!memory.data || (notes.length === 0 && !memory.data.profile)}
          >
            Forget everything
          </Button>
        </section>
      </div>

      <AddNoteDialog
        open={adding}
        onOpenChange={setAdding}
        onAdded={(path) => {
          setAdding(false);
          setSelected(path);
        }}
      />
      <AlertDialog open={forgetting} onOpenChange={setForgetting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Forget everything?</AlertDialogTitle>
            <AlertDialogDescription>
              Your assistant will no longer know anything it learned about you or the people in
              your life. This can't be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() =>
                api
                  .forgetEverything()
                  .then(() => toast.success("Everything was forgotten"))
                  .catch((e) => toast.error(e.message))
              }
            >
              Forget everything
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <h2 className="font-mono text-[11.5px] font-medium tracking-[0.06em] text-faint uppercase">
      {children}
    </h2>
  );
}

function LearningCard({ learning }: { learning: boolean }) {
  const [busy, setBusy] = useState(false);
  const toggle = async () => {
    setBusy(true);
    try {
      await api.setMemoryLearning(!learning);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-center gap-4 rounded-2xl border bg-background px-5 py-4 shadow-xs">
      <ShieldCheck className="size-5 shrink-0 text-private" />
      <div className="min-w-0 flex-1">
        <p className="text-[14.5px] font-medium">Learn from conversations</p>
        <p className="text-[13px] leading-relaxed text-muted-foreground">
          {learning
            ? "Your assistant notices lasting things you tell it, like who's who in your life, and remembers them."
            : "Paused. Nothing new is remembered, but your assistant still uses what it already knows."}
        </p>
      </div>
      <button
        role="switch"
        aria-checked={learning}
        aria-label="Learn from conversations"
        disabled={busy}
        onClick={toggle}
        className={cn(
          "relative h-7 w-12 shrink-0 rounded-full transition-colors disabled:opacity-60",
          learning ? "bg-private" : "bg-input",
        )}
      >
        <span
          className={cn(
            "absolute top-1 left-1 size-5 rounded-full bg-white shadow-sm transition-transform",
            learning && "translate-x-5",
          )}
        />
      </button>
    </div>
  );
}

function ProfileCard({ profile, limit }: { profile: string; limit: number }) {
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const value = draft ?? profile;
  const count = [...value].length;
  const save = async () => {
    setBusy(true);
    try {
      await api.saveMemoryProfile(value);
      setDraft(null);
      toast.success("Saved");
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="flex flex-col gap-3">
      <SectionLabel>About you</SectionLabel>
      <div className="flex flex-col gap-3 rounded-2xl border bg-background p-5 shadow-xs">
        <p className="text-[13px] text-muted-foreground">
          The essentials your assistant always keeps in mind. Details live in the notes below.
        </p>
        <Textarea
          value={value}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Nothing yet. Your assistant fills this in as you talk, or you can write a few lines about yourself."
          aria-label="About you"
          className="min-h-32 text-[14.5px] leading-relaxed"
        />
        <div className="flex items-center justify-between">
          <span className={cn("text-[12.5px]", count > limit ? "text-destructive" : "text-faint")}>
            {count} of {limit} characters
          </span>
          <div className="flex gap-2">
            {draft !== null && (
              <Button variant="ghost" size="sm" onClick={() => setDraft(null)}>
                Cancel
              </Button>
            )}
            <Button size="sm" onClick={save} disabled={draft === null || busy || count > limit}>
              {busy && <Loader2 className="animate-spin" />} Save
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}

function NoteButton({
  note,
  active,
  onClick,
}: {
  note: MemoryNoteSummary;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-current={active ? "true" : undefined}
      className={cn(
        "flex flex-col rounded-lg px-2 py-1.5 text-left transition-colors",
        active ? "bg-background shadow-xs" : "hover:bg-background/70",
      )}
    >
      <span className="truncate text-[13.5px] font-medium">{note.title}</span>
      {note.preview && <span className="truncate text-[12px] text-faint">{forYou(note.preview)}</span>}
    </button>
  );
}

function NoteEditor({ path }: { path: string }) {
  const note = useQuery({ queryKey: keys.memoryNote(path), queryFn: () => api.memoryNote(path) });
  const [title, setTitle] = useState<string | null>(null);
  const [body, setBody] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState(false);

  if (note.isLoading || !note.data) return <Skeleton className="m-5 h-48 rounded-xl" />;
  const n = note.data;
  const dirty = (title !== null && title !== n.title) || (body !== null && body !== n.body);

  const save = async () => {
    setBusy(true);
    try {
      await api.saveMemoryNote(path, body ?? n.body, title ?? n.title);
      setTitle(null);
      setBody(null);
      toast.success("Saved");
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col gap-3 p-5">
      <Input
        value={title ?? n.title}
        onChange={(e) => setTitle(e.target.value)}
        aria-label="Title"
        className="h-auto border-0 px-0 text-[18px] font-semibold shadow-none focus-visible:ring-0"
      />
      <p className="text-[12.5px] text-faint">
        {sourceLabel[n.source]} · {relativeDay(n.updated_at)}
      </p>
      <Textarea
        value={body ?? n.body}
        onChange={(e) => setBody(e.target.value)}
        aria-label="Note"
        className="min-h-48 flex-1 text-[14.5px] leading-relaxed"
      />
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" className="text-muted-foreground" onClick={() => setDeleting(true)}>
          <Trash2 /> Delete
        </Button>
        <div className="flex gap-2">
          {dirty && (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setTitle(null);
                setBody(null);
              }}
            >
              Cancel
            </Button>
          )}
          <Button size="sm" onClick={save} disabled={!dirty || busy}>
            {busy && <Loader2 className="animate-spin" />} Save
          </Button>
        </div>
      </div>
      <AlertDialog open={deleting} onOpenChange={setDeleting}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{n.title}”?</AlertDialogTitle>
            <AlertDialogDescription>Your assistant will forget what this note says.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              onClick={() => api.deleteMemoryNote(path).catch((e) => toast.error(e.message))}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function AddNoteDialog({
  open,
  onOpenChange,
  onAdded,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAdded: (path: string) => void;
}) {
  const [folder, setFolder] = useState("people");
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);

  const add = async () => {
    const name = title.trim();
    if (!name) return;
    setBusy(true);
    try {
      // The daemon turns the name into a tidy path (people/léa.md).
      const note = await api.saveMemoryNote(`${folder}/${name}`, body, name);
      setTitle("");
      setBody("");
      onAdded(note.path);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add a note</DialogTitle>
          <DialogDescription>Something your assistant should know.</DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
        >
          <div className="grid grid-cols-[1fr_1.4fr] gap-3">
            <div className="flex flex-col gap-1.5">
              <Label>Kind</Label>
              <Select value={folder} onValueChange={setFolder}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {FOLDERS.map((f) => (
                    <SelectItem key={f.id} value={f.id}>
                      {f.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="memory-title">{folder === "people" ? "Name" : "Title"}</Label>
              <Input
                id="memory-title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={folder === "people" ? "Sam" : "Mornings"}
                autoFocus
              />
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="memory-body">What to remember</Label>
            <Textarea
              id="memory-body"
              value={body}
              onChange={(e) => setBody(e.target.value)}
              placeholder={"- My brother\n- Birthday on 3 May\n- Loves climbing"}
              className="min-h-28"
            />
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy || !title.trim() || !body.trim()}>
              {busy && <Loader2 className="animate-spin" />} Add
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Notes speak of "the user"; a preview reads better addressed to the reader. */
function forYou(text: string) {
  return text
    .replace(/\bThe user's\b/g, "Your")
    .replace(/\bthe user's\b/g, "your")
    .replace(/\bThe user is\b/g, "You are")
    .replace(/\bthe user is\b/g, "you are")
    .replace(/\bThe user\b/g, "You")
    .replace(/\bthe user\b/g, "you");
}

function relativeDay(ms: number) {
  const days = Math.floor((Date.now() - ms) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 30) return `${days} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" });
}
