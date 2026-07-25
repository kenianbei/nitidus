//! Contact photo rendering: PHOTO bytes (or a local file URI) decoded
//! once per selection and drawn through ratatui-image's negotiated
//! terminal protocol. Everything degrades: no terminal graphics, no
//! photo, or an undecodable image all fall back to the detail row's
//! textual `[N bytes]` placeholder.

use std::sync::{Arc, Mutex};

use bevy::prelude::Resource;
use image::DynamicImage;
use nitidus_contacts::{Contact, PhotoSource};
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

pub const PHOTO_ROWS: u16 = 8;
const FILE_URI_PREFIX: &str = "file://";

/// The negotiated terminal graphics protocol; `None` headless or when
/// the terminal answered the capability query with nothing usable.
#[derive(Resource, Default)]
pub struct PhotoPicker(pub Option<Picker>);

impl PhotoPicker {
    /// Queries the terminal — call before the TUI takes over stdio (the
    /// query does its own raw-mode dance and would race the app's input
    /// reader afterwards).
    pub fn detect() -> Self {
        Self(Picker::from_query_stdio().ok())
    }
}

/// A ready-to-render photo, keyed by contact and content fingerprint
/// so both selection changes and `:set-photo` re-decode exactly when
/// the picture actually changed.
#[derive(Clone)]
pub(super) struct PhotoCell {
    uid: String,
    fingerprint: usize,
    pub(super) protocol: Arc<Mutex<StatefulProtocol>>,
}

pub(super) fn photo_cell(
    picker: &Picker,
    contact: &Contact,
    previous: Option<&PhotoCell>,
) -> Option<PhotoCell> {
    let fingerprint = photo_fingerprint(contact)?;
    if let Some(cell) = previous
        && cell.uid == contact.uid()
        && cell.fingerprint == fingerprint
    {
        return Some(cell.clone());
    }
    let image = decode_photo(contact)?;
    Some(PhotoCell {
        uid: contact.uid().to_owned(),
        fingerprint,
        protocol: Arc::new(Mutex::new(picker.new_resize_protocol(image))),
    })
}

fn photo_fingerprint(contact: &Contact) -> Option<usize> {
    match contact.photo()? {
        PhotoSource::Bytes(bytes) => Some(bytes.len()),
        PhotoSource::Uri(uri) => Some(uri.len()),
    }
}

fn decode_photo(contact: &Contact) -> Option<DynamicImage> {
    match contact.photo()? {
        PhotoSource::Bytes(bytes) => image::load_from_memory(bytes).ok(),
        PhotoSource::Uri(uri) => load_local_uri(uri),
    }
}

/// Only local sources: `file://` URIs and absolute paths. Remote photo
/// URLs are never fetched (same no-remote-content stance as the pager).
fn load_local_uri(uri: &str) -> Option<DynamicImage> {
    let path = uri.strip_prefix(FILE_URI_PREFIX).unwrap_or(uri);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    image::load_from_memory(&bytes).ok()
}

const PHOTO_MAX_EDGE: u32 = 256;
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

/// `P`/`:set-photo`: a path applies directly; no argument browses.
pub fn set_photo(world: &mut bevy::prelude::World, path_argument: Option<&str>) {
    if super::mutate::selected_contact(world).is_none() {
        return;
    }
    match path_argument {
        Some(path) => apply_photo_pick(world, std::path::PathBuf::from(path)),
        None => crate::explorer::open_explorer(
            world,
            crate::explorer::ExplorerRequest {
                title: "set photo".to_owned(),
                extensions: IMAGE_EXTENSIONS,
                start_dir: None,
                on_pick: Box::new(apply_photo_pick),
            },
        ),
    }
}

fn apply_photo_pick(world: &mut bevy::prelude::World, path: std::path::PathBuf) {
    match load_downscaled_jpeg(&path) {
        Ok(jpeg_bytes) => {
            super::mutate::apply_mutation(world, move |contact| contact.set_photo_jpeg(&jpeg_bytes))
        }
        Err(problem) => super::mutate::warn(world, format!("set-photo failed: {problem}")),
    }
}

/// Bounded embedding: whatever came in, at most 256px on the long edge
/// as JPEG — a `.vcf` should stay a text file, not a photo archive.
fn load_downscaled_jpeg(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let image = image::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let scaled = image.thumbnail(PHOTO_MAX_EDGE, PHOTO_MAX_EDGE);
    let flattened = DynamicImage::ImageRgb8(scaled.to_rgb8());
    let mut out = std::io::Cursor::new(Vec::new());
    flattened
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|error| error.to_string())?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use base64::Engine as _;

    fn png_bytes() -> Vec<u8> {
        let image = DynamicImage::new_rgb8(2, 2);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn contact_with_inline_photo() -> Contact {
        let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes());
        let mut contact = Contact::new("Pic");
        contact
            .add_entry_line(&format!("PHOTO:data:image/png;base64,{encoded}"))
            .unwrap();
        contact
    }

    #[test]
    fn inline_base64_photo_decodes() {
        let contact = contact_with_inline_photo();
        assert!(matches!(contact.photo(), Some(PhotoSource::Bytes(_))));
        let image = decode_photo(&contact).unwrap();
        assert_eq!((image.width(), image.height()), (2, 2));
    }

    #[test]
    fn file_uri_photo_decodes_and_remote_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("face.png");
        std::fs::write(&path, png_bytes()).unwrap();
        let mut contact = Contact::new("Pic");
        contact
            .add_entry_line(&format!("PHOTO:file://{}", path.display()))
            .unwrap();
        assert!(decode_photo(&contact).is_some());

        let mut remote = Contact::new("Remote");
        remote
            .add_entry_line("PHOTO:https://example.com/face.png")
            .unwrap();
        assert!(
            decode_photo(&remote).is_none(),
            "remote photo urls must never be fetched"
        );
    }

    #[test]
    fn missing_photo_is_none() {
        assert!(decode_photo(&Contact::new("Plain")).is_none());
    }
}
