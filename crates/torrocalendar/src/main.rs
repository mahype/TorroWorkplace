//! `torrocalendar` — calendar for the tiled desktop.
//!
//! An Iced window with the fixed app id `torro-calendar`. Data comes from the core
//! (`torroworkplace daemon`) over its socket; `--demo` runs on sample data instead.
//! Day, week, month and agenda views; events can be created, edited and deleted.
//!
//! With `--demo`, `--open <state>` starts in a given state and `--now <YYYY-MM-DDTHH:MM>` fixes the
//! time; `--appearance light|dark|system` overrides the stored appearance (see [`app::Launch`]).

mod app;
mod editor;
mod views;

use std::process::ExitCode;

use torro_core::AppId;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{} {}", AppId::CALENDAR.binary, env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // Refuse bad parameters before anything starts (the core, a window).
    let launch = match app::Launch::parse(&args) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{}: {err}", AppId::CALENDAR.binary);
            return ExitCode::from(64);
        }
    };
    let source = launch.source(&args);
    let result =
        iced::application(move || app::App::new(source.clone(), launch.clone()), app::App::update, app::App::view)
            .settings(torro_gui::settings())
            .title(app::App::title)
            .theme(app::App::theme)
            .scale_factor(app::App::scale_factor)
            .subscription(app::App::subscription)
            .window(torro_gui::window(AppId::CALENDAR))
            .run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}: {err}", AppId::CALENDAR.binary);
            ExitCode::FAILURE
        }
    }
}
