use std::io::Cursor;

fn main() {
    // Propagate DEP_TAURI_DEV to the compilation environment so
    // generate_context!() can read it. In Cargo, DEP_* vars from linked
    // crates are automatically available during compilation. In Bazel,
    // they're only available during build.rs (via build_script_env /
    // link_deps) unless explicitly re-emitted as cargo:rustc-env.
    if let Ok(dev) = std::env::var("DEP_TAURI_DEV") {
        println!("cargo:rustc-env=DEP_TAURI_DEV={dev}");
    }

    tauri_build::build();
    precreate_codegen_cache();
}

/// Pre-create cache files in OUT_DIR for Bazel compatibility.
///
/// Tauri's `generate_context!()` proc macro writes processed data to OUT_DIR
/// via `write_if_changed()`. In Bazel, OUT_DIR is read-only during compilation
/// (only writable during build.rs). By pre-creating these files here, the proc
/// macro's `write_if_changed()` finds matching content and skips the write.
///
/// Cache files:
/// 1. RGBA-decoded icon bytes (default window icon)
/// 2. Raw PNG icon bytes (macOS app icon)
/// 3. Info.plist XML (macOS embed-plist, dev mode only)
/// 4. Brotli-compressed frontend assets (frontendDist)
fn precreate_codegen_cache() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let manifest_path = std::path::Path::new(&manifest_dir);

    precreate_icon_cache(&out_dir, manifest_path);

    // Only macOS dev mode generates the plist cache.
    if cfg!(target_os = "macos") && std::env::var("DEP_TAURI_DEV").as_deref() == Ok("true") {
        precreate_plist_cache(&out_dir, manifest_path);
    }

    precreate_frontend_cache(&out_dir, manifest_path);
}

/// Pre-create icon hash files matching `CachedIcon::new_png()` and
/// `CachedIcon::new_raw()` in tauri-codegen.
fn precreate_icon_cache(out_dir: &str, manifest_path: &std::path::Path) {
    let icon_path = manifest_path.join("icons/icon.png");
    if !icon_path.exists() {
        return;
    }

    let raw_bytes = std::fs::read(&icon_path).expect("failed to read icon");

    // 1. RGBA-decoded variant — used by CachedIcon::new_png() for the default
    //    window icon on all platforms.
    let decoder = png::Decoder::new(Cursor::new(&raw_bytes));
    let mut reader = decoder
        .read_info()
        .expect("failed to decode icon PNG header");
    let mut rgba = Vec::with_capacity(reader.output_buffer_size());
    while let Ok(Some(row)) = reader.next_row() {
        rgba.extend(row.data());
    }
    write_cache_file(out_dir, &rgba);

    // 2. Raw-bytes variant — used by CachedIcon::new_raw() for the macOS app
    //    icon in dev mode.
    write_cache_file(out_dir, &raw_bytes);
}

/// Pre-create the Info.plist cache file matching context.rs lines 303-350.
///
/// On macOS in dev mode, `generate_context!()` builds a plist from the Tauri
/// config (product_name, version) and caches the XML via `Cached::try_from()`.
fn precreate_plist_cache(out_dir: &str, manifest_path: &std::path::Path) {
    let config_path = manifest_path.join("tauri.conf.json");
    if !config_path.exists() {
        return;
    }

    let config_str = std::fs::read_to_string(&config_path).expect("failed to read tauri.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(&config_str).expect("failed to parse tauri.conf.json");

    // Read an existing Info.plist or start with an empty dict, matching
    // context.rs behavior.
    let info_plist_path = manifest_path.join("Info.plist");
    let mut info_plist = if info_plist_path.exists() {
        plist::Value::from_file(&info_plist_path).expect("failed to read Info.plist")
    } else {
        plist::Value::Dictionary(Default::default())
    };

    if let Some(dict) = info_plist.as_dictionary_mut() {
        // bundle.macos.bundle_name ?? productName
        let bundle_name = config
            .pointer("/bundle/macos/bundleName")
            .and_then(|v| v.as_str())
            .or_else(|| config.get("productName").and_then(|v| v.as_str()));
        if let Some(name) = bundle_name {
            dict.insert("CFBundleName".into(), name.into());
        }

        if let Some(version) = config.get("version").and_then(|v| v.as_str()) {
            let bundle_version = config
                .pointer("/bundle/macos/bundleVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(version);
            dict.insert("CFBundleShortVersionString".into(), version.into());
            dict.insert("CFBundleVersion".into(), bundle_version.into());
        }
    }

    // Serialize to XML, matching the exact plist crate output that
    // tauri-codegen produces.
    let mut buf = std::io::BufWriter::new(Vec::new());
    info_plist
        .to_writer_xml(&mut buf)
        .expect("failed to serialize plist");
    let xml_bytes = buf.into_inner().expect("flush failed");
    let xml_string = String::from_utf8_lossy(&xml_bytes).into_owned();

    // Cached::try_from(String) converts to Vec<u8> then hashes.
    write_cache_file(out_dir, xml_string.as_bytes());
}

/// Write `content` to `$OUT_DIR/{blake3_hex}`, matching tauri-codegen's
/// `Cached::try_from()` which uses the BLAKE3 hash as the filename.
fn write_cache_file(out_dir: &str, content: &[u8]) {
    let hash = blake3::hash(content);
    let hex = hash.to_hex();
    let path = std::path::Path::new(out_dir).join(hex.as_str());
    // Skip if already exists with correct content (idempotent).
    if path.exists() && std::fs::read(&path).is_ok_and(|existing| existing == content) {
        return;
    }
    std::fs::write(&path, content).expect("failed to write cache file");
}

/// Pre-create brotli-compressed frontend asset cache for Bazel compatibility.
///
/// When `frontendDist` is set in tauri.conf.json, `generate_context!()` reads
/// each file, blake3-hashes the raw bytes, brotli-compresses them, and writes
/// to `$OUT_DIR/tauri-codegen-assets/{hash}.{ext}`. In Bazel, OUT_DIR is
/// read-only during proc macro execution, so we pre-create the file here.
fn precreate_frontend_cache(out_dir: &str, manifest_path: &std::path::Path) {
    let index_path = manifest_path.join("loading-dist/index.html");
    if !index_path.exists() {
        return;
    }

    let raw_bytes = std::fs::read(&index_path).expect("failed to read index.html");

    // Hash raw bytes — with csp=null, tauri-codegen does not modify the HTML
    let hash = blake3::hash(&raw_bytes);
    let hex = hash.to_hex();

    let assets_dir = std::path::Path::new(out_dir).join("tauri-codegen-assets");
    std::fs::create_dir_all(&assets_dir).expect("failed to create tauri-codegen-assets dir");

    let out_path = assets_dir.join(format!("{}.html", hex));
    if out_path.exists() {
        return; // already cached
    }

    // Brotli compress — match tauri-codegen's compression_settings()
    let mut input = Cursor::new(&raw_bytes);
    let mut output = Vec::new();
    let params = brotli::enc::BrotliEncoderParams {
        quality: if cfg!(debug_assertions) { 2 } else { 9 },
        ..Default::default()
    };
    brotli::BrotliCompress(&mut input, &mut output, &params).expect("failed to brotli compress");
    std::fs::write(&out_path, &output).expect("failed to write frontend cache");
}
