#![cfg_attr(windows, windows_subsystem = "windows")]

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[cfg(windows)]
const DEFAULT_CONFIG: &str = r"C:\ProgramData\iran-split\helper.toml";
#[cfg(not(windows))]
const DEFAULT_CONFIG: &str = "/etc/iran-split/helper.toml";

#[derive(Debug, Parser)]
#[command(version, about = "Privileged service for Iran Split Desktop")]
struct Arguments {
    /// Root-owned helper configuration written by the installer.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Copy this process and Mihomo into a system location and start the helper.
    #[arg(long)]
    install: bool,
    /// Stop and remove the packaged Windows helper task.
    #[arg(long)]
    uninstall: bool,
    /// Packaged Mihomo executable used with `--install`.
    #[arg(long)]
    mihomo: Option<PathBuf>,
    /// Runtime generation staging directory used with `--install`.
    #[arg(long)]
    staging_dir: Option<PathBuf>,
    /// TUN interface name used with `--install`.
    #[arg(long)]
    tun_name: Option<String>,
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();
}

fn config_path(arguments: &Arguments) -> PathBuf {
    arguments
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG))
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let arguments = Arguments::parse();
    if arguments.install || arguments.uninstall {
        return Err("Linux helper installation uses install-helper.sh".into());
    }
    iran_split_helper::run_linux(&config_path(&arguments)).await?;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let arguments = Arguments::parse();
    if arguments.install || arguments.uninstall {
        return Err("macOS helper installation uses the launchd installer".into());
    }
    iran_split_helper::run_macos(&config_path(&arguments)).await?;
    Ok(())
}

#[cfg(windows)]
fn arguments_or_exit() -> Arguments {
    match Arguments::try_parse() {
        Ok(arguments) => arguments,
        Err(error) => {
            if let Some(message) = iran_split_helper::clap_install_error_message(&error) {
                iran_split_helper::persist_install_error(&message);
            }
            error.exit()
        }
    }
}

#[cfg(windows)]
fn install_from_args(arguments: &Arguments) -> Result<(), Box<dyn std::error::Error>> {
    let mihomo = arguments
        .mihomo
        .as_ref()
        .ok_or("Windows helper install requires --mihomo")?;
    let staging_dir = arguments
        .staging_dir
        .as_ref()
        .ok_or("Windows helper install requires --staging-dir")?;
    let tun_name = arguments
        .tun_name
        .as_ref()
        .ok_or("Windows helper install requires --tun-name")?;
    iran_split_helper::install(mihomo, staging_dir, tun_name)?;
    Ok(())
}

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let arguments = arguments_or_exit();
    if arguments.uninstall {
        iran_split_helper::uninstall()?;
        return Ok(());
    }
    if arguments.install {
        let result = install_from_args(&arguments);
        if let Err(error) = &result {
            iran_split_helper::persist_install_error(&error.to_string());
        }
        result?;
        return Ok(());
    }
    let result = iran_split_helper::run_named_pipe(&config_path(&arguments)).await;
    if let Err(error) = &result {
        iran_split_helper::persist_install_error(&error.to_string());
    }
    result?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn main() {
    eprintln!("iran-split-helper is unsupported on this target");
    std::process::exit(1);
}
