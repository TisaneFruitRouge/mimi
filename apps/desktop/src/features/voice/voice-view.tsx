import { useState } from "react";
import { motion } from "motion/react";
import {
  AudioLines,
  Check,
  ChevronDown,
  Ear,
  Gauge,
  Loader2,
  MessageCircle,
  MoreHorizontal,
  Play,
  Square,
} from "lucide-react";
import { cn } from "cn";
import { toast } from "sonner";

import type { Settings } from "@/bindings/Settings";
import type { VoiceInfo } from "@/bindings/VoiceInfo";
import type { VoicePack } from "@/bindings/VoicePack";
import type { VoiceSettings } from "@/bindings/VoiceSettings";
import type { VoiceStyle } from "@/bindings/VoiceStyle";
import { Grouped, IconTile, Page, PageHeader, Pill, Row, Section } from "@/components/page";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { mod } from "@/lib/platform";
import { useAssistantName, useSettings, useVoice } from "@/lib/queries";
import { toggleSpeaking, useSpeech } from "@/lib/speech";

const wrap = "[&_.truncate]:whitespace-normal";

/** Settings › Voice: talking to the assistant and hearing it, all on this computer. */
export function VoiceView() {
  const settings = useSettings().data;
  const voice = useVoice().data;
  const assistant = useAssistantName();

  const save = (change: Partial<VoiceSettings>) => {
    if (!settings) return;
    const next: Settings = { ...settings, voice: { ...settings.voice, ...change } };
    api.putSettings(next).catch((e) => toast.error((e as Error).message));
  };

  return (
    <Page>
      <PageHeader
        title="Voice"
        subtitle={`Talk to ${assistant} and hear it answer. Your voice is understood on this computer and never sent anywhere.`}
      />
      {settings && voice && (
        <>
          <Section title="Understanding you">
            <Grouped>
              {voice.recognizers.map((r) => (
                <RecognizerRow
                  key={r.id}
                  pack={r}
                  chosen={r.id === voice.listening.id}
                  recommended={!settings.voice.recognizer && r.id === voice.listening.id}
                  onChoose={() => save({ recognizer: r.id })}
                />
              ))}
            </Grouped>
            <Grouped>
              <Row
                icon={
                  <IconTile size="sm" className="bg-lime-soft text-lime-deep">
                    <Ear />
                  </IconTile>
                }
                title="Send as soon as you stop talking"
                detail={
                  settings.voice.send_when_done
                    ? "What you say is sent right away."
                    : `Otherwise your words go in the message box first, so you can check them. Talk with the microphone, or ${mod}⇧Space.`
                }
                className={wrap}
                trailing={
                  <Switch
                    checked={settings.voice.send_when_done}
                    onCheckedChange={(send_when_done) => save({ send_when_done })}
                  />
                }
              />
            </Grouped>
          </Section>

          <Section title="Reading aloud">
            <Grouped>
              <Row
                icon={
                  <IconTile size="sm" className="bg-[#5e5ce6] text-white">
                    <AudioLines />
                  </IconTile>
                }
                title="Voices"
                detail={
                  voice.style === "natural"
                    ? "Natural: lifelike, best on a recent computer."
                    : "Light: quick on any computer, a little less lifelike."
                }
                className={wrap}
                trailing={
                  <StyleChoice
                    value={voice.style}
                    recommended={voice.recommended_style}
                    onChange={(style) => save({ style })}
                  />
                }
              />
              <Row
                icon={
                  <IconTile size="sm" className="bg-fill text-muted-foreground">
                    <Gauge />
                  </IconTile>
                }
                title="Speed"
                detail={settings.voice.speed === 100 ? "Normal" : `${(settings.voice.speed / 100).toFixed(2)}×`}
                className="min-h-[52px]"
                trailing={
                  <input
                    type="range"
                    min={75}
                    max={150}
                    step={5}
                    value={settings.voice.speed}
                    onChange={(e) => save({ speed: Number(e.target.value) })}
                    aria-label="Reading speed"
                    className="w-40 accent-[var(--lime-deep)]"
                  />
                }
              />
            </Grouped>
            <Languages
              voices={voice.voices}
              packs={voice.packs}
              style={voice.style}
              chosen={settings.voice.voices}
              onChoose={(language, id) => save({ voices: { ...settings.voice.voices, [language]: id } })}
            />
            <p className="px-1 type-footnote text-muted-foreground">
              Each reply is read in its own language. A voice downloads the first time it's needed.
            </p>
          </Section>

          <Section title="Messaging apps">
            <Grouped>
              <Row
                icon={
                  <IconTile size="sm" className="bg-network-soft text-network">
                    <MessageCircle />
                  </IconTile>
                }
                title="Answer voice messages with a voice message"
                detail="In Telegram and Matrix, after the written reply. Signal can't play them, so it gets the text only."
                className={wrap}
                trailing={
                  <Switch
                    checked={settings.voice.reply_with_voice}
                    onCheckedChange={(reply_with_voice) => save({ reply_with_voice })}
                  />
                }
              />
            </Grouped>
          </Section>

          <OnThisComputer packs={[...voice.recognizers, ...voice.packs]} />
        </>
      )}
    </Page>
  );
}

