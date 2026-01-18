use crate::analysis::{VadSnapshot, VadState};
use crate::ducking::{OutputStream, RestoreGuard};
use crate::logging::elogln;
use crate::ControlMode;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Terminal;
use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

thread_local! {
    static UI_TERMINAL: RefCell<Option<Terminal<CrosstermBackend<io::Stdout>>>> =
        const { RefCell::new(None) };
}

pub struct GuiModeGuard;

impl Drop for GuiModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(Show);
        UI_TERMINAL.with(|term| {
            let _ = term.borrow_mut().take();
        });
        let _ = stdout.execute(LeaveAlternateScreen);
    }
}

pub fn enter_gui_mode() -> io::Result<GuiModeGuard> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    let _ = stdout.execute(EnterAlternateScreen);
    let _ = stdout.execute(Clear(ClearType::All));
    let _ = stdout.execute(MoveTo(0, 0));
    let _ = stdout.execute(Hide);
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let _ = terminal.clear();
    UI_TERMINAL.with(|term| {
        *term.borrow_mut() = Some(terminal);
    });
    Ok(GuiModeGuard)
}

pub enum GuiSelectResult {
    Selected(usize),
    Refresh,
    Quit,
}

pub fn select_voice_source_gui(
    list: &[OutputStream],
    default_index: usize,
) -> io::Result<GuiSelectResult> {
    let mut cursor = default_index.min(list.len().saturating_sub(1));
    loop {
        render_gui_selection(list, cursor);
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if cursor + 1 < list.len() {
                            cursor += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if !list.is_empty() {
                            return Ok(GuiSelectResult::Selected(cursor));
                        }
                    }
                    KeyCode::Char('r') => return Ok(GuiSelectResult::Refresh),
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(GuiSelectResult::Quit),
                    _ => {}
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_gui_input(
    guard_t: &Rc<RefCell<Option<Arc<Mutex<RestoreGuard>>>>>,
    mode_t: &Rc<RefCell<ControlMode>>,
    vad_t: &Rc<RefCell<VadState>>,
    gui_log_t: &Rc<RefCell<Vec<String>>>,
    quit_flag_t: &Arc<AtomicBool>,
    threshold_live: &Rc<RefCell<f32>>,
    duck_factor_live: &Rc<RefCell<f32>>,
    hold_live: &Rc<RefCell<u64>>,
    gui_enabled: bool,
) {
    const SENS_STEP: f32 = 0.0025;
    const SENS_MIN: f32 = 0.0025;
    const SENS_MAX: f32 = 0.2;
    const DUCK_STEP_PCT: f32 = 5.0;
    const HOLD_STEP_MS: u64 = 50;
    const HOLD_MAX_MS: u64 = 2000;

    while event::poll(std::time::Duration::from_millis(0)).unwrap_or(false) {
        if let Ok(Event::Key(key)) = event::read() {
            elogln(
                gui_enabled,
                format!(
                    "key={:?} mods={:?} kind={:?}",
                    key.code, key.modifiers, key.kind
                ),
            );
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char(c) => {
                    if c == ' ' {
                        let mut mode = mode_t.borrow_mut();
                        if *mode == ControlMode::AutoVad {
                            *mode = ControlMode::ManualRestored;
                            if let Some(guard) = guard_t.borrow().as_ref() {
                                let mut guard = guard.lock().unwrap();
                                if guard.ducked {
                                    guard.restore();
                                }
                            }
                            gui_log_t.borrow_mut().push("mode -> ManualRestored".into());
                        } else {
                            // reset VAD
                            *mode = ControlMode::AutoVad;
                            {
                                let mut vad = vad_t.borrow_mut();
                                vad.voice_active = false;
                                vad.above_start = None;
                                vad.last_above = None;
                            }
                            if let Some(guard) = guard_t.borrow().as_ref() {
                                let mut guard = guard.lock().unwrap();
                                if guard.ducked {
                                    guard.restore();
                                }
                            }
                            gui_log_t.borrow_mut().push("mode -> AutoVad".into());
                        }
                        continue;
                    }
                    let lower = c.to_ascii_lowercase();
                    if lower == 'w' {
                        let mut thr = threshold_live.borrow_mut();
                        *thr = (*thr + SENS_STEP).clamp(SENS_MIN, SENS_MAX);
                        gui_log_t
                            .borrow_mut()
                            .push(format!("threshold -> {:.4}", *thr));
                    } else if lower == 's' {
                        let mut thr = threshold_live.borrow_mut();
                        *thr = (*thr - SENS_STEP).clamp(SENS_MIN, SENS_MAX);
                        gui_log_t
                            .borrow_mut()
                            .push(format!("threshold -> {:.4}", *thr));
                    } else if lower == 'a' {
                        let mut factor = duck_factor_live.borrow_mut();
                        let mut pct = (1.0 - *factor) * 100.0;
                        pct = (pct - DUCK_STEP_PCT).clamp(0.0, 100.0);
                        *factor = (1.0 - pct / 100.0).clamp(0.0, 1.0);
                        gui_log_t
                            .borrow_mut()
                            .push(format!("duck -> {pct:.0}%"));
                    } else if lower == 'd' {
                        let mut factor = duck_factor_live.borrow_mut();
                        let mut pct = (1.0 - *factor) * 100.0;
                        pct = (pct + DUCK_STEP_PCT).clamp(0.0, 100.0);
                        *factor = (1.0 - pct / 100.0).clamp(0.0, 1.0);
                        gui_log_t
                            .borrow_mut()
                            .push(format!("duck -> {pct:.0}%"));
                    } else if lower == 'q' {
                        let mut hold = hold_live.borrow_mut();
                        *hold = hold.saturating_sub(HOLD_STEP_MS);
                        gui_log_t.borrow_mut().push(format!("hold -> {} ms", *hold));
                    } else if lower == 'e' {
                        let mut hold = hold_live.borrow_mut();
                        *hold = (*hold + HOLD_STEP_MS).min(HOLD_MAX_MS);
                        gui_log_t.borrow_mut().push(format!("hold -> {} ms", *hold));
                    } else if lower == 'x' {
                        gui_log_t.borrow_mut().push("quit requested via gui".into());
                        quit_flag_t.store(true, Ordering::Relaxed);
                    }
                }
                KeyCode::Esc => {
                    gui_log_t.borrow_mut().push("quit requested via gui".into());
                    quit_flag_t.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    }
}

#[allow(
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]
pub fn render_gui(
    label: String,
    reason: String,
    mode: ControlMode,
    snapshot: &VadSnapshot,
    energy: f32,
    threshold_live: f32,
    duck_factor_live: f32,
    hold_ms: u64,
    log: &[String],
) {
    let _ = log;
    UI_TERMINAL.with(|term| {
        let mut term_ref = term.borrow_mut();
        let Some(terminal) = term_ref.as_mut() else {
            return;
        };
        let auto_on = mode == ControlMode::AutoVad;
        let voice_active = snapshot.voice_active;
        let ducking_on = snapshot.applied_duck;
        let level = (energy * 20.0).clamp(0.0, 1.0);
        let sens_fill = (1.0 - threshold_live).clamp(0.0, 1.0);
        let duck_fill = (1.0 - duck_factor_live).clamp(0.0, 1.0);
        let hold_fill = (hold_ms as f32 / 1000.0).clamp(0.0, 1.0);

        let _ = terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(6),
                    Constraint::Length(9),
                    Constraint::Length(3),
                ])
                .split(size);

            draw_status(f, chunks[0], &label, &reason, auto_on, ducking_on);
            draw_voice(f, chunks[1], level, voice_active);
            draw_controls(
                f,
                chunks[2],
                threshold_live,
                duck_factor_live,
                hold_ms,
                sens_fill,
                duck_fill,
                hold_fill,
            );
            draw_help(f, chunks[3]);
        });
    });
}

