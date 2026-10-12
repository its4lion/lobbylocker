//! Unit tests for the platform module.
use super::needs_dmabuf_workaround;

#[test]
fn enables_workaround_for_wayland_only() {
    assert!(needs_dmabuf_workaround(true, false, None, false));
    assert!(needs_dmabuf_workaround(false, true, None, false));
    assert!(needs_dmabuf_workaround(
        false,
        false,
        Some("wayland"),
        false
    ));
    assert!(!needs_dmabuf_workaround(false, false, None, false));
    assert!(!needs_dmabuf_workaround(true, true, Some("x11"), false));
}

#[test]
fn never_overwrites_explicit_renderer_configuration() {
    assert!(!needs_dmabuf_workaround(true, true, None, true));
    assert!(!needs_dmabuf_workaround(true, true, Some("wayland"), true));
}
