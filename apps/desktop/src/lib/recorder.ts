/**
 * The microphone, for talking to the assistant. Records with Web Audio (the same in the
 * desktop app and a browser), and hands back a 16 kHz mono WAV: what the speech
 * recognizer listens at, and small enough to send (a minute is about 2 MB).
 */
import { isTauri } from "@/lib/transport";
import { isMac } from "@/lib/platform";

/** Longest recording, in seconds; it stops by itself after that. */
export const MAX_RECORDING_SECONDS = 5 * 60;
const RATE = 16_000;

export type MicProblem = "denied" | "missing" | "unavailable" | "other";

export class MicError extends Error {
  constructor(
    readonly problem: MicProblem,
    message: string,
  ) {
    super(message);
  }
}

function micError(e: unknown): MicError {
  const name = (e as DOMException)?.name;
  if (name === "NotAllowedError" || name === "SecurityError") {
    return new MicError(
      "denied",
      isTauri
        ? isMac
          ? "Mimi isn't allowed to use the microphone. Allow it in System Settings › Privacy & Security › Microphone."
          : "Mimi isn't allowed to use the microphone."
        : "This page isn't allowed to use the microphone. Allow it in your browser's site settings.",
    );
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return new MicError("missing", "No microphone was found. Check that one is connected and turned on.");
  }
  if (name === "NotReadableError") {
    return new MicError("other", "The microphone is busy or not working. Try again, or close other apps using it.");
  }
  return new MicError("other", "The microphone couldn't be started.");
}

export interface Recording {
  /** Stops and returns what was recorded: a WAV file, base64-encoded. */
  finish(): Promise<string>;
  /** Stops and throws it away. */
  cancel(): void;
}

/**
 * Starts recording. `onLevel` gets the loudness (0–1) about ten times a second, for a
 * live waveform; `onLimit` fires when the recording reaches its longest.
 */
export async function startRecording(onLevel: (level: number) => void, onLimit: () => void): Promise<Recording> {
  const devices = navigator.mediaDevices;
  if (!devices?.getUserMedia) {
    throw new MicError("unavailable", "The microphone isn't available here.");
  }
  let stream: MediaStream;
  try {
    stream = await devices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true, channelCount: 1 },
    });
  } catch (e) {
    throw micError(e);
  }
  const ctx = new AudioContext();
  if (ctx.state === "suspended") await ctx.resume().catch(() => {});
  const source = ctx.createMediaStreamSource(stream);
  // ScriptProcessor is old but works in every webview Mimi runs in, unlike worklets.
  const node = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  let length = 0;
  let stopped = false;
  const limit = MAX_RECORDING_SECONDS * ctx.sampleRate;
  node.onaudioprocess = (e) => {
    if (stopped) return;
    const data = new Float32Array(e.inputBuffer.getChannelData(0));
    chunks.push(data);
    length += data.length;
    let sum = 0;
    for (const s of data) sum += s * s;
    onLevel(Math.min(1, Math.sqrt(sum / data.length) * 5));
    if (length >= limit) onLimit();
  };
  source.connect(node);
  // A processor only runs when connected onwards; it writes silence.
  node.connect(ctx.destination);

  const stop = () => {
    if (stopped) return;
    stopped = true;
    node.onaudioprocess = null;
    node.disconnect();
    source.disconnect();
    stream.getTracks().forEach((t) => t.stop());
  };
  const rate = ctx.sampleRate;
  return {
    async finish() {
      stop();
      const samples = new Float32Array(length);
      let at = 0;
      for (const c of chunks) {
        samples.set(c, at);
        at += c.length;
      }
      void ctx.close();
      return toBase64(wav(await resample(samples, rate), RATE));
    },
    cancel() {
      stop();
      void ctx.close();
    },
  };
}

/** The recording at 16 kHz, through the browser's own (filtered) resampler. */
async function resample(samples: Float32Array, rate: number): Promise<Float32Array> {
  if (rate === RATE || samples.length === 0) return samples;
  const frames = Math.max(1, Math.ceil((samples.length * RATE) / rate));
  const offline = new OfflineAudioContext(1, frames, RATE);
  const buffer = offline.createBuffer(1, samples.length, rate);
  buffer.copyToChannel(samples as Float32Array<ArrayBuffer>, 0);
  const node = offline.createBufferSource();
  node.buffer = buffer;
  node.connect(offline.destination);
  node.start();
  return (await offline.startRendering()).getChannelData(0);
}

function wav(samples: Float32Array, rate: number): Uint8Array {
  const out = new DataView(new ArrayBuffer(44 + samples.length * 2));
  const text = (at: number, s: string) => [...s].forEach((c, i) => out.setUint8(at + i, c.charCodeAt(0)));
  text(0, "RIFF");
  out.setUint32(4, 36 + samples.length * 2, true);
  text(8, "WAVEfmt ");
  out.setUint32(16, 16, true);
  out.setUint16(20, 1, true); // PCM
  out.setUint16(22, 1, true); // mono
  out.setUint32(24, rate, true);
  out.setUint32(28, rate * 2, true);
  out.setUint16(32, 2, true);
  out.setUint16(34, 16, true);
  text(36, "data");
  out.setUint32(40, samples.length * 2, true);
  samples.forEach((s, i) => out.setInt16(44 + i * 2, Math.max(-1, Math.min(1, s)) * 0x7fff, true));
  return new Uint8Array(out.buffer);
}

export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
