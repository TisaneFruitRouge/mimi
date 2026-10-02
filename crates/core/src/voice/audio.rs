//! Sound in and out: voice notes and recordings decoded to the 16 kHz mono samples the
//! recognizers take, and speech saved as WAV (for the app) or Ogg/Opus (voice messages).
//!
//! Decoding is symphonia's (pure Rust) plus libopus for Opus, which symphonia lacks:
//! Telegram and Element record Ogg/Opus, Signal and iPhones AAC, browsers WebM/Opus or
//! MP4/AAC, and Mimi's own composer sends WAV.

use std::io::Cursor;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

/// What the recognizers listen at.
pub const RECOGNIZER_RATE: u32 = 16_000;
/// Longest recording Mimi listens to.
pub const MAX_SECONDS: u32 = 15 * 60;
/// Largest recording accepted, encoded (a quarter of an hour of WAV at 48 kHz is ~90 MB;
/// voice notes are far smaller).
pub const MAX_BYTES: usize = 48 * 1024 * 1024;

/// Mono samples at a sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Sound {
    pub fn duration_ms(&self) -> u32 {
        if self.rate == 0 {
            return 0;
        }
        (self.samples.len() as u64 * 1000 / u64::from(self.rate)) as u32
    }

    /// The same sound at another rate.
    pub fn resampled(self, rate: u32) -> Sound {
        if self.rate == rate || self.samples.is_empty() {
            return Sound { rate, ..self };
        }
        let Some(resampler) = sherpa_onnx::LinearResampler::create(self.rate as i32, rate as i32)
        else {
            return self;
        };
        Sound {
            rate,
            samples: resampler.resample(&self.samples, true),
        }
    }
}

/// Decodes a recording in any of the formats apps send, mixed down to mono. Refuses
/// what isn't sound, and stops at [`MAX_SECONDS`].
pub fn decode(bytes: Vec<u8>) -> Result<Sound, String> {
    const UNREADABLE: &str = "That recording couldn't be read.";
    if bytes.len() > MAX_BYTES {
        return Err("That recording is too long.".to_owned());
    }
    let mut codecs = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut codecs);
    codecs.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();

    let source = MediaSourceStream::new(
        Box::new(Cursor::new(bytes)),
        MediaSourceStreamOptions::default(),
    );
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            source,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| UNREADABLE.to_owned())?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| UNREADABLE.to_owned())?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| UNREADABLE.to_owned())?
        .clone();
    let mut decoder = codecs
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|_| UNREADABLE.to_owned())?;

    let mut rate = params.sample_rate.unwrap_or(0);
    let mut samples = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // A truncated file: keep what was read.
            Err(SymphoniaError::IoError(_)) if !samples.is_empty() => break,
            Err(_) => return Err(UNREADABLE.to_owned()),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            // A damaged packet: skip it, like players do.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => return Err(UNREADABLE.to_owned()),
        };
        let spec = audio.spec();
        rate = spec.rate();
        let channels = spec.channels().count().max(1);
        interleaved.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut interleaved);
        samples.extend(
            interleaved
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        if rate > 0 && samples.len() as u64 > u64::from(rate) * u64::from(MAX_SECONDS) {
            return Err(format!(
                "That recording is too long: Mimi listens to at most {} minutes.",
                MAX_SECONDS / 60
            ));
        }
    }
    if rate == 0 {
        return Err(UNREADABLE.to_owned());
    }
    Ok(Sound { rate, samples })
}

