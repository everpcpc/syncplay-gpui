use std::path::Path;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let resource_dir = Path::new("../../packaging/windows");
    let resource_file = resource_dir.join("syncplay.rc");
    println!("cargo:rerun-if-changed={}", resource_file.display());
    println!(
        "cargo:rerun-if-changed={}",
        resource_dir.join("syncplay.ico").display()
    );
    embed_resource::compile(
        &resource_file,
        embed_resource::ParamsIncludeDirs([resource_dir]),
    )
    .manifest_required()
    .unwrap();
}
