use mimi_protocol::Paths;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            // Signal's libraries log linking codes, and who wrote or deleted what in
            // other chats: keep only their errors.
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new(
                    "info,presage=error,libsignal_service=error,libsignal_protocol=error",
                )
            }),
        )
        .init();
    mimi_core::daemon::run(Paths::resolve()?).await
}