/// Splits a long recording into pieces of at most `max_seconds`, each cut at the
/// quietest moment of its last few seconds, so no word is cut in two.
pub fn pieces(sound: &Sound, max_seconds: u32) -> Vec<&[f32]> {
    let rate = sound.rate as usize;
    let max = rate * max_seconds as usize;
    let window = rate / 10; // 100 ms
    let look_back = (rate * 5).min(max / 2);
    let mut out = Vec::new();
    let mut rest = &sound.samples[..];
    while rest.len() > max {
        // The quietest 100 ms in the last seconds before the limit.
        let from = max - look_back;
        let mut cut = max;
        let mut quietest = f32::MAX;
        let mut at = from;
        while at + window <= max {
            let energy: f32 = rest[at..at + window].iter().map(|s| s * s).sum();
            if energy < quietest {
                quietest = energy;
                cut = at + window / 2;
            }
            at += window / 2;
        }
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// Whether anything louder than room noise was recorded.
pub fn has_sound(sound: &Sound) -> bool {
    sound.samples.iter().any(|s| s.abs() > 0.01)
}

/// A 16-bit mono WAV file.
pub fn wav(sound: &Sound) -> Vec<u8> {
    let data_len = (sound.samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sound.rate.to_le_bytes());
    out.extend_from_slice(&(sound.rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &sound.samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

/// What voice messages are made of: Opus in an Ogg file (RFC 7845), at 24 kHz.
pub struct VoiceNote {
    pub ogg: Vec<u8>,
    pub duration_ms: u32,
    /// Loudness over time (0–1023, about one value per 100 ms, at most 100), for the
    /// waveform Matrix apps draw.
    pub waveform: Vec<u16>,
}

/// Encodes speech as a voice message.
pub fn voice_note(sound: Sound) -> Result<VoiceNote, String> {
    use ogg::writing::{PacketWriteEndInfo, PacketWriter};
    use opus::{Application, Bitrate, Channels, Encoder};

    const RATE: u32 = 24_000;
    const FRAME: usize = RATE as usize / 50; // 20 ms
    const SERIAL: u32 = 0x4d494d49;
    let sound = sound.resampled(RATE);
    let duration_ms = sound.duration_ms();
    let waveform = waveform(&sound);

    let fail = |e: opus::Error| format!("Couldn't record the voice message: {e}");
    let mut encoder = Encoder::new(RATE, Channels::Mono, Application::Voip).map_err(fail)?;
    encoder.set_bitrate(Bitrate::Bits(32_000)).map_err(fail)?;
    // Granule positions count 48 kHz samples, whatever the input rate.
    let pre_skip = encoder.get_lookahead().map_err(fail)? as u16 * (48_000 / RATE) as u16;

    let mut writer = PacketWriter::new(Vec::new());
    let io = |e: std::io::Error| e.to_string();
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // version
    head.push(1); // channels
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&RATE.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes()); // gain
    head.push(0); // mapping family
    writer
        .write_packet(head, SERIAL, PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;
    let vendor = b"Mimi";
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor);
    tags.extend_from_slice(&0u32.to_le_bytes());
    writer
        .write_packet(tags, SERIAL, PacketWriteEndInfo::EndPage, 0)
        .map_err(io)?;

    // The encoder's delay is made up for with silence at the end.
    let mut samples = sound.samples;
    samples.extend(std::iter::repeat_n(0.0, FRAME + pre_skip as usize));
    let frames: Vec<&[f32]> = samples.chunks(FRAME).collect();
    let mut packet = vec![0u8; 4000];
    let mut granule: u64 = 0;
    for (i, frame) in frames.iter().enumerate() {
        let mut input = frame.to_vec();
        input.resize(FRAME, 0.0);
        let len = encoder.encode_float(&input, &mut packet).map_err(fail)?;
        granule += 960;
        let last = i + 1 == frames.len();
        let end = if last {
            PacketWriteEndInfo::EndStream
        } else if (i + 1) % 50 == 0 {
            // A page a second, like other encoders.
            PacketWriteEndInfo::EndPage
        } else {
            PacketWriteEndInfo::NormalPacket
        };
        let granule = if last {
            // The real end, so players don't play the padding.
            u64::from(pre_skip) + u64::from(duration_ms) * 48
        } else {
            granule
        };
        writer
            .write_packet(packet[..len].to_vec(), SERIAL, end, granule)
            .map_err(io)?;
    }
    Ok(VoiceNote {
        ogg: writer.into_inner(),
        duration_ms,
        waveform,
    })
}

fn waveform(sound: &Sound) -> Vec<u16> {
    let step = (sound.samples.len() / 100)
        .max(sound.rate as usize / 10)
        .max(1);
    let levels: Vec<f32> = sound
        .samples
        .chunks(step)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let loudest = levels.iter().copied().fold(0.0_f32, f32::max).max(1e-6);
    levels
        .iter()
        .map(|l| ((l / loudest) * 1023.0).round() as u16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second of a 440 Hz tone, then a second of silence.
    fn tone(rate: u32) -> Sound {
        let n = rate as usize;
        let mut samples: Vec<f32> = (0..n)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect();
        samples.extend(std::iter::repeat_n(0.0, n));
        Sound { rate, samples }
    }

    #[test]
    fn wav_files_decode_to_what_was_saved() {
        let sound = tone(16_000);
        let back = decode(wav(&sound)).unwrap();
        assert_eq!(back.rate, 16_000);
        assert_eq!(back.samples.len(), sound.samples.len());
        assert!((back.samples[100] - sound.samples[100]).abs() < 0.001);
        assert_eq!(back.duration_ms(), 2000);
    }

    #[test]
    fn voice_notes_are_ogg_opus_that_decodes_again() {
        let note = voice_note(tone(22_050)).unwrap();
        assert_eq!(&note.ogg[..4], b"OggS");
        assert_eq!(note.duration_ms, 2000);
        assert!(!note.waveform.is_empty() && note.waveform.len() <= 101);
        // Loud first, quiet after.
        assert!(note.waveform[0] > 900 && *note.waveform.last().unwrap() < 10);

        let back = decode(note.ogg).unwrap();
        let seconds = back.samples.len() as f32 / back.rate as f32;
        assert!((seconds - 2.0).abs() < 0.1, "{seconds}");
        assert!(has_sound(&back));
    }

    #[test]
    fn resampling_keeps_the_length_in_time() {
        let sound = tone(48_000).resampled(RECOGNIZER_RATE);
        assert_eq!(sound.rate, RECOGNIZER_RATE);
        let expected = 2 * RECOGNIZER_RATE as i64;
        assert!((sound.samples.len() as i64 - expected).abs() < 200);
    }

    #[test]
    fn what_isnt_sound_is_refused() {
        assert!(decode(b"definitely not audio".to_vec()).is_err());
        assert!(decode(Vec::new()).is_err());
        let silence = Sound {
            rate: 16_000,
            samples: vec![0.0; 16_000],
        };
        assert!(!has_sound(&silence));
    }

    #[test]
    fn long_recordings_are_cut_where_its_quiet() {
        // Tone and silence, one second each, for a minute.
        let mut samples = Vec::new();
        for _ in 0..30 {
            samples.extend(tone(1_000).samples);
        }
        let sound = Sound {
            rate: 1_000,
            samples,
        };
        let pieces = pieces(&sound, 25);
        assert_eq!(pieces.iter().map(|p| p.len()).sum::<usize>(), 60_000);
        assert!(pieces.iter().all(|p| p.len() <= 25_000));
        // Each cut is in a silent second.
        let mut at = 0;
        for p in &pieces[..pieces.len() - 1] {
            at += p.len();
            assert!((at % 2_000) >= 1_000, "cut at {at}");
        }
    }
}
