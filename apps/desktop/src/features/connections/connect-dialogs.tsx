import { useEffect, useState } from "react";
import QRCode from "qrcode";
import { CircleCheck, ExternalLink, Loader2, Lock, TriangleAlert } from "lucide-react";
import { cn } from "cn";

import type { Connection } from "@/bindings/Connection";
import type { ConnectionSetup } from "@/bindings/ConnectionSetup";
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
import { api } from "@/lib/api";
import { useConnections } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

export type ConnectKind = "google_calendar" | "caldav" | "telegram";

export function ConnectDialog({ kind, onClose }: { kind: ConnectKind | null; onClose: () => void }) {
  return (
    <Dialog open={kind !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-5 sm:max-w-lg">
        {kind === "google_calendar" && <GoogleCalendar onDone={onClose} />}
        {kind === "caldav" && <CalDav onDone={onClose} />}
        {kind === "telegram" && <Telegram onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

/** Submits a setup and keeps the error to show inline. */
function useConnect() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<Connection | null>(null);
  const connect = async (setup: ConnectionSetup) => {
    setBusy(true);
    setError(null);
    try {
      setCreated(await api.connect(setup));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return { busy, error, created, connect, reset: () => setCreated(null) };
}

function Steps({ children }: { children: React.ReactNode }) {
  return <ol className="flex flex-col gap-3 text-[14px] leading-relaxed">{children}</ol>;
}

function Step({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-subtle font-mono text-[12px] text-muted-foreground">
        {n}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2 pt-0.5">{children}</div>
    </li>
  );
}

function Note({ icon = "lock", children }: { icon?: "lock" | "warn"; children: React.ReactNode }) {
  return (
    <div
      className={cn(
        "flex gap-2.5 rounded-xl px-3.5 py-3 text-[13px] leading-relaxed",
        icon === "lock" ? "bg-private-soft text-private" : "bg-cloud-soft text-cloud",
      )}
    >
      {icon === "lock" ? <Lock className="mt-0.5 size-4 shrink-0" /> : <TriangleAlert className="mt-0.5 size-4 shrink-0" />}
      <span>{children}</span>
    </div>
  );
}

function Done({ connection, onDone, extra }: { connection: Connection; onDone: () => void; extra?: React.ReactNode }) {
  return (
    <>
      <div className="flex flex-col items-center gap-3 py-4 text-center">
        <CircleCheck className="size-10 text-private" strokeWidth={1.6} />
        <div>
          <p className="text-[16px] font-medium">{connection.name} is connected</p>
          <p className="text-[13.5px] text-muted-foreground">{connection.detail}</p>
        </div>
      </div>
      <DialogFooter className="sm:justify-between">
        {extra ?? <span />}
        <Button onClick={onDone}>Done</Button>
      </DialogFooter>
    </>
  );
}

function GoogleCalendar({ onDone }: { onDone: () => void }) {
  const [url, setUrl] = useState("");
  const { busy, error, created, connect, reset } = useConnect();

  if (created) {
    return (
      <Done
        connection={created}
        onDone={onDone}
        extra={
          <Button
            variant="ghost"
            onClick={() => {
              setUrl("");
              reset();
            }}
          >
            Add another calendar
          </Button>
        }
      />
    );
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect Google Calendar</DialogTitle>
        <DialogDescription>
          No Google sign-in: your assistant reads the calendar through its private address.
        </DialogDescription>
      </DialogHeader>
      <Steps>
        <Step n={1}>
          <span>Open your Google Calendar settings.</span>
          <Button
            variant="outline"
            size="sm"
            className="self-start"
            onClick={() => openExternal("https://calendar.google.com/calendar/r/settings")}
          >
            <ExternalLink /> Open Google Calendar settings
          </Button>
        </Step>
        <Step n={2}>
          <span>
            On the left, under <b>Settings for my calendars</b>, pick a calendar and choose{" "}
            <b>Integrate calendar</b>.
          </span>
        </Step>
        <Step n={3}>
          <span>
            Copy the <b>Secret address in iCal format</b> and paste it here.
          </span>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              connect({ integration: "google_calendar", ics_url: url });
            }}
          >
            <Input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
              aria-label="Secret address in iCal format"
              className="font-mono text-[12.5px]"
              autoFocus
            />
            <Button type="submit" disabled={busy || !url.trim()}>
              {busy && <Loader2 className="animate-spin" />} Connect
            </Button>
          </form>
          {error && <p className="text-[13px] text-destructive">{error}</p>}
        </Step>
      </Steps>
      <Note>
        Anyone with this address can read the calendar, so it's stored encrypted on this computer.
        To add events, your assistant prepares them in Google Calendar and you press Save.
      </Note>
    </>
  );
}

const caldavServices = [
  {
    id: "icloud",
    name: "iCloud",
    server: "https://caldav.icloud.com",
    user: "Apple Account email",
    help: "Create an app-specific password in your Apple Account, under Sign-In and Security.",
    helpUrl: "https://account.apple.com/account/manage",
  },
  {
    id: "fastmail",
    name: "Fastmail",
    server: "https://caldav.fastmail.com",
    user: "Fastmail email",
    help: "Create an app password in Fastmail's settings, under Privacy & Security, with calendar access.",
    helpUrl: "https://app.fastmail.com/settings/security/apppasswords",
  },
  {
    id: "nextcloud",
    name: "Nextcloud",
    server: "",
    user: "Nextcloud username",
    help: "Create an app password in Nextcloud, under Settings › Security.",
    helpUrl: null,
  },
  {
    id: "other",
    name: "Other",
    server: "",
    user: "Username",
    help: "Use the address, username and password your calendar service gives for CalDAV.",
    helpUrl: null,
  },
] as const;

function CalDav({ onDone }: { onDone: () => void }) {
  const [serviceId, setServiceId] = useState<(typeof caldavServices)[number]["id"]>("icloud");
  const service = caldavServices.find((s) => s.id === serviceId)!;
  const [server, setServer] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const { busy, error, created, connect } = useConnect();

  if (created) return <Done connection={created} onDone={onDone} />;

  const serverUrl = service.server || server;
  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect a calendar account</DialogTitle>
        <DialogDescription>Read and add events in iCloud, Fastmail, Nextcloud and other calendars.</DialogDescription>
      </DialogHeader>
      <div className="flex gap-1.5">
        {caldavServices.map((s) => (
          <button
            key={s.id}
            onClick={() => setServiceId(s.id)}
            className={cn(
              "h-8 rounded-lg border px-3 text-[13px] font-medium transition-colors",
              s.id === serviceId ? "border-foreground bg-foreground text-background" : "hover:bg-subtle",
            )}
          >
            {s.name}
          </button>
        ))}
      </div>
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          connect({ integration: "caldav", server_url: serverUrl, username, password });
        }}
      >
        {!service.server && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="caldav-server">Server address</Label>
            <Input
              id="caldav-server"
              value={server}
              onChange={(e) => setServer(e.target.value)}
              placeholder={serviceId === "nextcloud" ? "https://cloud.example.com" : "https://"}
              className="font-mono text-[13px]"
            />
          </div>
        )}
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="caldav-user">{service.user}</Label>
          <Input id="caldav-user" value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="caldav-password">App password</Label>
          <Input
            id="caldav-password"
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="off"
          />
          <p className="text-[12.5px] leading-relaxed text-muted-foreground">
            {service.help}{" "}
            {service.helpUrl && (
              <button type="button" className="font-medium text-foreground underline underline-offset-2" onClick={() => openExternal(service.helpUrl!)}>
                Open
              </button>
            )}
          </p>
        </div>
        {error && <p className="text-[13px] text-destructive">{error}</p>}
        <DialogFooter>
          <Button type="submit" disabled={busy || !serverUrl.trim() || !username.trim() || !password}>
            {busy && <Loader2 className="animate-spin" />} Connect
          </Button>
        </DialogFooter>
      </form>
      <Note>Your password is stored encrypted on this computer and only ever sent to {service.name === "Other" ? "your calendar server" : service.name}.</Note>
    </>
  );
}

