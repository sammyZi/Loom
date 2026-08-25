fn main() {
    println!("cargo:rerun-if-changed=setup.rc");
    println!("cargo:rerun-if-changed=../../cli/icon.ico");
    // Same icon as the app, so the download is recognisable before it runs.
    embed_resource::compile("setup.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("embed installer icon");
}
