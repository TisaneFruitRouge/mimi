import { useEffect, useState } from "react";
import { keepPreviousData, type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Loader2 } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MergeRequest } from "@/bindings/MergeRequest";
import type { MergeResult } from "@/bindings/MergeResult";
import type { PersonSummary } from "@/bindings/PersonSummary";
import { Grouped } from "@/components/page";
import { ChannelIcon, PersonAvatar, channelInfo } from "@/components/people";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { labelText } from "@/features/people/person-dialogs";
import { api, keys } from "@/lib/api";

/** Who a merge starts from: the person kept, and who is folded into them. */
export type MergePlan = { keep: string; others: string[] };

/** Numbers and addresses that tell two Sams apart, on one quiet line. */
export function reachLine(p: PersonSummary) {
  return p.reach.join(" · ") || p.nickname || "No number or address yet";
}

/**
 * "Merge with…": everyone else, searchable by name, number or address, to pick the
 * person who is the same as `person`. Arrow keys move, Enter picks.
 */
export function MergePicker({
  person,
  onClose,
  onPick,
}: {
  person: { id: string; name: string } | null;
  onClose: () => void;
  onPick: (other: string) => void;
}) {
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (person) setQuery("");
  }, [person]);
  const people = useQuery({
    queryKey: keys.peopleList(query),
    queryFn: () => api.people(query),
    placeholderData: keepPreviousData,
    enabled: !!person,
  });
  const others = (people.data ?? []).filter((p) => p.id !== person?.id);

  return (
    <Dialog open={!!person} onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        showCloseButton={false}
        className="top-[18%] translate-y-0 gap-0 overflow-hidden rounded-[20px] p-0 sm:max-w-[520px]"
      >
        <DialogHeader className="gap-0.5 px-4 pt-4 pb-3 text-left">
          <DialogTitle className="type-headline">Merge {person?.name} with…</DialogTitle>
          <DialogDescription className="type-subhead">Choose the contact that is the same person.</DialogDescription>
        </DialogHeader>
        {/* The daemon searches (names, numbers, addresses): no filtering here. */}
        <Command shouldFilter={false} loop>
          <CommandInput value={query} onValueChange={setQuery} placeholder="Name, number or email" />
          <CommandList className="max-h-[380px] p-1.5">
            <CommandEmpty>{people.isLoading ? "Looking…" : `No one matches “${query}”.`}</CommandEmpty>
            {others.map((p) => (
              <CommandItem key={p.id} value={p.id} onSelect={() => onPick(p.id)} className="gap-3 rounded-[10px] px-2.5 py-2">
                <PersonAvatar id={p.id} name={p.name} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate type-callout font-medium">{p.name}</span>
                  <span className="block truncate type-footnote text-muted-foreground">{reachLine(p)}</span>
                </span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}

/** Says a merge happened, with Undo (exact: everyone back under their own id). */
export function mergedToast(done: MergeResult, onUndone: (id: string) => void) {
  toast.success(`${done.person.name} is one contact now`, {
    duration: 8000,
    action: {
      label: "Undo",
      onClick: () =>
        api
          .undoMerge(done.merge_id)
          .then((p) => {
            toast.success("Separated again");
            onUndone(p.id);
          })
          .catch((e) => toast.error((e as Error).message)),
    },
  });
}

/** Takes merged-away people out of every cached list at once, before the daemon's event. */
function dropMerged(qc: QueryClient, ids: string[]) {
  qc.setQueriesData<PersonSummary[]>({ queryKey: ["people", "list"] }, (old) =>
    old?.filter((p) => !ids.includes(p.id)),
  );
}

/**
 * The confirmation before merging: the name to keep and every way to reach the merged
 * person, each once. A toast offers Undo right after.
 */
export function MergeDialog({
  plan,
  onClose,
  onMerged,
}: {
  plan: MergePlan | null;
  onClose: () => void;
  /** With the kept person's id: after the merge, and again after an undo. */
  onMerged: (id: string) => void;
}) {
  const qc = useQueryClient();
  // Keep the last plan while the sheet animates out.
  const [shown, setShown] = useState<MergePlan | null>(plan);
  if (plan && plan !== shown) setShown(plan);
  const request: MergeRequest | null = shown ? { keep: shown.keep, others: shown.others, name: null } : null;
  const preview = useQuery({
    queryKey: ["people", "merge-preview", shown?.keep, ...(shown?.others ?? [])],
    queryFn: () => api.mergePreview(request!),
    enabled: !!plan,
    retry: false,
  });
  const [choice, setChoice] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (plan) {
      setChoice(null);
      setTyped("");
    }
  }, [plan]);

  const data = preview.data;
  const names = data?.names ?? [];
  const picked = choice ?? names[0] ?? "";
  const name = (picked === "" ? typed : picked).trim();
  const count = shown ? shown.others.length + 1 : 0;

  const merge = async () => {
    if (!shown || !name) return;
    setBusy(true);
    try {
      const done = await api.mergeMany({ keep: shown.keep, others: shown.others, name });
      dropMerged(qc, shown.others);
      onClose();
      onMerged(done.person.id);
      mergedToast(done, onMerged);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={!!plan} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="gap-5 bg-canvas sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle className="type-title">Merge {count} contacts</DialogTitle>
          <DialogDescription>
            They become one person, with every way to reach them. Your address books aren't changed.
          </DialogDescription>
        </DialogHeader>

        {preview.isError ? (
          <p className="type-callout text-destructive">{(preview.error as Error).message}</p>
        ) : !data ? (
          <Loader2 className="mx-auto size-5 animate-spin text-faint" />
        ) : (
          <div className="flex max-h-[55vh] flex-col gap-5 overflow-y-auto">
            <div className="flex items-center gap-3">
              <div className="flex -space-x-2">
                {data.people.slice(0, 5).map((p) => (
                  <span key={p.id} className="rounded-full ring-2 ring-canvas">
                    <PersonAvatar id={p.id} name={p.name} />
                  </span>
                ))}
              </div>
              <p className="min-w-0 flex-1 truncate type-callout text-muted-foreground">
                {data.people.map((p) => p.name).join(", ")}
              </p>
            </div>

            <section className="flex flex-col gap-2.5">
              <h3 className="section-label">Name</h3>
              <div role="radiogroup" aria-label="Name" className="grouped flex flex-col">
                {names.map((n) => (
                  <NameChoice key={n} checked={picked === n} onSelect={() => setChoice(n)}>
                    {n}
                  </NameChoice>
                ))}
                <NameChoice checked={picked === ""} onSelect={() => setChoice("")}>
                  {picked === "" ? (
                    <Input
                      value={typed}
                      onChange={(e) => setTyped(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onKeyDown={(e) => e.key === "Enter" && merge()}
                      placeholder="Another name"
                      aria-label="Another name"
                      className="h-8"
                      autoFocus
                    />
                  ) : (
                    <span className="text-muted-foreground">Another name…</span>
                  )}
                </NameChoice>
              </div>
            </section>

            <section className="flex flex-col gap-2.5">
              <h3 className="section-label">How to reach them</h3>
              {data.handles.length === 0 ? (
                <p className="type-callout text-muted-foreground">No number or address yet.</p>
              ) : (
                <Grouped>
                  {data.handles.map((h) => (
                    <div key={h.id} className="flex min-h-[48px] items-center gap-3 px-4 py-2">
                      <ChannelIcon channel={h.channel} className="size-4" />
                      <div className="min-w-0 flex-1">
                        <div className="truncate type-callout">{h.value}</div>
                        <div className="truncate type-footnote text-muted-foreground">
                          {channelInfo[h.channel].label}
                          {h.label && ` · ${labelText(h.label)}`} ·{" "}
                          {h.source_id ? `from ${h.source_name}` : "added by you"}
                        </div>
                      </div>
                    </div>
                  ))}
                </Grouped>
              )}
            </section>

            <p className="type-footnote text-muted-foreground">
              Changed your mind later? Open the merged contact and choose “Not the same person” on
              any of its cards.
            </p>
          </div>
        )}

        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={merge} disabled={busy || !data || !name}>
            {busy && <Loader2 className="animate-spin" />} Merge
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function NameChoice({
  checked,
  onSelect,
  children,
}: {
  checked: boolean;
  onSelect: () => void;
  children: React.ReactNode;
}) {
  return (
    <div
      role="radio"
      aria-checked={checked}
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.target === e.currentTarget && (e.key === " " || e.key === "Enter")) {
          e.preventDefault();
          onSelect();
        }
      }}
      className={cn(
        "flex min-h-[48px] cursor-default items-center gap-3 px-4 py-2 type-callout outline-none transition-colors hover:bg-[rgb(118_118_128/0.06)] focus-visible:bg-[rgb(118_118_128/0.1)]",
      )}
    >
      <span className="min-w-0 flex-1 truncate">{children}</span>
      <Check className={cn("size-4 shrink-0 text-lime-deep", !checked && "invisible")} />
    </div>
  );
}
