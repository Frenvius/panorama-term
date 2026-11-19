#[tokio::main]
async fn main() {
    sidecar::init_process();
    sidecar::brain_main().await;
}
