mod rooms;
mod session;
mod relay;

use anyhow::Result;
use tracing::info;

pub const TCP_PORT: u16 = 7878;
pub const UDP_PORT: u16 = 7879;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("tucha_server=debug".parse()?),
        )
        .init();

    let state = rooms::ServerState::new();

    info!("tucha-server starting");
    info!("TCP signaling on :{TCP_PORT}");
    info!("UDP relay      on :{UDP_PORT}");

    tokio::try_join!(
        session::run_tcp(state.clone(), TCP_PORT),
        relay::run_udp(state.clone(), UDP_PORT),
    )?;

    Ok(())
}
