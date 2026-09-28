import { cn } from "cn";

import { AssistantGlyph } from "@/components/assistant-avatar";

/** The app's mark: the character on a small warm-white tile, like the app icon. */
export function LogoMark({ className }: { className?: string }) {
  return (
    <div
      className={cn(
        "flex size-7 shrink-0 items-center justify-center rounded-[8px] bg-[linear-gradient(#fffefa,#f1efe6)] shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(0_0_0/0.08)]",
        className,
      )}
    >
      <AssistantGlyph className="size-[80%]" />
    </div>
  );
}