function RecognizerRow({
  pack,
  chosen,
  recommended,
  onChoose,
}: {
  pack: VoicePack;
  chosen: boolean;
  recommended: boolean;
  onChoose: () => void;
}) {
  const download = () => api.downloadVoicePack(pack.id).catch((e) => toast.error((e as Error).message));
  return (
    <div>
      <Row
        onClick={onChoose}
        title={
          <span className="flex items-center gap-2">
            {pack.label}
            {recommended && <Pill>Suits your language</Pill>}
          </span>
        }
        detail={pack.error ?? pack.detail}
        className={cn(wrap, pack.error && "[&_.text-muted-foreground]:text-destructive")}
        trailing={chosen && <Check className="size-4 text-lime-deep" strokeWidth={2.6} />}
        accessory={
          pack.state === "missing" ? (
            <Button size="sm" variant={chosen ? "lime" : "secondary"} onClick={download}>
              Download · {formatBytes(pack.bytes)}
            </Button>
          ) : pack.state === "downloading" ? (
            <Button size="sm" variant="ghost" onClick={() => api.cancelVoicePack(pack.id).catch(() => {})}>
              Stop
            </Button>
          ) : undefined
        }
      />
      {pack.state === "downloading" && <DownloadProgress pack={pack} />}
    </div>
  );
}

function DownloadProgress({ pack }: { pack: VoicePack }) {
  const done = pack.done_bytes ?? 0;
  return (
    <div className="flex items-center gap-3 px-4 pb-3">
      <Progress value={pack.bytes ? (done / pack.bytes) * 100 : 0} className="flex-1" />
      <span className="type-footnote text-muted-foreground tabular-nums">
        {formatBytes(done)} of {formatBytes(pack.bytes)}
      </span>
    </div>
  );
}