fn render_gui_selection(list: &[OutputStream], cursor: usize) {
    UI_TERMINAL.with(|term| {
        let mut term_ref = term.borrow_mut();
        let Some(terminal) = term_ref.as_mut() else {
            return;
        };
        let _ = terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(size);

            let mut lines: Vec<Line> = Vec::with_capacity(list.len() + 2);
            lines.push(Line::from(vec![
                Span::styled("Select voice source", Style::default().fg(Color::Yellow)),
            ]));
            if list.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("No outputs yet. Press r to refresh.", Style::default()),
                ]));
            }
            for (i, s) in list.iter().enumerate() {
                let marker = if i == cursor { ">" } else { " " };
                let text = format!(
                    "{} [{:02}] id={} app=\"{}\" media=\"{}\" node=\"{}\"",
                    marker,
                    i + 1,
                    s.id,
                    s.app,
                    s.media,
                    s.node
                );
                lines.push(Line::raw(text));
            }
            let block = Block::default().borders(Borders::ALL);
            let paragraph = Paragraph::new(lines).block(block);
            f.render_widget(paragraph, chunks[0]);

            let help = Line::from(vec![
                Span::styled("keys: ", Style::default().fg(Color::DarkGray)),
                Span::raw("Up/Down, Enter=select, r=refresh, Esc/Q=quit"),
            ]);
            let help_block = Paragraph::new(help).block(Block::default().borders(Borders::TOP));
            f.render_widget(help_block, chunks[1]);
        });
    });
}

