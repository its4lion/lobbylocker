//! Connections page: an explicitly requested, read-only socket snapshot.

use crate::{icons, state::*};
use lobbylocker_core::{connections::Connection, i18n::Translator};
use rune::{col, el, row, text, Cx, El, ScrollSpec};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, TextInput};

pub fn connections_page(s: &AppState, _cx: &mut Cx<AppState>, k: &Kit, t: &Translator) -> El {
    let scanning = s.busy.as_deref() == Some("connections");
    let query = s.connections_search.text.trim().to_lowercase();
    let visible: Vec<_> = s
        .connections
        .iter()
        .enumerate()
        .filter(|(_, connection)| matches_filter(connection, s.connections_filter, &query))
        .collect();

    let scan = Button::new(t.text("Scan connections"))
        .variant(ButtonVariant::Outline)
        .icon(&icons::REFRESH)
        .disabled(s.busy.is_some())
        .on_press(|state: &mut AppState| {
            if state.busy.is_none() {
                state.busy = Some("connections".into());
                state.connections_error.clear();
                request_connections(state);
            }
        })
        .build(k)
        .key("connections-scan");

    let title = text(t.text("Connections"))
        .key("connections-title")
        .font(rune::TextStyle::new(20.0, 600.0))
        .color(k.c.text_base)
        .grow(1.0)
        .shrink();
    let heading_row = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .justify_between()
        .child(title)
        .child(scan);
    let heading = col().w_full().gap(6.0).child(heading_row).child(
        text(t.text("A snapshot of remote connections. Scan to refresh."))
            .font(rune::tok::type_::BODY)
            .color(k.c.text_muted)
            .w_full()
            .wrap(),
    );

    let notice = row()
        .w_full()
        .gap(8.0)
        .items_start()
        .child(
            rune::icon(&icons::INFO, 15.0)
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            text(t.text("Connections are not verified game servers. This view is read-only."))
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_muted)
                .grow(1.0)
                .min_w(0.0)
                .wrap(),
        );

    let search = TextInput::new(&s.connections_search)
        .placeholder(t.text("Search address, process, or port…"))
        .leading_icon(&icons::SEARCH)
        .leading_icon_size(14.0)
        .width(None)
        .on_edit(|state: &mut AppState| &mut state.connections_search)
        .build(k)
        .key("connections-search");
    let filters = row()
        .key("connections-filters")
        .role("radiogroup")
        .label(t.text("Protocol"))
        .focus_group()
        .focus_follows()
        .w_full()
        .gap(4.0)
        .center_y();
    let all = filter_button(s, k, t, ConnectionFilter::All, "All", "all");
    let tcp = filter_button(s, k, t, ConnectionFilter::Tcp, "TCP", "tcp");
    let udp = filter_button(s, k, t, ConnectionFilter::Udp, "UDP", "udp");
    let filters = filters.child(all).child(tcp).child(udp);

    let mut page = col()
        .key("connections-page")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .pt(20.0)
        .pb(16.0)
        .gap(14.0)
        .child(heading)
        .child(notice);
    if !s.connections_error.is_empty() {
        let title = if s.connections_scanned {
            "Scan failed. Showing the previous snapshot."
        } else {
            "Scan failed."
        };
        page = page.child(
            col()
                .key("connections-error")
                .w_full()
                .padding(12.0)
                .gap(4.0)
                .border(1.0, k.c.border_muted)
                .radius(8.0)
                .fill(k.c.layer_01)
                .child(
                    text(t.text(title))
                        .font(rune::tok::type_::BODY)
                        .color(k.c.text_base)
                        .w_full()
                        .wrap(),
                )
                .child(
                    text(s.connections_error.clone())
                        .font(rune::tok::type_::LABEL)
                        .color(k.c.text_muted)
                        .w_full()
                        .wrap(),
                ),
        );
    }
    page = page.child(col().w_full().gap(8.0).child(search).child(filters));

    let summary = if scanning {
        t.text("Scanning connections…").to_string()
    } else if s.connections_scanned {
        t.format(
            "{shown} of {total} connections",
            &[
                ("shown", &visible.len().to_string()),
                ("total", &s.connections.len().to_string()),
            ],
        )
    } else {
        t.text("No scan yet").to_string()
    };
    let mut list = col()
        .key("connections-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_edge_fade(16.0);
    if !s.connections_scanned && !s.connections_error.is_empty() {
        list = list.child(empty_message(
            k,
            t.text("Scan failed."),
            t.text("Scan again to refresh the snapshot."),
        ));
    } else if !s.connections_scanned && s.connections.is_empty() {
        list = list.child(empty_message(
            k,
            t.text("No scan yet"),
            t.text("Run a scan to see remote connections."),
        ));
    } else if s.connections.is_empty() {
        list = list.child(empty_message(
            k,
            t.text("No remote connections found"),
            t.text("Scan again to refresh the snapshot."),
        ));
    } else if visible.is_empty() {
        list = list.child(empty_message(
            k,
            t.text("No matching connections"),
            t.text("Try a different search or protocol."),
        ));
    } else {
        for (row_index, (index, connection)) in visible.into_iter().enumerate() {
            if row_index > 0 {
                list = list.child(el().w_full().h(1.0).fill(k.c.border_muted).a11y_hidden());
            }
            list = list.child(connection_row(index, connection, k, t));
        }
    }
    page.child(
        col()
            .key("connections-card")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .border(1.0, k.c.border_muted)
            .radius(10.0)
            .clip_rounded(10.0)
            .fill(k.c.layer_01)
            .child(
                row()
                    .key("connections-summary")
                    .w_full()
                    .px(16.0)
                    .py(12.0)
                    .center_y()
                    .child(
                        text(summary)
                            .font(rune::TextStyle::new(12.0, 500.0))
                            .color(k.c.text_muted),
                    ),
            )
            .child(el().w_full().h(1.0).fill(k.c.border_muted).a11y_hidden())
            .child(list),
    )
}

