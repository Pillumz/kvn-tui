use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap};
use unicode_width::UnicodeWidthChar;

use crate::app::model::{Model, Overlay, SourceRow};
use crate::ui::styles::Theme;
use crate::ui::widgets::StatusBar;

/// Render the full application UI into the terminal frame.
pub fn draw(frame: &mut Frame, model: &Model) {
    let area = frame.area();

    // Main vertical layout: content + status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    draw_main(frame, model, chunks[0]);
    draw_status_bar(frame, model, chunks[1]);

    match model.overlay {
        Overlay::Help => draw_help(frame, area),
        Overlay::ConfirmDelete => draw_confirm_delete(frame, model, area),
        Overlay::Error => draw_error(frame, area, model.status.text()),
        Overlay::RoutingMode => draw_routing_mode(frame, model, area),
        Overlay::GeoRegions => draw_geo_region(frame, model, area),
        Overlay::None => {}
    }
}

/// Draw the main content area with the Sources list and logs.
fn draw_main(frame: &mut Frame, model: &Model, area: Rect) {
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_sources(frame, model, content_chunks[0]);

    let log_block = Block::default()
        .title(" Logs ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    // Show the most recent log lines that fit in the available area.
    let available_height = content_chunks[1].height.saturating_sub(2) as usize;
    let start = model.logs.len().saturating_sub(available_height);
    let log_text: Vec<Line> = model
        .logs
        .iter()
        .skip(start)
        .map(|l| {
            let style = if l.starts_with("[error]") {
                Theme::error()
            } else {
                Theme::normal()
            };
            Line::from(Span::styled(l.as_str(), style))
        })
        .collect();

    let logs = Paragraph::new(log_text)
        .block(log_block)
        .wrap(Wrap { trim: true });
    frame.render_widget(logs, content_chunks[1]);
}

/// Draw the bottom status bar.
fn draw_status_bar(frame: &mut Frame, model: &Model, area: Rect) {
    let status = StatusBar::new(model);
    frame.render_widget(status, area);
}

/// Draw the help popup overlay.
fn draw_help(frame: &mut Frame, area: Rect) {
    let header =
        Row::new(vec!["Key", "Action"]).style(Theme::accent().add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = vec![
        Row::new(vec!["j / Down", "Move down"]),
        Row::new(vec!["k / Up", "Move up"]),
        Row::new(vec!["g", "Go to first"]),
        Row::new(vec!["G", "Go to last"]),
        Row::new(vec!["Enter", "Connect to selected profile"]),
        Row::new(vec!["p", "Paste from clipboard"]),
        Row::new(vec!["d", "Delete selected source"]),
        Row::new(vec!["m", "Routing mode (popup list)"]),
        Row::new(vec!["o", "Geo region"]),
        Row::new(vec!["u", "Update subscription or geo"]),
        Row::new(vec!["i", "Cycle subscription auto-update"]),
        Row::new(vec!["e", "Open profiles.json in $EDITOR"]),
        Row::new(vec!["a", "Toggle auto-connect"]),
        Row::new(vec!["r", "Reconnect"]),
        Row::new(vec!["s", "Stop / disconnect"]),
        Row::new(vec!["q / Esc", "Detach TUI"]),
        Row::new(vec!["Ctrl+C", "Quit"]),
        Row::new(vec!["?", "Show this help"]),
    ];

    let needed = rows.len() as u16 + 1 + 2 + 1; // data rows + header + borders + padding
    let percent = ((needed * 100) / area.height).clamp(50, 90);
    let popup_area = centered_rect(POPUP_WIDTH_PERCENT, percent, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .style(Theme::popup_bg());

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let table = Table::new(rows, [Constraint::Length(12), Constraint::Min(1)]).header(header);

    frame.render_widget(table, inner);
}

/// Draw the delete confirmation dialog.
fn draw_confirm_delete(frame: &mut Frame, model: &Model, area: Rect) {
    use crate::app::model::SourceRow;
    let message = match model.selected_row() {
        Some(SourceRow::SubscriptionHeader(_)) => "Delete selected subscription and its profiles?",
        _ => "Delete selected profile?",
    };
    draw_modal(
        frame,
        area,
        " Confirm ",
        vec![
            Line::from(Span::styled(message, Theme::error())),
            Line::from(""),
            Line::from("Press y to confirm, n to cancel"),
        ],
    );
}

/// Draw an error message popup.
fn draw_error(frame: &mut Frame, area: Rect, message: &str) {
    draw_modal(
        frame,
        area,
        " Error ",
        vec![
            Line::from(Span::styled("Error", Theme::error())),
            Line::from(""),
            Line::from(message),
            Line::from(""),
            Line::from("Press any key to dismiss"),
        ],
    );
}

const POPUP_WIDTH_PERCENT: u16 = 60;
const POPUP_HEIGHT_PERCENT: u16 = 50;

/// Helper to render a centered popup with a border and text.
fn draw_modal(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line>) {
    let popup_area = centered_rect(POPUP_WIDTH_PERCENT, POPUP_HEIGHT_PERCENT, area);

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Theme::border())
        .style(Theme::popup_bg());

    let paragraph = Paragraph::new(lines)
        .block(block)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, popup_area);
}

/// Draw the routing mode selection modal.
fn draw_routing_mode(frame: &mut Frame, model: &Model, area: Rect) {
    let modes = model.config.settings.geo_routing.available_modes();
    let labels: Vec<&str> = modes.iter().map(|m| m.as_str()).collect();
    draw_selection_modal(
        frame,
        area,
        "Select routing mode",
        " Routing Mode ",
        &labels,
        model.routing_selected,
    );
}

/// Draw the geo region selection modal.
fn draw_geo_region(frame: &mut Frame, model: &Model, area: Rect) {
    let labels = [
        "🇷🇺 Russia",
        "🇨🇳 China",
        "🇮🇷 Iran",
        "🌍 Global (no geo rules)",
    ];
    draw_selection_modal(
        frame,
        area,
        "Select geo region",
        " Geo Region ",
        &labels,
        model.geo_region_selected,
    );
}

fn draw_selection_modal(
    frame: &mut Frame,
    area: Rect,
    heading: &str,
    modal_title: &str,
    items: &[&str],
    selected: usize,
) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(heading, Theme::accent())),
        Line::from(""),
    ];
    for (i, label) in items.iter().enumerate() {
        let marker = if i == selected { "> " } else { "  " };
        let style = if i == selected {
            Theme::accent().add_modifier(Modifier::BOLD)
        } else {
            Theme::normal()
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", marker, label),
            style,
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("j/k navigate, Enter confirm, Esc cancel"));
    draw_modal(frame, area, modal_title, lines);
}

