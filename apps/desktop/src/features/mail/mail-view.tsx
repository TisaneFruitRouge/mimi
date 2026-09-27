import { Mail } from "lucide-react";

import { IconTile } from "@/components/page";
import { Button } from "@/components/ui/button";
import type { Section } from "@/features/shell/top-bar";
import type { Draft } from "@/lib/draft";

/**
 * The Mail panel. A placeholder until email arrives: the email feature replaces this
 * file, keeping the props (navigation, "Ask Mimi about this", opening a person).
 */
export function MailView({
  onSection,
}: {
  onSection: (s: Section) => void;
  onAsk: (draft: Draft) => void;
  onOpenPerson: (id: string) => void;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 pt-[60px] text-center">
      <IconTile size="lg" className="bg-[#efe9fb] text-[#6146ad]">
        <Mail />
      </IconTile>
      <h1 className="type-title">Your email, sorted</h1>
      <p className="max-w-[400px] type-body text-muted-foreground">
        Connect your email in Settings. Mimi sorts what needs a reply from the rest, on this
        computer.
      </p>
      <Button className="mt-2" onClick={() => onSection("connections")}>
        Open Connections
      </Button>
    </div>
  );
}
