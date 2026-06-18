use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::model::Model;
use crate::config::profile::GeoRegion;
use crate::ui::styles::Theme;

/// Widget that renders the bottom status bar.
pub struct StatusBar<'a> {
    model: &'a Model,
}

impl<'a> StatusBar<'a> {
    pub fn new(model: &'a Model) -> Self {
        Self { model }
    }
}

/// Truncate a string to fit within a visual width, appending "..." if truncated.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    if max_width <= 3 {
        return ".".repeat(max_width);
    }
    let mut result = String::with_capacity(max_width);
    for (len, ch) in s.chars().enumerate() {
        if len + 1 > max_width.saturating_sub(3) {
            result.push_str("...");
            return result;
        }
        result.push(ch);
    }
    result
}

impl<'a> Widget for StatusBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (status, style) = match self.model.connection {
            crate::app::model::ConnectionState::Connected => ("[CONNECTED]", Theme::success()),
            crate::app::model::ConnectionState::ConnectPending => ("[CONNECTING]", Theme::status()),
            _ => ("[DISCONNECTED]", Theme::error()),
        };

        let routing = format!(
            "[{}]",
            self.model.config.settings.geo_routing.mode().as_str()
        );

        let is_global =
            self.model.config.settings.geo_routing.current_region == Some(GeoRegion::Global);

        let mut spans = vec![Span::styled(status, style)];

        if self.model.config.settings.auto_connect {
            spans.push(Span::raw(" "));
            spans.push(Span::styled("[AUTO]", Theme::accent()));
        }

        if self.model.config.settings.kill_switch {
            spans.push(Span::raw(" "));
            spans.push(Span::styled("[KS]", Theme::accent()));
        }

        let dns = &self.model.config.settings.dns;
        let dns_label = if dns.fakeip_enabled {
            "fakeip".to_string()
        } else {
            dns.final_server_entry()
                .map(|s| s.kind_label().to_string())
                .unwrap_or_else(|| "?".to_string())
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("[DNS: {}]", dns_label),
            Theme::accent(),
        ));

        spans.push(Span::raw(" "));
        spans.push(Span::styled(routing, Theme::accent()));

        if !is_global {
            let geo_info = if self.model.geo_updating {
                "[Geo: updating...]".to_string()
            } else {
                match self.model.geo_last_updated {
                    Some(ref dt) => format!("[Geo: {}]", dt),
                    None => "[Geo: never]".to_string(),
                }
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(geo_info, Theme::accent()));
        }

        spans.push(Span::raw(" "));

        let fixed_width: usize = spans.iter().map(|s| s.content.len()).sum();
        let available = area.width as usize;
        let status_text = self.model.status.text();
        let max_status = available.saturating_sub(fixed_width);
        let truncated = truncate_to_width(status_text, max_status);

        spans.push(Span::styled(truncated, Theme::normal()));

        let text = Line::from(spans);

        let paragraph = Paragraph::new(text).alignment(Alignment::Left);

        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::model::ConnectionState;
    use crate::config::profile::{Profile, Protocol};
    use crate::test_helpers::{buffer_to_string, model_with_profiles};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn status_bar_shows_disconnected() {
        let model = model_with_profiles(vec![]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("[DISCONNECTED]"));
        assert!(content.contains("[Global]"));
        // Verify red color for disconnected
        let idx = content.find('[').unwrap();
        assert_eq!(
            buf.content[idx].style().fg,
            Some(ratatui::style::Color::Red)
        );
    }

    #[test]
    fn status_bar_connect_pending_shows_connecting() {
        use crate::app::model::ConnectionState;
        let mut model = model_with_profiles(vec![]);
        model.connection = ConnectionState::ConnectPending;
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("[CONNECTING]"));
        // Verify yellow color for connecting
        let idx = content.find('[').unwrap();
        assert_eq!(
            buf.content[idx].style().fg,
            Some(ratatui::style::Color::Yellow)
        );
    }

    #[test]
    fn status_bar_connected_snapshot() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Alpha".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::Connected;
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn status_bar_connected_auto_snapshot() {
        let mut model = model_with_profiles(vec![Profile::new(
            "Alpha".to_string(),
            Protocol::Vless,
            "1.1.1.1".to_string(),
            443,
            "u1".to_string(),
        )]);
        model.connection = ConnectionState::Connected;
        model.config.settings.auto_connect = true;
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn status_bar_geo_updating_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_updating = true;
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }

    #[test]
    fn status_bar_global_region_hides_geo_info() {
        let mut model = model_with_profiles(vec![]);
        model
            .config
            .settings
            .geo_routing
            .set_region(GeoRegion::Global);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        let content: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("[DISCONNECTED]"));
        assert!(content.contains("[Global]"));
        assert!(!content.contains("[Geo:"));
    }

    #[test]
    fn status_bar_long_message_truncated_snapshot() {
        let mut model = model_with_profiles(vec![]);
        model.geo_last_updated = Some("2026-05-31 13:41".to_string());
        model.status = crate::app::model::AppStatus::Error(
            "Connection failed: sing-box exited immediately (code: Some(1)). stderr: FATAL[0000] create service: parse outbound[0].server_settings.address: lookup example.com: no such host".to_string(),
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
        StatusBar::new(&model).render(Rect::new(0, 0, 80, 1), &mut buf);
        insta::assert_snapshot!(buffer_to_string(&buf));
    }
}
