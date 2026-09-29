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
    pub fn check(&mut self, headset: &DeviceState, keyboard: &DeviceState, threshold: u8) {
        Self::check_device(
            "Headset",
            headset,
            threshold,
            &mut self.headset_notified,
        );
        Self::check_device(
            "Keychron K2 HE",
            keyboard,
            threshold,
            &mut self.keyboard_notified,
        );
    }

    fn check_device(label: &str, device: &DeviceState, threshold: u8, already_notified: &mut bool) {
        let Some(percent) = device.percent else {
            return;
        };

        // "unter 20 %" means strictly below the configured threshold.
        if percent < threshold {
            if !*already_notified {
                if let Err(error) = notify_low_battery(label, percent) {
                    warn!(device = label, %error, "could not show low-battery notification");
                }
                // Latch the notification even when the desktop notification service
                // returns an error, so a broken notification daemon does not cause
                // a notification attempt every refresh cycle.
                *already_notified = true;
            }
        } else {
            // Re-arm once the device reaches the threshold again. A later drop
            // below the threshold will create a new notification.
            *already_notified = false;
        }
    }
}

fn notify_low_battery(label: &str, percent: u8) -> notify_rust::error::Result<()> {
    Notification::new()
        .appname("dual-battery-tray")
        .summary(&format!("Akku fast leer: {label}"))
        .body(&format!("Nur noch {percent} % Akku verfügbar."))
        .icon("battery-low")
        .timeout(Timeout::Milliseconds(6000))
        .show()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::LowBatteryNotifier;
    use crate::state::DeviceState;

    fn device(percent: Option<u8>) -> DeviceState {
        DeviceState {
            name: "test".into(),
            percent,
            charging: None,
            error: None,
        }
    }

    #[test]
    fn below_threshold_is_latched() {
        let mut notifier = LowBatteryNotifier::default();
        notifier.check(&device(Some(19)), &device(Some(50)), 20);
        assert!(notifier.headset_notified);
        notifier.check(&device(Some(18)), &device(Some(50)), 20);
        assert!(notifier.headset_notified);
    }

    #[test]
    fn threshold_rearms_after_recovery() {
        let mut notifier = LowBatteryNotifier::default();
        notifier.check(&device(Some(19)), &device(Some(50)), 20);
        assert!(notifier.headset_notified);
        notifier.check(&device(Some(20)), &device(Some(50)), 20);
        assert!(!notifier.headset_notified);
        notifier.check(&device(Some(19)), &device(Some(50)), 20);
        assert!(notifier.headset_notified);
    }

    #[test]
    fn missing_value_does_not_trigger_or_clear_latch() {
        let mut notifier = LowBatteryNotifier::default();
        notifier.check(&device(Some(19)), &device(Some(50)), 20);
        notifier.check(&device(None), &device(Some(50)), 20);
        assert!(notifier.headset_notified);
    }
}
