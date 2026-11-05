//! "GTDCraft Calendar": the small helper app that does GTDCraft's EventKit work as its own app, so macOS can ask
//! for (and keep) Calendar permission however GTDCraft is started. See src/calendar.rs.
#[allow(dead_code)]
#[path = "../calendar.rs"]
mod calendar;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(calendar::helper_main(&args));
}
