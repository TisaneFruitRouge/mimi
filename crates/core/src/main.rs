use mimi_protocol::Paths;
use tracing_subscriber::EnvFilter;

/// Mimi's own messages, and only problems from the messaging libraries: Matrix's is
/// chatty (it reports expected "not found" answers as errors), and Signal's log linking
/// codes and who wrote or deleted what in other chats.
const DEFAULT_LOG: &str = "info,matrix_sdk=warn,matrix_sdk::http_client=off,matrix_sdk_base=warn,matrix_sdk_crypto=error,matrix_sdk_sqlite=warn,presage=error,libsignal_service=error,libsignal_protocol=error,symphonia_core=error,symphonia_format_isomp4=error,symphonia_adapter_libopus=error";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG)),
        )
        .init();
    mimi_core::daemon::run(Paths::resolve()?).await
}
