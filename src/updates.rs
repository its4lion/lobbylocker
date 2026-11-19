//! Update checks and installation run on a Rune worker; the UI owns consent.

use crate::{state::AppState, update_install::Target};
use lobbylocker_core::{
    i18n::Translator,
    update::{self, AvailableUpdate},
    Result,
};
use rune::{col, row, text, El, Latest, Tx, Waker};
use rune_kit::{Button, ButtonVariant, Dialog, Kit};
use std::path::PathBuf;

#[derive(Clone)]
pub enum UpdateNotice {
    Preview(String),
    Release(AvailableUpdate),
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Checking,
    Current,
    Installing,
    Installed {
        path: PathBuf,
        restart_error: Option<String>,
    },
    Failed(String),
}

enum Done {
    Checked(Result<Option<AvailableUpdate>>),
    Installed(Result<Option<PathBuf>>),
}

#[derive(Default)]
pub(crate) struct Updates {
    worker: Latest<Done>,
    target: Option<Target>,
    phase: Phase,
    open: bool,
}

pub(crate) fn setup(s: &mut AppState, waker: Waker) {
    s.updates.worker.set_waker(waker);
    if !rune::is_headless() && s.update_notice.is_none() {
        s.updates.target = Target::detect();
        check(s);
    }
}

fn check(s: &mut AppState) {
    let Some(target) = &s.updates.target else {
        return;
    };
    s.updates.phase = Phase::Checking;
    s.updates.worker.request(target.platform, |platform| {
        Done::Checked(update::check(platform))
    });
}

fn install(s: &mut AppState) {
    if s.busy.is_some()
        || s.fw_latest.is_pending()
        || s.scan_latest.is_pending()
        || matches!(
            s.updates.phase,
            Phase::Checking | Phase::Installing | Phase::Installed { .. }
        )
    {
        return;
    }
    let (Some(target), Some(UpdateNotice::Release(release))) =
        (&s.updates.target, &s.update_notice)
    else {
        return;
    };
    s.updates.phase = Phase::Installing;
    s.updates
        .worker
        .request((target.clone(), release.clone()), |(target, release)| {
            Done::Installed(target.install(release))
        });
}

fn restart(s: &mut AppState, tx: &mut Tx) {
    let Phase::Installed {
        path,
        restart_error,
    } = &mut s.updates.phase
    else {
        return;
    };
    match std::process::Command::new(path)
        .env_remove("APPIMAGE")
        .env_remove("APPDIR")
        .env_remove("ARGV0")
        .env_remove("OWD")
        .spawn()
    {
        Ok(_) => tx.quit(),
        Err(e) => *restart_error = Some(e.to_string()),
    }
}

pub(crate) fn poll(s: &mut AppState, tx: &mut Tx) -> bool {
    let Some(done) = s.updates.worker.take() else {
        return false;
    };
    s.updates.phase = match done {
        Done::Checked(Ok(Some(release))) => {
            s.update_notice = Some(UpdateNotice::Release(release));
            Phase::Idle
        }
        Done::Checked(Ok(None)) => Phase::Current,
        Done::Installed(Ok(Some(path))) => {
            s.update_notice = None;
            Phase::Installed {
                path,
                restart_error: None,
            }
        }
        Done::Installed(Ok(None)) => {
            tx.quit();
            Phase::Idle
        }
        Done::Checked(Err(error)) | Done::Installed(Err(error)) => Phase::Failed(error),
    };
    true
}

/// Both footers expose the same control so compact mode can check for updates too.
pub(crate) fn button(s: &AppState, k: &Kit, t: &Translator) -> Option<El> {
    let label = match s.updates.phase {
        Phase::Checking => "Checking for updates…",
        Phase::Installing => "Installing update…",
        Phase::Installed { .. } => "Restart to update",
        _ if s.update_notice.is_some() => "Update",
        _ if s.updates.target.is_some() => "Check for updates",
        _ => return None,
    };
    Some(
        Button::new(t.text(label))
            .variant(ButtonVariant::Outline)
            .on_press(|s: &mut AppState| {
                s.updates.open = true;
                if s.update_notice.is_none()
                    && !matches!(
                        s.updates.phase,
                        Phase::Checking | Phase::Installing | Phase::Installed { .. }
                    )
                {
                    check(s);
                }
            })
            .build(k)
            .key("update-button"),
    )
}

pub(crate) fn dialog(s: &AppState, k: &Kit, t: &Translator, layer: &mut Vec<El>) {
    if !s.updates.open {
        return;
    }
    let mut action = None;
    let installing = matches!(s.updates.phase, Phase::Installing);
    let message = match &s.updates.phase {
        Phase::Checking => t.text("Checking for updates…").to_string(),
        Phase::Installing => t.text("Downloading and verifying the update…").to_string(),
        Phase::Installed { restart_error, .. } => {
            action = Some(
                Button::new(t.text("Restart LobbyLocker"))
                    .on_press_tx(restart)
                    .build(k),
            );
            if let Some(error) = restart_error {
                format!("{}\n{error}", t.text("Could not restart LobbyLocker."))
            } else {
                t.text("The update is installed. Restart LobbyLocker to use it.")
                    .to_string()
            }
        }
        phase => {
            if let Some(notice) = &s.update_notice {
                let version = match notice {
                    UpdateNotice::Preview(version) => version.clone(),
                    UpdateNotice::Release(release) => release.version().to_string(),
                };
                action = Some(
                    Button::new(t.text("Install update"))
                        .disabled(
                            matches!(notice, UpdateNotice::Preview(_))
                                || s.busy.is_some()
                                || s.fw_latest.is_pending()
                                || s.scan_latest.is_pending(),
                        )
                        .on_press(install)
                        .build(k)
                        .key("install-update"),
                );
                if let Phase::Failed(error) = phase {
                    format!("{}\n{error}", t.text("Could not install the update."))
                } else {
                    t.format(
                        "LobbyLocker v{version} is available.",
                        &[("version", &version)],
                    )
                }
            } else if let Phase::Failed(error) = phase {
                action = Some(Button::new(t.text("Retry")).on_press(check).build(k));
                format!("{}\n{error}", t.text("Could not check for updates."))
            } else {
                t.text("LobbyLocker is up to date.").to_string()
            }
        }
    };
    Dialog::new("software-update", t.text("Update"))
        .open(true)
        .fit()
        .width(s.window_size.0.min(420.0) - 40.0)
        .click_outside(!installing)
        .on_close(|s: &mut AppState| {
            if !matches!(s.updates.phase, Phase::Installing) {
                s.updates.open = false;
            }
        })
        .child(
            col()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .child(
                    text(message)
                        .w_full()
                        .wrap()
                        .color(k.c.text_muted)
                        .key("update-status"),
                )
                .child(
                    row()
                        .w_full()
                        .justify_end()
                        .gap(8.0)
                        .child(
                            Button::new(t.text("Close"))
                                .variant(ButtonVariant::Ghost)
                                .disabled(installing)
                                .on_press(|s: &mut AppState| s.updates.open = false)
                                .build(k)
                                .key("close-update"),
                        )
                        .children(action),
                ),
        )
        .build(k, layer);
}
