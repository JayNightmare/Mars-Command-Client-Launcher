fn main() {
    // Public deployment configuration only; never embed desktop/provider tokens.
    for key in [
        "MARS_COMMUNITY_API_BASE",
        "MARS_WEBSITE_BASE",
        "MARS_SPONSORS_URL",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = std::env::var(key) {
            println!("cargo:rustc-env={key}={value}");
        }
    }
    tauri_build::build()
}
