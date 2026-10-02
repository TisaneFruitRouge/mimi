import { useEffect, useRef, useState } from "react";
import { motion } from "motion/react";
import { ArrowUp, Check, Loader2, Mic, X } from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { VoicePack } from "@/bindings/VoicePack";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { DaemonError, api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { mod } from "@/lib/platform";
import { useAssistantName, useVoice } from "@/lib/queries";
import { MAX_RECORDING_SECONDS, MicError, type Recording, startRecording } from "@/lib/recorder";

export type DictationPhase = "idle" | "starting" | "recording" | "transcribing";

/** How many loudness readings the live waveform shows. */
const BARS = 90;

/**
 * Talking instead of typing: records, has the daemon turn it into words, and hands them
 * over (`onWords`). Nothing is kept: the recording only lives until it's written down.
 */
export function useDictation(onWords: (text: string) => void) {
  const [phase, setPhase] = useState<DictationPhase>("idle");
  const [levels, setLevels] = useState<number[]>([]);
  const [seconds, setSeconds] = useState(0);
  const recording = useRef<Recording | null>(null);
  const timer = useRef<ReturnType<typeof setInterval>>(undefined);
  const finishRef = useRef<() => void>(() => {});

  const reset = () => {
    clearInterval(timer.current);
    recording.current = null;
    setLevels([]);
    setSeconds(0);
  };

  const start = async () => {
    if (phase !== "idle") return;
    setPhase("starting");
    // Loads the recognizer while the user talks, so the words come back sooner.
    api.prepareListening().catch(() => {});
    try {
      recording.current = await startRecording(
        (level) => setLevels((l) => [...l.slice(-(BARS - 1)), level]),
        () => finishRef.current(),
      );
    } catch (e) {
      reset();
      setPhase("idle");
      toast.error(e instanceof MicError ? e.message : (e as Error).message);
      return;
    }
    const began = Date.now();
    timer.current = setInterval(() => setSeconds(Math.floor((Date.now() - began) / 1000)), 250);
    setPhase("recording");
  };

  const finish = async () => {
    const rec = recording.current;
    if (!rec) return;
    recording.current = null;
    clearInterval(timer.current);
    setPhase("transcribing");
    try {
      const heard = await api.transcribe(await rec.finish());
      if (heard.text.trim()) onWords(heard.text.trim());
      else toast("I didn't catch any words. Try again a little closer to the microphone.");
    } catch (e) {
      const err = e as DaemonError;
      toast.error(err.message);
    } finally {
      reset();
      setPhase("idle");
    }
  };
  finishRef.current = () => void finish();

  const cancel = () => {
    recording.current?.cancel();
    reset();
    setPhase("idle");
  };
  const cancelRef = useRef(cancel);
  cancelRef.current = cancel;

  // While talking, Return finishes and Escape throws it away.
  useEffect(() => {
    if (phase !== "recording") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancelRef.current();
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        finishRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase]);

  // Leaving the chat mid-sentence stops the microphone.
  useEffect(
    () => () => {
      recording.current?.cancel();
      clearInterval(timer.current);
    },
    [],
  );

  return { phase, levels, seconds, start, finish, cancel };
}

/** The download that lets the assistant understand speech, while it's needed. */
function useListening(): VoicePack | null {
  return useVoice().data?.listening ?? null;
}

/**
 * The microphone in the composer's corner, where Send is while there's nothing to send.
 * The first time, it explains the one-time download and offers it.
 */
export function MicButton({ onStart, disabled }: { onStart: () => void; disabled?: boolean }) {
  const listening = useListening();
  const [open, setOpen] = useState(false);
  const ready = listening?.state === "ready";
  const downloading = listening?.state === "downloading";
  const progress =
    downloading && listening?.bytes ? Math.round(((listening.done_bytes ?? 0) / listening.bytes) * 100) : 0;

  const button = (
    <motion.button
      key="mic"
      initial={{ scale: 0.6, opacity: 0 }}
      animate={{ scale: 1, opacity: 1 }}
      exit={{ scale: 0.6, opacity: 0 }}
      transition={{ type: "spring", stiffness: 600, damping: 30 }}
      onClick={() => (ready ? onStart() : setOpen(true))}
      disabled={disabled || !listening}
      aria-label="Talk"
      className="pressable relative ml-1 flex size-8 items-center justify-center rounded-full bg-fill text-foreground hover:bg-[rgb(118_118_128/0.18)] disabled:text-[#a1a1a6]"
    >
      {downloading && <ProgressRing value={progress} />}
      <Mic className="size-[17px]" strokeWidth={2.2} />
    </motion.button>
  );

  if (ready) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>{button}</TooltipTrigger>
        <TooltipContent>Talk · {mod}⇧Space</TooltipContent>
      </Tooltip>
    );
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{button}</PopoverTrigger>
      <PopoverContent align="end" side="top" className="w-[300px] p-4">
        {listening && <ListeningSetup pack={listening} />}
      </PopoverContent>
    </Popover>
  );
}

