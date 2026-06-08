#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    franchisetech_connector::run_headless().await;
}
