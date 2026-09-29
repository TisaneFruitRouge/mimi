import { useState } from "react";
import { Copy, Loader2, MoreHorizontal, Pencil, Trash2, X } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Channel } from "@/bindings/Channel";
import type { Handle } from "@/bindings/Handle";
import type { NewHandle } from "@/bindings/NewHandle";
import type { Person } from "@/bindings/Person";
import { copyText } from "@/components/app-context-menu";
import { ChannelIcon, channelInfo } from "@/components/people";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api } from "@/lib/api";

const channels: Channel[] = ["phone", "email", "telegram", "signal", "whatsapp", "matrix"];

const placeholders: Record<Channel, string> = {
  phone: "+41 79 123 45 67",
  email: "name@example.com",
  telegram: "@username",
  signal: "+41 79 123 45 67",
  whatsapp: "+41 79 123 45 67",
  matrix: "@name:matrix.org",
  other: "",
};

export function sourcesLine(p: Person) {
  const names = [
    ...new Set([
      ...p.sources.map((s) => (s.source_id ? s.source_name : "added by you")),
      ...(p.manual ? ["added by you"] : []),
    ]),
  ];
  if (names.length === 1 && names[0] === "added by you") return "Added by you";
  return names.length ? `From ${names.join(" and ")}` : "";
}

/** A label as people read it: "mobile" → "Mobile"; address books' own words kept. */
export function labelText(label: string) {
  return label.charAt(0).toUpperCase() + label.slice(1);
}

/** Labels offered for each way to reach someone (any other can be typed). */
const labelChoices: Partial<Record<Channel, string[]>> = {
  phone: ["mobile", "home", "work"],
  email: ["home", "work"],
  signal: ["mobile", "work"],
  whatsapp: ["mobile", "work"],
};

