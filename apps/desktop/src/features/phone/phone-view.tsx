import { useEffect, useRef, useState } from "react";
import { CheckCircle2, Copy, Globe, Loader2, MoreHorizontal, Plus, ShieldCheck, Smartphone, Wifi } from "lucide-react";
import { toast } from "sonner";

import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { Device } from "@/bindings/Device";
import type { PairingOffer } from "@/bindings/PairingOffer";
import type { RemoteAccess } from "@/bindings/RemoteAccess";
import { api } from "@/lib/api";
import { useAssistantName, useRemote } from "@/lib/queries";

/**
 * Settings › Phone: pair an iPhone with a QR code, see and remove paired phones, and
 * (rarely) choose the relay that helps them reach this computer from other networks.
 */
export function PhoneView() {
  const remote = useRemote().data;
  const assistant = useAssistantName();
  const [pairing, setPairing] = useState(false);
  const [renaming, setRenaming] = useState<Device | null>(null);
  const [removing, setRemoving] = useState<Device | null>(null);
  const [relayOpen, setRelayOpen] = useState(false);
  const devices = remote?.devices ?? [];

  return (
    <Page>
      <PageHeader
        title="Phone"
        subtitle={`Talk to ${assistant} from your iPhone, at home or away. Your phone connects to this computer, with no Mimi server in between.`}
        action={
          <Button variant="lime" onClick={() => setPairing(true)}>
            <Plus /> Pair a phone
          </Button>
        }
      />

      <Section title="Your phones">
        {devices.length === 0 ? (
          <Grouped>
            <Row
              icon={
                <IconTile size="sm" className="bg-fill text-muted-foreground">
                  <Smartphone />
                </IconTile>
              }
              title="No phone yet"
              detail="Install Mimi on your iPhone, then choose Pair a phone and scan the code."
              className="[&_.truncate]:whitespace-normal"
            />
          </Grouped>
        ) : (
          <Grouped>
            {devices.map((d) => (
              <Row
                key={d.id}
                icon={
                  <IconTile size="sm" className="bg-[#30d158] text-white">
                    <Smartphone />
                  </IconTile>
                }
                title={d.name}
                detail={deviceDetail(d)}
                trailing={
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Options for ${d.name}`}
                        className="rounded-full text-muted-foreground"
                      >
                        <MoreHorizontal />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={() => setRenaming(d)}>Rename…</DropdownMenuItem>
                      <DropdownMenuItem variant="destructive" onSelect={() => setRemoving(d)}>
                        Remove
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                }
              />
            ))}
          </Grouped>
        )}
        {remote?.error && <p className="px-1 type-subhead text-destructive">{remote.error}</p>}
      </Section>

      <Section title="How your phone connects">
        <Grouped>
          <Row
            icon={
              <IconTile size="sm" className="bg-private-soft text-private">
                <ShieldCheck />
              </IconTile>
            }
            title="Encrypted from end to end"
            detail="Your phone checks it's talking to this computer, and only phones you paired get in. Remove a phone here and it's cut off at once."
            className="[&_.truncate]:whitespace-normal"
          />
          <Row
            icon={
              <IconTile size="sm" className="bg-network-soft text-network">
                <Wifi />
              </IconTile>
            }
            title="Straight to this computer when it can"
            detail="On the same Wi-Fi, and on most other networks, your phone connects directly."
            className="[&_.truncate]:whitespace-normal"
          />
          <Row
            icon={
              <IconTile size="sm" className="bg-network-soft text-network">
                <Globe />
              </IconTile>
            }
            title={remote?.relay.kind === "custom" ? "Through your own relay otherwise" : "Through a public relay otherwise"}
            detail={relayDetail(remote)}
            className="[&_.truncate]:whitespace-normal"
            trailing={
              <Button variant="secondary" size="sm" onClick={() => setRelayOpen(true)} disabled={!remote}>
                Change…
              </Button>
            }
          />
        </Grouped>
      </Section>

      <PairDialog open={pairing} onClose={() => setPairing(false)} known={devices} />
      <RenameDialog device={renaming} onClose={() => setRenaming(null)} />
      <RelayDialog open={relayOpen} remote={remote} onClose={() => setRelayOpen(false)} />
      <AlertDialog open={!!removing} onOpenChange={(o) => !o && setRemoving(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {removing?.name}?</AlertDialogTitle>
            <AlertDialogDescription>
              It's disconnected right away and can't reach this computer again unless you pair it anew.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => removing && api.removePhone(removing.id).catch((e) => toast.error((e as Error).message))}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

function deviceDetail(d: Device) {
  if (d.connection === "direct") return "Connected now";
  if (d.connection === "relayed") return "Connected now, through the relay";
  if (d.last_seen_at) return `Last connected ${since(d.last_seen_at)}`;
  return "Paired";
}

function relayDetail(remote: RemoteAccess | undefined) {
  const base =
    remote?.relay.kind === "custom"
      ? "When a direct connection isn't possible, your relay passes the encrypted messages along."
      : "When a direct connection isn't possible, a relay run by iroh's makers passes the encrypted messages along. It can't read them, but it can see that your phone and this computer are talking.";
  if (!remote?.running) return base;
  return remote.relay_connected
    ? `${base} Your phone can reach this computer from anywhere.`
    : `${base} The relay can't be reached right now, so your phone may only connect on the same network.`;
}

function since(ms: number) {
  const min = Math.round((Date.now() - ms) / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 24) return h === 1 ? "an hour ago" : `${h} hours ago`;
  const d = Math.round(h / 24);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

/**
 * Shows a one-time QR code until a phone scans it. A fresh code replaces an expired one,
 * and closing the dialog withdraws it.
 */
function PairDialog({ open, onClose, known }: { open: boolean; onClose: () => void; known: Device[] }) {
  const [offer, setOffer] = useState<PairingOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [paired, setPaired] = useState<Device | null>(null);
  // Phones already paired when the dialog opened: a new one appearing means it worked.
  const before = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (!open) return;
    before.current = new Set(known.map((d) => d.id));
    setPaired(null);
    setError(null);
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const fetchOffer = () =>
      api
        .pairPhone()
        .then((o) => {
          if (cancelled) return;
          setOffer(o);
          timer = setTimeout(fetchOffer, Math.max(5_000, o.expires_at - Date.now() - 5_000));
        })
        .catch((e) => !cancelled && setError((e as Error).message));
    fetchOffer();
    return () => {
      cancelled = true;
      clearTimeout(timer);
      setOffer(null);
      api.cancelPairing().catch(() => {});
    };
    // Only when opening: `known` changes as the phone pairs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open || paired) return;
    const fresh = known.find((d) => !before.current.has(d.id));
    if (fresh) setPaired(fresh);
  }, [known, open, paired]);

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[440px]">
        <DialogHeader>
          <DialogTitle>{paired ? "Your phone is paired" : "Pair a phone"}</DialogTitle>
          <DialogDescription>
            {paired
              ? `${paired.name} can now reach this computer, wherever it is.`
              : "Open Mimi on your iPhone, tap Pair with your computer, and point the camera at this code."}
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-[264px] items-center justify-center">
          {paired ? (
            <CheckCircle2 className="size-16 text-lime-deep" strokeWidth={1.6} />
          ) : error ? (
            <p className="max-w-[300px] text-center type-callout text-destructive">{error}</p>
          ) : offer ? (
            <img
              src={`data:image/svg+xml;utf8,${encodeURIComponent(offer.qr_svg)}`}
              alt="Pairing code"
              className="size-[264px] rounded-[14px] bg-white p-2 shadow-[var(--shadow-card)]"
            />
          ) : (
            <Loader2 className="size-6 animate-spin text-muted-foreground" />
          )}
        </div>
        {!paired && offer && (
          <Button
            variant="ghost"
            size="sm"
            className="-mt-2 self-center text-muted-foreground"
            onClick={() =>
              navigator.clipboard
                .writeText(offer.link)
                .then(() => toast.success("Link copied. Paste it in Mimi on your phone."))
                .catch(() => toast.error("Couldn't copy the link."))
            }
          >
            <Copy /> Copy as a link
          </Button>
        )}
        {!paired && (
          <p className="text-center type-footnote text-muted-foreground">
            The code works once and changes every few minutes. Keep it to yourself: anyone who scans it can
            use your assistant.
          </p>
        )}
        <DialogFooter>
          <Button variant={paired ? "default" : "ghost"} onClick={onClose}>
            {paired ? "Done" : "Cancel"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function RenameDialog({ device, onClose }: { device: Device | null; onClose: () => void }) {
  const [name, setName] = useState("");
  useEffect(() => setName(device?.name ?? ""), [device]);
  const save = () => {
    if (!device || !name.trim()) return;
    api
      .renamePhone(device.id, name.trim())
      .then(onClose)
      .catch((e) => toast.error((e as Error).message));
  };
  return (
    <Dialog open={!!device} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[400px]">
        <DialogHeader>
          <DialogTitle>Rename phone</DialogTitle>
        </DialogHeader>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={60} autoFocus aria-label="Name" />
          <DialogFooter className="mt-4">
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!name.trim()}>
              Save
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The public relay (default) or the user's own `iroh-relay`. */
function RelayDialog({ open, remote, onClose }: { open: boolean; remote?: RemoteAccess; onClose: () => void }) {
  const [own, setOwn] = useState(false);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!open || !remote) return;
    setOwn(remote.relay.kind === "custom");
    setUrl(remote.relay.kind === "custom" ? remote.relay.url : "");
    setError(null);
  }, [open, remote]);
  const current = remote?.relay.kind === "custom" ? remote.relay.url : null;
  // Phones learn a relay of the user's own from the code they scan.
  const repair = (remote?.devices.length ?? 0) > 0 && own && url.trim() !== current;

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setRelay(own ? { kind: "custom", url: url.trim() } : { kind: "default" });
      onClose();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>Relay</DialogTitle>
          <DialogDescription>
            The relay only helps when your phone can't connect to this computer directly. Everything it passes
            along is encrypted.
          </DialogDescription>
        </DialogHeader>
        <div role="radiogroup" aria-label="Relay" className="flex flex-col gap-2">
          {(
            [
              [false, "Public relay", "Run by iroh's makers (number 0). Nothing to set up."],
              [true, "Your own relay", "A relay you run yourself (iroh-relay). Nobody else is involved."],
            ] as const
          ).map(([value, title, detail]) => (
            <button
              key={title}
              role="radio"
              aria-checked={own === value}
              onClick={() => setOwn(value)}
              className={
                "flex flex-col gap-0.5 rounded-[14px] px-4 py-3 text-left transition-colors " +
                (own === value ? "bg-fill" : "hover:bg-[rgb(118_118_128/0.06)]")
              }
            >
              <span className="type-callout font-medium">{title}</span>
              <span className="type-subhead text-muted-foreground">{detail}</span>
            </button>
          ))}
        </div>
        {own && (
          <div className="flex flex-col gap-1.5">
            <label htmlFor="relay-url" className="type-subhead font-medium">
              Your relay's address
            </label>
            <Input
              id="relay-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        )}
        {repair && (
          <p className="type-subhead text-muted-foreground">
            Phones you already paired will need to scan a new code to use this relay.
          </p>
        )}
        {error && <p className="type-subhead text-destructive">{error}</p>}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={save} disabled={busy || (own && !url.trim())}>
            {busy && <Loader2 className="animate-spin" />} Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
