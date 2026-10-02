import { Loader2, Square, Volume2 } from "lucide-react";
import { cn } from "cn";

import { useVoice } from "@/lib/queries";
import { toggleSpeaking, useSpeech } from "@/lib/speech";

/**
 * Reads a reply aloud, on this computer. The first time, its voice downloads first: the
 * button shows how far along that is.
 */
export function ReadAloudButton({ id, text, className }: { id: string; text: string; className?: string }) {
  const speech = useSpeech(id);
  const packs = useVoice().data?.packs;
  const pack = speech?.phase === "downloading" ? packs?.find((p) => p.id === speech.pack) : undefined;
  const label = !speech
    ? "Read aloud"
    : speech.phase === "downloading"
      ? `Getting a voice ready${pack?.bytes ? ` (${Math.round(((pack.done_bytes ?? 0) / pack.bytes) * 100)}%)` : ""}… Click to stop`
      : "Stop reading";
  return (
    <button
      onClick={() => toggleSpeaking(id, text)}
      aria-label={label}
      title={label}
      className={cn(
        "flex size-7 items-center justify-center rounded-full transition-opacity hover:bg-fill hover:text-foreground focus-visible:opacity-100",
        speech ? "text-lime-deep opacity-100" : "opacity-0 group-hover:opacity-100",
        className,
      )}
    >
      {!speech ? (
        <Volume2 className="size-3.5" />
      ) : speech.phase === "playing" ? (
        <Square className="size-3 fill-current" />
      ) : (
        <Loader2 className="size-3.5 animate-spin" />
      )}
    </button>
  );
}
