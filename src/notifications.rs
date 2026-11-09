use crate::state::DeviceState;
use notify_rust::{Notification, Timeout};
use tracing::warn;

pub const DEFAULT_LOW_BATTERY_THRESHOLD: u8 = 20;

#[derive(Debug, Default)]
pub struct LowBatteryNotifier {
    headset_notified: bool,
    keyboard_notified: bool,
    mouse_notified: bool,
}

impl LowBatteryNotifier {
    pub async fn check(
        &mut self,
        headset: &DeviceState,
        keyboard: &DeviceState,
        mouse: &DeviceState,
        threshold: u8,
    ) {
        // One latch per device; the array keeps the per-device bookkeeping
        // identical instead of copying the same block three times.
        let devices = [
            (headset, &mut self.headset_notified, "Headset"),
            (keyboard, &mut self.keyboard_notified, "Keyboard"),
            (mouse, &mut self.mouse_notified, "Mouse"),
        ];
        for (device, notified, fallback) in devices {
            let Some(percent) = device.percent else {
                continue;
            };
            let should_notify = percent < threshold && !*notified;
            *notified = percent < threshold;
            if should_notify {
                let label = device.display_name(fallback).to_string();
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

#[cfg(test)]
mod tests {
    use super::LowBatteryNotifier;
    use crate::state::DeviceState;

    /// The notification itself needs a desktop session; these tests only
    /// verify the per-device latch logic by observing the struct fields.
    fn device(percent: Option<u8>) -> DeviceState {
        DeviceState {
            name: "Test".into(),
            percent,
            charging: None,
            error: None,
        }
    }

    fn notified(notifier: &LowBatteryNotifier) -> (bool, bool, bool) {
        (
            notifier.headset_notified,
            notifier.keyboard_notified,
            notifier.mouse_notified,
        )
    }

    #[tokio::test]
    async fn latches_per_device_independently() {
        let mut notifier = LowBatteryNotifier::default();
        // Below threshold: the notify attempt may fail without a session, but
        // the latch must still arm.
        notifier
            .check(&device(Some(10)), &device(Some(90)), &device(Some(90)), 20)
            .await;
        assert_eq!(notified(&notifier), (true, false, false));

        // Still below: no re-notification (latched).
        notifier
            .check(&device(Some(10)), &device(Some(90)), &device(Some(90)), 20)
            .await;
        assert_eq!(notified(&notifier), (true, false, false));

        // Mouse drops too: only the mouse latch arms.
        notifier
            .check(&device(Some(10)), &device(Some(90)), &device(Some(15)), 20)
            .await;
        assert_eq!(notified(&notifier), (true, false, true));

        // Recovery re-arms; exactly-20 is not low.
        notifier
            .check(&device(Some(20)), &device(Some(20)), &device(Some(20)), 20)
            .await;
        assert_eq!(notified(&notifier), (false, false, false));
    }

    #[tokio::test]
    async fn missing_reading_never_latches() {
        let mut notifier = LowBatteryNotifier::default();
        notifier
            .check(&device(None), &device(None), &device(None), 20)
            .await;
        assert_eq!(notified(&notifier), (false, false, false));
    }
}