/** One way to reach someone, with a "…" menu: copy, and change or remove what the user added. */
export function HandleRow({ person, handle: h }: { person: Person; handle: Handle }) {
  const [editing, setEditing] = useState(false);
  const own = !h.source_id;
  if (editing) {
    return (
      <div className="p-2">
        <HandleForm
          initial={{ channel: h.channel, value: h.value, label: h.label }}
          submitLabel="Save"
          onCancel={() => setEditing(false)}
          onSubmit={async (next) => {
            await api.updateHandle(person.id, h.id, next);
            setEditing(false);
          }}
        />
      </div>
    );
  }
  return (
    <div className="flex min-h-[52px] items-center gap-3 px-4 py-2">
      <ChannelIcon channel={h.channel} className="size-4" />
      <div className="min-w-0 flex-1">
        <div className="truncate type-callout select-text">{h.value}</div>
        <div className="truncate type-footnote text-muted-foreground">
          {h.label ? labelText(h.label) : channelInfo[h.channel].label}
          {h.label && ` · ${channelInfo[h.channel].label}`} · {own ? "added by you" : `from ${h.source_name}`}
        </div>
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={`More for ${h.value}`} className="rounded-full text-faint">
            <MoreHorizontal />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-[240px]">
          <DropdownMenuItem onSelect={() => copyText(h.value)}>
            <Copy /> Copy
          </DropdownMenuItem>
          {own ? (
            <>
              <DropdownMenuItem onSelect={() => setEditing(true)}>
                <Pencil /> Change…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                onSelect={() => api.removeHandle(person.id, h.id).catch((e) => toast.error(e.message))}
              >
                <Trash2 /> Remove
              </DropdownMenuItem>
            </>
          ) : (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel className="type-footnote font-normal text-muted-foreground">
                From {h.source_name}: change it in the address book, and Mimi follows.
              </DropdownMenuLabel>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

export function HandleForm({
  onSubmit,
  onCancel,
  initial,
  submitLabel = "Add",
}: {
  onSubmit: (h: NewHandle) => Promise<void>;
  onCancel?: () => void;
  /** Editing an existing one: its current values. */
  initial?: { channel: Channel; value: string; label: string | null };
  submitLabel?: string;
}) {
  const [channel, setChannel] = useState<Channel>(initial?.channel ?? "phone");
  const [value, setValue] = useState(initial?.value ?? "");
  const [label, setLabel] = useState(initial?.label ?? "");
  const choices = labelChoices[channel] ?? [];
  // Typing a label of one's own: shown while it isn't one of the choices.
  const [custom, setCustom] = useState(!!initial?.label && !choices.includes(initial.label));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit({ channel, value, label: label.trim() || null });
      setValue("");
      setLabel("");
      setCustom(false);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const chip = (active: boolean) =>
    cn(
      "h-7 rounded-full px-3 type-subhead transition-colors",
      active ? "bg-foreground text-background" : "bg-fill text-muted-foreground hover:bg-[rgb(118_118_128/0.2)]",
    );

  return (
    <form
      className="surface flex flex-col gap-2.5 p-3"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="flex gap-2">
        <Select
          value={channel}
          onValueChange={(v) => {
            const next = v as Channel;
            setChannel(next);
            if (!custom && label && !(labelChoices[next] ?? []).includes(label)) setLabel("");
          }}
        >
          <SelectTrigger className="w-36" aria-label="How">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {channels.map((c) => (
              <SelectItem key={c} value={c}>
                {channelInfo[c].label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={placeholders[channel]}
          aria-label="Number, address or username"
          autoFocus
        />
      </div>
      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Label">
        <span className="mr-1 type-subhead text-muted-foreground">Label</span>
        <button
          type="button"
          className={chip(!custom && !label)}
          onClick={() => {
            setCustom(false);
            setLabel("");
          }}
        >
          None
        </button>
        {choices.map((c) => (
          <button
            key={c}
            type="button"
            aria-pressed={!custom && label === c}
            className={chip(!custom && label === c)}
            onClick={() => {
              setCustom(false);
              setLabel(c);
            }}
          >
            {labelText(c)}
          </button>
        ))}
        <button
          type="button"
          aria-pressed={custom}
          className={chip(custom)}
          onClick={() => {
            setCustom(true);
            if (choices.includes(label)) setLabel("");
          }}
        >
          Other…
        </button>
        {custom && (
          <Input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Like school or holiday home"
            aria-label="Label"
            className="h-7 w-48"
            autoFocus
          />
        )}
      </div>
      <div className="flex justify-end gap-2">
        {onCancel && (
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        )}
        <Button type="submit" disabled={busy || !value.trim()}>
          {busy && <Loader2 className="animate-spin" />} {submitLabel}
        </Button>
      </div>
      {error && <p className="type-subhead text-destructive">{error}</p>}
    </form>
  );
}

export function AddPersonDialog({
  open,
  onOpenChange,
  onAdded,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAdded: (id: string) => void;
}) {
  const [name, setName] = useState("");
  const [nickname, setNickname] = useState("");
  const [handles, setHandles] = useState<NewHandle[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const close = (o: boolean) => {
    onOpenChange(o);
    if (!o) {
      setName("");
      setNickname("");
      setHandles([]);
      setError(null);
    }
  };

  const add = async () => {
    setBusy(true);
    setError(null);
    try {
      const person = await api.addPerson({ name, nickname: nickname || null, handles });
      close(false);
      onAdded(person.id);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="gap-6 bg-canvas sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle className="type-title">Add someone</DialogTitle>
          <DialogDescription>Kept on this computer. Your assistant can then mention and reach them.</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-person-name">Name</Label>
            <Input id="new-person-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="new-person-nickname">Nickname</Label>
            <Input
              id="new-person-nickname"
              value={nickname}
              onChange={(e) => setNickname(e.target.value)}
              placeholder="Optional, like Mum or Sammy"
            />
          </div>
          {handles.length > 0 && (
            <div className="grouped flex flex-col">
              {handles.map((h, i) => (
                <div key={i} className="flex min-h-[48px] items-center gap-3 px-4 py-2 type-callout">
                  <ChannelIcon channel={h.channel} className="size-4" />
                  <span className="min-w-0 flex-1 truncate">
                    {h.value}
                    {h.label && <span className="text-faint"> · {labelText(h.label)}</span>}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Remove ${h.value}`}
                    className="text-faint"
                    onClick={() => setHandles(handles.filter((_, j) => j !== i))}
                  >
                    <X />
                  </Button>
                </div>
              ))}
            </div>
          )}
          <HandleForm onSubmit={async (h) => setHandles([...handles, h])} />
          {error && <p className="type-subhead text-destructive">{error}</p>}
        </div>
        <DialogFooter>
          <Button onClick={add} disabled={busy || !name.trim()}>
            {busy && <Loader2 className="animate-spin" />} Add {name.trim() || "person"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
