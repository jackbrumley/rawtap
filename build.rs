//! Windows only: embeds the app icon and version information in rawtap.exe,
//! so Explorer, Task Manager and Apps & features show RawTap's icon, name and
//! version. `FileVersion` and `ProductVersion` come from Cargo.toml.

fn main() {
    println!("cargo:rerun-if-changed=packaging/icons/rawtap.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("packaging/icons/rawtap.ico")
        .set("ProductName", "RawTap")
        .set(
            "FileDescription",
            "RawTap: raw serial and TCP monitor and console",
        )
        .set("CompanyName", "Jack Brumley")
        .set("LegalCopyright", "Jack Brumley, GNU AGPLv3");
    // A failed resource compile fails the build: a Windows release must carry them.
    if let Err(error) = resource.compile() {
        panic!("Could not embed the Windows icon and version information: {error}");
    }
}