/// Draw the unified Sources list: standalone profiles and subscription trees.
fn draw_sources(frame: &mut Frame, model: &Model, area: Rect) {
    let block = Block::default()
        .title(" Sources ")
        .borders(Borders::ALL)
        .border_style(Theme::border());

    let inner_width = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let rows = model.source_rows();

    if rows.is_empty() {
        lines.push(Line::from(
            "No sources. Press p to paste a profile or subscription URL from clipboard.",
        ));
    } else {
        // Single pass over rows to collect indices — avoids O(n²) position() searches.
        let sub_count = model.config.subscriptions.len();
        let mut standalone: Vec<(usize, usize)> = Vec::new(); // (row_idx, profile_idx)
        let mut sub_header_idx = vec![0usize; sub_count];
        let mut sub_profile_rows: Vec<Vec<(usize, usize)>> = vec![Vec::new(); sub_count];
        for (i, row) in rows.iter().enumerate() {
            match row {
                SourceRow::StandaloneProfile(idx) => standalone.push((i, *idx)),
                SourceRow::SubscriptionHeader(idx) => sub_header_idx[*idx] = i,
                SourceRow::SubscriptionProfile {
                    sub_idx,
                    profile_idx,
                } => {
                    sub_profile_rows[*sub_idx].push((i, *profile_idx));
                }
            }
        }

        // Standalone profiles group.
        if !standalone.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Standalone profiles ({})", standalone.len()),
                Theme::accent().add_modifier(Modifier::BOLD),
            )));
            let last = standalone.len() - 1;
            for (pos, (row_idx, profile_idx)) in standalone.iter().enumerate() {
                lines.push(profile_line(
                    model,
                    *profile_idx,
                    *row_idx,
                    pos == last,
                    inner_width,
                ));
            }
            lines.push(Line::from(""));
        }

        // Subscription groups.
        for (sub_idx, sub) in model.config.subscriptions.iter().enumerate() {
            let header_idx = sub_header_idx[sub_idx];
            let is_selected = model.selected == header_idx;
            let header_style = if is_selected {
                Theme::selected()
            } else {
                Theme::normal()
            };
            let profiles = &sub_profile_rows[sub_idx];
            let header_text = format!(
                "Subscription: {} ({}) [{}]",
                sub.name,
                profiles.len(),
                sub.auto_update.label(),
            );
            let header_text = if is_selected {
                pad_to_visual_width(&header_text, inner_width)
            } else {
                header_text
            };
            lines.push(Line::from(Span::styled(header_text, header_style)));

            let last = profiles.len().saturating_sub(1);
            for (pos, (row_idx, profile_idx)) in profiles.iter().enumerate() {
                lines.push(profile_line(
                    model,
                    *profile_idx,
                    *row_idx,
                    pos == last,
                    inner_width,
                ));
            }
            lines.push(Line::from(""));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Visual width of a string, counting Unicode characters according to their
/// display width.
fn visual_width(s: &str) -> usize {
    s.chars().filter_map(UnicodeWidthChar::width).sum()
}

/// Truncate a string to fit within a visual width, appending "..." if truncated.
fn truncate_to_visual_width(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let total_width = visual_width(s);
    if total_width <= max_width {
        return s.to_string();
    }
    let mut result = String::new();
    let mut width = 0;
    let limit = max_width.saturating_sub(3);
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > limit {
            result.push_str("...");
            return result;
        }
        result.push(ch);
        width += cw;
    }
    result
}