function StyleChoice({
  value,
  recommended,
  onChange,
}: {
  value: VoiceStyle;
  recommended: VoiceStyle;
  onChange: (style: VoiceStyle) => void;
}) {
  const choices: { value: VoiceStyle; label: string }[] = [
    { value: "natural", label: "Natural" },
    { value: "light", label: "Light" },
  ];
  return (
    <div role="radiogroup" aria-label="Voices" className="flex shrink-0 rounded-[9px] bg-fill p-[2px]">
      {choices.map((c) => {
        const active = c.value === value;
        return (
          <button
            key={c.value}
            role="radio"
            aria-checked={active}
            title={c.value === recommended ? "Suits this computer" : undefined}
            onClick={() => onChange(c.value)}
            className={cn(
              "relative h-[26px] rounded-[7px] px-3 text-[13px] font-medium transition-colors duration-200",
              active ? "text-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {active && (
              <motion.span
                layoutId="voice-style"
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

/** "English (UK)" → "English". */
const languageOf = (v: VoiceInfo) => v.language_name.replace(/\s*\(.*\)$/, "");
const baseOf = (v: VoiceInfo) => v.language.split("-")[0];

/** Every language with its voices: the one used, a choice where there's one, and a sample. */
function Languages({
  voices,
  packs,
  style,
  chosen,
  onChoose,
}: {
  voices: VoiceInfo[];
  packs: VoicePack[];
  style: VoiceStyle;
  chosen: Record<string, string | undefined>;
  onChoose: (language: string, id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const languages = new Map<string, VoiceInfo[]>();
  for (const v of voices) languages.set(baseOf(v), [...(languages.get(baseOf(v)) ?? []), v]);
  const ready = new Set(packs.filter((p) => p.state === "ready").map((p) => p.id));
  const rows = [...languages.entries()].sort(([, a], [, b]) => languageOf(a[0]).localeCompare(languageOf(b[0])));

  return (
    <Grouped>
        <Row
          onClick={() => setOpen(!open)}
          title="Voice for each language"
          detail={`${rows.length} languages`}
          className="min-h-[52px]"
          trailing={<ChevronDown className={cn("size-4 text-faint transition-transform", open && "rotate-180")} />}
        />
        {open &&
          rows.map(([language, options]) => {
            const current =
              options.find((o) => o.id === chosen[language]) ??
              options.find((o) => o.style === style) ??
              options[0];
            return (
              <Row
                key={language}
                title={languageOf(options[0])}
                detail={ready.has(current.pack) ? "On this computer" : "Downloads when first needed"}
                className="min-h-[52px]"
                trailing={
                  <>
                    <SampleButton voice={current} />
                    {options.length > 1 ? (
                      <Select value={current.id} onValueChange={(id) => onChoose(language, id)}>
                        <SelectTrigger size="sm" className="w-[190px]">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {options.map((o) => (
                            <SelectItem key={o.id} value={o.id}>
                              {o.name} · {o.style === "natural" ? "natural" : "light"}
                              {o.language_name !== languageOf(o) &&
                                `, ${o.language_name.replace(/^.*\((.*)\)$/, "$1")}`}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <span className="w-[190px] type-subhead text-muted-foreground">
                        {current.name} · {current.style === "natural" ? "natural" : "light"}
                      </span>
                    )}
                  </>
                }
              />
            );
          })}
    </Grouped>
  );
}

/** Plays a short sentence in a voice (downloading it first if needed). */
function SampleButton({ voice }: { voice: VoiceInfo }) {
  const id = `sample:${voice.id}`;
  const speech = useSpeech(id);
  return (
    <button
      onClick={() => toggleSpeaking(id, "", voice.id)}
      aria-label={speech ? "Stop" : `Hear ${voice.name}`}
      title={speech?.phase === "downloading" ? "Getting this voice ready… Click to stop" : undefined}
      className="pressable flex size-8 items-center justify-center rounded-full text-muted-foreground hover:bg-fill hover:text-foreground"
    >
      {!speech ? (
        <Play className="size-3.5 fill-current" />
      ) : speech.phase === "playing" ? (
        <Square className="size-3 fill-current" />
      ) : (
        <Loader2 className="size-3.5 animate-spin" />
      )}
    </button>
  );
}

/** What's downloaded (or downloading), how big, and a way to remove it. */
function OnThisComputer({ packs }: { packs: VoicePack[] }) {
  const here = packs.filter((p) => p.state !== "missing");
  if (!here.length) return null;
  const remove = (p: VoicePack) => api.removeVoicePack(p.id).catch((e) => toast.error((e as Error).message));
  return (
    <Section title="On this computer">
      <Grouped>
        {here.map((p) => (
          <div key={p.id}>
            <Row
              title={p.label}
              detail={p.state === "ready" ? formatBytes(p.bytes) : "Downloading…"}
              className="min-h-[52px]"
              accessory={
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button size="icon" variant="ghost" aria-label="More">
                      <MoreHorizontal />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    {p.state === "downloading" && (
                      <DropdownMenuItem onSelect={() => api.cancelVoicePack(p.id).catch(() => {})}>
                        Stop downloading
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem variant="destructive" onSelect={() => remove(p)}>
                      Remove from this computer
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              }
            />
            {p.state === "downloading" && <DownloadProgress pack={p} />}
          </div>
        ))}
      </Grouped>
    </Section>
  );
}
