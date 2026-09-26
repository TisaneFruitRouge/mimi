import { useState } from "react";
import {
  CalendarDays,
  Check,
  Hash,
  Mail,
  MessageCircle,
  Phone,
  Plug,
  Send,
  ShieldCheck,
  Users,
} from "lucide-react";
import { cn } from "cn";

import type { Integration } from "@/bindings/Integration";
import type { IntegrationCategory } from "@/bindings/IntegrationCategory";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { useIntegrations } from "@/lib/queries";

const look: Record<string, { icon: typeof Plug; tone: string }> = {
  google_calendar: { icon: CalendarDays, tone: "bg-event-soft text-event" },
  caldav: { icon: CalendarDays, tone: "bg-event-soft text-event" },
  google_contacts: { icon: Users, tone: "bg-[#fbeedd] text-[#9a5a12]" },
  carddav: { icon: Users, tone: "bg-[#fbeedd] text-[#9a5a12]" },
  telegram: { icon: Send, tone: "bg-[#def3f7] text-[#136c86]" },
  signal: { icon: MessageCircle, tone: "bg-[#e5ecfb] text-[#3353a8]" },
  matrix: { icon: Hash, tone: "bg-subtle text-foreground" },
  whatsapp: { icon: Phone, tone: "bg-[#e1f4e6] text-[#1d7a3a]" },
  email: { icon: Mail, tone: "bg-[#efe9fb] text-[#6146ad]" },
};
const lookOf = (id: string) => look[id] ?? { icon: Plug, tone: "bg-subtle text-foreground" };

const categoryLabel: Record<IntegrationCategory, string> = {
  calendar: "Calendar",
  contacts: "Contacts",
  messaging: "Messaging",
  email: "Email",
};

export function ConnectionsView() {
  const integrations = useIntegrations();
  const [open, setOpen] = useState<Integration | null>(null);
  const all = integrations.data ?? [];
  const connected = all.filter((i) => i.status === "connected");
  const rest = all.filter((i) => i.status !== "connected");

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[960px] flex-col gap-9 px-6 pt-6 pb-12">
        <div className="flex flex-col gap-1.5">
          <h1 className="text-[30px] font-semibold tracking-[-0.03em]">Connections</h1>
          <p className="max-w-xl text-[15px] text-muted-foreground">
            Give your assistant access to the apps you use. It only sees what you connect, and
            always asks before it sends or changes anything.
          </p>
        </div>

        <section className="flex flex-col gap-3">
          <SectionLabel>Connected</SectionLabel>
          {connected.length === 0 ? (
            <div className="flex items-center gap-4 rounded-2xl border border-dashed bg-background/60 px-5 py-5">
              <div className="flex size-10 items-center justify-center rounded-xl bg-subtle text-faint">
                <Plug className="size-5" />
              </div>
              <div>
                <p className="text-[14.5px] font-medium">Nothing connected yet</p>
                <p className="text-[13px] text-muted-foreground">
                  Connected apps show up here, with exactly what the assistant can do with each.
                </p>
              </div>
            </div>
          ) : (
            <div className="flex flex-col divide-y rounded-2xl border bg-background">
              {connected.map((i) => (
                <IntegrationRow key={i.id} integration={i} onOpen={() => setOpen(i)} />
              ))}
            </div>
          )}
        </section>

        <section className="flex flex-col gap-3">
          <SectionLabel>Add more</SectionLabel>
          <div className="grid grid-cols-3 gap-3">
            {integrations.isLoading &&
              [0, 1, 2, 3, 4, 5].map((i) => <Skeleton key={i} className="h-40 rounded-2xl" />)}
            {rest.map((i) => (
              <IntegrationCard key={i.id} integration={i} onOpen={() => setOpen(i)} />
            ))}
          </div>
        </section>

        <div className="flex items-center gap-3 rounded-2xl bg-private-soft px-5 py-4 text-[13.5px] text-private">
          <ShieldCheck className="size-[18px] shrink-0" />
          Sign-ins are stored encrypted on this computer. Disconnecting an app deletes them.
        </div>
      </div>

      <IntegrationDialog integration={open} onClose={() => setOpen(null)} />
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

function IntegrationIcon({ id, size = "md" }: { id: string; size?: "md" | "lg" }) {
  const { icon: Icon, tone } = lookOf(id);
  return (
    <div
      className={cn(
        "flex shrink-0 items-center justify-center",
        size === "lg" ? "size-12 rounded-2xl" : "size-10 rounded-xl",
        tone,
      )}
    >
      <Icon className={size === "lg" ? "size-6" : "size-5"} />
    </div>
  );
}

function IntegrationCard({ integration: i, onOpen }: { integration: Integration; onOpen: () => void }) {
  const soon = i.status === "coming_soon";
  return (
    <button
      onClick={onOpen}
      className="group flex flex-col gap-4 rounded-2xl border bg-background p-4 text-left shadow-xs transition hover:-translate-y-px hover:shadow-md"
    >
      <div className="flex items-start justify-between">
        <IntegrationIcon id={i.id} />
        <span className="font-mono text-[10.5px] tracking-wide text-faint uppercase">
          {categoryLabel[i.category]}
        </span>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-[14.5px] font-medium">{i.name}</span>
        <span className="text-[13px] leading-snug text-muted-foreground">{i.description}</span>
      </div>
      <span
        className={cn(
          "mt-auto inline-flex h-8 items-center justify-center rounded-[9px] text-[12.5px] font-medium",
          soon ? "bg-subtle text-faint" : "bg-foreground text-background",
        )}
      >
        {soon ? "Coming soon" : "Connect"}
      </span>
    </button>
  );
}

function IntegrationRow({ integration: i, onOpen }: { integration: Integration; onOpen: () => void }) {
  return (
    <button onClick={onOpen} className="flex items-center gap-4 px-4 py-3.5 text-left hover:bg-subtle/60">
      <IntegrationIcon id={i.id} />
      <div className="min-w-0 flex-1">
        <div className="text-[14.5px] font-medium">{i.name}</div>
        <div className="truncate text-[13px] text-muted-foreground">{i.abilities.join(" · ")}</div>
      </div>
      <span className="font-mono text-[11.5px] text-private">connected</span>
    </button>
  );
}

function IntegrationDialog({ integration: i, onClose }: { integration: Integration | null; onClose: () => void }) {
  return (
    <Dialog open={!!i} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        {i && (
          <>
            <DialogHeader className="flex-row items-center gap-4 space-y-0 text-left">
              <IntegrationIcon id={i.id} size="lg" />
              <div className="flex flex-col gap-1">
                <DialogTitle>{i.name}</DialogTitle>
                <DialogDescription>{i.description}</DialogDescription>
              </div>
            </DialogHeader>
            <div className="flex flex-col gap-2.5">
              <span className="text-sm font-medium">What your assistant can do with it</span>
              <ul className="flex flex-col gap-2">
                {i.abilities.map((a) => (
                  <li key={a} className="flex items-start gap-2.5 text-[13.5px]">
                    <Check className="mt-0.5 size-4 shrink-0 text-private" />
                    {a}
                  </li>
                ))}
              </ul>
            </div>
            {i.status === "coming_soon" && (
              <p className="rounded-xl bg-subtle px-4 py-3 text-[13px] leading-relaxed text-muted-foreground">
                This connection isn’t available yet. It’s part of an upcoming update.
              </p>
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
