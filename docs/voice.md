# Voice

The user can talk to the assistant and hear it answer: the composer's microphone,
voice messages in the messaging apps, and replies read aloud. Speech is understood and
made on this computer, by models Mimi downloads once; nothing is sent anywhere. The
daemon side is `crates/core/src/voice/` (with `channels/voice.rs` for the messaging
apps), the UI is `features/chat/voice.tsx` (microphone), `features/chat/read-aloud.tsx`
and `lib/speech.ts` (reading aloud), `lib/recorder.ts` (recording) and
`features/voice/voice-view.tsx` (Settings › Voice). A live conversation mode (talking
back and forth hands-free) is planned for later; this is its groundwork.

## Rules

- **Local only.** Recognizers and voices run in `mimid` through sherpa-onnx. Never add a
  cloud speech service to the default path; if one ever comes, it's a choice the user
  makes, shown as cloud like any cloud model.
- **Recordings are never kept.** A recording lives in memory until it's written down;
  only its words are stored, as an ordinary message marked `spoken`. Never log what
  was said: log how long, never what.
- **The user sees what was heard before anything is done about it.** In the app the
  words go in the message box to check first (unless "Send as soon as you stop talking"
  is on); in the messaging apps Mimi says back what it heard ("🎤 “…”") before
  answering. Spoken messages carry a note to the model that a word may be misheard.
- **Spoken words are the user's words**, with the same weight and the same approval
  rule as typed ones: a voice message can't approve anything that a typed one couldn't,
  and approval prompts are still answered by reply, reaction or button.
- **Messaging apps download a recording only once the sender is known** to be the owner
  (or someone they trust, in their own chat), within 20 MB, like photos.
- **Downloads are pinned.** Every archive in `catalog.json` has its size and SHA-256;
  `download.rs` checks it before unpacking, unpacks only plain files inside the pack's
  own folder, and moves the folder into place last, so "the folder exists" means ready.
- **Copy names no models.** The Voice page says "Understands 25 European languages" and
  "Natural" / "Light", never the models' names (those are in this document).

## Models

`catalog.json` lists what can be downloaded, all from the sherpa-onnx releases on
GitHub (`.tar.bz2`, unpacked into `<data>/voice/<id>/` without the archive's own
folder):

- **Recognizers.** NVIDIA's Parakeet TDT 0.6B v3 (int8, 487 MB, CC-BY-4.0): 25 European
  languages, the language found by itself, about 20× faster than real time on a laptop
  CPU. OpenAI's Whisper large-v3 turbo (int8, 564 MB, MIT): almost every language,
  about 3× faster than real time; it hears 28 s at a time, so long recordings are cut at
  their quietest moment (`audio::pieces`). The recommended one is the first that
  understands the computer's language (`sys-locale`); the user can pick the other.
- **Voices.** Natural: Kokoro v1.0 (fp32, 350 MB, Apache-2.0) for English (US and UK),
  French, Spanish, Italian, Brazilian Portuguese and Chinese, about 4× faster than real
  time (the int8 build is slower on CPUs). Light: one Piper voice per language (60–80 MB,
  about 30× faster than real time) for 22 languages, only voices trained on data in the
  public domain, CC0 or CC-BY (check the voice's `MODEL_CARD` before adding one).
  Natural is recommended from the Standard hardware tier up; a language without a voice
  of the chosen style uses the other style.

Adding a model: put its archive in `catalog.json` with `bytes` and `sha256` (GitHub
shows asset digests; for older assets, download and hash it), and the files sherpa-onnx
needs. `catalog::tests` check that every download is pinned.

## Listening

`voice::transcribe(state, bytes)` decodes any recording (`audio::decode`: symphonia for
WAV, Ogg, WebM, MP4/AAC, MP3, FLAC; libopus through `symphonia-adapter-libopus` for
Opus), mixes it to mono, resamples it to 16 kHz with sherpa-onnx's resampler, and runs
the recognizer (`engine::Listener`) in `spawn_blocking`. Recordings over 15 minutes or
48 MB are refused. Silence comes back as no words. The recognizer is loaded on first use
(1–3 s) and kept until 10 minutes without use (`unload_if_idle`); `POST
/v1/voice/prepare` loads it while the user is still talking.

**The composer.** With nothing to send, the microphone takes Send's place (and
⌘/Ctrl ⇧ Space talks from anywhere in the chat). The first click explains the one-time
download and offers it, with progress on the button. While recording, a bar with Cancel,
a live waveform and the time replaces the text field; Return or the lime button
finishes, Escape cancels. `lib/recorder.ts` records with Web Audio (a
`ScriptProcessorNode`, which every webview Mimi runs in supports), resamples to 16 kHz
with an `OfflineAudioContext` and sends a WAV, base64, to `POST /v1/voice/transcribe`.
The words land in the message box (or are sent at once), and the message goes with
`spoken: true`. Five minutes at most.

