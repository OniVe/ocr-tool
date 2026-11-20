//! Embed the application icon and version metadata into the Windows binary.
//!
//! Only compiled on Windows: other targets build without resources.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/ocr-tool.ico");

    #[cfg(windows)]
    {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
        );
        let icon = manifest_dir.join("../../assets/ocr-tool.ico");
        if !icon.is_file() {
            panic!("icon not found: {}", icon.display());
        }

        let mut res = winres::WindowsResource::new();
        res.set_icon(icon.to_str().expect("icon path is valid UTF-8"));
        res.set("FileDescription", "ocr-tool command line interface");
        res.set("ProductName", "ocr-tool");
        res.set("OriginalFilename", "ocr-tool.exe");
        res.compile().expect("failed to compile Windows resources");
    }
}
