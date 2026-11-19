#[tokio::main]
async fn main() {
    sidecar::init_process();
    sidecar::host_main();
}
