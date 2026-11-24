fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = "proto/tailscale.proto";
    println!("cargo:rerun-if-changed={schema}");
    let descriptors = protox::compile([schema], ["proto"])?;
    tonic_prost_build::configure()
        .build_server(false)
        .type_attribute(".", "#[derive(serde::Serialize)]")
        .type_attribute(".", "#[serde(rename_all = \"camelCase\")]")
        .field_attribute(
            "daemon.TailscalePeer.tailscaleIPs",
            "#[serde(rename = \"tailscaleIps\")]",
        )
        .compile_fds(descriptors)?;
    Ok(())
}
