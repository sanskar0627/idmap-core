mod client;

use anyhow::Result;
use dkg_tcp::env_loader::init_env;
use tracing_subscriber::fmt;

/// Entry point (only calls run_client)
#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_max_level(tracing::Level::INFO) // show info and above
        .with_target(true) // include target (module path)
        .with_thread_ids(true) // optional: include thread ids
        .init();

    // load the env variables
    init_env(env!("CARGO_MANIFEST_DIR"));

    // start the process
    client::run_client().await
}
