//! Files the user sends with a message: only pictures for now (videos may follow; voice
//! is transcribed instead, see `crate::voice`).
//!
//! Every picture is normalised on arrival, whichever way it came (the app, a browser,
//! Telegram, Signal, Matrix): decoded (refusing what isn't a picture), turned upright
//! from its EXIF orientation, shrunk to at most [`MAX_SIDE`] pixels on its long side and
//! saved again as JPEG or PNG. Saving it again drops every piece of metadata the file
//! carried (location, camera, time taken). The result goes into the encrypted database
//! with the message, never into a loose file, and goes when the message does.

use std::collections::HashMap;
use std::io::Cursor;

use base64::Engine;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use mimi_protocol::{Attachment, AttachmentKind, NewAttachment};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::{Db, DbError, enum_str, parse_enum, parse_uuid};

/// Most photos with one message.
pub const MAX_PER_MESSAGE: usize = 10;
/// Largest file accepted, before it's shrunk.
pub const MAX_UPLOAD_BYTES: usize = 20 * 1024 * 1024;
/// Largest request body for sending a message: every photo is base64 (4/3 larger), and
/// the app shrinks big photos before sending them.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
/// Long side of a picture as kept, in pixels: enough to read a ticket or a screenshot,
/// and what cloud models work at anyway.
pub const MAX_SIDE: u32 = 1568;
/// A picture kept as PNG (screenshots stay sharp) that would be larger than this is
/// kept as JPEG instead.
const MAX_PNG_BYTES: usize = 3 * 1024 * 1024;
/// Largest picture decoded, in pixels per side and in memory: 100 megapixels.
const MAX_DECODE_SIDE: u32 = 20_000;
const MAX_DECODE_ALLOC: u64 = 400 * 1024 * 1024;

/// A file as it arrived, before it's checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    pub data: Vec<u8>,
    /// The name it came with, if any.
    pub name: Option<String>,
    /// The content type the sender claims. Only a hint.
    pub mime: Option<String>,
}

impl Upload {
    pub fn new(data: Vec<u8>, name: Option<String>, mime: Option<String>) -> Self {
        Self { data, name, mime }
    }

    /// An upload from the API: base64 content.
    pub fn from_api(new: NewAttachment) -> Result<Self, String> {
        let raw = new.data.trim();
        // A data URL is fine too.
        let raw = raw
            .strip_prefix("data:")
            .and_then(|r| r.split_once(";base64,").map(|(_, d)| d))
            .unwrap_or(raw);
        if raw.len() / 4 * 3 > MAX_UPLOAD_BYTES + 3 {
            return Err(too_large(new.name.as_deref()));
        }
        let engine = base64::engine::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        );
        let data = engine
            .decode(raw)
            .map_err(|_| "A photo didn't arrive whole. Try attaching it again.".to_owned())?;
        Ok(Self {
            data,
            name: new.name,
            mime: new.mime,
        })
    }
}

/// A picture ready to keep: what clients see, and its content.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub meta: Attachment,
    pub data: Vec<u8>,
}

fn quoted(name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("“{}”", n.chars().take(60).collect::<String>()),
        None => "That photo".to_owned(),
    }
}

fn too_large(name: Option<&str>) -> String {
    format!(
        "{} is too large. Photos can be up to {} MB.",
        quoted(name),
        MAX_UPLOAD_BYTES / (1024 * 1024)
    )
}

/// iPhones save photos as HEIC, which can't be read here yet.
fn is_heic(data: &[u8]) -> bool {
    data.len() > 12
        && &data[4..8] == b"ftyp"
        && matches!(
            &data[8..12],
            b"heic" | b"heix" | b"mif1" | b"msf1" | b"heim" | b"heis" | b"avif"
        )
}

