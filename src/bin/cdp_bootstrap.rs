use clap::Parser;
use mbus::browser::CdpBrowser;
use mbus::config::{CliOverrides, load_config};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "cdp_bootstrap",
    version,
    about = "Validate browser startup only"
)]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    headless: Option<bool>,
    #[arg(long)]
    initial_url: Option<String>,
    #[arg(long)]
    cdp_url: Option<String>,
    #[arg(long)]
    browser_executable: Option<PathBuf>,
    #[arg(long)]
    browser_launch_timeout_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    browser_no_sandbox: Option<bool>,
    #[arg(long = "browser-arg")]
    browser_args: Vec<String>,
    #[arg(long, value_parser = clap::value_parser!(bool))]
    browser_keep_user_data_dir: Option<bool>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let browser_args = if cli.browser_args.is_empty() {
        None
    } else {
        Some(cli.browser_args)
    };
    let overrides = CliOverrides {
        headless: cli.headless,
        initial_url: cli.initial_url,
        cdp_url: cli.cdp_url,
        browser_executable: cli.browser_executable,
        browser_launch_timeout_ms: cli.browser_launch_timeout_ms,
        browser_no_sandbox: cli.browser_no_sandbox,
        browser_args,
        browser_keep_user_data_dir: cli.browser_keep_user_data_dir,
        ..CliOverrides::default()
    };

    let config = match load_config(cli.config.as_deref(), overrides) {
        Ok(config) => config.browser,
        Err(err) => {
            eprintln!("cdp_bootstrap config_error {err}");
            std::process::exit(1);
        }
    };

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
