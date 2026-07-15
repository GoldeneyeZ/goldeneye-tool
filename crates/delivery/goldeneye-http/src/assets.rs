pub struct EmbeddedAsset {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

pub fn find(path: &str) -> Option<&'static EmbeddedAsset> {
    ASSETS
        .binary_search_by_key(&path, |asset| asset.path)
        .ok()
        .map(|index| &ASSETS[index])
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("md" | "txt" | "sha256") | None => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
