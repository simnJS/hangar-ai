//! The files a model points at: a glTF's `.bin` and textures, an OBJ's `.mtl`
//! and its maps. They resolve next to the model, as the old viewer's scheme
//! resolved relative URLs, and a decoder turns the pictures into pixels on as
//! many threads as the machine has.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine;

use super::{Cancel, LoadOptions};
use crate::formats::MAX_MODEL_BYTES;
use crate::model::Image;

/// Where a model's own files are looked for.
pub(crate) struct Resources<'a> {
    pub base: &'a Path,
    pub options: &'a LoadOptions,
}

/// `%20` back to a space, as a URI inside a glTF spells a file name.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 3 <= bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Resources<'_> {
    /// The file a URI names: relative to the model, or a `file:` URL. Anything
    /// that would have the preview fetch from the network is refused, and so
    /// is a file outside the folders the app allowed, if it named any.
    pub fn resolve(&self, uri: &str) -> Result<PathBuf, String> {
        let uri = uri.trim();
        if let Some((scheme, _)) = uri.split_once(':') {
            // A Windows drive letter is a path, not a scheme.
            let is_drive = scheme.len() == 1 && scheme.as_bytes()[0].is_ascii_alphabetic();
            if !is_drive && !scheme.eq_ignore_ascii_case("file") {
                return Err(format!(
                    "{uri} is not a file next to the model: the preview does not fetch from the network"
                ));
            }
        }
        let raw = uri
            .strip_prefix("file://")
            .or_else(|| uri.strip_prefix("file:"))
            .unwrap_or(uri);
        let decoded = percent_decode(raw).replace('\\', "/");
        let path = self.base.join(decoded);
        match &self.options.roots {
            None => Ok(path),
            Some(roots) => {
                let canonical = path.canonicalize().map_err(|err| format!("{uri}: {err}"))?;
                if roots.iter().any(|root| canonical.starts_with(root)) {
                    Ok(canonical)
                } else {
                    Err(format!("{uri} is outside the folders the preview may read"))
                }
            }
        }
    }

    /// The bytes behind a URI: a `data:` URI's payload, or a file.
    pub fn read(&self, uri: &str) -> Result<Vec<u8>, String> {
        if let Some(rest) = uri.trim().strip_prefix("data:") {
            let (header, payload) = rest
                .split_once(',')
                .ok_or_else(|| "a data URI without a comma".to_string())?;
            return if header.ends_with(";base64") {
                base64::engine::general_purpose::STANDARD
                    .decode(payload.trim())
                    .map_err(|err| format!("a data URI that is not valid base64: {err}"))
            } else {
                Ok(percent_decode(payload).into_bytes())
            };
        }
        let path = self.resolve(uri)?;
        read_capped(&path).map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// A file read whole, held to the same cap as the model itself.
pub(crate) fn read_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_MODEL_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MODEL_BYTES {
        return Err(std::io::Error::other("larger than the preview reads"));
    }
    Ok(bytes)
}

