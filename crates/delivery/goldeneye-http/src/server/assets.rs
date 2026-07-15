use serde_json::json;

use crate::assets;

use super::request::safe_asset_path;
use super::response::Payload;

pub(super) fn static_payload(path: &str, base_path: &str, head: bool) -> Payload {
    if path == "/runtime-config.js" {
        return runtime_config_payload(base_path, head);
    }

    let requested = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if !safe_asset_path(requested) {
        return Payload::json(400, json!({ "error": "invalid asset path" }));
    }

    if requested == "index.html" || (!requested.contains('.') && assets::find(requested).is_none())
    {
        return index_payload(base_path, head);
    }
    asset_payload(requested, head)
}

fn runtime_config_payload(base_path: &str, head: bool) -> Payload {
    let encoded = serde_json::to_string(base_path).expect("base path serializes");
    let body = format!("globalThis.__GOLDENEYE_UI_CONFIG__ = {{ apiBasePath: {encoded} }};\n")
        .into_bytes();
    Payload::bytes(
        200,
        "text/javascript; charset=utf-8",
        body,
        "no-store",
        head,
    )
}

fn index_payload(base_path: &str, head: bool) -> Payload {
    let Some(index) = assets::find("index.html") else {
        return Payload::json(500, json!({ "error": "UI index missing" }));
    };
    let html = rewrite_index(index.bytes, base_path);
    Payload::bytes(
        200,
        assets::content_type("index.html"),
        html,
        "no-store",
        head,
    )
}

fn asset_payload(requested: &str, head: bool) -> Payload {
    let Some(asset) = assets::find(requested) else {
        return Payload::json(404, json!({ "error": "not found" }));
    };
    let cache = if requested.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=3600"
    };
    Payload::bytes(
        200,
        assets::content_type(requested),
        asset.bytes.to_vec(),
        cache,
        head,
    )
}

fn rewrite_index(bytes: &[u8], base_path: &str) -> Vec<u8> {
    let html = String::from_utf8_lossy(bytes);
    let asset_prefix = format!("{base_path}/assets/");
    let runtime = format!("{base_path}/runtime-config.js");
    html.replace("/assets/", &asset_prefix)
        .replace("./runtime-config.js", &runtime)
        .into_bytes()
}
