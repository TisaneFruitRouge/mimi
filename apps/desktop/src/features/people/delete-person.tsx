import { useState } from "react";
import { type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, UserX } from "lucide-react";
import { toast } from "sonner";

import type { Person } from "@/bindings/Person";
import type { PersonSummary } from "@/bindings/PersonSummary";
import { Grouped, IconTile, Row } from "@/components/page";
import { PersonAvatar } from "@/components/people";
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
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { api, keys } from "@/lib/api";
import { useAssistantName } from "@/lib/queries";

/** Who is about to be deleted: enough to name them before their details load. */
export type Doomed = { id: string; name: string };

/** What deleting someone does, in plain words. */
function consequences(name: string, p: Person | undefined, assistant: string) {
  const kept = `What ${assistant} remembers about them is kept.`;
  // Only cards from address books can be brought back; what was added by hand can't.
  if (p && !p.sources.some((s) => s.source_id !== null)) {
    return `${name} and the ways to reach them you added will be deleted. ${kept} This can't be undone.`;
  }
  const own = p?.handles.some((h) => h.source_id === null)
    ? " The numbers and addresses you added here are deleted."
    : "";
  return (
    `${name} will be removed from Mimi only. Your address books and email aren't changed, ` +
    `and ${name} won't reappear when they refresh.${own} ${kept} ` +
    "You can bring them back later from Removed contacts."
  );
}

/** Takes someone out of every cached list at once, before the daemon's event arrives. */
function dropFromLists(qc: QueryClient, id: string) {
  qc.setQueriesData<PersonSummary[]>({ queryKey: ["people", "list"] }, (old) => old?.filter((p) => p.id !== id));
  qc.removeQueries({ queryKey: keys.person(id) });
}

/** Brings someone back, then says so (or why not). */
export async function restorePerson(id: string, name: string, onRestored?: (id: string) => void) {
  try {
    const p = await api.restorePerson(id);
    toast.success(`${p.name || name} is back`);
    onRestored?.(p.id);
  } catch (e) {
    toast.error((e as Error).message);
  }
}

/**
 * The macOS-style confirmation for deleting someone. Deleting removes them from Mimi
 * only: address books and mail accounts are never changed.
 */
export function DeletePersonDialog({
  person,
  onClose,
  onDeleted,
  onRestored,
}: {
  person: Doomed | null;
  onClose: () => void;
  /** Called once they're gone, before the list refetches. */
  onDeleted: (id: string) => void;
  onRestored: (id: string) => void;
}) {
  const qc = useQueryClient();
  const assistant = useAssistantName();
  const details = useQuery({
    queryKey: keys.person(person?.id ?? ""),
    queryFn: () => api.person(person!.id),
    enabled: !!person,
    retry: false,
  });
  // Keep the last person while the dialog animates out.
  const [shown, setShown] = useState<Doomed | null>(person);
  if (person && person.id !== shown?.id) setShown(person);
  const name = shown?.name ?? "";

  const remove = async () => {
    if (!shown) return;
    const { id } = shown;
    try {
      const removed = await api.removePerson(id);
      dropFromLists(qc, id);
      onDeleted(id);
      toast.success(
        removed ? `${name} was removed from Mimi` : `${name} was deleted`,
        removed
          ? { action: { label: "Undo", onClick: () => restorePerson(removed.id, name, onRestored) } }
          : undefined,
      );
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <AlertDialog open={!!person} onOpenChange={(open) => !open && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete {name}?</AlertDialogTitle>
          <AlertDialogDescription>
            {consequences(name, person ? details.data : undefined, assistant)}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={remove}>
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

const shortDate = (ms: number) =>
  new Date(ms).toLocaleDateString([], {
    day: "numeric",
    month: "short",
    year: new Date(ms).getFullYear() === new Date().getFullYear() ? undefined : "numeric",
  });

/** People deleted from Mimi, each with a way to bring them back. */
export function RemovedContactsDialog({
  open,
  onOpenChange,
  onRestored,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRestored: (id: string) => void;
}) {
  const removed = useQuery({ queryKey: keys.removedPeople, queryFn: api.removedPeople, enabled: open });
  const [busy, setBusy] = useState<string | null>(null);
  const list = removed.data ?? [];

  const bringBack = async (id: string, name: string) => {
    setBusy(id);
    await restorePerson(id, name, (restored) => {
      onOpenChange(false);
      onRestored(restored);
    });
    setBusy(null);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-5 bg-canvas sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle className="type-title">Removed contacts</DialogTitle>
          <DialogDescription>
            People you deleted from Mimi. Your address books and email still have them; bring
            someone back to see them here again.
          </DialogDescription>
        </DialogHeader>
        {removed.isLoading ? (
          <Loader2 className="mx-auto size-5 animate-spin text-faint" />
        ) : list.length === 0 ? (
          <div className="flex flex-col items-center gap-2 py-6 text-center">
            <IconTile size="md" className="bg-fill text-muted-foreground">
              <UserX />
            </IconTile>
            <p className="type-callout text-muted-foreground">No one has been removed.</p>
          </div>
        ) : (
          <Grouped className="max-h-[50vh] overflow-y-auto">
            {list.map((r) => (
              <Row
                key={r.id}
                icon={<PersonAvatar id={r.id} name={r.name} />}
                title={r.name}
                detail={`From ${r.sources.join(", ")} · removed ${shortDate(r.removed_at)}`}
                trailing={
                  <Button size="sm" variant="secondary" disabled={busy !== null} onClick={() => bringBack(r.id, r.name)}>
                    {busy === r.id && <Loader2 className="animate-spin" />} Bring back
                  </Button>
                }
              />
            ))}
          </Grouped>
        )}
      </DialogContent>
    </Dialog>
  );
}
