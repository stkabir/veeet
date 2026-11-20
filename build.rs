fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/logo/app-icon.ico");

    // Incrusta el icono en el .exe solo al compilar para Windows desde Windows
    // (winresource solo es build-dependency en ese caso, ver Cargo.toml).
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/logo/app-icon.ico");
        res.set("ProductName", "Veeet");
        res.set("CompanyName", "Stay True");
        res.set("LegalCopyright", "Hecho por Stay True · staytruemx.com");
        if let Err(err) = res.compile() {
            panic!("no se pudo incrustar el icono de Windows: {err}");
        }
    }
}
