import { useState } from "react";
import { Loader2, X } from "lucide-react";
import { toast } from "sonner";

import type { Channel } from "@/bindings/Channel";
import type { Handle } from "@/bindings/Handle";
import type { NewHandle } from "@/bindings/NewHandle";
import type { Person } from "@/bindings/Person";
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
  const names = [...new Set([...p.sources.map((s) => s.source_name), ...(p.manual ? ["added by you"] : [])])];
  return names.length ? `From ${names.join(" and ")}` : "";
}

export function HandleRow({ person, handle: h }: { person: Person; handle: Handle }) {
  return (
    <div className="flex min-h-[52px] items-center gap-3 px-4 py-2">
      <ChannelIcon channel={h.channel} className="size-4" />
      <div className="min-w-0 flex-1">
        <div className="truncate type-callout">{h.value}</div>
        <div className="truncate type-footnote text-muted-foreground">
          {channelInfo[h.channel].label}
          {h.label && ` · ${h.label}`} · {h.source_id ? `from ${h.source_name}` : "added by you"}
        </div>
      </div>
      {!h.source_id && (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Remove ${h.value}`}
          className="text-faint"
          onClick={() => api.removeHandle(person.id, h.id).catch((e) => toast.error(e.message))}
        >
          <X />
        </Button>
      )}
    </div>
  );
}

export function HandleForm({
  onSubmit,
  onCancel,
}: {
  onSubmit: (h: NewHandle) => Promise<void>;
  onCancel?: () => void;
}) {
  const [channel, setChannel] = useState<Channel>("phone");
  const [value, setValue] = useState("");
  const [label, setLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await onSubmit({ channel, value, label: label || null });
      setValue("");
      setLabel("");
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="surface flex flex-col gap-2 p-3"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="flex gap-2">
        <Select value={channel} onValueChange={(v) => setChannel(v as Channel)}>
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
      <div className="flex gap-2">
        <Input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="Label, like mobile or work (optional)"
          aria-label="Label"
        />
        {onCancel && (
          <Button type="button" variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        )}
        <Button type="submit" disabled={busy || !value.trim()}>
          {busy && <Loader2 className="animate-spin" />} Add
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
                    {h.label && <span className="text-faint"> · {h.label}</span>}
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