fn draw_status(
    f: &mut ratatui::Frame<'_>,
    area: Rect,
    label: &str,
    reason: &str,
    auto_on: bool,
    ducking_on: bool,
) {
    let auto_color = if ducking_on {
        Color::Yellow
    } else {
        Color::DarkGray
    };
    let auto_text = if auto_on { "ON" } else { "OFF" };
    let line1 = Line::from(vec![
        Span::styled("Selected: ", Style::default().fg(Color::DarkGray)),
        Span::raw(label),
        Span::raw("    "),
        Span::styled("Auto Ducking: ", Style::default().fg(Color::DarkGray)),
        Span::styled(auto_text, Style::default().fg(auto_color)),
    ]);
    let line2 = Line::from(vec![
        Span::styled("reason: ", Style::default().fg(Color::DarkGray)),
        Span::raw(reason),
    ]);
    let block = Block::default().borders(Borders::BOTTOM);
    let paragraph = Paragraph::new(vec![line1, line2]).block(block);
    f.render_widget(paragraph, area);
}

#[allow(clippy::cast_lossless)]
fn draw_voice(f: &mut ratatui::Frame<'_>, area: Rect, level: f32, active: bool) {
    let color = if active {
        Color::Green
    } else {
        Color::DarkGray
    };
    let label = if active { "ACTIVE" } else { "INACTIVE" };
    let gauge = Gauge::default()
        .block(Block::default().title("VOICE").borders(Borders::ALL))
        .gauge_style(Style::default().fg(color))
        .ratio(level as f64)
        .label(Span::raw(label));
    f.render_widget(gauge, area);
}

#[allow(
    clippy::cast_lossless,
    clippy::too_many_arguments,
    clippy::uninlined_format_args
)]
fn draw_controls(
    f: &mut ratatui::Frame<'_>,
    area: Rect,
    threshold_live: f32,
    duck_factor_live: f32,
    hold_ms: u64,
    sens_fill: f32,
    duck_fill: f32,
    hold_fill: f32,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    let sens = Gauge::default()
        .block(Block::default().title("Sensitivity").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Gray))
        .ratio(sens_fill as f64)
        .label(Span::raw(format!("{:.3}", threshold_live)));
    let duck = Gauge::default()
        .block(Block::default().title("Duck Amount").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Yellow))
        .ratio(duck_fill as f64)
        .label(Span::raw(format!(
            "{:.0}%",
            (1.0 - duck_factor_live) * 100.0
        )));
    let hold = Gauge::default()
        .block(Block::default().title("Hold").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Gray))
        .ratio(hold_fill as f64)
        .label(Span::raw(format!("{} ms", hold_ms)));

    f.render_widget(sens, rows[0]);
    f.render_widget(duck, rows[1]);
    f.render_widget(hold, rows[2]);
}

fn draw_help(f: &mut ratatui::Frame<'_>, area: Rect) {
    let line = Line::from(vec![
        Span::styled("keys: ", Style::default().fg(Color::DarkGray)),
        Span::raw("W/S=sens  A/D=duck  Q/E=hold  Space=auto  Esc/x=quit"),
    ]);
    let paragraph = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
    f.render_widget(paragraph, area);
}
