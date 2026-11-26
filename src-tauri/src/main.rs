#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(not(feature = "embedded"))]
fn main() {
    mjeku_desktop_lib::run();
}

// Build-i `embedded` është vetëm librari për PROGRAMERI POS Mobile; s'ka program më vete.
#[cfg(feature = "embedded")]
fn main() {}
