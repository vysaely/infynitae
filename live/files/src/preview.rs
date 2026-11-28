//! Asynchronous right-side Preview loader: bounded image decoding (PNG, JPEG, WebP, GIF via the
//! `image` crate with dimension and memory caps; SVG intentionally treated as text), 16 KiB capped
//! UTF-8 text snippets, symlink-safe folder item counts capped at 1,000, and metadata.

use crate::mime::{plain_type_name, MimeDb};
use image::imageops::FilterType;
use image::{ImageReader, Limits};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::UNIX_EPOCH;

const MAX_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 64_000_000;
const MAX_IMAGE_DIM: u32 = 16_384;
const MAX_IMAGE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
const MAX_TEXT_LINES: usize = 24;
const MAX_FOLDER_COUNT: usize = 1_000;

#[derive(Clone, Debug)]
pub struct PreviewImage {
    pub orig_w: u32,
    pub orig_h: u32,
    pub thumb_w: u32,
    pub thumb_h: u32,
    /// Premultiplied BGRA8 bytes matching Cairo's `Format::ARgb32` on little-endian Linux.
    pub bgra: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum PreviewContent {
    Empty,
    Loading {
        name: String,
    },
    Image {
        name: String,
        image: PreviewImage,
        type_label: String,
        size: u64,
        modified_secs: Option<i64>,
    },
    Text {
        name: String,
        snippet: String,
        type_label: String,
        size: u64,
        modified_secs: Option<i64>,
    },
    Folder {
        name: String,
        item_count: usize,
        capped: bool,
        modified_secs: Option<i64>,
    },
    File {
        name: String,
        type_label: String,
        size: u64,
        modified_secs: Option<i64>,
    },
    Multiple {
        count: usize,
        total_size: u64,
    },
}

pub fn load_preview_sync(path: &Path, mime_db: &MimeDb, max_w: u32, max_h: u32) -> PreviewContent {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());

    let sym = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return PreviewContent::File {
                name,
                type_label: "Unreadable".to_string(),
                size: 0,
                modified_secs: None,
            }
        }
    };
    let meta = if sym.file_type().is_symlink() {
        fs::metadata(path).unwrap_or(sym)
    } else {
        sym
    };
    let modified_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    if meta.is_dir() {
        let (count, capped) = count_folder_items(path);
        return PreviewContent::Folder {
            name,
            item_count: count,
            capped,
            modified_secs,
        };
    }

    let size = meta.len();
    let mime = mime_db.guess_mime(&name);
    let type_label = plain_type_name(&mime, false);

    if mime_db.is_previewable_image(&mime) && size <= MAX_IMAGE_FILE_BYTES {
        if let Some(img) = decode_preview_image(path, max_w, max_h) {
            return PreviewContent::Image {
                name,
                image: img,
                type_label,
                size,
                modified_secs,
            };
        }
    }

    if mime_db.is_previewable_text(&mime) {
        if let Some(snippet) = read_text_snippet(path) {
            return PreviewContent::Text {
                name,
                snippet,
                type_label,
                size,
                modified_secs,
            };
        }
    }

    PreviewContent::File {
        name,
        type_label,
        size,
        modified_secs,
    }
}

fn count_folder_items(dir: &Path) -> (usize, bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, false);
    };
    let mut count = 0usize;
    for item in entries {
        if item.is_ok() {
            count += 1;
            if count > MAX_FOLDER_COUNT {
                return (MAX_FOLDER_COUNT, true);
            }
        }
    }
    (count, false)
}

fn decode_preview_image(path: &Path, max_w: u32, max_h: u32) -> Option<PreviewImage> {
    let mut reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIM);
    limits.max_image_height = Some(MAX_IMAGE_DIM);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    reader.limits(limits);

    let (orig_w, orig_h) = reader.into_dimensions().ok()?;
    if orig_w == 0 || orig_h == 0 || (orig_w as u64) * (orig_h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }

    let mut reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIM);
    limits.max_image_height = Some(MAX_IMAGE_DIM);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    reader.limits(limits);

    let dyn_img = reader.decode().ok()?;
    let target_w = max_w.max(1);
    let target_h = max_h.max(1);
    let scaled = if orig_w > target_w || orig_h > target_h {
        dyn_img.resize(target_w, target_h, FilterType::Triangle)
    } else {
        dyn_img
    };
    let rgba = scaled.to_rgba8();
    let (thumb_w, thumb_h) = rgba.dimensions();
    let mut bgra = Vec::with_capacity((thumb_w as usize) * (thumb_h as usize) * 4);
    for px in rgba.pixels() {
        let a = px[3] as u16;
        let r = ((px[0] as u16 * a + 127) / 255) as u8;
        let g = ((px[1] as u16 * a + 127) / 255) as u8;
        let b = ((px[2] as u16 * a + 127) / 255) as u8;
        bgra.push(b);
        bgra.push(g);
        bgra.push(r);
        bgra.push(px[3]);
    }
    Some(PreviewImage {
        orig_w,
        orig_h,
        thumb_w,
        thumb_h,
        bgra,
    })
}

