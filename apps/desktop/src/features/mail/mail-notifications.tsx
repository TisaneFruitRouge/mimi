import { useQuery } from "@tanstack/react-query";
import { EyeOff, Mail } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { MailNotifications } from "@/bindings/MailNotifications";
import type { NewMailNotify } from "@/bindings/NewMailNotify";
import { Grouped, IconTile, Row, Section } from "@/components/page";
import { Switch } from "@/components/ui/switch";
import { api, keys } from "@/lib/api";
import { useSettings } from "@/lib/queries";

const choices: { value: NewMailNotify; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "important", label: "Important" },
  { value: "all", label: "All" },
];

/**
 * Settings › Reminders & notifications: which new emails show as a notification on this
 * computer, and whether it says who wrote and the subject (see docs/email.md).
 */
export function MailNotificationsGroup() {
  const settings = useSettings().data;
  const overview = useQuery({ queryKey: keys.mailOverview(), queryFn: () => api.mailOverview() }).data;
  const current: MailNotifications = settings?.mail_notifications ?? { notify: "important", show_details: true };
  const save = (next: MailNotifications) => {
    if (!settings) return;
    api.putSettings({ ...settings, mail_notifications: next }).catch((e) => toast.error((e as Error).message));
  };
  // Whether something sorts new mail, as the daemon decides: without it, "important"
  // can't be told apart.
  const sorting =
    !!overview?.sorting && (overview.sorter === "jev" ? overview.jev_connected : overview.model_locality !== null);
  const detail = {
    off: "Off. New email still shows in Mail.",
    important: sorting
      ? "Email that needs a reply or looks important, once it's sorted (within a few minutes)."
      : "Your mail isn't being sorted, so: every new email except newsletters and automatic messages.",
    all: "Every new email in your inbox, newsletters included.",
  }[current.notify];
  return (
    <Section title="New email">
      <Grouped>
        <Row
          icon={
            <IconTile className="bg-[#efe9fb] text-[#6146ad]">
              <Mail />
            </IconTile>
          }
          title="Notify me about"
          detail={detail}
          className="[&_.truncate]:whitespace-normal"
          trailing={
            <div role="radiogroup" aria-label="Notify me about new email" className="flex rounded-[9px] bg-fill p-[2px]">
              {choices.map((c) => (
                <button
                  key={c.value}
                  role="radio"
                  aria-checked={current.notify === c.value}
                  disabled={!settings}
                  onClick={() => current.notify !== c.value && save({ ...current, notify: c.value })}
                  className={cn(
                    "h-7 rounded-[7px] px-3 type-subhead font-medium whitespace-nowrap transition-colors",
                    current.notify === c.value
                      ? "bg-background text-foreground shadow-[0_0_0_0.5px_rgb(0_0_0/0.06),0_1px_3px_rgb(0_0_0/0.12)]"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {c.label}
                </button>
              ))}
            </div>
          }
        />
        {current.notify !== "off" && (
          <Row
            icon={
              <IconTile className="bg-fill text-foreground">
                <EyeOff />
              </IconTile>
            }
            title="Show who it's from and the subject"
            detail={
              current.show_details
                ? "Turn off if others can see your screen, like when it's locked. Suspicious email never shows its subject."
                : "Notifications only say “New email”."
            }
            className="[&_.truncate]:whitespace-normal"
            trailing={
              <Switch
                checked={current.show_details}
                disabled={!settings}
                onCheckedChange={(on) => save({ ...current, show_details: on })}
                aria-label="Show who it's from and the subject"
              />
            }
          />
        )}
      </Grouped>
    </Section>
  );
}
