fn main() {
    // The icon/version resource is generated for the MSVC toolchain used by the
    // official Windows builds. GNU/MinGW builds still work without an icon.
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "msvc" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "PTZControl - Modern PTZ camera control");
        res.set("ProductName", "PTZControl");
        res.set("LegalCopyright", "Copyright (C) 2026 PTZControl contributors - GPL-3.0-or-later");
        if let Err(err) = res.compile() {
            println!("cargo:warning=winresource: {err}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
}