/// Checks, turns upright, shrinks and saves a picture again without its metadata.
/// `index` numbers pictures without a name ("Photo 2").
pub fn normalize(upload: &Upload, index: usize) -> Result<Prepared, String> {
    let name = upload.name.as_deref();
    if upload.data.len() > MAX_UPLOAD_BYTES {
        return Err(too_large(name));
    }
    let unreadable = || {
        format!(
            "{} isn't a picture that can be read here. Send a JPEG, PNG, GIF or WebP picture.",
            quoted(name)
        )
    };
    if is_heic(&upload.data) {
        return Err(format!(
            "{} is a HEIC photo, which can't be read here yet. Share it as a JPEG, or send a screenshot of it.",
            quoted(name)
        ));
    }
    let mut reader = ImageReader::new(Cursor::new(upload.data.as_slice()))
        .with_guessed_format()
        .map_err(|_| unreadable())?;
    let format = reader.format().ok_or_else(unreadable)?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP
    ) {
        return Err(unreadable());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| decode_error(&e, name))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|e| decode_error(&e, name))?;
    picture.apply_orientation(orientation);
    if picture.width() == 0 || picture.height() == 0 {
        return Err(unreadable());
    }
    if picture.width().max(picture.height()) > MAX_SIDE {
        // Keeps the proportions, fitting inside the square.
        picture = picture.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Lanczos3);
    }

    // Screenshots and drawings stay PNG (sharp text); photos become JPEG.
    let transparent = picture.color().has_alpha();
    let keep_png = format == ImageFormat::Png || transparent && format != ImageFormat::Jpeg;
    let (mime, data) = match keep_png.then(|| encode_png(&picture)).flatten() {
        Some(png) if png.len() <= MAX_PNG_BYTES => ("image/png", png),
        _ => ("image/jpeg", encode_jpeg(&picture).ok_or_else(unreadable)?),
    };
    let meta = Attachment {
        id: Uuid::now_v7(),
        kind: AttachmentKind::Image,
        mime: mime.to_owned(),
        name: file_name(name, index, if mime == "image/png" { "png" } else { "jpg" }),
        size: data.len() as u64,
        width: Some(picture.width()),
        height: Some(picture.height()),
    };
    Ok(Prepared { meta, data })
}

fn decode_error(e: &image::ImageError, name: Option<&str>) -> String {
    match e {
        image::ImageError::Limits(_) => format!("{} is too large to read.", quoted(name)),
        _ => format!(
            "{} couldn't be read: it may be damaged, or not a picture.",
            quoted(name)
        ),
    }
}

fn encode_png(picture: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut out);
    // 8 bits per channel: 16-bit pictures are rare and twice the size.
    let picture = if picture.color().has_alpha() {
        DynamicImage::ImageRgba8(picture.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(picture.to_rgb8())
    };
    picture.write_with_encoder(encoder).ok()?;
    Some(out)
}

/// JPEG has no transparency: see-through parts go on white, as they'd show on a page.
fn encode_jpeg(picture: &DynamicImage) -> Option<Vec<u8>> {
    let rgb = if picture.color().has_alpha() {
        let rgba = picture.to_rgba8();
        let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
        for (to, from) in rgb.pixels_mut().zip(rgba.pixels()) {
            let a = u32::from(from[3]);
            for c in 0..3 {
                to[c] = ((u32::from(from[c]) * a + 255 * (255 - a)) / 255) as u8;
            }
        }
        rgb
    } else {
        picture.to_rgb8()
    };
    let mut out = Vec::new();
    for quality in [85, 70] {
        out.clear();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        DynamicImage::ImageRgb8(rgb.clone())
            .write_with_encoder(encoder)
            .ok()?;
        if out.len() <= MAX_PNG_BYTES {
            break;
        }
    }
    Some(out)
}

/// The name a picture is kept under: its own, made safe, with the extension of what
/// it now is; else "Photo 2.jpg".
fn file_name(given: Option<&str>, index: usize, extension: &str) -> String {
    let stem = given
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n))
        .map(|n| match n.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => stem,
            _ => n,
        })
        .map(|n| {
            n.chars()
                .filter(|c| !c.is_control())
                .take(80)
                .collect::<String>()
                .trim()
                .trim_start_matches('.')
                .to_owned()
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("Photo {}", index + 1));
    format!("{stem}.{extension}")
}

/// Normalises every upload of one message, off the async threads (decoding is work).
pub async fn prepare(uploads: Vec<Upload>) -> Result<Vec<Prepared>, String> {
    if uploads.len() > MAX_PER_MESSAGE {
        return Err(format!(
            "At most {MAX_PER_MESSAGE} photos can go with one message."
        ));
    }
    if uploads.is_empty() {
        return Ok(Vec::new());
    }
    tokio::task::spawn_blocking(move || {
        uploads
            .iter()
            .enumerate()
            .map(|(i, u)| normalize(u, i))
            .collect()
    })
    .await
    .map_err(|_| "The photos couldn't be read. Try again.".to_owned())?
}

