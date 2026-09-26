//! Console entry for `BiFlow.exe probe`.
//!
//! The desktop binary is a Windows GUI program, so the report is also written
//! beside `debug.log` where a terminal cannot show it.

use iran_split_ipc::egress::{probe_adapter, probe_config, EgressProbe};
use std::fs;
use std::path::PathBuf;

#[must_use]
pub fn run_probe_cli(args: &[String]) -> i32 {
    let reports = reports_for(args);
    write_report(&reports);
    for report in &reports {
        println!("{}", report.detail);
    }
    i32::from(!reports.iter().all(|report| report.ok))
}

pub fn reports_for(args: &[String]) -> Vec<EgressProbe> {
    let adapter = args.iter().find(|arg| !arg.starts_with('-'));
    match adapter {
        Some(name) if !name.eq_ignore_ascii_case("windscribe") => {
            vec![probe_adapter(name)]
        }
        _ => probe_config(&installed_config()),
    }
}

pub fn installed_config() -> String {
    fs::read_to_string(installed_config_path()).unwrap_or_default()
}

fn installed_config_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\ProgramData\iran-split\runtime\config.yaml")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/var/lib/iran-split/runtime/config.yaml")
    }
}

pub fn write_report(reports: &[EgressProbe]) {
    let Some(directory) = dirs::data_local_dir() else {
        return;
    };
    let directory = directory.join("biflow");
    if let Err(error) = fs::create_dir_all(&directory) {
        tracing::warn!(
            event = "egress.report_write_failed",
            cause = error.kind().to_string(),
            "could not create the egress probe directory"
        );
        return;
    }
    let text = reports
        .iter()
        .map(|report| report.detail.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(error) = fs::write(directory.join("probe-report.txt"), text) {
        tracing::warn!(
            event = "egress.report_write_failed",
            cause = error.kind().to_string(),
            "could not write the egress probe report"
        );
    }
}
