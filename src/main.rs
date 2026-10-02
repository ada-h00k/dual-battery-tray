use crate::icon;
use crate::state::Snapshot;
use ksni::{menu::StandardItem, Tray, TrayMethods};
use std::path::Path;
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct BatteryTray {
    pub snapshot: Snapshot,
    pub low_battery_threshold: u8,
    refresh_notify: Arc<Notify>,
    keyboard_refresh_tx: Sender<()>,
}

impl BatteryTray {
    pub fn new(
        snapshot: Snapshot,
        low_battery_threshold: u8,
        refresh_notify: Arc<Notify>,
        keyboard_refresh_tx: Sender<()>,
    ) -> Self {
        Self {
            snapshot,
            low_battery_threshold,
            refresh_notify,
            keyboard_refresh_tx,
        }
    }

    fn is_low(&self) -> bool {
        is_below(self.snapshot.headset.percent, self.low_battery_threshold)
            || is_below(self.snapshot.keyboard.percent, self.low_battery_threshold)
    }
}

impl Tray for BatteryTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "dual-battery-tray".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Hardware
    }

    // Always keep the tray item active.
    //
    // Do NOT use NeedsAttention here because KDE Plasma may animate
    // or otherwise highlight the tray icon.
    fn status(&self) -> ksni::Status {
        ksni::Status::Active
    }

    // Use the Breeze battery icon normally.
    //
    // When the battery is low, return an empty icon name so that the
    // tray host falls back to our explicit red pixmap instead.
    fn icon_name(&self) -> String {
        if self.is_low() {
            String::new()
        } else {
            "battery".into()
        }
    }

    // Prefer the installed Breeze-dark theme for the normal icon.
    fn icon_theme_path(&self) -> String {
        [
            "/usr/share/icons/breeze-dark",
            "/usr/local/share/icons/breeze-dark",
        ]
        .into_iter()
        .find(|path| Path::new(path).is_dir())
        .unwrap_or("")
        .into()
    }

    // Explicit pixmap fallback.
    //
    // When the battery is low, use the bundled static red icon.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.is_low() {
            icon::red()
        } else {
            icon::white()
        }
    }

    fn title(&self) -> String {
        format!(
            "{} / {}",
            fmt_pct(self.snapshot.headset.percent),
            fmt_pct(self.snapshot.keyboard.percent)
        )
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Dual Battery".into(),
            description: format!(
                "Headset: {}\nKeyboard: {}",
                fmt_pct(self.snapshot.headset.percent),
                fmt_pct(self.snapshot.keyboard.percent)
            ),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let headset = StandardItem {
            label: format!(
                "Headset: {}{}",
                fmt_pct(self.snapshot.headset.percent),
                charging_suffix(self.snapshot.headset.charging)
            ),
            enabled: false,
            ..Default::default()
        };

        let keyboard = StandardItem {
            label: format!(
                "Keychron K2 HE: {}{}",
                fmt_pct(self.snapshot.keyboard.percent),
                charging_suffix(self.snapshot.keyboard.charging)
            ),
            enabled: false,
            ..Default::default()
        };

        let status = StandardItem {
            label: self.status_line(),
            enabled: false,
            ..Default::default()
        };

        let refresh_notify = Arc::clone(&self.refresh_notify);
        let keyboard_refresh_tx = self.keyboard_refresh_tx.clone();

        let refresh = StandardItem {
            label: "Force refresh".into(),
            icon_name: "view-refresh".into(),
            activate: Box::new(move |_| {
                refresh_notify.notify_one();
                let _ = keyboard_refresh_tx.send(());
            }),
            ..Default::default()
        };

        let quit = StandardItem {
            label: "Quit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        };

        vec![
            headset.into(),
            keyboard.into(),
            ksni::MenuItem::Separator,
            status.into(),
            ksni::MenuItem::Separator,
            refresh.into(),
            ksni::MenuItem::Separator,
            quit.into(),
        ]
    }
}

pub async fn spawn(tray: BatteryTray) -> Result<ksni::Handle<BatteryTray>, ksni::Error> {
    tray.spawn().await
}

impl BatteryTray {
    fn status_line(&self) -> String {
        let h = self
            .snapshot
            .headset
            .error
            .as_deref()
            .unwrap_or("OpenLinkHub: OK");

        let k = self
            .snapshot
            .keyboard
            .error
            .as_deref()
            .unwrap_or("Keyboard: OK");

        format!("{h} · {k}")
    }
}

fn is_below(value: Option<u8>, threshold: u8) -> bool {
    value.is_some_and(|p| p < threshold)
}

fn fmt_pct(value: Option<u8>) -> String {
    value
        .map(|v| format!("{v}%"))
        .unwrap_or_else(|| "—".into())
}

fn charging_suffix(value: Option<bool>) -> &'static str {
    if value == Some(true) {
        " ⚡"
    } else {
        ""
    }
}
```

Der relevante Wechsel ist:

```rust
fn icon_name(&self) -> String {
    if self.is_low() {
        String::new()
    } else {
        "battery".into()
    }
}
