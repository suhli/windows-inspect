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
