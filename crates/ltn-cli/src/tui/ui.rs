use super::app::{App, AppMode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

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
        Span::styled("🌩️ L T N . E N G ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled(format!("Provider: {}", app.provider), Style::default().fg(Color::Cyan)),
        Span::raw(" | "),
        Span::styled(
            if app.authenticated { "🟢 Connected" } else { "🔴 Credentials Required" },
            Style::default().fg(if app.authenticated { Color::Green } else { Color::Red }),
        ),
        Span::raw(" | "),
        Span::styled(format!("Status: {}", app.status_message), Style::default().fg(Color::Gray)),
    ];
    let header = Paragraph::new(Line::from(header_text))
        .block(Block::default().borders(Borders::ALL).title(" ⚡ Lightning Workload ADE ").border_style(Style::default().fg(Color::Yellow)));
    f.render_widget(header, chunks[0]);

    // 2. Main Content Split (Sidebar + Timeline Stream)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[1]);

    // Sidebar: Workspace & Hardware Feedback Loop Metrics
    let mut sidebar_items = vec![
        ListItem::new(Span::styled("📁 Workspace: /ltn-workspace", Style::default().add_modifier(Modifier::BOLD))),
        ListItem::new(Span::raw(" ├── 🐍 main.py")),
        ListItem::new(Span::raw(" ├── ⚙️ config.toml")),
        ListItem::new(Span::raw(" └── 🐳 Dockerfile")),
        ListItem::new(Span::raw("")),
        ListItem::new(Span::styled("⚡ Feedback Loop Metrics", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
    ];

    if let Some(m) = app.feedback_loop.latest() {
        sidebar_items.push(ListItem::new(Span::raw(format!(" CPU: {:.1}%", m.cpu_usage_pct))));
        sidebar_items.push(ListItem::new(Span::raw(format!(" RAM: {} MB", m.ram_used_mb))));
        sidebar_items.push(ListItem::new(Span::raw(format!(" GPU: {} MB VRAM", m.gpu_vram_mb))));
        sidebar_items.push(ListItem::new(Span::raw(format!(" Health: {}", m.status))));
    }

    let sidebar = List::new(sidebar_items)
        .block(Block::default().borders(Borders::ALL).title(" Context & Metrics ").border_style(Style::default().fg(Color::Blue)));
    f.render_widget(sidebar, main_chunks[0]);

    // Timeline Stream: AGY Cards & Blueprint Proposals
    let mut timeline_items = Vec::new();
    for card in &app.timeline {
        let style = match card.card_type.as_str() {
            "ACTION" => Style::default().fg(Color::Green),
            "DIFF" => Style::default().fg(Color::Magenta),
            _ => Style::default().fg(Color::LightBlue),
        };
        timeline_items.push(ListItem::new(vec![
            Line::from(Span::styled(&card.title, style.add_modifier(Modifier::BOLD))),
            Line::from(Span::raw(&card.body)),
            Line::from(Span::raw("──────────────")),
        ]));
    }

    let timeline_widget = List::new(timeline_items)
        .block(Block::default().borders(Borders::ALL).title(" 🌩️ AGY Agent Stream ").border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(timeline_widget, main_chunks[1]);

    // 3. Floating Prompt Deck with 🌩️ ❯ Prefix
    let input_widget = Paragraph::new(format!("🌩️ ❯ {}", app.input))
        .block(Block::default().borders(Borders::ALL).title(" Prompt Deck (/architect, /doctor, /login, /quit) ").border_style(Style::default().fg(Color::Magenta)));
    f.render_widget(input_widget, chunks[2]);

    // 4. Overlays: Onboarding Modal Screen
    if app.mode == AppMode::Onboarding {
        let area = centered_rect(60, 50, f.size());
        f.render_widget(Clear, area);

        let onboarding_text = vec![
            Line::from(Span::styled("🌩️ ltn.eng First-Run Cloud Onboarding", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
            Line::from(Span::raw("Welcome to Lightning Engine ADE. Please set up your Cloud Provider.")),
            Line::from(Span::raw("")),
            Line::from(Span::styled("1. Cloud Provider: Beam Cloud (https://platform.beam.cloud/signup)", Style::default().fg(Color::Cyan))),
            Line::from(Span::raw("2. Press [S] to open signup page in browser.")),
            Line::from(Span::raw("3. Press [Enter] to accept default local credentials and start.")),
        ];

        let modal = Paragraph::new(onboarding_text)
            .block(Block::default().borders(Borders::ALL).title(" Cloud Setup Wizard ").border_style(Style::default().fg(Color::Green)));
        f.render_widget(modal, area);
    }

    // 5. Overlays: Blueprint Review Box
    if app.mode == AppMode::BlueprintReview {
        let area = centered_rect(70, 40, f.size());
        f.render_widget(Clear, area);

        let plan_body = app.active_blueprint.as_deref().unwrap_or("No blueprint active");
        let review_text = vec![
            Line::from(Span::styled("🏛️ Proposed Architecture Blueprint", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))),
            Line::from(Span::raw(plan_body)),
            Line::from(Span::raw("")),
            Line::from(Span::styled("[A] Accept & Apply Plan    [R] Reject Plan", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        ];

        let modal = Paragraph::new(review_text)
            .block(Block::default().borders(Borders::ALL).title(" Architecture Review ").border_style(Style::default().fg(Color::Magenta)));
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