/// Pad a string on the right with spaces up to the target visual width.
fn pad_to_visual_width(s: &str, target: usize) -> String {
    let mut result = s.to_string();
    let width = visual_width(s);
    if width < target {
        result.push_str(&" ".repeat(target - width));
    }
    result
}

/// Truncate and pad a string to exactly the target visual width.
fn fit_to_visual_width(s: &str, target: usize) -> String {
    pad_to_visual_width(&truncate_to_visual_width(s, target), target)
}

/// Width reserved for the tree prefix at the start of a profile row.
const PREFIX_WIDTH: usize = 2;
/// Minimum width for the profile name column.
const MIN_NAME_WIDTH: usize = 8;
/// Width reserved for the protocol column.
const PROTOCOL_WIDTH: usize = 6;
/// Minimum width for the address:port column.
const MIN_ADDR_WIDTH: usize = 11;
/// Fixed overhead: prefix + protocol + two spaces between columns.
const FIXED_OVERHEAD: usize = PREFIX_WIDTH + PROTOCOL_WIDTH + 2;

/// Build one profile row for the Sources list.
fn profile_line(
    model: &Model,
    profile_idx: usize,
    row_idx: usize,
    is_last: bool,
    inner_width: usize,
) -> Line<'static> {
    use ratatui::text::Span;

    let profile = &model.config.profiles[profile_idx];
    let is_selected = model.selected == row_idx;

    let is_connected = model.active_profile_id == Some(profile.id);

    let prefix = if is_last { "└ " } else { "├ " };

    let addr_port = format!("{}:{}", profile.address, profile.port);
    let remaining = inner_width.saturating_sub(FIXED_OVERHEAD);
    let ideal_addr = visual_width(&addr_port).max(MIN_ADDR_WIDTH);
    let addr_width = ideal_addr.min(remaining.saturating_sub(MIN_NAME_WIDTH));
    let name_width = remaining.saturating_sub(addr_width).max(MIN_NAME_WIDTH);

    let name_col = fit_to_visual_width(&profile.name, name_width);
    let protocol_col = fit_to_visual_width(&profile.protocol.to_string(), PROTOCOL_WIDTH);
    let addr_col = truncate_to_visual_width(&addr_port, addr_width);

    let style = if is_selected && is_connected {
        Theme::selected_connected()
    } else if is_selected {
        Theme::selected()
    } else if is_connected {
        Theme::success()
    } else {
        Theme::normal()
    };

    let addr_col_padded = if is_selected {
        pad_to_visual_width(&addr_col, addr_width)
    } else {
        addr_col
    };

    let used = PREFIX_WIDTH + name_width + 1 + PROTOCOL_WIDTH + 1 + visual_width(&addr_col_padded);
    let trailing = inner_width.saturating_sub(used);

    let mut spans = vec![
        Span::styled(prefix, style),
        Span::styled(name_col, style),
        Span::styled(" ", style),
        Span::styled(protocol_col, style),
        Span::styled(" ", style),
        Span::styled(addr_col_padded, style),
    ];
    if is_selected && trailing > 0 {
        spans.push(Span::styled(" ".repeat(trailing), style));
    }
    Line::from(spans)
}

