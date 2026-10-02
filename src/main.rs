mod config;
mod icon;
mod keychron;
mod notifications;
mod openlinkhub;
mod state;
mod tray;

use anyhow::{Context, Result};
use config::Config;
use keychron::KeychronReader;
use notifications::LowBatteryNotifier;
use openlinkhub::OpenLinkHubReader;
use state::Snapshot;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Notify;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{error, info, warn};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    init_logging();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config_path = args
        .windows(2)
        .find(|w| w[0] == "--config")
        .map(|w| PathBuf::from(&w[1]))
        .or_else(default_config_path);

    let config = Config::load(config_path.as_deref())?;

    if args.iter().any(|a| a == "--probe") {
        return probe(&config).await;
    }

    let refresh = Duration::from_secs(config.refresh_seconds().max(5));
    let low_battery_threshold = config.low_battery_threshold();
    let snapshot = Snapshot::default();
    let refresh_notify = Arc::new(Notify::new());
    let (keyboard_refresh_tx, keyboard_refresh_rx) = std::sync::mpsc::channel::<()>();
    let tray = tray::BatteryTray::new(
        snapshot.clone(),
        low_battery_threshold,
        refresh_notify.clone(),
        keyboard_refresh_tx.clone(),
    );
    let handle = tray::spawn(tray)
        .await
        .context("could not create system tray")?;

    let openlink = OpenLinkHubReader::new(config.openlinkhub.clone())?;
    let mut headset_ticker = interval(refresh);
    headset_ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    // The HID side owns one dedicated blocking thread. It opens the HID device
    // once and keeps it open, avoiding a fresh hidapi enumeration every poll.
    let (keyboard_tx, mut keyboard_rx) = mpsc::unbounded_channel();
    let _keyboard_worker = KeychronReader::new(config.keyboard.clone())?.spawn_worker(
        refresh,
        keyboard_tx,
        keyboard_refresh_rx,
    );

    info!(
        refresh_seconds = refresh.as_secs(),
        low_battery_threshold,
        runtime = "single-thread",
        "dual-battery-tray started"
    );

    let mut current = Snapshot::default();
    let mut last_sent = Snapshot::default();
    let mut has_sent = false;
    let mut low_battery = LowBatteryNotifier::default();

    loop {
        tokio::select! {
            _ = headset_ticker.tick() => {
                refresh_headset(&openlink, &mut current).await;
                if update_if_changed(&handle, &mut current, &mut last_sent, &mut has_sent, &mut low_battery, low_battery_threshold).await.is_some() {
                    return Ok(());
                }
            }
            _ = refresh_notify.notified() => {
                refresh_headset(&openlink, &mut current).await;
                if update_if_changed(&handle, &mut current, &mut last_sent, &mut has_sent, &mut low_battery, low_battery_threshold).await.is_some() {
                    return Ok(());
                }
            }
            Some(value) = keyboard_rx.recv() => {
                current.keyboard = value;
                if update_if_changed(&handle, &mut current, &mut last_sent, &mut has_sent, &mut low_battery, low_battery_threshold).await.is_some() {
                    return Ok(());
                }
            }
        }
    }
}

async fn refresh_headset(openlink: &OpenLinkHubReader, current: &mut Snapshot) {
    match openlink.read().await {
        Ok(value) => current.headset = value,
        Err(e) => {
            warn!(error = %e, "OpenLinkHub read failed");
            current.headset.error = Some(e.to_string());
        }
    }
}

async fn update_if_changed(
    handle: &ksni::Handle<tray::BatteryTray>,
    current: &mut Snapshot,
    last_sent: &mut Snapshot,
    has_sent: &mut bool,
    low_battery: &mut LowBatteryNotifier,
    threshold: u8,
) -> Option<()> {
    low_battery
    .check(&current.headset, &current.keyboard, threshold)
    .await;
        .check(&current.headset, &current.keyboard, threshold)
        .await;

    let changed = !*has_sent || last_sent != current;
    if changed {
        let next = current.clone();
        if handle
            .update(move |tray| tray.snapshot = next)
            .await
            .is_none()
        {
            error!("tray service has been shut down");
            return Some(());
        }
        *last_sent = current.clone();
        *has_sent = true;
    }

    None
}

async fn probe(config: &Config) -> Result<()> {
    println!("== OpenLinkHub ==");
    let openlink = OpenLinkHubReader::new(config.openlinkhub.clone())?;
    match openlink.read().await {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
        Err(e) => println!("error: {e}"),
    }

    println!("\n== Keychron HID ==");
    let keychron = KeychronReader::new(config.keyboard.clone())?;
    for line in keychron.probe()? {
        println!("{line}");
    }
    Ok(())
}

fn init_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dual_battery_tray=info".into()),
        )
        .try_init();
}

fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(dirs_fallback)
        .map(|base| base.join("dual-battery-tray/config.toml"))
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}