/// Keeps a message's pictures, in order.
pub async fn save(db: &Db, message_id: Uuid, items: Vec<Prepared>) -> Result<(), DbError> {
    if items.is_empty() {
        return Ok(());
    }
    let now = crate::now_ms();
    db.call(move |c| {
        let tx = c.transaction()?;
        for (position, item) in items.into_iter().enumerate() {
            let m = item.meta;
            tx.execute(
                "INSERT INTO message_attachments
                     (id, message_id, position, kind, mime, name, size, width, height, data, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    m.id.to_string(),
                    message_id.to_string(),
                    position as i64,
                    enum_str(m.kind),
                    m.mime,
                    m.name,
                    m.size as i64,
                    m.width,
                    m.height,
                    item.data,
                    now,
                ],
            )?;
        }
        tx.commit()
    })
    .await
}

const COLUMNS: &str = "a.id, a.kind, a.mime, a.name, a.size, a.width, a.height";

fn meta(row: &rusqlite::Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: parse_uuid(row, 0)?,
        kind: parse_enum(row, 1)?,
        mime: row.get(2)?,
        name: row.get(3)?,
        size: row.get::<_, i64>(4)? as u64,
        width: row.get(5)?,
        height: row.get(6)?,
    })
}

/// What each message of a conversation has attached, by message.
pub(crate) fn in_conversation(
    c: &rusqlite::Connection,
    conversation_id: Uuid,
) -> rusqlite::Result<HashMap<Uuid, Vec<Attachment>>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {COLUMNS}, a.message_id FROM message_attachments a
         JOIN messages m ON m.id = a.message_id
         WHERE m.conversation_id = ?1
         ORDER BY a.message_id, a.position"
    ))?;
    let mut out: HashMap<Uuid, Vec<Attachment>> = HashMap::new();
    for row in stmt.query_map([conversation_id.to_string()], |r| {
        Ok((parse_uuid(r, 7)?, meta(r)?))
    })? {
        let (message, attachment) = row?;
        out.entry(message).or_default().push(attachment);
    }
    Ok(out)
}

/// One attachment and its content.
pub async fn get(db: &Db, id: Uuid) -> Result<Option<(Attachment, Vec<u8>)>, DbError> {
    db.call(move |c| {
        c.query_row(
            &format!("SELECT {COLUMNS}, a.data FROM message_attachments a WHERE a.id = ?1"),
            [id.to_string()],
            |r| Ok((meta(r)?, r.get(7)?)),
        )
        .optional()
    })
    .await
}

/// The conversation a picture was sent in.
pub async fn conversation_of(db: &Db, id: Uuid) -> Result<Option<Uuid>, DbError> {
    db.call(move |c| {
        c.query_row(
            "SELECT m.conversation_id FROM message_attachments a
             JOIN messages m ON m.id = a.message_id WHERE a.id = ?1",
            [id.to_string()],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map(|id| id.and_then(|id| id.parse().ok()))
}

/// The pictures of these messages, with their content, in order.
pub async fn contents(
    db: &Db,
    messages: Vec<Uuid>,
) -> Result<HashMap<Uuid, Vec<(Attachment, Vec<u8>)>>, DbError> {
    db.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS}, a.data FROM message_attachments a
             WHERE a.message_id = ?1 ORDER BY a.position"
        ))?;
        let mut out = HashMap::new();
        for id in messages {
            let items = stmt
                .query_map([id.to_string()], |r| Ok((meta(r)?, r.get(7)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.insert(id, items);
        }
        Ok(out)
    })
    .await
}

/// A downloaded file's content, stopping at [`MAX_UPLOAD_BYTES`]: `None` when it's
/// larger, or the download broke off.
pub async fn read_capped(mut res: reqwest::Response) -> Option<Vec<u8>> {
    if res
        .content_length()
        .is_some_and(|n| n > MAX_UPLOAD_BYTES as u64)
    {
        return None;
    }
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await.ok()? {
        if out.len() + chunk.len() > MAX_UPLOAD_BYTES {
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    Some(out)
}

/// "a photo" / "3 photos", for notes to the model and the user.
pub fn count_label(n: usize) -> String {
    match n {
        1 => "a photo".to_owned(),
        n => format!("{n} photos"),
    }
}

#[cfg(test)]
pub(crate) mod tests;
