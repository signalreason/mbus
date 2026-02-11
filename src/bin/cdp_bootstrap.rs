use mbus::browser::{CdpBrowser, CdpConfig};

#[tokio::main]
async fn main() {
    let config = CdpConfig::default();
    match CdpBrowser::bootstrap(config).await {
        Ok(()) => {
            println!("cdp_bootstrap ok");
        }
        Err(err) => {
            eprintln!(
                "cdp_bootstrap launch_error code={} message={}",
                err.code, err.message
            );
            std::process::exit(1);
        }
    }
}
