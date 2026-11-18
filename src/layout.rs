//! Window layout: compact vs. expanded, and the resize observation that
//! drives the automatic switch between them.

/// Window width (logical points) below which the app switches to the
/// compact layout.
pub const COMPACT_WIDTH_THRESHOLD: f32 = 720.0;
/// Default expanded window size.
pub const EXPANDED_WIDTH: f32 = 1240.0;
pub const EXPANDED_HEIGHT: f32 = 860.0;
/// Default compact window size.
pub const COMPACT_WIDTH: f32 = 420.0;
pub const COMPACT_HEIGHT: f32 = 800.0;

use crate::state::AppState;
use crate::state::Tab;

/// Switch the window layout without changing game or firewall preferences.
pub fn set_compact(s: &mut AppState, tx: &mut rune::Tx, compact: bool) {
    if compact == s.compact {
        return;
    }
    if compact {
        if s.window_size.0 >= COMPACT_WIDTH_THRESHOLD {
            s.expanded_size = s.window_size;
        }
        enter_compact(s);
        // A deliberate choice stays until the window is resized past the
        // compact width by hand.
        s.compact_auto = false;
    } else {
        if s.window_size.0 < COMPACT_WIDTH_THRESHOLD {
            s.compact_size = s.window_size;
        }
        s.compact = false;
        s.compact_auto = false;
        s.compact_game_open = false;
    }
    s.layout_transition = true;
    s.layout_origin_width = s.window_size.0;
    s.layout_target = if compact {
        s.compact_size
    } else {
        s.expanded_size
    };
    save_window_layout(s);
    // The layout changes with the window, so don't interpolate the old and
    // new shell geometry while the native resize is being applied.
    tx.instant();
    tx.resize_window_instant(s.layout_target);
}

/// Remember user-resized dimensions and make the narrow layout usable if the
/// expanded window is manually dragged below its normal minimum width. A
/// window that grows back to the full width returns to the expanded layout.
pub fn observe_window_layout(s: &mut AppState, [_, _, width, height]: [f32; 4]) {
    if width < 1.0 || height < 1.0 {
        return;
    }
    let observed_before = s.layout_observed;
    s.layout_observed = true;
    if s.layout_transition {
        if (width - s.layout_target.0).abs() > 1.0 && (width - s.layout_origin_width).abs() <= 1.0 {
            return;
        }
        s.layout_transition = false;
    }
    s.window_size = (width, height);
    if s.compact {
        s.compact_size = (width, height);
        if s.compact_auto && width >= COMPACT_WIDTH_THRESHOLD {
            // The window is wide again: back to the expanded layout.
            exit_compact(s);
            s.expanded_size = (width, height);
            save_window_layout(s);
        }
    } else if width < COMPACT_WIDTH_THRESHOLD && observed_before {
        enter_compact(s);
        s.compact_size = (width, height);
        save_window_layout(s);
    } else if width >= COMPACT_WIDTH_THRESHOLD {
        s.expanded_size = (width, height);
    }
}

fn enter_compact(s: &mut AppState) {
    s.compact = true;
    s.compact_auto = true;
    s.tab = Tab::Regions;
    s.editing = false;
    s.editing_region = None;
    s.adding_area = None;
    s.language_open = false;
    s.settings_language_open = false;
    s.compact_game_open = false;
}

/// Leave the compact layout for the expanded one, keeping the window as it is.
fn exit_compact(s: &mut AppState) {
    s.compact = false;
    s.compact_game_open = false;
    s.layout_transition = false;
}

fn save_window_layout(s: &mut AppState) {
    let value = if s.compact { "compact" } else { "expanded" };
    if let Err(error) = std::fs::write(&s.layout_path, value) {
        s.error = format!("Could not save window layout: {error}");
    }
}
