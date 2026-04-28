use anyhow::{Context, Result};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use std::io::{stdout, Write};
use std::time::Duration;

use crate::config::Config;
use crate::duck::DuckingSettings;
use crate::vad;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Row {
    Sensitivity,
    DuckPercent,
    Hold,
}

impl Row {
    const ALL: [Self; 3] = [Self::Sensitivity, Self::DuckPercent, Self::Hold];

    fn previous(self) -> Self {
        match self {
            Self::Sensitivity => Self::Hold,
            Self::DuckPercent => Self::Sensitivity,
            Self::Hold => Self::DuckPercent,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Sensitivity => Self::DuckPercent,
            Self::DuckPercent => Self::Hold,
            Self::Hold => Self::Sensitivity,
        }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("enable terminal raw mode")?;
        execute!(stdout(), EnterAlternateScreen, Hide).context("enter tuner screen")?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(stdout(), Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

pub fn run() -> Result<()> {
    let _terminal = TerminalGuard::enter()?;
    let mut selected = Row::Sensitivity;
    let mut settings = load_settings()?;

    loop {
        draw(settings, selected)?;
        if !event::poll(Duration::from_millis(250)).context("poll tuner input")? {
            continue;
        }

        match event::read().context("read tuner input")? {
            Event::Key(key) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => break,
                KeyCode::Up | KeyCode::Char('k') => selected = selected.previous(),
                KeyCode::Down | KeyCode::Char('j') => selected = selected.next(),
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => {
                    adjust(&mut settings, selected, -1);
                    save_settings(settings)?;
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('+') => {
                    adjust(&mut settings, selected, 1);
                    save_settings(settings)?;
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    Ok(())
}

fn load_settings() -> Result<DuckingSettings> {
    let config = Config::load_or_default()?;
    Ok(DuckingSettings {
        duck_percent: config.duck_percent,
        vad_threshold: config.vad_threshold,
        hold_ms: config.hold_ms,
    }
    .clamped())
}

fn save_settings(settings: DuckingSettings) -> Result<()> {
    let mut config = Config::load_or_default()?;
    let settings = settings.clamped();
    config.duck_percent = settings.duck_percent;
    config.vad_threshold = settings.vad_threshold;
    config.hold_ms = settings.hold_ms;
    config.save()
}

fn adjust(settings: &mut DuckingSettings, row: Row, direction: i32) {
    match row {
        Row::Sensitivity => {
            // Rechts = empfindlicher, also niedrigere RMS-Schwelle.
            settings.vad_threshold =
                (settings.vad_threshold - direction as f32 * 0.0025).clamp(0.0025, 0.2);
        }
        Row::DuckPercent => {
            let value = i32::from(settings.duck_percent) + direction;
            settings.duck_percent = value.clamp(0, 100) as u8;
        }
        Row::Hold => {
            let value = settings.hold_ms as i64 + i64::from(direction) * 50;
            settings.hold_ms = value.clamp(0, 4_000) as u64;
        }
    }
    *settings = settings.clamped();
}

fn draw(settings: DuckingSettings, selected: Row) -> Result<()> {
    let mut out = stdout();
    execute!(out, Clear(ClearType::All), MoveTo(0, 0)).context("draw tuner")?;
    write_line(&mut out, "pw-duck Regler")?;
    write_line(&mut out, "")?;
    write_line(
        &mut out,
        "↑/↓ wählen · ←/→ ändern · q/Esc schließen · Änderungen wirken live über config.toml",
    )?;
    write_line(&mut out, "")?;

    for row in Row::ALL {
        draw_row(&mut out, row, row == selected, settings)?;
    }

    write_line(&mut out, "")?;
    write_line(
        &mut out,
        "Hinweis: Wenn es ohne Sprache ducked, Empfindlichkeit nach links reduzieren (Schwelle höher).",
    )?;
    out.flush().context("flush tuner")
}

fn draw_row(
    out: &mut impl Write,
    row: Row,
    selected: bool,
    settings: DuckingSettings,
) -> Result<()> {
    let (label, value, percent) = match row {
        Row::Sensitivity => {
            let sensitivity = sensitivity_percent(settings.vad_threshold);
            let value = if vad::is_disabled_threshold(settings.vad_threshold) {
                "  0%  AUS".to_string()
            } else {
                format!(
                    "{:>3}%  Schwelle {:.4}, Start {:.4}",
                    sensitivity,
                    settings.vad_threshold,
                    vad::start_threshold(settings.vad_threshold)
                )
            };
            ("Empfindlichkeit", value, sensitivity)
        }
        Row::DuckPercent => (
            "Ducking-Lautstärke",
            format!("{:>3}%", settings.duck_percent),
            settings.duck_percent,
        ),
        Row::Hold => (
            "Hold",
            format!("{:>4} ms", settings.hold_ms),
            ((settings.hold_ms.min(4_000) * 100) / 4_000) as u8,
        ),
    };

    if selected {
        execute!(out, SetAttribute(Attribute::Reverse))?;
    }
    execute!(
        out,
        Print(format!("{:<20} [{}] {}\r\n", label, bar(percent), value))
    )?;
    if selected {
        execute!(out, SetAttribute(Attribute::Reset))?;
    }
    Ok(())
}

fn write_line(out: &mut impl Write, text: &str) -> Result<()> {
    execute!(out, Print(text), Print("\r\n")).context("write tuner line")
}

fn bar(percent: u8) -> String {
    let filled = (usize::from(percent).min(100) + 5) / 10;
    let empty = 10usize.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn sensitivity_percent(threshold: f32) -> u8 {
    let min = 0.0025f32;
    let max = 0.2f32;
    let normalized = 1.0 - ((threshold.clamp(min, max) - min) / (max - min));
    (normalized * 100.0).round().clamp(0.0, 100.0) as u8
}
