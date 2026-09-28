import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import {
  BellRing,
  CalendarCog,
  CalendarDays,
  CalendarPlus,
  MoreHorizontal,
  Plus,
  Send,
  ShieldCheck,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Autonomy } from "@/bindings/Autonomy";
import type { PermissionKind } from "@/bindings/PermissionKind";
import type { PermissionRuleView } from "@/bindings/PermissionRuleView";
import type { PermissionTarget } from "@/bindings/PermissionTarget";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { PersonAvatar } from "@/components/people";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { api, keys } from "@/lib/api";
import { useAssistantName } from "@/lib/queries";

/** Icons the daemon may name for a kind; anything else gets a shield. */
const icons: Record<string, typeof Send> = {
  send: Send,
  "calendar-plus": CalendarPlus,
  "calendar-cog": CalendarCog,
  "bell-ring": BellRing,
};

const targetKey = (t: PermissionTarget) => `${t.kind}:${t.id}`;

/** Settings › Permissions: what the assistant may do without asking first, kind by kind. */
export function PermissionsView() {
  const assistant = useAssistantName();
  const qc = useQueryClient();
  const kinds = useQuery({ queryKey: keys.permissions, queryFn: api.permissions });

  const save = async (kind: PermissionKind, autonomy: Autonomy, rules: PermissionRuleView[]) => {
    try {
      const next = await api.setPermission(kind.id, {
        autonomy,
        rules: rules.map((r) => ({ target: r.target, autonomy: r.autonomy })),
      });
      qc.setQueryData(keys.permissions, next);
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Page>
      <PageHeader title="Permissions" subtitle={`What ${assistant} may do for you without asking first.`} />
      <Section title="Actions">
        <div className="flex flex-col gap-3">
          {kinds.isLoading && [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[68px] rounded-[16px]" />)}
          {kinds.data?.map((k) => (
            <KindCard
              key={k.id}
              kind={k}
              onChange={(autonomy, rules) => save(k, autonomy, rules)}
            />
          ))}
        </div>
      </Section>
      <p className="-mt-6 max-w-[640px] px-1 type-footnote text-muted-foreground">
        Everything {assistant} does shows in the chat, whichever you choose. Emails, websites and messages from
        other people can hide instructions meant for {assistant}; asking first is what stops them, so choose
        Automatic only for what you're comfortable with.
      </p>
    </Page>
  );
}

function KindCard({
  kind: k,
  onChange,
}: {
  kind: PermissionKind;
  onChange: (autonomy: Autonomy, rules: PermissionRuleView[]) => void;
}) {
  const Icon = icons[k.icon] ?? ShieldCheck;
  const setRule = (target: PermissionTarget, autonomy: Autonomy) =>
    onChange(
      k.autonomy,
      k.rules.map((r) => (targetKey(r.target) === targetKey(target) ? { ...r, autonomy } : r)),
    );
  const removeRule = (target: PermissionTarget) =>
    onChange(
      k.autonomy,
      k.rules.filter((r) => targetKey(r.target) !== targetKey(target)),
    );
  const addRule = (target: PermissionTarget, label: string) =>
    onChange(k.autonomy, [
      ...k.rules,
      { target, label, missing: false, autonomy: k.autonomy === "ask" ? "automatic" : "ask" },
    ]);

  return (
    <div className="flex flex-col gap-2">
      <Grouped>
        <Row
          icon={
            <IconTile size="sm" className="text-white">
              <span className="flex size-full items-center justify-center rounded-[inherit]" style={{ background: k.color }}>
                <Icon />
              </span>
            </IconTile>
          }
          title={k.title}
          detail={k.autonomy === "ask" ? k.ask_detail : k.automatic_detail}
          className="py-3 [&_.truncate]:whitespace-normal"
          trailing={
            <Segmented
              id={k.id}
              label={k.title}
              value={k.autonomy}
              onChange={(v) => v !== k.autonomy && onChange(v, k.rules)}
            />
          }
        />
        {k.targets !== "none" &&
          k.rules.map((r) => (
            <ExceptionRow
              key={targetKey(r.target)}
              kind={k}
              rule={r}
              onChange={(a) => a !== r.autonomy && setRule(r.target, a)}
              onRemove={() => removeRule(r.target)}
            />
          ))}
        {k.targets !== "none" && <AddException kind={k} onAdd={addRule} />}
      </Grouped>
      {k.note && <p className="max-w-[640px] px-4 type-footnote text-muted-foreground">{k.note}</p>}
    </div>
  );
}

function ExceptionRow({
  kind: k,
  rule: r,
  onChange,
  onRemove,
}: {
  kind: PermissionKind;
  rule: PermissionRuleView;
  onChange: (a: Autonomy) => void;
  onRemove: () => void;
}) {
  return (
    <Row
      className="min-h-[52px] pl-[58px]"
      icon={<TargetIcon target={r.target} label={r.label} />}
      title={<span className={cn(r.missing && "text-muted-foreground")}>{r.label}</span>}
      detail={r.missing ? "This exception no longer applies." : undefined}
      trailing={
        <>
          {!r.missing && (
            <Segmented
              id={`${k.id}-${targetKey(r.target)}`}
              label={`${k.title}: ${r.label}`}
              value={r.autonomy}
              onChange={onChange}
              small
            />
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Options for ${r.label}`}
                className="rounded-full text-muted-foreground"
              >
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem variant="destructive" onSelect={onRemove}>
                Remove exception
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      }
    />
  );
}

function TargetIcon({ target, label }: { target: PermissionTarget; label: string }) {
  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars, enabled: target.kind === "calendar" });
  if (target.kind === "person") return <PersonAvatar id={target.id} name={label} size="sm" />;
  const color = calendars.data?.find((c) => c.id === target.id)?.color ?? "#8e8e93";
  return (
    <span className="flex size-7 shrink-0 items-center justify-center rounded-[8px] bg-fill">
      <span className="size-2.5 rounded-full" style={{ background: color }} />
    </span>
  );
}

/** "Add a person…" / "Add a calendar…", with a searchable list. */
function AddException({
  kind: k,
  onAdd,
}: {
  kind: PermissionKind;
  onAdd: (target: PermissionTarget, label: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const person = k.targets === "person";
  const taken = new Set(k.rules.map((r) => targetKey(r.target)));
  const people = useQuery({
    queryKey: keys.peopleList(""),
    queryFn: () => api.people(""),
    enabled: open && person,
  });
  const calendars = useQuery({ queryKey: keys.calendars, queryFn: api.calendars, enabled: open && !person });
  const options: { target: PermissionTarget; label: string; color?: string }[] = person
    ? (people.data ?? [])
        .filter((p) => p.channels.includes("email"))
        .map((p) => ({ target: { kind: "person", id: p.id }, label: p.name }))
    : (calendars.data ?? []).map((c) => ({ target: { kind: "calendar", id: c.id }, label: c.name, color: c.color }));
  const choices = options.filter((o) => !taken.has(targetKey(o.target)));
  const heading =
    k.autonomy === "ask" ? "Without asking, for:" : "Always ask first, for:";

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button className="flex min-h-[44px] w-full items-center gap-3.5 pr-4 pl-[58px] text-left type-callout text-muted-foreground transition-colors hover:bg-[rgb(118_118_128/0.06)] hover:text-foreground">
          <Plus className="size-4" />
          {person ? "Add a person…" : "Add a calendar…"}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[320px] overflow-hidden p-0">
        <Command>
          <p className="px-4 pt-3 pb-1 type-footnote font-semibold text-muted-foreground">{heading}</p>
          <CommandInput placeholder={person ? "Search people" : "Search calendars"} />
          <CommandList className="max-h-[260px]">
            <CommandEmpty>
              {person ? "No one with an email address." : "No calendars."}
            </CommandEmpty>
            {choices.map((o) => (
              <CommandItem
                key={targetKey(o.target)}
                value={`${o.label} ${o.target.id}`}
                onSelect={() => {
                  setOpen(false);
                  onAdd(o.target, o.label);
                }}
                className="gap-2.5"
              >
                {o.target.kind === "person" ? (
                  <PersonAvatar id={o.target.id} name={o.label} size="sm" />
                ) : (
                  <CalendarDays style={{ color: o.color }} />
                )}
                <span className="truncate">{o.label}</span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

const choices: { value: Autonomy; label: string }[] = [
  { value: "ask", label: "Ask first" },
  { value: "automatic", label: "Automatic" },
];

function Segmented({
  id,
  label,
  value,
  small = false,
  onChange,
}: {
  id: string;
  label: string;
  value: Autonomy;
  small?: boolean;
  onChange: (value: Autonomy) => void;
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
              "relative rounded-[7px] font-medium transition-colors duration-200",
              small ? "h-[22px] px-2.5 text-[12px]" : "h-[26px] px-3 text-[13px]",
              active ? "text-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {active && (
              <motion.span
                layoutId={`permission-${id}`}
                className="absolute inset-0 rounded-[7px] bg-background shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                transition={{ type: "spring", stiffness: 520, damping: 38 }}
              />
            )}
            <span className="relative">{c.label}</span>
          </button>
        );
      })}
    </div>
  );
}
