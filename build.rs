fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico").set("ProductName", "qBit-Distant-UI").set("FileDescription", "qBit-Distant-UI");
        res.compile().expect("ressource Windows (icône)");
    }
}
