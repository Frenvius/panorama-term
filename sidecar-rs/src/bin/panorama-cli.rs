#[tokio::main]
async fn main() {
    sidecar::init_process();
    sidecar::cli_main();
}