/// Pixels from the bytes of a picture file. The format is read from the
/// content, not the name: a `.png` that is a JPEG still opens.
pub fn decode_image(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"\xabKTX 20\xbb\r\n\x1a\n") {
        return Err("KTX2 textures are not supported by the preview".into());
    }
    let decoded = image::load_from_memory(bytes).map_err(|err| err.to_string())?;
    let rgba = decoded.into_rgba8();
    Ok(Image {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// Where a picture comes from.
pub(crate) enum ImageSource {
    Bytes(Vec<u8>),
    Uri(String),
}

/// Decodes pictures on several threads: a model with a dozen 4K textures
/// would otherwise spend most of its load in one decoder after another. The
/// results are in the order of the sources.
pub(crate) fn decode_all(
    resources: &Resources<'_>,
    sources: Vec<ImageSource>,
    cancel: &Cancel,
) -> Vec<Result<Image, String>> {
    let count = sources.len();
    if count == 0 {
        return Vec::new();
    }
    let threads = std::thread::available_parallelism()
        .map_or(2, usize::from)
        .min(count)
        .min(8);
    let next = AtomicUsize::new(0);
    let results = std::sync::Mutex::new((0..count).map(|_| None).collect::<Vec<_>>());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= count || cancel.is_cancelled() {
                        break;
                    }
                    let outcome = match &sources[i] {
                        ImageSource::Bytes(bytes) => decode_image(bytes),
                        ImageSource::Uri(uri) => {
                            resources.read(uri).and_then(|bytes| decode_image(&bytes))
                        }
                    };
                    results.lock().unwrap()[i] = Some(outcome);
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|outcome| outcome.unwrap_or_else(|| Err("cancelled".into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources<'a>(base: &'a Path, options: &'a LoadOptions) -> Resources<'a> {
        Resources { base, options }
    }

    #[test]
    fn decodes_percent_escapes_in_a_file_name() {
        assert_eq!(percent_decode("my%20wood%2Bgrain.png"), "my wood+grain.png");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn resolves_a_relative_uri_next_to_the_model() {
        let options = LoadOptions::default();
        let r = resources(Path::new("/game/props"), &options);
        assert_eq!(
            r.resolve("textures/wood%20dark.png").unwrap(),
            Path::new("/game/props/textures/wood dark.png")
        );
        assert_eq!(
            r.resolve("a\\b.png").unwrap(),
            Path::new("/game/props/a/b.png")
        );
    }

    #[test]
    fn refuses_to_fetch_from_the_network() {
        let options = LoadOptions::default();
        let r = resources(Path::new("/game"), &options);
        let error = r.resolve("https://example.com/a.png").unwrap_err();
        assert!(error.contains("network"), "{error}");
    }

    #[test]
    fn keeps_to_the_allowed_folders_when_it_is_given_some() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("inside");
        std::fs::create_dir(&inside).unwrap();
        std::fs::write(inside.join("a.png"), b"x").unwrap();
        std::fs::write(dir.path().join("secret.bin"), b"x").unwrap();
        let options = LoadOptions {
            roots: Some(vec![inside.canonicalize().unwrap()]),
        };
        let r = resources(&inside, &options);
        assert!(r.resolve("a.png").is_ok());
        let error = r.resolve("../secret.bin").unwrap_err();
        assert!(error.contains("outside"), "{error}");
    }

    #[test]
    fn reads_a_data_uri() {
        let options = LoadOptions::default();
        let r = resources(Path::new("."), &options);
        assert_eq!(
            r.read("data:application/octet-stream;base64,aGVsbG8=")
                .unwrap(),
            b"hello"
        );
        assert!(r.read("data:nonsense").is_err());
    }

    #[test]
    fn decodes_a_picture_by_its_content() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_image(&png).unwrap();
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(&decoded.rgba[..4], &[10, 20, 30, 255]);
        assert!(decode_image(b"not a picture").is_err());
        assert!(
            decode_image(b"\xabKTX 20\xbb\r\n\x1a\n....")
                .unwrap_err()
                .contains("KTX2")
        );
    }

    #[test]
    fn decodes_several_pictures_in_order() {
        let mut png = Vec::new();
        for shade in [1u8, 2, 3] {
            let mut one = Vec::new();
            image::RgbaImage::from_pixel(1, 1, image::Rgba([shade, 0, 0, 255]))
                .write_to(&mut std::io::Cursor::new(&mut one), image::ImageFormat::Png)
                .unwrap();
            png.push(ImageSource::Bytes(one));
        }
        png.insert(1, ImageSource::Bytes(b"broken".to_vec()));
        let options = LoadOptions::default();
        let results = decode_all(
            &resources(Path::new("."), &options),
            png,
            &Cancel::default(),
        );
        assert_eq!(results.len(), 4);
        assert_eq!(results[0].as_ref().unwrap().rgba[0], 1);
        assert!(results[1].is_err());
        assert_eq!(results[2].as_ref().unwrap().rgba[0], 2);
        assert_eq!(results[3].as_ref().unwrap().rgba[0], 3);
    }
}