**The desktop app's microphone.** On Linux, WebKitGTK only lets a page use the
microphone when told to: `allow_microphone` in `src-tauri/src/lib.rs` turns on media
streams and grants the page's microphone requests, and nothing else (no camera, no
location, no notifications). On macOS, wry grants WebKit's requests and macOS asks the
user once, with `NSMicrophoneUsageDescription` from `src-tauri/Info.plist`.

**Messaging apps.** Telegram (`voice` and `audio`), Signal (an audio attachment in Note
to Self: `classify::recording`) and Matrix (`m.audio`, decrypted by matrix-sdk; the
owner's chat and trusted people's) hand the recording to `channels::voice::hear`. If
the recognizer isn't downloaded yet, the first voice message starts the download, says
so once ("a one-time download of 487 MB…") and is answered when it's done (or says why
not, and where to try again). Then Mimi says back what it heard and hands the words to
`photos::deliver_spoken`, so a spoken "add these dates" takes held photos along like a
typed one.

## Speaking

`speakable::plain` turns the reply's Markdown into what one would say: no formatting,
code blocks left out, short inline code read, tables row by row, addresses as "a link",
no emoji. `speakable::parts` cuts it at sentence ends, the first part short (80
characters) so speech starts quickly, the rest up to 240; at most 6,000 characters are
read. The language is found with `whatlang` among the languages there are voices for,
else the computer's; the voice is the one the user chose for that language
(`VoiceSettings.voices`, by language without region), else one of the chosen style, in
the computer's region first (British English in the UK).

`POST /v1/voice/speak {text, part}` returns one part as a WAV (base64) and how many
parts there are; if the voice isn't downloaded yet, it starts the download and returns
`downloading` with the pack. `lib/speech.ts` plays one text at a time: it asks for the
next part while the current one plays and queues them back to back in Web Audio, so
there are no gaps on computers that make speech faster than real time. With `voice`
and no text, it reads a sample sentence in that voice's language (Settings › Voice).

**Voice messages back.** With "Answer voice messages with a voice message" on, a reply
to a voice message is also sent as one, after the written reply: the whole reply as
speech, Opus in Ogg (`audio::voice_note`, 24 kHz, 32 kb/s, with a waveform for Matrix),
through `Channel::send_voice`: Telegram's `sendVoice`, Matrix as an `m.audio` voice
message (encrypted like everything else in an encrypted room). Signal gets text only:
iPhones can't play Ogg. If the voice isn't downloaded yet, its download starts and this
reply goes without.

## API

`GET /v1/voice` (`VoiceStatus`: the recognizer in use and the choices, the style and
the recommended one, every voice, the packs on this computer), `POST /voice/transcribe`
(409 `voice_not_ready` without a recognizer), `POST /voice/prepare`, `POST
/voice/speak`, `POST /voice/packs/{id}/download|cancel`, `DELETE /voice/packs/{id}`.
`VoiceChanged` events carry the status on every download step and settings change.
Settings: `Settings.voice` (`send_when_done`, `recognizer`, `style`, `voices`, `speed`
75–150, `reply_with_voice`), checked by `voice::validate`.

## Building

`sherpa-onnx` (Apache-2.0) links its prebuilt static libraries (onnxruntime, kaldi,
espeak-ng) at build time, downloaded by its build script from the sherpa-onnx GitHub
release; espeak-ng is GPL-3.0, compatible with Mimi's AGPL. `opusic-sys` builds libopus
from source with CMake. Nothing extra ships next to `mimid`.

## Tests

`voice::tests` (voice choice, settings, status, not-ready paths), `voice::audio::tests`
(decoding, Ogg/Opus round trip, cutting long recordings), `voice::speakable::tests`,
`voice::download::tests` (unpacking, nothing outside the folder), `catalog::tests`, and
`api/tests/voice_flow.rs` (the routes, spoken messages and the model's note, Telegram
voice messages heard and answered aloud, the first one downloading the recognizer).
Flows replace the models with `Voice::fake_words`. The real models:

```sh
# a folder with the unpacked `kokoro` and `parakeet-v3` packs, e.g. <data>/voice
MIMI_TEST_VOICE=~/.local/share/mimi/voice cargo test -p mimi-core live_voice -- --ignored
```

It reads a French sentence aloud and transcribes it again, as WAV and as a voice message.
