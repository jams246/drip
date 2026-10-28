fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/drip.rc");
    println!("cargo:rerun-if-changed=assets/drip.ico");
    embed_resource::compile_for("assets/drip.rc", ["drip"], embed_resource::NONE)
        .manifest_required()?;
    Ok(())
}
