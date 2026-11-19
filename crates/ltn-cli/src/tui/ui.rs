use super::app::{App, AppMode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

const COLOR_CYAN: Color = Color::Rgb(122, 162, 247);
const COLOR_PURPLE: Color = Color::Rgb(187, 154, 247);
const COLOR_GREEN: Color = Color::Rgb(158, 206, 106);
const COLOR_YELLOW: Color = Color::Rgb(224, 175, 104);
const COLOR_RED: Color = Color::Rgb(247, 118, 142);
const COLOR_MUTED: Color = Color::Rgb(86, 95, 137);

fn render_bar(pct: f64, width: usize) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * (width as f64)).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header Bar
            Constraint::Min(8),    // Main AGY Stream Timeline & Sidebar
            Constraint::Length(3), // Floating Prompt Bar
        ])
        .split(f.size());

    // 1. Header Bar with 🌩️ Logo & Status Badges
    let header_text = vec![
        Span::styled(
            "🌩️  L T N . E N G ",
            Style::default()
                .fg(COLOR_YELLOW)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "⚡ ADE ",
            Style::default()
                .fg(COLOR_PURPLE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled("Target: ", Style::default().fg(COLOR_MUTED)),
        Span::styled(
            &app.provider,
            Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(
            if app.authenticated {
                "● READY"
            } else {
                "○ AUTH PENDING"
            },
            Style::default()
                .fg(if app.authenticated {
                    COLOR_GREEN
                } else {
                    COLOR_YELLOW
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" │ "),
        Span::styled(
            format!("Status: {}", app.status_message),
            Style::default().fg(COLOR_MUTED),
        ),
    ];
    let header = Paragraph::new(Line::from(header_text)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ⚡ Autonomous Distributed Infrastructure Engine ")
            .border_style(Style::default().fg(COLOR_CYAN)),
    );
    f.render_widget(header, chunks[0]);

    // 2. Main Content Split (Sidebar + Timeline Stream)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[1]);

    // Sidebar: Real Directory Entries & Live Hardware Feedback Loop Metrics
    let cwd_display = std::env::current_dir()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|_| "workspace".to_string());

    let mut sidebar_items = vec![ListItem::new(Span::styled(
        format!("📁 Workspace: /{}", cwd_display),
        Style::default()
            .fg(COLOR_YELLOW)
            .add_modifier(Modifier::BOLD),
    ))];

    if let Ok(entries) = std::fs::read_dir(".") {
        let mut count = 0;
        for entry in entries.flatten() {
            if count >= 4 {
                sidebar_items.push(ListItem::new(Span::styled(
                    " └── ...",
                    Style::default().fg(COLOR_MUTED),
                )));
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" {
                continue;
            }
            let icon = if entry.path().is_dir() {
                "📁"
            } else {
                "📄"
            };
            sidebar_items.push(ListItem::new(Span::raw(format!(" ├── {} {}", icon, name))));
            count += 1;
        }
    }

    sidebar_items.push(ListItem::new(Span::raw("")));
    sidebar_items.push(ListItem::new(Span::styled(
        "⚡ Live Host Telemetry",
        Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
    )));

    if let Some(m) = app.feedback_loop.latest() {
        let cpu_color = if m.cpu_usage_pct > 80.0 {
            COLOR_RED
        } else if m.cpu_usage_pct > 50.0 {
            COLOR_YELLOW
        } else {
            COLOR_GREEN
        };
        let cpu_bar = render_bar(m.cpu_usage_pct as f64, 8);
        sidebar_items.push(ListItem::new(Line::from(vec![
            Span::raw(" CPU: "),
            Span::styled(cpu_bar, Style::default().fg(cpu_color)),
            Span::styled(
                format!(" {:.1}%", m.cpu_usage_pct),
                Style::default().fg(cpu_color).add_modifier(Modifier::BOLD),
            ),
        ])));

        let ram_pct = (m.ram_used_mb as f64 / 32768.0 * 100.0).clamp(0.0, 100.0);
        let ram_bar = render_bar(ram_pct, 8);
        sidebar_items.push(ListItem::new(Line::from(vec![
            Span::raw(" RAM: "),
            Span::styled(ram_bar, Style::default().fg(COLOR_PURPLE)),
            Span::styled(
                format!(" {}MB", m.ram_used_mb),
                Style::default()
                    .fg(COLOR_PURPLE)
                    .add_modifier(Modifier::BOLD),
            ),
        ])));
        sidebar_items.push(ListItem::new(Line::from(vec![
            Span::raw(" Engine: "),
            Span::styled(m.status.clone(), Style::default().fg(COLOR_GREEN)),
        ])));
    }

    sidebar_items.push(ListItem::new(Span::raw("")));
    sidebar_items.push(ListItem::new(Span::styled(
        "⌨️ Key Commands",
        Style::default()
            .fg(COLOR_PURPLE)
            .add_modifier(Modifier::BOLD),
    )));
    sidebar_items.push(ListItem::new(Span::styled(
        " /analyze [path]   Profile Workload",
        Style::default().fg(COLOR_MUTED),
    )));
    sidebar_items.push(ListItem::new(Span::styled(
        " /architect [path] 3-Tier Blueprint",
        Style::default().fg(COLOR_MUTED),
    )));
    sidebar_items.push(ListItem::new(Span::styled(
        " /explain [path]   CTO System Design",
        Style::default().fg(COLOR_MUTED),
    )));
    sidebar_items.push(ListItem::new(Span::styled(
        " /deploy [path]    5-Stage Lifecycle",
        Style::default().fg(COLOR_MUTED),
    )));
    sidebar_items.push(ListItem::new(Span::styled(
        " /doctor           Diagnostics",
        Style::default().fg(COLOR_MUTED),
    )));

    let sidebar = List::new(sidebar_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Context & Telemetry ")
            .border_style(Style::default().fg(COLOR_PURPLE)),
    );
    f.render_widget(sidebar, main_chunks[0]);

    // Timeline Stream: Scrollable AGY Cards & Blueprint Proposals
    let mut timeline_items = Vec::new();
    let visible_cards = app.timeline.iter().skip(app.scroll_offset);

    for card in visible_cards {
        let style = match card.card_type.as_str() {
            "ACTION" => Style::default().fg(COLOR_GREEN),
            "DIFF" => Style::default().fg(COLOR_PURPLE),
            _ => Style::default().fg(COLOR_CYAN),
        };
        timeline_items.push(ListItem::new(vec![
            Line::from(Span::styled(
                &card.title,
                style.add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw(&card.body)),
            Line::from(Span::styled(
                "───────────────────────────────────────────────────",
                Style::default().fg(COLOR_MUTED),
            )),
        ]));
    }

    let timeline_title = format!(
        " 🌩️ Agent Timeline Stream [Card {}/{} | PgUp/PgDn to scroll] ",
        if app.timeline.is_empty() {
            0
        } else {
            app.scroll_offset + 1
        },
        app.timeline.len()
    );

    let timeline_widget = List::new(timeline_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(timeline_title)
            .border_style(Style::default().fg(COLOR_MUTED)),
    );
    f.render_widget(timeline_widget, main_chunks[1]);

    // 3. Floating Prompt Deck with 🌩️ ❯ Prefix & Command History Indicators
    let input_title = " ⚡ Command Prompt Deck [↑/↓ History | /help | /quit] ";
    let input_widget = Paragraph::new(format!("🌩️ ❯ {}", app.input)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(input_title)
            .border_style(Style::default().fg(COLOR_GREEN)),
    );
    f.render_widget(input_widget, chunks[2]);

    // 4. Overlays: Onboarding Modal Screen
    if app.mode == AppMode::Onboarding {
        let area = centered_rect(60, 50, f.size());
        f.render_widget(Clear, area);

        let onboarding_text = vec![
            Line::from(Span::styled(
                "🌩️ ltn.eng First-Run Cloud Onboarding",
                Style::default()
                    .fg(COLOR_YELLOW)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw(
                "Welcome to Lightning Engine ADE. Please set up your Cloud Provider.",
            )),
            Line::from(Span::raw("")),
            Line::from(Span::styled(
                "1. Cloud Provider: Local Docker / AWS / GCP / Azure",
                Style::default().fg(COLOR_CYAN),
            )),
            Line::from(Span::raw("2. Press [S] to open setup docs in browser.")),
            Line::from(Span::raw(
                "3. Press [Enter] to accept default credentials and start workspace.",
            )),
        ];

        let modal = Paragraph::new(onboarding_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Cloud Setup Wizard ")
                .border_style(Style::default().fg(COLOR_GREEN)),
        );
        f.render_widget(modal, area);
    }

    // 5. Overlays: Blueprint Review Box
    if app.mode == AppMode::BlueprintReview {
        let area = centered_rect(75, 55, f.size());
        f.render_widget(Clear, area);

        let plan_body = app
            .active_blueprint
            .as_deref()
            .unwrap_or("No blueprint active");
        let mut review_text = vec![
            Line::from(Span::styled(
                "🏛️ Proposed Multi-Cloud Architecture Options",
                Style::default()
                    .fg(COLOR_PURPLE)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::raw("")),
        ];

        for line in plan_body.lines() {
            review_text.push(Line::from(Span::raw(line)));
        }

        review_text.push(Line::from(Span::raw("")));
        review_text.push(Line::from(Span::styled(
            "[A] Accept & Apply Blueprint    [R] Reject Blueprint",
            Style::default()
                .fg(COLOR_GREEN)
                .add_modifier(Modifier::BOLD),
        )));

        let modal = Paragraph::new(review_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Architecture Review ")
                .border_style(Style::default().fg(COLOR_CYAN)),
        );
        f.render_widget(modal, area);
    }
}

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
