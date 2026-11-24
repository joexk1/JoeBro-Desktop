//! Windows: embed the app icon and version info into the .exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/joebro.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/joebro.ico");
        res.set("ProductName", "JoeBro");
        res.set("FileDescription", "JoeBro");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
