fn main() {
    println!("cargo:rerun-if-changed=windows-inspect.exe.manifest");
    slint_build::compile("ui/app.slint").expect("Slint UI compile failed");

    if std::env::var("TARGET")
        .map(|target| target.contains("windows-msvc"))
        .unwrap_or(false)
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:windows-inspect.exe.manifest");
    }
}
