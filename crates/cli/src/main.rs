use std::net::IpAddr;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use diagwire::{Outcome, Session, ecu_name, hsfz, uds};
use tracing_subscriber::EnvFilter;

/// BMW diagnostics over ENET (F/G-series). Set RUST_LOG=debug to see raw frames.
#[derive(Parser)]
#[command(name = "diagwire", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Find vehicles on the ENET link.
    Discover {
        #[arg(long, default_value_t = 2000)]
        timeout_ms: u64,
    },
    /// Read fault memory from one ECU.
    Dtc {
        /// Vehicle IP address. Discovered automatically if omitted.
        #[arg(long)]
        ip: Option<IpAddr>,
        /// ECU diagnostic address in hex, e.g. 12 for the DME.
        #[arg(long, value_parser = parse_hex_u8)]
        ecu: u8,
        /// DTC status mask in hex.
        #[arg(long, value_parser = parse_hex_u8, default_value = "FF")]
        mask: u8,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().cmd {
        Cmd::Discover { timeout_ms } => {
            let vehicles = hsfz::discover(Duration::from_millis(timeout_ms)).await?;
            if vehicles.is_empty() {
                println!("No vehicles answered. Check the ENET cable and that the ignition is on.");
            }
            for v in vehicles {
                println!("{}  VIN {}", v.ip, v.vin.as_deref().unwrap_or("?"));
            }
        }
        Cmd::Dtc { ip, ecu, mask } => {
            let ip = match ip {
                Some(ip) => ip,
                None => find_vehicle().await?,
            };
            let mut session = Session::connect(ip)
                .await
                .with_context(|| format!("connecting to {ip}"))?;
            match session.read_dtcs(ecu, mask).await? {
                Outcome::Complete(report) => print_report(ecu, &report),
                other => bail!("{}: {}; no fault list was read", label(ecu), describe(&other)),
            }
        }
    }
    Ok(())
}

async fn find_vehicle() -> Result<IpAddr> {
    let vehicles = hsfz::discover(Duration::from_secs(2)).await?;
    match vehicles.as_slice() {
        [] => bail!("no vehicle found; pass --ip or check the ENET cable"),
        [one] => Ok(one.ip),
        many => bail!("{} vehicles answered; pick one with --ip", many.len()),
    }
}

fn parse_hex_u8(s: &str) -> Result<u8, String> {
    let digits = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u8::from_str_radix(digits, 16).map_err(|e| format!("{s:?} is not a hex byte: {e}"))
}

fn label(ecu: u8) -> String {
    match ecu_name(ecu) {
        Some(name) => format!("0x{ecu:02X} {name}"),
        None => format!("0x{ecu:02X}"),
    }
}

/// Says why a read produced no fault list. Never called for `Complete`.
fn describe<T>(outcome: &Outcome<T>) -> String {
    match outcome {
        Outcome::Complete(_) => "read complete".into(),
        Outcome::Negative(nrc) => format!("refused the read: {} (NRC 0x{nrc:02X})", uds::nrc_name(*nrc)),
        Outcome::PendingExpired => "said it was still working and never sent a final answer".into(),
        Outcome::NoResponse => "no response".into(),
        Outcome::Malformed => "sent a response that could not be validated".into(),
    }
}

fn print_report(ecu: u8, report: &uds::DtcReport) {
    println!("{}: {} fault(s)", label(ecu), report.dtcs.len());
    for dtc in &report.dtcs {
        println!("  {dtc}  status {:?}", dtc.status);
    }
}
