fn main() {
    // Mjeku brenda PROGRAMERI POS Mobile: pa Tauri. Ndërtohet si në celular (cfg `mobile`).
    if std::env::var_os("CARGO_FEATURE_EMBEDDED").is_some() {
        println!("cargo:rustc-check-cfg=cfg(mobile)");
        println!("cargo:rustc-check-cfg=cfg(desktop)");
        println!("cargo:rustc-cfg=mobile");
        return;
    }
    #[cfg(feature = "desktop-shell")]
    tauri_build::build()
}
