import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import { Check, Hand, MessageCircleHeart } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Approver } from "@/bindings/Approver";
import type { CalendarInfo } from "@/bindings/CalendarInfo";
import type { Person } from "@/bindings/Person";
import type { PersonAccess } from "@/bindings/PersonAccess";
import type { PersonAccessUpdate } from "@/bindings/PersonAccessUpdate";
import { Grouped, IconTile, Row, Section } from "@/components/page";
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
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { api, keys } from "@/lib/api";

/**
 * "Mimi for Maya": whether someone may ask the assistant things from their own Matrix
 * account, the calendars shared with them, and who approves what they ask for. Their
 * conversations are theirs: nothing here shows them.
 */
export function AssistantAccess({ person: p, assistant }: { person: Person; assistant: string }) {
  const qc = useQueryClient();
  const key = keys.personExtra(p.id, "access");
  const access = useQuery({ queryKey: key, queryFn: () => api.personAccess(p.id), retry: false });
  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars });
  const [confirmOff, setConfirmOff] = useState(false);
  const first = p.nickname || p.name.split(/\s+/)[0];

  // An older daemon doesn't know this question: say nothing.
  if (access.isError) return null;
  const title = `${assistant} for ${first}`;
  if (!access.data) {
    return (
      <Section title={title}>
        <Skeleton className="h-[60px] rounded-[16px]" />
      </Section>
    );
  }
  const a = access.data;

  const save = async (change: PersonAccessUpdate) => {
    // Shown at once; the daemon's answer (or the error) settles it.
    const before = a;
    qc.setQueryData<PersonAccess>(key, { ...a, ...change });
    try {
      qc.setQueryData(key, await api.setPersonAccess(p.id, change));
    } catch (e) {
      qc.setQueryData(key, before);
      toast.error((e as Error).message);
    }
  };

  const reachable = a.addresses.length > 0 && a.assistant_addresses.length > 0;
  const detail =
    a.assistant_addresses.length === 0
      ? `Connect ${assistant} to Matrix in Settings › Connections first.`
      : a.addresses.length === 0
        ? `Add ${first}'s Matrix address under “How to reach them” first.`
        : a.enabled
          ? `${first} writes to ${a.assistant_addresses[0]} from ${a.addresses.join(" or ")} on Matrix.`
          : `Lets ${first} message ${assistant} from Matrix, in a chat of their own.`;

  return (
    <Section title={title}>
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="bg-lime-soft text-lime-deep">
              <MessageCircleHeart />
            </IconTile>
          }
          title={`${first} can ask ${assistant} things`}
          detail={detail}
          className="py-3 [&_.truncate]:whitespace-normal"
          trailing={
            <Switch
              checked={a.enabled}
              disabled={!a.enabled && !reachable}
              aria-label={`${first} can ask ${assistant} things`}
              onCheckedChange={(on) => (on ? save({ enabled: true }) : setConfirmOff(true))}
            />
          }
        />
        {a.enabled && (
          <Row
            icon={
              <IconTile size="sm" className="bg-fill text-muted-foreground">
                <Hand />
              </IconTile>
            }
            title="Who approves what they ask for"
            detail={
              a.approver === "guest"
                ? `${first} does, in their chat with ${assistant}.`
                : `You do. Only what ${assistant} is about to do reaches you, never their conversation.`
            }
            className="py-3 [&_.truncate]:whitespace-normal"
            trailing={
              <Choice
                id={`approver-${p.id}`}
                label="Who approves what they ask for"
                value={a.approver}
                choices={[
                  { value: "guest", label: first },
                  { value: "owner", label: "You" },
                ]}
                onChange={(approver) => approver !== a.approver && save({ approver })}
              />
            }
          />
        )}
      </Grouped>

      {a.enabled && (
        <Calendars
          first={first}
          assistant={assistant}
          all={calendars.data ?? []}
          loading={calendars.isLoading}
          shared={a.calendars}
          onChange={(ids) => save({ calendars: ids })}
        />
      )}

      <p className="max-w-[640px] px-4 type-footnote text-muted-foreground">
        {a.enabled
          ? `${first}'s conversations with ${assistant} are theirs: they don't show up for you. ${assistant} doesn't share your email, memory, contacts or other calendars with them.`
          : `Off until you turn it on. ${assistant} never shares your email, memory or contacts with anyone.`}
      </p>

      <AlertDialog open={confirmOff} onOpenChange={setConfirmOff}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Stop {first} asking {assistant} things?</AlertDialogTitle>
            <AlertDialogDescription>
              Their conversations with {assistant} and the reminders it set for them are deleted, and{" "}
              {assistant} leaves the chat they opened. You can turn this on again later.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => save({ enabled: false })}>
              Turn off
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Section>
  );
}

/** The calendars shared with them: a checklist of the user's calendars, in their colours. */
function Calendars({
  first,
  assistant,
  all,
  loading,
  shared,
  onChange,
}: {
  first: string;
  assistant: string;
  all: CalendarInfo[];
  loading: boolean;
  shared: string[];
  onChange: (ids: string[]) => void;
}) {
  const toggle = (id: string) =>
    onChange(shared.includes(id) ? shared.filter((c) => c !== id) : [...shared, id]);
  return (
    <>
      <h3 className="mt-2 px-1 section-label">Calendars {first} can use</h3>
      {loading ? (
        <Skeleton className="h-[48px] rounded-[16px]" />
      ) : all.length === 0 ? (
        <p className="px-1 type-callout text-muted-foreground">
          Connect a calendar in Settings › Connections to share it with {first}.
        </p>
      ) : (
        <Grouped>
          {all.map((c) => {
            const on = shared.includes(c.id);
            return (
              <button
                key={c.id}
                role="checkbox"
                aria-checked={on}
                onClick={() => toggle(c.id)}
                className="flex min-h-[48px] w-full items-center gap-3.5 px-4 py-2 text-left transition-colors hover:bg-[rgb(118_118_128/0.06)]"
              >
                <span className="size-3 shrink-0 rounded-full" style={{ background: c.color }} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate type-body">{c.name}</span>
                  {!c.writable && (
                    <span className="block truncate type-subhead text-muted-foreground">
                      Read only: {first} can see it, not change it
                    </span>
                  )}
                </span>
                <Check
                  className={cn("size-[18px] shrink-0 text-foreground transition-opacity", !on && "opacity-0")}
                  strokeWidth={2.4}
                />
              </button>
            );
          })}
        </Grouped>
      )}
      {all.length > 0 && shared.length === 0 && (
        <p className="max-w-[640px] px-4 type-footnote text-muted-foreground">
          None yet: if {first} asks about a calendar, {assistant} says you haven't shared one.
        </p>
      )}
    </>
  );
}

/** A small segmented control, as in Settings › Permissions. */
function Choice({
  id,
  label,
  value,
  choices,
  onChange,
}: {
  id: string;
  label: string;
  value: Approver;
  choices: { value: Approver; label: string }[];
  onChange: (value: Approver) => void;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex shrink-0 rounded-[9px] bg-fill p-[2px]">
      {choices.map((c) => {
        const active = c.value === value;
        return (
          <button
            key={c.value}
            role="radio"
            aria-checked={active}
            onClick={() => onChange(c.value)}
            className={cn(
              "relative h-[26px] max-w-[120px] rounded-[7px] px-3 text-[13px] font-medium transition-colors duration-200",
              active ? "text-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {active && (
              <motion.span
                layoutId={id}
                className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                transition={{ type: "spring", stiffness: 520, damping: 38 }}
              />
            )}
            <span className="relative block truncate">{c.label}</span>
          </button>
        );
      })}
    </div>
  );
}
