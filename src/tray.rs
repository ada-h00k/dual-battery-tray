use crate::icon;
use crate::state::Snapshot;
use ksni::{menu::StandardItem, Tray, TrayMethods};
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Notify;
use std::path::Path;

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

    fn id(&self) -> String { "dual-battery-tray".into() }

    fn category(&self) -> ksni::Category { ksni::Category::Hardware }

    fn status(&self) -> ksni::Status {
        if self.is_low() {
            ksni::Status::NeedsAttention
        } else {
            ksni::Status::Active
        }
    }

    // Let KDE/Breeze-dark provide the normal battery icon.
    fn icon_name(&self) -> String { "battery".into() }

    // Prefer the installed Breeze-dark theme when it is present. This keeps the
    // normal icon as KDE's own artwork instead of forcing a bundled redraw.
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

    // ARGB pixmaps are retained as a fallback for tray hosts that do not load
    // the themed icon.
    fn icon_pixmap(&self) -> Vec<ksni::Icon> { icon::white() }

    // When low battery needs attention, provide an explicit red version so the
    // visual state does not depend on the current desktop theme's warning color.
    fn attention_icon_pixmap(&self) -> Vec<ksni::Icon> { icon::red() }

    fn title(&self) -> String {
        format!("{} / {}", fmt_pct(self.snapshot.headset.percent), fmt_pct(self.snapshot.keyboard.percent))
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
            label: format!("Headset: {}{}", fmt_pct(self.snapshot.headset.percent), charging_suffix(self.snapshot.headset.charging)),
            enabled: false,
            ..Default::default()
        };
        let keyboard = StandardItem {
            label: format!("Keychron K2 HE: {}{}", fmt_pct(self.snapshot.keyboard.percent), charging_suffix(self.snapshot.keyboard.charging)),
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
        let h = self.snapshot.headset.error.as_deref().unwrap_or("OpenLinkHub: OK");
        let k = self.snapshot.keyboard.error.as_deref().unwrap_or("Keyboard: OK");
        format!("{h} · {k}")
    }
}

fn is_below(value: Option<u8>, threshold: u8) -> bool {
    value.is_some_and(|p| p < threshold)
}

fn fmt_pct(value: Option<u8>) -> String {
    value.map(|v| format!("{v}%")).unwrap_or_else(|| "—".into())
}

fn charging_suffix(value: Option<bool>) -> &'static str {
    if value == Some(true) { " ⚡" } else { "" }
}
