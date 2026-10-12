//! Process-local webview compatibility settings, applied before GTK or worker startup.

#[cfg(target_os = "linux")]
fn needs_dmabuf_workaround(
    wayland_display: bool,
    wayland_session: bool,
    gdk_backend: Option<&str>,
    renderer_override: bool,
) -> bool {
    if renderer_override {
        return false;
    }
    match gdk_backend {
        Some("x11") => false,
        Some("wayland") => true,
        _ => wayland_display || wayland_session,
    }
}

pub fn configure_webview() {
    #[cfg(target_os = "linux")]
    {
        let backend = std::env::var("GDK_BACKEND").ok();
        let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        let wayland_session = std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
        if needs_dmabuf_workaround(
            wayland_display,
            wayland_session,
            backend.as_deref(),
            std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some(),
        ) {
            // Must precede GTK/WebKit initialization and any spawned threads. This
            // only changes this process, not the user's shell or system settings.
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            eprintln!("Using the WebKit DMA-BUF compatibility workaround for Linux Wayland.");
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../tests/unit/platform.rs"]
mod tests;