function Telegram({ onDone }: { onDone: () => void }) {
  const [token, setToken] = useState("");
  const { busy, error, created, connect } = useConnect();
  // The connection updates live (via events) once the owner presses Start.
  const live = useConnections().data?.find((c) => c.id === created?.id) ?? created;

  if (live && live.status === "ok") {
    return <Done connection={live} onDone={onDone} />;
  }
  if (live) return <TelegramPairing connection={live} />;

  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect Telegram</DialogTitle>
        <DialogDescription>Chat with your assistant from your phone, through a bot only you can use.</DialogDescription>
      </DialogHeader>
      <Steps>
        <Step n={1}>
          <span>
            Open <b>@BotFather</b>, Telegram's official bot for creating bots.
          </span>
          <Button variant="outline" size="sm" className="self-start" onClick={() => openExternal("https://t.me/BotFather")}>
            <ExternalLink /> Open @BotFather
          </Button>
        </Step>
        <Step n={2}>
          <span>
            Send <code className="rounded bg-subtle px-1 font-mono text-[13px]">/newbot</code>, then choose a name
            and a username ending in “bot”.
          </span>
        </Step>
        <Step n={3}>
          <span>Paste the token BotFather gives you.</span>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              connect({ integration: "telegram", bot_token: token });
            }}
          >
            <Input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="123456789:AA…"
              aria-label="Bot token"
              className="font-mono text-[13px]"
              autoFocus
            />
            <Button type="submit" disabled={busy || !token.trim()}>
              {busy && <Loader2 className="animate-spin" />} Connect
            </Button>
          </form>
          {error && <p className="text-[13px] text-destructive">{error}</p>}
        </Step>
      </Steps>
      <Note icon="warn">
        Telegram messages aren't end-to-end encrypted: Telegram can read what you and your assistant
        say there. Your bot ignores everyone but you.
      </Note>
    </>
  );
}

function TelegramPairing({ connection }: { connection: Connection }) {
  const [qr, setQr] = useState<string | null>(null);
  const url = connection.action_url;
  useEffect(() => {
    if (url) QRCode.toDataURL(url, { margin: 1, width: 360, color: { dark: "#16171a", light: "#ffffff" } }).then(setQr);
  }, [url]);

  return (
    <>
      <DialogHeader>
        <DialogTitle>One last step</DialogTitle>
        <DialogDescription>
          Open {connection.name} in Telegram and press <b>Start</b>. That makes it yours.
        </DialogDescription>
      </DialogHeader>
      <div className="flex items-center gap-5">
        <div className="shrink-0 rounded-2xl border bg-white p-2">
          {qr ? <img src={qr} alt="QR code to open your bot" className="size-36" /> : <div className="size-36" />}
        </div>
        <div className="flex flex-col gap-3 text-[14px] leading-relaxed">
          <span>Scan with your phone's camera, or open it on this computer.</span>
          {url && (
            <Button className="self-start" onClick={() => openExternal(url)}>
              <ExternalLink /> Open {connection.name}
            </Button>
          )}
          <span className="flex items-center gap-2 text-[13px] text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" /> Waiting for you to press Start…
          </span>
        </div>
      </div>
    </>
  );
}
