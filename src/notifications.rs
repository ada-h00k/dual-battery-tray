use crate::state::DeviceState;
use notify_rust::{Notification, Timeout};
use tracing::warn;

pub const DEFAULT_LOW_BATTERY_THRESHOLD: u8 = 20;

#[derive(Debug, Default)]
pub struct LowBatteryNotifier {
    headset_notified: bool,
    keyboard_notified: bool,
}

impl LowBatteryNotifier {
    pub async fn check(&mut self, headset: &DeviceState, keyboard: &DeviceState, threshold: u8) {
        if let Some(percent) = headset.percent {
            let should_notify = percent < threshold && !self.headset_notified;
            self.headset_notified = percent < threshold;
            if should_notify {
                let label = headset.display_name("Headset").to_string();
                if let Err(error) = notify_low_battery(&label, percent).await {
                    warn!(
                        device = %label,
                        %error,
                        "could not show low-battery notification"
                    );
                }
            }
        }

        if let Some(percent) = keyboard.percent {
            let should_notify = percent < threshold && !self.keyboard_notified;
            self.keyboard_notified = percent < threshold;
            if should_notify {
                let label = keyboard.display_name("Keyboard").to_string();
                if let Err(error) = notify_low_battery(&label, percent).await {
                    warn!(
                        device = %label,
                        %error,
                        "could not show low-battery notification"
                    );
                }
            }
        }
    }
}

async fn notify_low_battery(label: &str, percent: u8) -> notify_rust::error::Result<()> {
    Notification::new()
        .appname("dual-battery-tray")
        .summary(&format!("Akku fast leer: {label}"))
        .body(&format!("Nur noch {percent} % Akku verfügbar."))
        .icon("battery-low")
        .timeout(Timeout::Milliseconds(6000))
        .show_async()
        .await
        .map(|_| ())
}
