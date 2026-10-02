/**
 * Reading aloud: one reply at a time, part by part. The daemon makes each part's speech
 * on this computer (`POST /voice/speak`); the next part is made while the current one
 * plays, and they're queued back to back in Web Audio, so a long reply starts after its
 * first sentence and plays without gaps.
 */
import { useSyncExternalStore } from "react";
import { toast } from "sonner";

import { api } from "@/lib/api";

export type SpeechState =
  | { id: string; phase: "preparing" }
  | { id: string; phase: "downloading"; pack: string }
  | { id: string; phase: "playing" };

let current: SpeechState | null = null;
let ctx: AudioContext | null = null;
/** Bumped by every play and stop, so a stale run stops at its next step. */
let run = 0;
const listeners = new Set<() => void>();

function set(state: SpeechState | null) {
  current = state;
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** What's being read now, for `id` (a message, or a voice in Settings). */
export function useSpeech(id: string): SpeechState | null {
  const state = useSyncExternalStore(subscribe, () => current);
  return state?.id === id ? state : null;
}

export function stopSpeaking() {
  run++;
  void ctx?.close();
  ctx = null;
  set(null);
}

/** Starts or stops reading `text` aloud. `voice` reads it with that voice instead. */
export function toggleSpeaking(id: string, text: string, voice?: string) {
  if (current?.id === id) {
    stopSpeaking();
    return;
  }
  void speak(id, text, voice);
}

function decode(base64: string): ArrayBuffer {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Waits for a voice download; false if it stopped or failed. */
async function untilReady(pack: string, mine: number): Promise<boolean> {
  for (;;) {
    await sleep(1500);
    if (mine !== run) return false;
    const status = await api.voice().catch(() => null);
    const found = status?.packs.find((p) => p.id === pack);
    if (found?.state === "ready") return true;
    if (!found || found.state === "missing") {
      if (found?.error) toast.error(found.error);
      return false;
    }
  }
}

async function speak(id: string, text: string, voice?: string) {
  stopSpeaking();
  const mine = ++run;
  // Made in the click, so the browser lets it play.
  const audio = new AudioContext();
  ctx = audio;
  set({ id, phase: "preparing" });
  const fetchPart = (part: number) => api.speak(text, part, voice);
  try {
    let next = 0;
    let parts = 1;
    let part = 0;
    let pending = fetchPart(0);
    while (part < parts) {
      const speech = await pending;
      if (mine !== run) return;
      if (speech.kind === "nothing") break;
      if (speech.kind === "downloading") {
        set({ id, phase: "downloading", pack: speech.pack.id });
        if (!(await untilReady(speech.pack.id, mine))) {
          if (mine === run) stopSpeaking();
          return;
        }
        set({ id, phase: "preparing" });
        pending = fetchPart(part);
        continue;
      }
      parts = speech.parts;
      const buffer = await audio.decodeAudioData(decode(speech.audio));
      if (mine !== run) return;
      const source = audio.createBufferSource();
      source.buffer = buffer;
      source.connect(audio.destination);
      const at = Math.max(next, audio.currentTime + 0.05);
      source.start(at);
      next = at + buffer.duration;
      if (current?.phase !== "playing") set({ id, phase: "playing" });
      part++;
      // Made while this part plays.
      if (part < parts) pending = fetchPart(part);
    }
    await sleep(Math.max(0, (next - audio.currentTime) * 1000));
    if (mine === run) stopSpeaking();
  } catch (e) {
    if (mine !== run) return;
    stopSpeaking();
    toast.error((e as Error).message);
  }
}
