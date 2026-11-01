//! winit 0.30 does not emit file-drop events on native Wayland.
//! Prefer its X11 backend when the desktop offers XWayland.
pub fn configure(options: &mut eframe::NativeOptions) {
    #[cfg(target_os = "linux")]
    if prefer_x11(std::env::var_os("DISPLAY").as_deref()) {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = options;
}
#[cfg(target_os = "linux")]
fn prefer_x11(display: Option<&std::ffi::OsStr>) -> bool {
    display.is_some_and(|display| !display.is_empty())
}
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn xwayland_is_used_when_available_without_disabling_wayland_only_startup() {
        assert!(prefer_x11(Some(std::ffi::OsStr::new(":0"))));
        assert!(!prefer_x11(None));
        assert!(!prefer_x11(Some(std::ffi::OsStr::new(""))));
    }
}
