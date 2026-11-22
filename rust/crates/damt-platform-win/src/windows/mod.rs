//! Win32 implementations of the frozen contracts.

mod autostart;
mod focus;
mod games;
mod idle;
mod motion;
mod notifier;
mod platform;
mod registry;
mod screen;
mod session;
#[cfg(feature = "store")]
mod store;
mod taskbar;

pub use autostart::{WinAutostart, is_packaged_app};
pub use focus::WinFocus;
pub use games::WinForeground;
pub use idle::WinIdle;
pub use motion::WinMotion;
pub use notifier::WinNotifier;
pub use platform::WinPlatform;
pub use screen::WinActiveScreen;
pub use session::WinSessionEvents;
#[cfg(feature = "store")]
pub use store::{query_app_license, query_app_license_async, request_purchase_async};
pub use taskbar::{WinAppTheme, WinTaskbarTheme};
