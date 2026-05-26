fn main() {
    println!("cargo:rerun-if-changed=windows-inspect.exe.manifest");
    println!("cargo:rerun-if-changed=assets/icon.png");
    slint_build::compile("ui/app.slint").expect("Slint UI compile failed");

    #[cfg(target_os = "windows")]
    embed_windows_exe_icon();

    if std::env::var("TARGET")
        .map(|target| target.contains("windows-msvc"))
        .unwrap_or(false)
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:windows-inspect.exe.manifest");
    }
}

#[cfg(target_os = "windows")]
fn embed_windows_exe_icon() {
    use std::io::Cursor;
    use std::path::Path;

    let png_path = Path::new("assets/icon.png");
    let ico_path = Path::new("target/icon.ico");

    let img = match image::open(png_path) {
        Ok(img) => img.to_rgba8(),
        Err(err) => {
            eprintln!("cargo:warning=failed to load assets/icon.png for exe icon: {err}");
            return;
        }
    };

    if let Some(parent) = ico_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16u32, 32, 48, 256] {
        let resized = image::imageops::resize(
            &img,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        );
        let entry = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        icon_dir.add_entry(ico::IconDirEntry::encode(&entry).expect("encode icon entry"));
    }

    let mut bytes = Vec::new();
    if icon_dir.write(Cursor::new(&mut bytes)).is_err() {
        eprintln!("cargo:warning=failed to write target/icon.ico");
        return;
    }
    if std::fs::write(ico_path, bytes).is_err() {
        eprintln!("cargo:warning=failed to save target/icon.ico");
        return;
    }

    if let Err(err) = winres::WindowsResource::new()
        .set_icon(ico_path.to_str().expect("icon path utf-8"))
        .compile()
    {
        eprintln!("cargo:warning=failed to embed exe icon: {err}");
    }
}
