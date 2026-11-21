fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/app-manifest.rc");
    println!("cargo:rerun-if-changed=packaging/brand/lobbylocker.ico");
    println!("cargo:rerun-if-changed=crates/core/assets/windows/lobbylocker.exe.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for(
            "packaging/windows/app-manifest.rc",
            ["lobbylocker-app"],
            embed_resource::NONE,
        )
        .manifest_required()
        .unwrap();
    }
}
