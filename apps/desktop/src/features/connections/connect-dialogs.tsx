import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import QRCode from "qrcode";
import { Check, ChevronRight, CircleCheck, Copy, ExternalLink, Loader2, Lock, TriangleAlert } from "lucide-react";
import { cn } from "cn";

import type { Connection } from "@/bindings/Connection";
import type { ConnectionSetup } from "@/bindings/ConnectionSetup";
import type { MailDiscovery } from "@/bindings/MailDiscovery";
import type { MailSecurity } from "@/bindings/MailSecurity";
import type { MailServers } from "@/bindings/MailServers";
import { copyText } from "@/components/app-context-menu";
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api, keys } from "@/lib/api";
import { useAssistantName, useConnections } from "@/lib/queries";
import { openExternal } from "@/lib/transport";

export type ConnectKind = "google_calendar" | "caldav" | "telegram" | "signal" | "matrix" | "email";

export function ConnectDialog({
  kind,
  reconnect,
  onClose,
}: {
  kind: ConnectKind | null;
  /** A Google connection to sign in again. */
  reconnect?: string;
  onClose: () => void;
}) {
  return (
    <Dialog open={kind !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-6 sm:max-w-[500px]">
        {kind === "google_calendar" && <GoogleCalendar onDone={onClose} reconnect={reconnect} />}
        {kind === "caldav" && <CalDav onDone={onClose} />}
        {kind === "telegram" && <Telegram onDone={onClose} />}
        {kind === "signal" && <Signal onDone={onClose} />}
        {kind === "matrix" && <Matrix onDone={onClose} />}
        {kind === "email" && <EmailAccount onDone={onClose} />}
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
  return <ol className="flex flex-col gap-4 type-callout">{children}</ol>;
}

function Step({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-fill type-footnote font-semibold text-muted-foreground tabular-nums">
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
        "flex gap-2.5 rounded-[14px] px-4 py-3 type-subhead",
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
        <CircleCheck className="size-12 text-private" strokeWidth={1.5} />
        <div>
          <p className="type-headline">{connection.name} is connected</p>
          <p className="type-callout text-muted-foreground">{connection.detail}</p>
        </div>
      </div>
      <DialogFooter className="sm:justify-between">
        {extra ?? <span />}
        <Button onClick={onDone}>Done</Button>
      </DialogFooter>
    </>
  );
}

/** Google Calendar: signing in with Google when this build can, else the private address. */
function GoogleCalendar({ onDone, reconnect }: { onDone: () => void; reconnect?: string }) {
  const info = useQuery({ queryKey: ["google-sign-in"], queryFn: api.googleSignInInfo });
  const [address, setAddress] = useState(false);
  if (info.isLoading) return <DialogTitle className="sr-only">Connect Google Calendar</DialogTitle>;
  if (info.data?.available && !address)
    return <GoogleSignInFlow onDone={onDone} reconnect={reconnect} onAddress={() => setAddress(true)} />;
  return <GoogleAddress onDone={onDone} signInAvailable={!!info.data?.available} />;
}

function GoogleSignInFlow({
  onDone,
  reconnect,
  onAddress,
}: {
  onDone: () => void;
  reconnect?: string;
  onAddress: () => void;
}) {
  const [signIn, setSignIn] = useState<{ id: string; url: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const status = useQuery({
    queryKey: ["google-sign-in", signIn?.id],
    queryFn: () => api.googleSignInStatus(signIn!.id),
    enabled: !!signIn,
    refetchInterval: (q) => (q.state.data?.state === "waiting" || !q.state.data ? 1000 : false),
  });
  const state = status.data;
  useEffect(() => {
    if (state?.state === "failed") {
      setError(state.error);
      setSignIn(null);
    }
  }, [state]);
  // Closing the dialog midway stops listening for Google's answer (harmless once done).
  useEffect(
    () => () => {
      if (signIn) api.cancelGoogleSignIn(signIn.id).catch(() => {});
    },
    [signIn],
  );

  if (state?.state === "done") return <Done connection={state.connection} onDone={onDone} />;

  const start = async () => {
    setStarting(true);
    setError(null);
    try {
      const started = await api.startGoogleSignIn(reconnect);
      setSignIn(started);
      await openExternal(started.url);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setStarting(false);
    }
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>{reconnect ? "Sign in to Google again" : "Connect Google Calendar"}</DialogTitle>
        <DialogDescription>
          Sign in with Google so your assistant can read your calendars and add, move or remove events, always
          with your OK unless you allow it in Permissions.
        </DialogDescription>
      </DialogHeader>
      {signIn ? (
        <div className="flex flex-col items-center gap-3 py-2 text-center">
          <Loader2 className="size-7 animate-spin text-muted-foreground" />
          <p className="type-callout">Finish signing in in your browser.</p>
          <p className="type-subhead text-muted-foreground">
            Choose your account, then allow both choices Google shows.
          </p>
          <Button variant="ghost" size="sm" onClick={() => openExternal(signIn.url)}>
            <ExternalLink /> Open the Google page again
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <Button size="lg" onClick={start} disabled={starting}>
            {starting && <Loader2 className="animate-spin" />} Sign in with Google
          </Button>
          {error && <p className="type-subhead text-destructive">{error}</p>}
        </div>
      )}
      <Note>
        Google sends its answer straight back to this computer, and the sign-in is stored encrypted here. Your
        events go only between this computer and Google.
      </Note>
      {!reconnect && !signIn && (
        <button
          onClick={onAddress}
          className="-mt-2 self-center type-subhead text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
        >
          Or read a calendar through its private address, without signing in
        </button>
      )}
    </>
  );
}

function GoogleAddress({ onDone, signInAvailable }: { onDone: () => void; signInAvailable: boolean }) {
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
          {signInAvailable
            ? "No Google sign-in: your assistant reads the calendar through its private address."
            : "Your assistant reads the calendar through its private address. Signing in with Google isn't available in this version of the app."}
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
              className="text-[14px]"
              autoFocus
            />
            <Button type="submit" disabled={busy || !url.trim()}>
              {busy && <Loader2 className="animate-spin" />} Connect
            </Button>
          </form>
          {error && <p className="type-subhead text-destructive">{error}</p>}
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
        <DialogDescription>Read, add and change events in iCloud, Fastmail, Nextcloud and other calendars.</DialogDescription>
      </DialogHeader>
      <div className="grid grid-cols-4 rounded-[10px] bg-fill p-[3px]">
        {caldavServices.map((s) => (
          <button
            key={s.id}
            onClick={() => setServiceId(s.id)}
            className={cn(
              "h-7 rounded-[7px] text-[13px] font-medium transition-all duration-200",
              s.id === serviceId
                ? "bg-background text-foreground shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                : "text-muted-foreground hover:text-foreground",
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
              className="text-[14px]"
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
          <p className="type-subhead text-muted-foreground">
            {service.help}{" "}
            {service.helpUrl && (
              <button type="button" className="font-medium text-foreground underline underline-offset-2" onClick={() => openExternal(service.helpUrl!)}>
                Open
              </button>
            )}
          </p>
        </div>
        {error && <p className="type-subhead text-destructive">{error}</p>}
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
              className="text-[14px]"
              autoFocus
            />
            <Button type="submit" disabled={busy || !token.trim()}>
              {busy && <Loader2 className="animate-spin" />} Connect
            </Button>
          </form>
          {error && <p className="type-subhead text-destructive">{error}</p>}
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
        <div className="shrink-0 rounded-[18px] bg-white p-2.5 shadow-[var(--shadow-card)]">
          {qr ? <img src={qr} alt="QR code to open your bot" className="size-36" /> : <div className="size-36" />}
        </div>
        <div className="flex flex-col gap-3 type-callout">
          <span>Scan with your phone's camera, or open it on this computer.</span>
          {url && (
            <Button className="self-start" onClick={() => openExternal(url)}>
              <ExternalLink /> Open {connection.name}
            </Button>
          )}
          <span className="flex items-center gap-2 type-subhead text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" /> Waiting for you to press Start…
          </span>
        </div>
      </div>
    </>
  );
}

/**
 * Signal: Mimi links to the user's own account as a device, like Signal Desktop. The
 * daemon gets the linking address from Signal and publishes it as the connection's
 * `action_url`; it's only ever shown here as a QR code made on this computer, never
 * opened as a link.
 */
function Signal({ onDone }: { onDone: () => void }) {
  const assistant = useAssistantName();
  const connections = useConnections().data;
  const { busy, error, created, connect } = useConnect();
  const existing = connections?.find((c) => c.integration === "signal");
  // The connection updates live (via events) as codes arrive and once it's scanned.
  const live = created ? (connections?.find((c) => c.id === created.id) ?? created) : undefined;
  // A link already under way (the dialog was closed meanwhile): show its code again.
  const linking = live ?? (existing?.status === "needs_action" && existing.action_url ? existing : undefined);
  const start = () => connect({ integration: "signal" });

  if (live && live.status === "ok") {
    return <Done connection={live} onDone={onDone} />;
  }
  if (linking) {
    return <SignalCode connection={linking} onRetry={start} busy={busy} />;
  }

  const again = existing && existing.status !== "ok";
  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect Signal</DialogTitle>
        <DialogDescription>
          Chat with {assistant} in your Note to Self, end-to-end encrypted. You link it from your
          phone, like Signal on a computer.
        </DialogDescription>
      </DialogHeader>
      <Steps>
        <Step n={1}>
          <span>Show a code to scan.</span>
          <Button className="self-start" disabled={busy} onClick={start}>
            {busy && <Loader2 className="animate-spin" />} {again ? "Link again" : "Show the code"}
          </Button>
          {(error || shownError(existing)) && (
            <p className="type-subhead text-destructive">{error ?? shownError(existing)}</p>
          )}
        </Step>
        <Step n={2}>
          <span>
            On your phone, open Signal, go to <b>Settings › Linked devices</b>, tap <b>+</b> and scan it.
          </span>
        </Step>
        <Step n={3}>
          <span>Write to {assistant} in <b>Note to Self</b>. Replies come back there.</span>
        </Step>
      </Steps>
      <Note>
        {assistant} only reads Note to Self. Like any linked device it receives your other chats too,
        but ignores them: nothing from them is read, kept or shown to your assistant.
      </Note>
      <Note icon="warn">
        Signal doesn't notify you about Note to Self, so messages and reminders from {assistant} there
        arrive silently.
      </Note>
    </>
  );
}

/** What went wrong with a Signal connection, when it isn't just waiting for a scan. */
function shownError(connection: Connection | undefined) {
  return connection?.status === "error" ? connection.detail : null;
}

function SignalCode({ connection, onRetry, busy }: { connection: Connection; onRetry: () => void; busy: boolean }) {
  const [qr, setQr] = useState<string | null>(null);
  const url = connection.action_url;
  useEffect(() => {
    if (!url) return setQr(null);
    let current = true;
    QRCode.toDataURL(url, { margin: 1, width: 480, color: { dark: "#16171a", light: "#ffffff" } }).then(
      (data) => current && setQr(data),
    );
    return () => {
      current = false;
    };
  }, [url]);

  return (
    <>
      <DialogHeader>
        <DialogTitle>Scan with Signal</DialogTitle>
        <DialogDescription>
          On your phone, open Signal, go to <b>Settings › Linked devices</b>, tap <b>+</b> and scan this code.
        </DialogDescription>
      </DialogHeader>
      <div className="flex flex-col items-center gap-4">
        <div className="rounded-[18px] bg-white p-3 shadow-[var(--shadow-card)]">
          {qr ? (
            <img src={qr} alt="Code to link Signal" className="size-56" />
          ) : (
            <div className="flex size-56 items-center justify-center">
              <Loader2 className="size-6 animate-spin text-muted-foreground" />
            </div>
          )}
        </div>
        <span
          className={cn(
            "flex items-center gap-2 type-subhead",
            connection.status === "error" ? "text-destructive" : "text-muted-foreground",
          )}
        >
          {connection.status !== "error" && url && <Loader2 className="size-3.5 animate-spin" />}
          {url ? "Waiting for you to scan… A new code appears if this one expires." : connection.detail}
        </span>
        {/* The daemon's passing states ("Getting a code…", "Retrying…") end with an ellipsis. */}
        {!url && !connection.detail.endsWith("…") && (
          <Button variant="secondary" size="sm" disabled={busy} onClick={onRetry}>
            {busy && <Loader2 className="animate-spin" />} Show a new code
          </Button>
        )}
      </div>
    </>
  );
}

/**
 * How to make the assistant's account on a server whose sign-up is closed, per server
 * program. `asks`: the command asks for the password itself (so it stays out of the
 * shell's history); otherwise it's in the command.
 */
const matrixServers = [
  {
    id: "synapse",
    name: "Synapse",
    where: "On the server, in a terminal:",
    command: (name: string) =>
      `register_new_matrix_user -c /etc/matrix-synapse/homeserver.yaml -u ${name} --no-admin`,
    asks: true,
    note: "Change the path if your homeserver.yaml is somewhere else.",
  },
  {
    id: "synapse-docker",
    name: "Synapse in Docker",
    where: "On the server, in a terminal:",
    command: (name: string) =>
      `docker exec -it synapse register_new_matrix_user -c /data/homeserver.yaml -u ${name} --no-admin`,
    asks: true,
    note: "Change synapse to your container's name if it's different.",
  },
  {
    id: "tuwunel",
    name: "Tuwunel, conduwuit or Continuwuity",
    where: "Send this in your server's admin room:",
    command: (name: string, password: string) => `!admin users create-user ${name} ${password}`,
    asks: false,
    note: null,
  },
  {
    id: "mas",
    name: "Matrix Authentication Service",
    where: "Where mas-cli runs:",
    command: (name: string, password: string) =>
      `mas-cli manage register-user --username ${name} --password ${password} --no-admin --yes`,
    asks: false,
    note: "Your server must allow signing in with a password, for apps like this one.",
  },
  {
    id: "dendrite",
    name: "Dendrite",
    where: "On the server, in a terminal:",
    command: (name: string) => `create-account -config /etc/dendrite/dendrite.yaml -username ${name}`,
    asks: true,
    note: "Change the path if your dendrite.yaml is somewhere else.",
  },
  {
    id: "dendrite-docker",
    name: "Dendrite in Docker",
    where: "On the server, in a terminal:",
    command: (name: string) =>
      `docker exec -it dendrite-monolith-1 create-account -config /etc/dendrite/dendrite.yaml -username ${name}`,
    asks: true,
    note: "Change dendrite-monolith-1 to your container's name if it's different (docker ps shows it).",
  },
] as const;

/** A strong password with nothing a shell or chat command would trip on. */
function newPassword() {
  const letters = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
  const bytes = crypto.getRandomValues(new Uint8Array(24));
  return Array.from(bytes, (b) => letters[b % letters.length]).join("");
}

/** The name part of a Matrix address: "@mimi:example.org" → "mimi". */
function localPart(address: string) {
  const name = address.trim().replace(/^@/, "").split(":")[0].toLowerCase();
  return /^[a-z0-9._=\/+-]+$/.test(name) ? name : "";
}

function Matrix({ onDone }: { onDone: () => void }) {
  const [ownServer, setOwnServer] = useState(false);
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  // The password suggested for an account made on the user's own server.
  const [suggested, setSuggested] = useState("");
  const [homeserver, setHomeserver] = useState("");
  const [showServer, setShowServer] = useState(false);
  const { busy, error, created, connect } = useConnect();
  // The connection updates live (via events) once the owner sends the code.
  const live = useConnections().data?.find((c) => c.id === created?.id) ?? created;

  if (live && live.status === "ok") {
    return <Done connection={live} onDone={onDone} />;
  }
  if (live) return <MatrixPairing connection={live} />;

  const ready = user.trim().length > 0 && password.length > 0;
  const addressField = (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor="matrix-user">Address</Label>
      <Input
        id="matrix-user"
        value={user}
        onChange={(e) => setUser(e.target.value)}
        placeholder={ownServer ? "@assistant:example.org" : "@my-assistant:matrix.org"}
        autoComplete="off"
        spellCheck={false}
        autoFocus
      />
    </div>
  );
  const form = (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!ready) return;
        connect({
          integration: "matrix",
          user,
          password,
          homeserver: homeserver.trim() || null,
        });
      }}
    >
      {!ownServer && addressField}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="matrix-password">Password</Label>
        <Input
          id="matrix-password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="off"
        />
      </div>
      <button
        type="button"
        onClick={() => setShowServer((v) => !v)}
        className="flex items-center gap-1 self-start type-subhead font-medium text-muted-foreground hover:text-foreground"
        aria-expanded={showServer}
      >
        <ChevronRight className={cn("size-3.5 transition-transform", showServer && "rotate-90")} />
        Server settings
      </button>
      {showServer && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="matrix-server">Server address</Label>
          <Input
            id="matrix-server"
            value={homeserver}
            onChange={(e) => setHomeserver(e.target.value)}
            placeholder="https://matrix.example.org"
            autoComplete="off"
            spellCheck={false}
          />
          <p className="type-subhead text-muted-foreground">
            Only needed when Mimi can't find the server from the address.
          </p>
        </div>
      )}
      {error && <p className="type-subhead text-destructive">{error}</p>}
      <Button type="submit" className="self-start" disabled={busy || !ready}>
        {busy && <Loader2 className="animate-spin" />} {busy ? "Signing in…" : "Connect"}
      </Button>
    </form>
  );

  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect Matrix</DialogTitle>
        <DialogDescription>
          Chat with your assistant from Element or any Matrix app, end-to-end encrypted.
        </DialogDescription>
      </DialogHeader>
      <div className="grid grid-cols-2 rounded-[10px] bg-fill p-[3px]" role="radiogroup" aria-label="Where its account lives">
        {[
          { own: false, label: "A public server" },
          { own: true, label: "My own server" },
        ].map((o) => (
          <button
            key={o.label}
            role="radio"
            aria-checked={ownServer === o.own}
            onClick={() => {
              setOwnServer(o.own);
              // On their own server they make the account: suggest a strong password. On a
              // public one they choose it when signing up.
              if (o.own && !password) {
                const p = newPassword();
                setPassword(p);
                setSuggested(p);
              } else if (!o.own && password === suggested) {
                setPassword("");
              }
            }}
            className={cn(
              "h-7 rounded-[7px] text-[13px] font-medium transition-all duration-200",
              ownServer === o.own
                ? "bg-background text-foreground shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {o.label}
          </button>
        ))}
      </div>
      {ownServer ? (
        <Steps>
          <Step n={1}>
            <span>Choose an address for your assistant on your server, separate from yours.</span>
            {addressField}
          </Step>
          <Step n={2}>
            <span>Create the account on your server.</span>
            <MatrixAccountCommand name={localPart(user)} password={password} />
          </Step>
          <Step n={3}>
            <span>Connect with the account's password.</span>
            {form}
          </Step>
        </Steps>
      ) : (
        <Steps>
          <Step n={1}>
            <span>
              Make a new account for your assistant, separate from yours, on matrix.org or any server
              that lets people sign up. If your server's sign-up is closed, choose <b>My own server</b>.
            </span>
            <Button
              variant="outline"
              size="sm"
              className="self-start"
              onClick={() => openExternal("https://app.element.io/#/register")}
            >
              <ExternalLink /> Create an account in Element
            </Button>
          </Step>
          <Step n={2}>
            <span>Enter the new account's address and password.</span>
            {form}
          </Step>
        </Steps>
      )}
      <Note>
        Your assistant uses only its own account, never yours, and talks only to you. Its password
        is used once to set up encryption and isn't kept.
      </Note>
    </>
  );
}

/** The command that makes the assistant's account, for the server program the user picks. */
function MatrixAccountCommand({ name, password }: { name: string; password: string }) {
  const [serverId, setServerId] = useState<(typeof matrixServers)[number]["id"]>("synapse");
  const server = matrixServers.find((s) => s.id === serverId)!;
  return (
    <div className="flex flex-col gap-2.5">
      <Select value={serverId} onValueChange={(v) => setServerId(v as typeof serverId)}>
        <SelectTrigger className="w-full" aria-label="Your server's program">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {matrixServers.map((s) => (
            <SelectItem key={s.id} value={s.id}>
              {s.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {name ? (
        <>
          <span className="type-subhead text-muted-foreground">{server.where}</span>
          <CopyBox text={server.command(name, password)} label="command" />
          {server.asks && (
            <>
              <span className="type-subhead text-muted-foreground">When it asks for a password, use this one:</span>
              <CopyBox text={password} label="password" />
            </>
          )}
          {server.note && <span className="type-subhead text-muted-foreground">{server.note}</span>}
        </>
      ) : (
        <span className="type-subhead text-muted-foreground">
          Enter an address above to see the command, like @assistant:example.org.
        </span>
      )}
    </div>
  );
}

/** Text to paste somewhere else, with a Copy button. */
function CopyBox({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  return (
    <div className="flex items-start gap-2 rounded-[10px] bg-subtle py-2 pr-2 pl-3">
      <code className="min-w-0 flex-1 font-mono text-[12.5px] leading-5 break-all select-all">{text}</code>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        aria-label={`Copy the ${label}`}
        className="shrink-0 rounded-full text-muted-foreground"
        onClick={() => {
          copyText(text);
          setCopied(true);
        }}
      >
        {copied ? <Check /> : <Copy />}
      </Button>
    </div>
  );
}

/** The six-digit code in a connection's "send this code" line. */
const pairingCode = (detail: string) => detail.match(/\b(\d{6})\b/)?.[1] ?? null;

function MatrixPairing({ connection }: { connection: Connection }) {
  const [qr, setQr] = useState<string | null>(null);
  const url = connection.action_url;
  const code = pairingCode(connection.detail);
  useEffect(() => {
    if (url) QRCode.toDataURL(url, { margin: 1, width: 360, color: { dark: "#16171a", light: "#ffffff" } }).then(setQr);
  }, [url]);

  if (connection.status === "error") {
    return (
      <DialogHeader>
        <DialogTitle>Something went wrong</DialogTitle>
        <DialogDescription>{connection.detail}</DialogDescription>
      </DialogHeader>
    );
  }
  return (
    <>
      <DialogHeader>
        <DialogTitle>One last step</DialogTitle>
        <DialogDescription>
          From your own Matrix account, start a chat with {connection.name} and send it this code.
          That makes it yours.
        </DialogDescription>
      </DialogHeader>
      {code && (
        <p className="mx-auto w-fit rounded-[14px] bg-subtle px-5 py-3 font-mono text-[28px] font-semibold tracking-[0.2em] tabular-nums">
          {code}
        </p>
      )}
      <div className="flex items-center gap-5">
        <div className="shrink-0 rounded-[18px] bg-white p-2.5 shadow-[var(--shadow-card)]">
          {qr ? <img src={qr} alt="QR code to open a chat with your assistant" className="size-36" /> : <div className="size-36" />}
        </div>
        <div className="flex flex-col gap-3 type-callout">
          <span>Scan with your phone's camera, or open it on this computer.</span>
          {url && (
            <Button className="self-start" onClick={() => openExternal(url)}>
              <ExternalLink /> Start the chat
            </Button>
          )}
          <span className="flex items-center gap-2 type-subhead text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" /> Waiting for your code…
          </span>
        </div>
      </div>
    </>
  );
}

const securityLabels: Record<MailSecurity, string> = {
  tls: "SSL/TLS",
  start_tls: "STARTTLS",
  plain: "None (this computer only)",
};

function EmailAccount({ onDone }: { onDone: () => void }) {
  const presets =
    useQuery({ queryKey: keys.mailPresets, queryFn: api.mailPresets, staleTime: Infinity }).data ?? [];
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [servers, setServers] = useState<MailServers>(emptyServers);
  // Server settings the user changed by hand win over what was discovered.
  const [edited, setEdited] = useState(false);
  const [showServers, setShowServers] = useState(false);
  const { busy, error, created, connect } = useConnect();

  // Work out the servers from the address, like mail apps do.
  const address = email.trim().toLowerCase();
  const looksComplete = /^[^@\s]+@[^@\s]+\.[^@\s]{2,}$/.test(address);
  const [debounced, setDebounced] = useState("");
  useEffect(() => {
    const t = setTimeout(() => setDebounced(looksComplete ? address : ""), 500);
    return () => clearTimeout(t);
  }, [address, looksComplete]);
  const discovery = useQuery({
    queryKey: ["mail", "discover", debounced],
    queryFn: () => api.mailDiscover(debounced),
    enabled: debounced !== "",
    staleTime: Infinity,
    retry: false,
  });
  const found = debounced === address ? discovery.data : undefined;
  const lookupError = debounced === address && discovery.error ? discovery.error.message : null;
  const searching = looksComplete && (debounced !== address || discovery.isFetching);

  // Pre-fill the server fields with what was found, unless the user typed their own.
  useEffect(() => {
    if (edited || !found) return;
    if (found.servers) setServers(found.servers);
    setShowServers(found.supported && !found.servers);
  }, [found, edited]);

  if (created) return <Done connection={created} onDone={onDone} />;

  const preset = presets.find((p) => p.id === found?.preset);
  const unsupported = found !== undefined && !found.supported;
  const needsServers = found !== undefined && !found.servers;
  const useCustom = edited || (needsServers && found?.supported);
  const help = found?.help ?? (found?.needs_app_password ? preset?.help : null);
  const helpUrl = found?.help_url ?? (found?.needs_app_password ? preset?.help_url : null);
  const ready =
    looksComplete &&
    password.length > 0 &&
    !unsupported &&
    !searching &&
    (!useCustom || (servers.imap_host.trim() !== "" && servers.smtp_host.trim() !== ""));
  const set = (patch: Partial<MailServers>) => {
    setEdited(true);
    setServers((s) => ({ ...s, ...patch }));
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Connect your email</DialogTitle>
        <DialogDescription>
          Your assistant reads, sorts and drafts replies. Nothing is sent without your OK.
        </DialogDescription>
      </DialogHeader>
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (!ready) return;
          connect(
            useCustom
              ? { integration: "email", email, password, preset: "other", servers }
              : { integration: "email", email, password, preset: found?.preset ?? null, servers: found?.servers ?? null },
          );
        }}
      >
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="mail-address">Email address</Label>
          <Input
            id="mail-address"
            type="email"
            value={email}
            onChange={(e) => {
              setEmail(e.target.value);
              setEdited(false);
            }}
            placeholder="you@example.com"
            autoComplete="off"
            autoFocus
          />
          {lookupError && !searching ? (
            <p className="type-subhead text-destructive">{lookupError}</p>
          ) : (
            <DiscoveryLine searching={searching} found={found} />
          )}
        </div>
        {unsupported ? (
          <Note icon="warn">{found?.help ?? "This mail service can't be connected."}</Note>
        ) : (
          <>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="mail-password">{found?.needs_app_password ? "App password" : "Password"}</Label>
              <Input
                id="mail-password"
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="off"
              />
              {help && (
                <p className="type-subhead text-muted-foreground">
                  {help}{" "}
                  {helpUrl && (
                    <button
                      type="button"
                      className="font-medium text-foreground underline underline-offset-2"
                      onClick={() => openExternal(helpUrl)}
                    >
                      Open
                    </button>
                  )}
                </p>
              )}
            </div>
            {looksComplete && !searching && (
              <div className="flex flex-col gap-3">
                <button
                  type="button"
                  onClick={() => setShowServers((v) => !v)}
                  className="flex items-center gap-1 self-start type-subhead font-medium text-muted-foreground hover:text-foreground"
                  aria-expanded={showServers}
                >
                  <ChevronRight className={cn("size-3.5 transition-transform", showServers && "rotate-90")} />
                  Server settings
                </button>
                {showServers && <CustomServers email={email} servers={servers} set={set} />}
              </div>
            )}
          </>
        )}
        {error && <p className="type-subhead text-destructive">{error}</p>}
        <DialogFooter>
          <Button type="submit" disabled={busy || !ready}>
            {busy && <Loader2 className="animate-spin" />} {busy ? "Checking…" : "Connect"}
          </Button>
        </DialogFooter>
      </form>
      <Note>
        Your password is stored encrypted on this computer and only sent to your mail service. The
        last 90 days of mail are copied here, so your assistant can search and sort it without
        sending it anywhere else.
      </Note>
    </>
  );
}

const emptyServers: MailServers = {
  imap_host: "",
  imap_port: 993,
  imap_security: "tls",
  smtp_host: "",
  smtp_port: 465,
  smtp_security: "tls",
  username: null,
};

/** Under the address: what Mimi worked out from it. */
function DiscoveryLine({ searching, found }: { searching: boolean; found: MailDiscovery | undefined }) {
  if (searching)
    return (
      <p className="flex items-center gap-1.5 type-subhead text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> Looking up your mail settings…
      </p>
    );
  if (!found || !found.supported) return null;
  if (!found.servers)
    return (
      <p className="type-subhead text-muted-foreground">
        Couldn't find the settings for this address. Enter them under Server settings, from your
        provider's help pages.
      </p>
    );
  return (
    <p className="flex items-center gap-1.5 type-subhead text-private">
      <CircleCheck className="size-3.5" />
      {found.provider ? `${found.provider}` : `Found your mail servers (${found.servers.imap_host})`}
    </p>
  );
}

function CustomServers({
  email,
  servers,
  set,
}: {
  email: string;
  servers: MailServers;
  set: (patch: Partial<MailServers>) => void;
}) {
  const server = (kind: "imap" | "smtp", label: string) => {
    const host = kind === "imap" ? servers.imap_host : servers.smtp_host;
    const port = kind === "imap" ? servers.imap_port : servers.smtp_port;
    const security = kind === "imap" ? servers.imap_security : servers.smtp_security;
    return (
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`mail-${kind}`}>{label}</Label>
        <div className="flex gap-2">
          <Input
            id={`mail-${kind}`}
            value={host}
            onChange={(e) => set(kind === "imap" ? { imap_host: e.target.value } : { smtp_host: e.target.value })}
            placeholder={kind === "imap" ? "imap.example.com" : "smtp.example.com"}
            className="min-w-0 flex-1 text-[14px]"
          />
          <Input
            value={port}
            inputMode="numeric"
            aria-label={`${label} port`}
            onChange={(e) => {
              const n = Number(e.target.value.replace(/\D/g, "")) || 0;
              set(kind === "imap" ? { imap_port: n } : { smtp_port: n });
            }}
            className="w-[72px] text-[14px] tabular-nums"
          />
          <Select
            value={security}
            onValueChange={(v) =>
              set(kind === "imap" ? { imap_security: v as MailSecurity } : { smtp_security: v as MailSecurity })
            }
          >
            <SelectTrigger className="w-[132px]" aria-label={`${label} encryption`}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {(Object.keys(securityLabels) as MailSecurity[]).map((s) => (
                <SelectItem key={s} value={s}>
                  {securityLabels[s]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
    );
  };
  return (
    <>
      {server("imap", "Incoming mail server (IMAP)")}
      {server("smtp", "Outgoing mail server (SMTP)")}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="mail-username">Username</Label>
        <Input
          id="mail-username"
          value={servers.username ?? ""}
          onChange={(e) => set({ username: e.target.value || null })}
          placeholder={email.trim() || "you@example.com"}
          autoComplete="off"
        />
        <p className="type-subhead text-muted-foreground">
          What you sign in to your mail with, not your name. Leave it empty to use your email
          address.
        </p>
      </div>
    </>
  );
}