fn read_text_snippet(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0u8; MAX_TEXT_BYTES];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.contains(&0) {
        return None;
    }
    // Trim an incomplete trailing UTF-8 codepoint caused by the 16 KiB cap.
    let valid_end = match std::str::from_utf8(&buf) {
        Ok(_) => buf.len(),
        Err(e) => {
            if e.error_len().is_some() {
                return None;
            }
            e.valid_up_to()
        }
    };
    let text = std::str::from_utf8(&buf[..valid_end]).ok()?;
    let mut lines = Vec::new();
    for line in text.lines().take(MAX_TEXT_LINES) {
        lines.push(line.replace('\t', "  "));
    }
    Some(lines.join("\n"))
}

pub struct PreviewLoaded {
    pub generation: u64,
    pub content: PreviewContent,
}

pub struct PreviewWorker {
    tx: mpsc::Sender<(u64, PathBuf, u32, u32)>,
    gen: Arc<AtomicU64>,
}

impl PreviewWorker {
    pub fn spawn(mime_db: Arc<MimeDb>, notify: impl Fn(PreviewLoaded) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<(u64, PathBuf, u32, u32)>();
        let gen = Arc::new(AtomicU64::new(0));
        let worker_gen = Arc::clone(&gen);

        thread::spawn(move || {
            while let Ok((mut g, mut path, mut mw, mut mh)) = rx.recv() {
                while let Ok((ng, np, nmw, nmh)) = rx.try_recv() {
                    g = ng;
                    path = np;
                    mw = nmw;
                    mh = nmh;
                }
                if worker_gen.load(Ordering::Relaxed) != g {
                    continue;
                }
                let content = load_preview_sync(&path, &mime_db, mw, mh);
                if worker_gen.load(Ordering::Relaxed) == g {
                    notify(PreviewLoaded {
                        generation: g,
                        content,
                    });
                }
            }
        });

        Self { tx, gen }
    }

    pub fn request(&self, path: PathBuf, max_w: u32, max_h: u32) -> u64 {
        let g = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.tx.send((g, path, max_w, max_h));
        g
    }

    pub fn cancel(&self) {
        self.gen.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_preview_caps_at_16kib_and_skips_binary() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-files-prev-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = MimeDb::default();

        let txt = dir.join("notes.txt");
        let big = "hello line\n".repeat(3000);
        fs::write(&txt, big.as_bytes()).unwrap();
        match load_preview_sync(&txt, &db, 200, 132) {
            PreviewContent::Text { snippet, .. } => {
                assert_eq!(snippet.lines().count(), MAX_TEXT_LINES);
            }
            other => panic!("expected text preview, got {other:?}"),
        }

        let bin = dir.join("fake.txt");
        fs::write(&bin, b"abc\0def").unwrap();
        assert!(matches!(
            load_preview_sync(&bin, &db, 200, 132),
            PreviewContent::File { .. }
        ));

        // SVG must not be decoded as an image; shown as text instead.
        let svg = dir.join("icon.svg");
        fs::write(&svg, b"<svg><circle r=\"4\"/></svg>").unwrap();
        assert!(matches!(
            load_preview_sync(&svg, &db, 200, 132),
            PreviewContent::Text { .. }
        ));

        // Real PNG decodes with dimensions.
        let png_path = dir.join("pic.png");
        let img = image::RgbaImage::from_fn(40, 20, |_, _| image::Rgba([100, 150, 200, 255]));
        img.save(&png_path).unwrap();
        match load_preview_sync(&png_path, &db, 200, 132) {
            PreviewContent::Image { image, .. } => {
                assert_eq!((image.orig_w, image.orig_h), (40, 20));
                assert_eq!(image.bgra.len(), 40 * 20 * 4);
            }
            other => panic!("expected image preview, got {other:?}"),
        }

        // Folder item count does not loop on a symlink pointing back to itself.
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("one.txt"), b"1").unwrap();
        let _ = std::os::unix::fs::symlink(&sub, sub.join("loop"));
        match load_preview_sync(&sub, &db, 200, 132) {
            PreviewContent::Folder {
                item_count, capped, ..
            } => {
                assert_eq!(item_count, 2);
                assert!(!capped);
            }
            other => panic!("expected folder preview, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
