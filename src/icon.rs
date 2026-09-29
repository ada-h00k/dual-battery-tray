use ksni::Icon;

// Fallback white icon for tray hosts that do not resolve the Breeze theme icon.
// Main icon: exact freedesktop icon name "battery" is requested from the desktop
// theme, so KDE Plasma with Breeze-dark uses its own Breeze battery artwork.
const WHITE_32: &[u8] = include_bytes!("../assets/battery-white-32.argb");
const WHITE_64: &[u8] = include_bytes!("../assets/battery-white-64.argb");
const RED_32: &[u8] = include_bytes!("../assets/battery-red-32.argb");
const RED_64: &[u8] = include_bytes!("../assets/battery-red-64.argb");

pub fn white() -> Vec<Icon> {
    vec![
        Icon { width: 32, height: 32, data: WHITE_32.to_vec() },
        Icon { width: 64, height: 64, data: WHITE_64.to_vec() },
    ]
}

pub fn red() -> Vec<Icon> {
    vec![
        Icon { width: 32, height: 32, data: RED_32.to_vec() },
        Icon { width: 64, height: 64, data: RED_64.to_vec() },
    ]
}
