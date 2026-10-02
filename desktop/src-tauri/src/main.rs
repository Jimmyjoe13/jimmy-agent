// Masque la fenêtre de console sur Windows : Jimmy est une application de
// bureau, pas un terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    jimmy_desktop_lib::run()
}