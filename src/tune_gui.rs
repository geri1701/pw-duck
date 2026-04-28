use anyhow::{Context, Result};
use gtk::prelude::*;
use gtk::{Adjustment, Application, ApplicationWindow, Box as GtkBox, Label, Orientation, Scale};

use crate::config::Config;
use crate::duck::DuckingSettings;
use crate::icons;
use crate::vad;

const APP_ID: &str = "dev.pw_duck.Tune";
const THRESHOLD_MIN: f32 = 0.0025;
const THRESHOLD_MAX: f32 = 0.2;

pub fn run() -> Result<()> {
    gtk::init().context("initialize GTK")?;
    install_icon_theme_path();
    gtk::Window::set_default_icon_name(icons::APP_ICON_NAME);
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run_with_args(&["pw-duck-tune-gui"]);
    Ok(())
}

fn install_icon_theme_path() {
    if let Some(display) = gtk::gdk::Display::default() {
        if let Some(path) = icons::icon_theme_path() {
            gtk::IconTheme::for_display(&display).add_search_path(path);
        }
    }
}

fn build_ui(app: &Application) {
    install_icon_theme_path();

    let settings = load_settings();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("pw-duck Tuner")
        .icon_name(icons::APP_ICON_NAME)
        .default_width(460)
        .default_height(280)
        .resizable(false)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 14);
    root.set_margin_top(18);
    root.set_margin_bottom(18);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let title = Label::new(Some("pw-duck Tuner"));
    title.set_xalign(0.0);
    title.add_css_class("title-2");
    root.append(&title);

    let sensitivity = Scale::new(
        Orientation::Horizontal,
        Some(&Adjustment::new(
            f64::from(sensitivity_percent(settings.vad_threshold)),
            0.0,
            100.0,
            1.0,
            10.0,
            0.0,
        )),
    );
    sensitivity.set_digits(0);
    sensitivity.set_hexpand(true);
    let sensitivity_value = Label::new(None);
    root.append(&slider_row(
        "Sensitivity",
        "0% = off, 100% = very sensitive",
        &sensitivity,
        &sensitivity_value,
    ));

    let duck_percent = Scale::new(
        Orientation::Horizontal,
        Some(&Adjustment::new(
            f64::from(settings.duck_percent),
            0.0,
            100.0,
            1.0,
            10.0,
            0.0,
        )),
    );
    duck_percent.set_digits(0);
    duck_percent.set_hexpand(true);
    let duck_value = Label::new(None);
    root.append(&slider_row(
        "Ducking volume",
        "Target volume while voice is active",
        &duck_percent,
        &duck_value,
    ));

    let hold = Scale::new(
        Orientation::Horizontal,
        Some(&Adjustment::new(
            settings.hold_ms as f64,
            0.0,
            4_000.0,
            50.0,
            250.0,
            0.0,
        )),
    );
    hold.set_digits(0);
    hold.set_hexpand(true);
    let hold_value = Label::new(None);
    root.append(&slider_row(
        "Hold",
        "How long ducking stays active after voice stops",
        &hold,
        &hold_value,
    ));

    let hint = Label::new(Some(
        "Changes are saved immediately and apply to the running tray.",
    ));
    hint.set_xalign(0.0);
    hint.add_css_class("dim-label");
    root.append(&hint);

    let update = {
        let sensitivity = sensitivity.clone();
        let duck_percent = duck_percent.clone();
        let hold = hold.clone();
        let sensitivity_value = sensitivity_value.clone();
        let duck_value = duck_value.clone();
        let hold_value = hold_value.clone();
        move || {
            let settings = settings_from_widgets(&sensitivity, &duck_percent, &hold);
            sensitivity_value.set_text(&sensitivity_label(settings.vad_threshold));
            duck_value.set_text(&format!("{}%", settings.duck_percent));
            hold_value.set_text(&format!("{} ms", settings.hold_ms));
            if let Err(err) = save_settings(settings) {
                eprintln!("Could not save tuner settings: {err:#}");
            }
        }
    };

    update();

    {
        let update = update.clone();
        sensitivity.connect_value_changed(move |_| update());
    }
    {
        let update = update.clone();
        duck_percent.connect_value_changed(move |_| update());
    }
    hold.connect_value_changed(move |_| update());

    window.set_child(Some(&root));
    window.present();
}

fn slider_row(title: &str, subtitle: &str, scale: &Scale, value: &Label) -> GtkBox {
    let outer = GtkBox::new(Orientation::Vertical, 4);

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let title_label = Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    title_label.add_css_class("heading");

    value.set_xalign(1.0);
    value.set_width_chars(14);

    header.append(&title_label);
    header.append(value);
    outer.append(&header);

    let subtitle_label = Label::new(Some(subtitle));
    subtitle_label.set_xalign(0.0);
    subtitle_label.add_css_class("dim-label");
    outer.append(&subtitle_label);
    outer.append(scale);

    outer
}

fn load_settings() -> DuckingSettings {
    Config::load_or_default()
        .map(|config| DuckingSettings {
            duck_percent: config.duck_percent,
            vad_threshold: config.vad_threshold,
            hold_ms: config.hold_ms,
        })
        .unwrap_or(DuckingSettings {
            duck_percent: 25,
            vad_threshold: 0.01,
            hold_ms: 700,
        })
        .clamped()
}

fn save_settings(settings: DuckingSettings) -> Result<()> {
    let mut config = Config::load_or_default()?;
    let settings = settings.clamped();
    config.duck_percent = settings.duck_percent;
    config.vad_threshold = settings.vad_threshold;
    config.hold_ms = settings.hold_ms;
    config.save()
}

fn settings_from_widgets(
    sensitivity: &Scale,
    duck_percent: &Scale,
    hold: &Scale,
) -> DuckingSettings {
    DuckingSettings {
        duck_percent: duck_percent.value().round().clamp(0.0, 100.0) as u8,
        vad_threshold: threshold_from_sensitivity(sensitivity.value().round() as u8),
        hold_ms: ((hold.value() / 50.0).round() * 50.0).clamp(0.0, 4_000.0) as u64,
    }
    .clamped()
}

fn sensitivity_label(threshold: f32) -> String {
    let percent = sensitivity_percent(threshold);
    if vad::is_disabled_threshold(threshold) {
        format!("{percent}% · OFF")
    } else {
        format!(
            "{percent}% · threshold {:.4} · start {:.4}",
            threshold,
            vad::start_threshold(threshold)
        )
    }
}

fn sensitivity_percent(threshold: f32) -> u8 {
    let normalized = 1.0
        - ((threshold.clamp(THRESHOLD_MIN, THRESHOLD_MAX) - THRESHOLD_MIN)
            / (THRESHOLD_MAX - THRESHOLD_MIN));
    (normalized * 100.0).round().clamp(0.0, 100.0) as u8
}

fn threshold_from_sensitivity(percent: u8) -> f32 {
    let normalized = f32::from(percent.min(100)) / 100.0;
    THRESHOLD_MIN + (1.0 - normalized) * (THRESHOLD_MAX - THRESHOLD_MIN)
}
