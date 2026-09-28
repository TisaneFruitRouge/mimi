import { motion } from "motion/react";
import { BellRing, CalendarPlus, Send } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Autonomy } from "@/bindings/Autonomy";
import type { Permissions } from "@/bindings/Permissions";
import { Grouped, IconTile, Page, PageHeader, Row, Section } from "@/components/page";
import { api } from "@/lib/api";
import { useAssistantName, useSettings } from "@/lib/queries";

const actions: {
  key: keyof Permissions;
  title: string;
  icon: typeof Send;
  tone: string;
  detail: Record<Autonomy, string>;
}[] = [
  {
    key: "send_mail",
    title: "Send emails",
    icon: Send,
    tone: "bg-[#0a84ff] text-white",
    detail: {
      ask: "Shows you the whole email to approve before it goes out.",
      automatic:
        "Sends on its own to your contacts and people you've emailed before. Still asks before writing to anyone new.",
    },
  },
  {
    key: "add_events",
    title: "Add calendar events",
    icon: CalendarPlus,
    tone: "bg-[#ff3b30] text-white",
    detail: {
      ask: "Shows you each event to approve before it's added.",
      automatic: "Adds events to your calendars on its own.",
    },
  },
  {
    key: "schedule",
    title: "Set reminders and routines",
    icon: BellRing,
    tone: "bg-[#ff9f0a] text-white",
    detail: {
      ask: "Shows you each reminder or routine, and each change, to approve first.",
      automatic: "Sets, changes and cancels them on its own. Each one shows in the chat with Undo.",
    },
  },
];

/** Settings › Permissions: what the assistant may do without asking first. */
export function PermissionsView() {
  const settings = useSettings().data;
  const assistant = useAssistantName();

  const set = async (key: keyof Permissions, value: Autonomy) => {
    if (!settings || settings.permissions[key] === value) return;
    try {
      await api.putSettings({ ...settings, permissions: { ...settings.permissions, [key]: value } });
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Page>
      <PageHeader title="Permissions" subtitle={`What ${assistant} may do for you without asking first.`} />
      <Section title="Actions">
        <Grouped>
          {actions.map((a) => {
            const value = settings?.permissions[a.key] ?? "ask";
            return (
              <Row
                key={a.key}
                icon={
                  <IconTile size="sm" className={a.tone}>
                    <a.icon />
                  </IconTile>
                }
                title={a.title}
                detail={a.detail[value]}
                className="py-3 [&_.truncate]:whitespace-normal"
                trailing={
                  <Segmented
                    label={a.title}
                    value={value}
                    disabled={!settings}
                    onChange={(v) => set(a.key, v)}
                  />
                }
              />
            );
          })}
        </Grouped>
      </Section>
      <p className="-mt-6 max-w-[640px] px-1 type-footnote text-muted-foreground">
        Everything {assistant} does shows in the chat, whichever you choose. Emails, websites and messages from
        other people can hide instructions meant for {assistant}; asking first is what stops them, so choose
        Automatic only for what you're comfortable with.
      </p>
    </Page>
  );
}

const choices: { value: Autonomy; label: string }[] = [
  { value: "ask", label: "Ask first" },
  { value: "automatic", label: "Automatic" },
];

function Segmented({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: Autonomy;
  disabled?: boolean;
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
            disabled={disabled}
            onClick={() => onChange(c.value)}
            className={cn(
              "relative h-[26px] rounded-[7px] px-3 text-[13px] font-medium transition-colors duration-200",
              active ? "text-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {active && (
              <motion.span
                layoutId={`permission-${label}`}
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
