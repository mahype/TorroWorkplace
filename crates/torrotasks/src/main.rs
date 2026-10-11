//! `torrotasks` — tasks for the tiled desktop.
//!
//! An Iced window with the fixed app id `torro-tasks`. Data comes from the core
//! (`torroworkplace daemon`) over its socket; `--demo` runs on sample data instead.
//! Lists, smart lists (Heute, Geplant, Alle), quick add, subtasks, due dates, priorities.

mod app;
mod detail;
mod list;

use torro_core::AppId;
use torro_gui::source::Source;

fn main() -> iced::Result {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", AppId::TASKS.binary, env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let source = Source::from_args(std::env::args().skip(1));
    iced::application(move || app::App::new(source.clone()), app::App::update, app::App::view)
        .title(app::App::title)
        .theme(|_: &app::App| torro_gui::theme::theme())
        .subscription(app::App::subscription)
        .window(torro_gui::window(AppId::TASKS))
        .run()
}