/** Explains and runs the download that lets the assistant understand speech. */
export function ListeningSetup({ pack }: { pack: VoicePack }) {
  const assistant = useAssistantName();
  const start = () => api.downloadVoicePack(pack.id).catch((e) => toast.error((e as Error).message));
  if (pack.state === "ready") {
    return (
      <>
        <p className="type-callout font-medium">Ready</p>
        <p className="mt-1 type-subhead text-muted-foreground">
          Click the microphone and start talking. Click again when you're done.
        </p>
      </>
    );
  }
  const done = pack.done_bytes ?? 0;
  return (
    <>
      <p className="type-callout font-medium">Talk instead of typing</p>
      <p className="mt-1 type-subhead text-muted-foreground">
        To understand you, {assistant} needs a one-time download ({formatBytes(pack.bytes)}). It runs on this
        computer: what you say is never sent anywhere.
      </p>
      {pack.state === "downloading" ? (
        <div className="mt-3">
          <Progress value={pack.bytes ? (done / pack.bytes) * 100 : 0} />
          <div className="mt-2 flex items-center justify-between">
            <span className="type-footnote text-muted-foreground">
              {formatBytes(done)} of {formatBytes(pack.bytes)}
            </span>
            <Button size="sm" variant="ghost" onClick={() => api.cancelVoicePack(pack.id).catch(() => {})}>
              Stop
            </Button>
          </div>
        </div>
      ) : (
        <>
          {pack.error && <p className="mt-2 type-footnote text-destructive">{pack.error}</p>}
          <Button size="sm" variant="lime" className="mt-3" onClick={start}>
            {pack.error ? "Try again" : "Download"}
          </Button>
        </>
      )}
    </>
  );
}

function ProgressRing({ value }: { value: number }) {
  const r = 15;
  const c = 2 * Math.PI * r;
  return (
    <svg className="absolute inset-0 -rotate-90" viewBox="0 0 32 32" aria-hidden>
      <circle cx="16" cy="16" r={r} fill="none" strokeWidth="2" className="stroke-lime-deep/20" />
      <circle
        cx="16"
        cy="16"
        r={r}
        fill="none"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={c * (1 - value / 100)}
        className="stroke-lime-deep transition-[stroke-dashoffset] duration-500"
      />
    </svg>
  );
}

function clock(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/**
 * Takes the text field's place while the user talks: cancel, a live waveform, and the
 * time. Then, while it's written down, says so.
 */
export function RecordingBar({
  phase,
  levels,
  seconds,
  onCancel,
}: {
  phase: DictationPhase;
  levels: number[];
  seconds: number;
  onCancel: () => void;
}) {
  const bars = [...Array(Math.max(0, BARS - levels.length)).fill(0), ...levels] as number[];
  const nearLimit = seconds >= MAX_RECORDING_SECONDS - 30;
  return (
    <div className="flex min-h-[52px] items-center gap-3 px-3 pt-3 pb-1">
      <button
        onClick={onCancel}
        aria-label="Cancel"
        disabled={phase === "transcribing"}
        className="pressable flex size-8 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-fill hover:text-foreground disabled:opacity-40"
      >
        <X className="size-[17px]" />
      </button>
      {phase === "transcribing" ? (
        <span className="shimmer flex-1 type-body">Writing down what you said…</span>
      ) : (
        <div className="flex h-8 flex-1 items-center gap-[3px] overflow-hidden" aria-label="Recording">
          {bars.map((level, i) => (
            <span
              key={i}
              className="w-[3px] shrink-0 rounded-full bg-lime-deep/70 transition-[height] duration-100"
              style={{ height: `${Math.max(3, Math.round(level * 28))}px` }}
            />
          ))}
        </div>
      )}
      <span
        className={cn(
          "w-10 shrink-0 text-right type-subhead tabular-nums",
          nearLimit ? "text-destructive" : "text-muted-foreground",
        )}
      >
        {phase === "starting" ? "" : clock(seconds)}
      </span>
    </div>
  );
}

/** The lime button that ends the recording: "done" to check the words, or send at once. */
export function FinishButton({
  phase,
  sendsAtOnce,
  onFinish,
}: {
  phase: DictationPhase;
  sendsAtOnce: boolean;
  onFinish: () => void;
}) {
  const busy = phase === "transcribing" || phase === "starting";
  return (
    <motion.button
      key="finish"
      initial={{ scale: 0.6, opacity: 0 }}
      animate={{ scale: 1, opacity: 1 }}
      exit={{ scale: 0.6, opacity: 0 }}
      transition={{ type: "spring", stiffness: 600, damping: 30 }}
      onClick={onFinish}
      disabled={busy}
      aria-label={sendsAtOnce ? "Send" : "Done"}
      className="pressable ml-1 flex size-8 items-center justify-center rounded-full bg-lime text-lime-ink shadow-[inset_0_0_0_0.5px_rgb(0_0_0/0.1),0_1px_2px_rgb(86_118_13/0.3)] hover:brightness-[0.97] disabled:opacity-70"
    >
      {busy ? (
        <Loader2 className="size-[17px] animate-spin" />
      ) : sendsAtOnce ? (
        <ArrowUp className="size-[17px]" strokeWidth={2.6} />
      ) : (
        <Check className="size-[17px]" strokeWidth={2.6} />
      )}
    </motion.button>
  );
}
