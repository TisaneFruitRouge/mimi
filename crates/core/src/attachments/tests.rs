use image::{GenericImageView, Rgb, RgbImage, Rgba, RgbaImage};

use super::*;

/// A JPEG photo, `w`×`h`, with an EXIF block saying it was taken sideways (orientation
/// 6: turn 90° clockwise) and carrying a description with a place in it.
pub(crate) fn sideways_jpeg_with_exif(w: u32, h: u32) -> Vec<u8> {
    let photo = RgbImage::from_fn(w, h, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
    let mut jpeg = Vec::new();
    DynamicImage::ImageRgb8(photo)
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut jpeg, 90,
        ))
        .unwrap();

    let secret = b"GPS 48.8566 N 2.3522 E, Rue Secrete\0";
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    // Orientation (SHORT) = 6.
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&6u16.to_le_bytes());
    tiff.extend_from_slice(&0u16.to_le_bytes());
    // ImageDescription (ASCII), stored after the directory.
    tiff.extend_from_slice(&0x010Eu16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&(secret.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&38u32.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(secret);

    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A PNG screenshot, with some see-through pixels.
pub(crate) fn screenshot_png(w: u32, h: u32) -> Vec<u8> {
    let shot = RgbaImage::from_fn(w, h, |x, _| {
        if x < 10 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([20, 20, 20, 255])
        }
    });
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(shot)
        .write_with_encoder(image::codecs::png::PngEncoder::new(&mut png))
        .unwrap();
    png
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn photos_are_turned_upright_shrunk_and_stripped_of_metadata() {
    let original = sideways_jpeg_with_exif(3000, 2000);
    assert!(contains(&original, b"Exif"));
    assert!(contains(&original, b"Rue Secrete"));

    let upload = Upload::new(
        original,
        Some("IMG_2041.JPG".into()),
        Some("image/jpeg".into()),
    );
    let kept = normalize(&upload, 0).unwrap();
    assert_eq!(kept.meta.kind, AttachmentKind::Image);
    assert_eq!(kept.meta.mime, "image/jpeg");
    assert_eq!(kept.meta.name, "IMG_2041.jpg");
    // Taken sideways: now upright (taller than wide), the long side shrunk to 1568.
    assert_eq!(
        (kept.meta.width, kept.meta.height),
        (Some(1045), Some(1568))
    );
    assert_eq!(kept.meta.size, kept.data.len() as u64);

    // Nothing of the original metadata is left.
    assert!(!contains(&kept.data, b"Exif"));
    assert!(!contains(&kept.data, b"Rue Secrete"));
    assert!(!contains(&kept.data, b"GPS"));
    let again = image::load_from_memory(&kept.data).unwrap();
    assert_eq!(again.dimensions(), (1045, 1568));
}

#[test]
fn screenshots_stay_png_and_small_pictures_keep_their_size() {
    let upload = Upload::new(screenshot_png(800, 600), None, None);
    let kept = normalize(&upload, 1).unwrap();
    assert_eq!(kept.meta.mime, "image/png");
    assert_eq!(kept.meta.name, "Photo 2.png");
    assert_eq!((kept.meta.width, kept.meta.height), (Some(800), Some(600)));
    let again = image::load_from_memory(&kept.data).unwrap();
    // Transparency is kept in PNG.
    assert_eq!(again.get_pixel(0, 0)[3], 0);
}

#[test]
fn what_is_not_a_picture_is_refused() {
    let text = Upload::new(
        b"Hello, this is not a picture".to_vec(),
        Some("notes.txt".into()),
        None,
    );
    let err = normalize(&text, 0).unwrap_err();
    assert!(err.contains("“notes.txt” isn't a picture"), "{err}");

    // A PDF that claims to be a JPEG is still a PDF.
    let pdf = Upload::new(
        b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n".to_vec(),
        Some("ticket.jpg".into()),
        Some("image/jpeg".into()),
    );
    assert!(normalize(&pdf, 0).is_err());

    // A truncated JPEG.
    let mut broken = sideways_jpeg_with_exif(64, 64);
    broken.truncate(200);
    assert!(normalize(&Upload::new(broken, None, None), 0).is_err());

    let mut heic = vec![0, 0, 0, 24];
    heic.extend_from_slice(b"ftypheic");
    heic.extend_from_slice(&[0; 32]);
    let err = normalize(&Upload::new(heic, Some("IMG_1.HEIC".into()), None), 0).unwrap_err();
    assert!(err.contains("HEIC"), "{err}");

    let huge = Upload::new(vec![0; MAX_UPLOAD_BYTES + 1], None, None);
    assert!(normalize(&huge, 0).unwrap_err().contains("too large"));
}

#[test]
fn names_are_made_safe() {
    assert_eq!(file_name(Some("../../etc/passwd"), 0, "jpg"), "passwd.jpg");
    assert_eq!(
        file_name(Some("ticket.final.webp"), 0, "png"),
        "ticket.final.png"
    );
    assert_eq!(file_name(Some(".hidden"), 2, "jpg"), "hidden.jpg");
    assert_eq!(file_name(Some("  "), 2, "jpg"), "Photo 3.jpg");
    assert_eq!(file_name(Some("a\u{7}b\nc.png"), 0, "png"), "abc.png");
}

#[test]
fn uploads_decode_with_or_without_padding() {
    let b64 = |s: &str| NewAttachment {
        data: s.to_owned(),
        name: None,
        mime: None,
    };
    assert_eq!(Upload::from_api(b64("aGk=")).unwrap().data, b"hi");
    assert_eq!(Upload::from_api(b64("aGk")).unwrap().data, b"hi");
    assert_eq!(
        Upload::from_api(b64("data:image/png;base64,aGk="))
            .unwrap()
            .data,
        b"hi"
    );
    assert!(Upload::from_api(b64("not base64!")).is_err());
}

#[tokio::test]
async fn at_most_ten_go_with_a_message() {
    let one = Upload::new(screenshot_png(20, 20), None, None);
    assert_eq!(prepare(vec![one.clone(); 10]).await.unwrap().len(), 10);
    assert!(
        prepare(vec![one; 11])
            .await
            .unwrap_err()
            .contains("At most 10")
    );
}
