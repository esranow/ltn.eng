pub mod app;
pub mod guardrails;
pub mod ui;

use app::{App, AppMode};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ltn_core::config::LtnSettings;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

pub fn run_tui(settings: &LtnSettings) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let is_authed = settings.provider.default_provider != "NONE";
    let mut app = App::new(is_authed, settings.provider.default_provider.clone());

    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("⚠️ TUI Exit error: {:?}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match app.mode {
                    AppMode::Onboarding => match key.code {
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            let _ = open::that("https://platform.beam.cloud/signup");
                            app.status_message = "Opened signup page in browser.".to_string();
                        }
                        KeyCode::Enter => {
                            app.mode = AppMode::MainWorkspace;
                            app.authenticated = true;
                            app.status_message = "Onboarding complete.".to_string();
                        }
                        KeyCode::Esc => {
                            return Ok(());
                        }
                        _ => {}
                    },
                    AppMode::BlueprintReview => match key.code {
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            app.accept_blueprint();
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            app.reject_blueprint();
                        }
                        KeyCode::Esc => {
                            app.mode = AppMode::MainWorkspace;
                        }
                        _ => {}
                    },
                    AppMode::MainWorkspace => match key.code {
                        KeyCode::Char(c) => {
                            app.input.push(c);
                        }
                        KeyCode::Backspace => {
                            app.input.pop();
                        }
                        KeyCode::Enter => {
                            if app.input == "/quit" || app.input == "exit" {
                                return Ok(());
                            }
                            app.submit_input();
                        }
                        KeyCode::Esc => {
                            return Ok(());
                        }
                        _ => {}
                    },
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
