use std::fmt::Display;

pub fn logln(gui_enabled: bool, msg: impl Display) {
    if gui_enabled {
        return;
    }
    println!("{msg}");
}

pub fn elogln(gui_enabled: bool, msg: impl Display) {
    if gui_enabled {
        return;
    }
    eprintln!("{msg}");
}