fn filter_button(
    s: &AppState,
    k: &Kit,
    t: &Translator,
    filter: ConnectionFilter,
    label: &str,
    key: &str,
) -> El {
    Button::new(t.text(label))
        .variant(if s.connections_filter == filter {
            ButtonVariant::Neutral
        } else {
            ButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Small)
        .on_press(move |state: &mut AppState| state.connections_filter = filter)
        .build(k)
        .key(format!("connections-filter-{key}"))
        .role("radio")
        .checked(s.connections_filter == filter)
}

fn empty_message(k: &Kit, title: &str, description: &str) -> El {
    col()
        .key("connections-empty")
        .w_full()
        .py(32.0)
        .px(16.0)
        .center()
        .gap(8.0)
        .child(
            rune::icon(&icons::ACTIVITY, 22.0)
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            text(title.to_string())
                .font(rune::TextStyle::new(13.0, 500.0))
                .color(k.c.text_base),
        )
        .child(
            text(description.to_string())
                .font(rune::tok::type_::LABEL)
                .color(k.c.text_muted)
                .wrap(),
        )
}

fn connection_row(index: usize, connection: &Connection, k: &Kit, t: &Translator) -> El {
    let process = process_name(&connection.process);
    let process = if process.is_empty() {
        t.text("Unknown process").to_string()
    } else {
        process
    };
    let remote = remote_address(connection);
    let state = match connection.state.to_ascii_uppercase().as_str() {
        "ESTAB" | "ESTABLISHED" => t.text("Established").to_string(),
        "UNCONN" => t.text("Unconnected").to_string(),
        _ if connection.state.is_empty() => t.text("Unknown").to_string(),
        _ => connection.state.clone(),
    };
    let protocol = connection.protocol.to_uppercase();
    row()
        .key(format!("connection-{index}"))
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .label(format!("{process}, {remote}, {protocol}, {state}"))
        .child(
            el().size(32.0, 32.0)
                .radius(8.0)
                .fill(k.c.layer_03)
                .center()
                .child(rune::icon(&icons::ACTIVITY, 16.0).color(k.c.icon_muted))
                .a11y_hidden(),
        )
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(
                    text(process)
                        .key(format!("connection-process-{index}"))
                        .font(rune::TextStyle::new(13.0, 500.0))
                        .color(k.c.text_base)
                        .truncate()
                        .help(connection.process.clone()),
                )
                .child(
                    text(remote.clone())
                        .key(format!("connection-remote-{index}"))
                        .font(rune::tok::type_::LABEL)
                        .color(k.c.text_muted)
                        .truncate()
                        .help(remote),
                ),
        )
        .child(
            col()
                .items_end()
                .gap(3.0)
                .child(
                    text(protocol)
                        .font(rune::TextStyle::new(11.0, 600.0))
                        .color(k.c.text_base),
                )
                .child(
                    text(state)
                        .font(rune::tok::type_::LABEL)
                        .color(k.c.text_muted),
                ),
        )
}

fn remote_address(connection: &Connection) -> String {
    if connection.remote_ip.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{}]:{}", connection.remote_ip, connection.remote_port)
    } else {
        format!("{}:{}", connection.remote_ip, connection.remote_port)
    }
}

/// Linux `ss` supplies process metadata; Windows supplies a process name.
fn process_name(raw: &str) -> String {
    let Some(mut rest) = raw.strip_prefix("users:(") else {
        return raw.trim().to_string();
    };
    let mut names = Vec::new();
    while let Some(start) = rest.find("(\"") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let name = &rest[..end];
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
        rest = &rest[end + 1..];
    }
    if names.is_empty() {
        raw.trim().to_string()
    } else {
        names.join(", ")
    }
}

fn matches_filter(connection: &Connection, filter: ConnectionFilter, query: &str) -> bool {
    let protocol_matches = match filter {
        ConnectionFilter::All => true,
        ConnectionFilter::Tcp => connection.protocol.eq_ignore_ascii_case("tcp"),
        ConnectionFilter::Udp => connection.protocol.eq_ignore_ascii_case("udp"),
    };
    protocol_matches
        && (query.is_empty()
            || [
                remote_address(connection),
                connection.remote_ip.clone(),
                connection.remote_port.to_string(),
                connection.process.clone(),
                process_name(&connection.process),
                connection.protocol.clone(),
                connection.state.clone(),
            ]
            .iter()
            .any(|value| value.to_lowercase().contains(query)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_process_metadata_and_ipv6_without_losing_search_data() {
        let connection = Connection {
            protocol: "tcp".into(),
            remote_ip: "2001:db8::5".into(),
            remote_port: 443,
            process: "users:((\"game\",pid=42,fd=3),(\"helper\",pid=43,fd=4))".into(),
            state: "ESTAB".into(),
        };
        assert_eq!(process_name(&connection.process), "game, helper");
        assert_eq!(process_name("game.exe"), "game.exe");
        assert_eq!(remote_address(&connection), "[2001:db8::5]:443");
        assert!(matches_filter(&connection, ConnectionFilter::Tcp, "pid=42"));
        assert!(matches_filter(&connection, ConnectionFilter::All, "helper"));
        assert!(!matches_filter(&connection, ConnectionFilter::Udp, ""));
    }
}