/// Compute a centered rectangle with given percentage sizes.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::model::{ConnectionState, Overlay};
    use crate::config::profile::{Profile, Protocol};
    use crate::test_helpers::{buffer_to_string, model_with_profiles};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot_terminal(model: &Model, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let frame = terminal.draw(|f| draw(f, model)).unwrap();
        buffer_to_string(frame.buffer)
    }

    #[test]
    fn centered_rect_60_50_in_100_100() {
        let area = Rect::new(0, 0, 100, 100);
        let popup = centered_rect(POPUP_WIDTH_PERCENT, POPUP_HEIGHT_PERCENT, area);
        assert_eq!(popup.x, 20);
        assert_eq!(popup.y, 25);
        assert_eq!(popup.width, 60);
        assert_eq!(popup.height, 50);
    }

    #[test]
    fn centered_rect_100_100_fills_area() {
        let area = Rect::new(10, 20, 80, 40);
        let popup = centered_rect(100, 100, area);
        assert_eq!(popup.x, 10);
        assert_eq!(popup.y, 20);
        assert_eq!(popup.width, 80);
        assert_eq!(popup.height, 40);
    }

    #[test]
    fn centered_rect_zero_area() {
        let area = Rect::new(0, 0, 0, 0);
        let popup = centered_rect(50, 50, area);
        assert_eq!(popup.width, 0);
        assert_eq!(popup.height, 0);
    }

    #[test]
    fn help_renders_commands() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(80, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let frame = terminal
            .draw(|frame| {
                let area = frame.area();
                draw_help(frame, area);
            })
            .unwrap();

        let content: String = frame.buffer.content.iter().map(|c| c.symbol()).collect();
        let expected = [
            ("j / Down", "Move down"),
            ("k / Up", "Move up"),
            ("g", "Go to first"),
            ("G", "Go to last"),
            ("Enter", "Connect to selected profile"),
            ("p", "Paste from clipboard"),
            ("d", "Delete selected source"),
            ("m", "Routing mode (popup list)"),
            ("u", "Update subscription or geo"),
            ("i", "Cycle subscription auto-update"),
            ("e", "Open profiles.json in $EDITOR"),
            ("r", "Reconnect"),
            ("s", "Stop / disconnect"),
            ("q / Esc", "Detach TUI"),
            ("Ctrl+C", "Quit"),
            ("?", "Show this help"),
        ];
        for (key, action) in expected {
            assert!(content.contains(key), "help should contain key: {}", key);
            assert!(
                content.contains(action),
                "help should contain action: {}",
                action
            );
        }
        assert!(content.contains("Help"), "should contain Help title");
    }

    #[test]
    fn draw_main_snapshot() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Alpha".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.logs.push_back("log line 1".to_string());
        model.logs.push_back("log line 2".to_string());
        model.connection = ConnectionState::Connected;
        model.active_profile_id = Some(model.config.profiles[0].id);
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_help_overlay_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.overlay = Overlay::Help;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 40));
    }

    #[test]
    fn draw_confirm_delete_overlay_snapshot() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Alpha".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.overlay = Overlay::ConfirmDelete;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_error_overlay_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.overlay = Overlay::Error;
        model.status = crate::app::model::AppStatus::Error("something went wrong".to_string());
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_routing_mode_overlay_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model
            .config
            .settings
            .geo_routing
            .set_region(crate::config::profile::GeoRegion::Ru);
        model.overlay = Overlay::RoutingMode;
        model.routing_selected = 2;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_geo_region_overlay_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.overlay = Overlay::GeoRegions;
        model.geo_region_selected = 1;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_sources_snapshot() {
        use crate::config::profile::{Subscription, SubscriptionAutoUpdate};
        use uuid::Uuid;

        let sub_id = Uuid::new_v4();
        let profiles = vec![
            Profile::new(
                "Alpha".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "Beta".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
            {
                let mut p = Profile::new(
                    "Gamma".to_string(),
                    Protocol::Vless,
                    "3.3.3.3".to_string(),
                    443,
                    "u3".to_string(),
                );
                p.subscription_id = Some(sub_id);
                p
            },
            {
                let mut p = Profile::new(
                    "Delta".to_string(),
                    Protocol::Vless,
                    "4.4.4.4".to_string(),
                    443,
                    "u4".to_string(),
                );
                p.subscription_id = Some(sub_id);
                p
            },
        ];
        let mut model = model_with_profiles(profiles);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.config.subscriptions.push(Subscription {
            id: sub_id,
            name: "Example".to_string(),
            url: "http://example.com/sub".to_string(),
            auto_update: SubscriptionAutoUpdate::Every1h,
            last_updated: None,
        });
        model.selected = 0;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_sources_long_name_truncated() {
        let profiles = vec![
            Profile::new(
                "VeryLongProfileNameThatMustBeTruncated".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "Second".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ];
        let mut model = model_with_profiles(profiles);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.selected = 0;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }

    #[test]
    fn draw_sources_connected_profile_colored() {
        let mut model = model_with_profiles(vec![
            Profile::new(
                "Alpha".to_string(),
                Protocol::Vless,
                "1.1.1.1".to_string(),
                443,
                "u1".to_string(),
            ),
            Profile::new(
                "Beta".to_string(),
                Protocol::Vless,
                "2.2.2.2".to_string(),
                443,
                "u2".to_string(),
            ),
        ]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.connection = ConnectionState::Connected;
        model.active_profile_id = Some(model.config.profiles[1].id);
        model.selected = 1;
        insta::assert_snapshot!(snapshot_terminal(&model, 80, 20));
    }
}
