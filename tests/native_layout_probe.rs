//! Optional compositor-backed test of the real Rune shell's resize handling.
//! Run on an isolated Wayland compositor with LOBBYLOCKER_TEST_WAYLAND_RESIZE=1.
//! This currently exposes https://github.com/anomalyco/rune/issues/372 on
//! native Wayland until Rune handles an immediately applied window resize.

#[cfg(target_os = "linux")]
mod probe {
    use lobbylocker_app::{state, view};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    static STAGE: AtomicU8 = AtomicU8::new(0);
    static START: OnceLock<Instant> = OnceLock::new();

    fn poll(s: &mut state::AppState, _: f64, tx: &mut rune::Tx) -> bool {
        tx.wake_after(Duration::from_millis(50));
        if START.get_or_init(Instant::now).elapsed() > Duration::from_secs(15) {
            eprintln!(
                "resize timed out: stage={}, size={:?}, transition={}",
                STAGE.load(Ordering::SeqCst),
                s.window_size,
                s.layout_transition
            );
            tx.quit();
            return false;
        }
        match STAGE.load(Ordering::SeqCst) {
            0 if s.layout_observed => {
                state::set_compact(s, tx, true);
                STAGE.store(1, Ordering::SeqCst);
                true
            }
            1 if !s.layout_transition && (s.window_size.0 - 420.0).abs() < 2.0 => {
                state::set_compact(s, tx, false);
                STAGE.store(2, Ordering::SeqCst);
                true
            }
            2 if !s.layout_transition && (s.window_size.0 - 1240.0).abs() < 2.0 => {
                STAGE.store(3, Ordering::SeqCst);
                tx.quit();
                false
            }
            _ => false,
        }
    }

    pub fn run() {
        if std::env::var_os("LOBBYLOCKER_TEST_WAYLAND_RESIZE").is_none() {
            return;
        }
        let config = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", config.path());
        std::env::set_var("LOBBYLOCKER_NO_FIREWALL", "1");
        rune::run(rune::AppSpec {
            poll: Some(poll),
            min_size: Some((360.0, 560.0)),
            fonts: rune_theme_opencode::FONTS,
            families: rune_theme_opencode::FAMILIES,
            ..rune::AppSpec::new(
                "LobbyLocker resize test",
                (1240.0, 860.0),
                state::init,
                view::view,
            )
        });
        assert_eq!(
            STAGE.load(Ordering::SeqCst),
            3,
            "both layouts should reach their native sizes"
        );
        println!("Rune compact and expanded layouts followed the native Wayland window");
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    probe::run();
}
