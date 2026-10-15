//! Shows the test mails of torro-htmlview (`tests/fixtures/`) with the `HtmlView` widget.
//!
//! `cargo run -p torro-htmlview --example fixtures` opens a window: the mails on the left, the chosen one on
//! the right in a scrollable reader, "Light · Dark" for the mail's colours, "360 · 700 · Fill" for the reader's
//! width. Select text and press Ctrl+C (⌘C), click a link (it is printed, not opened).
//!
//! `-- --shoot <dir>` paints every mail in light and dark at 360 and 700 logical pixels into
//! `<dir>/m96-<mail>-<scheme>-<width>.png` without a window, the way the widget paints it (`Content::snapshot`),
//! and exits. `--scale <n>` paints at n device pixels per logical pixel (default 1). Web pictures stay off in
//! both, so nothing touches the network; the inline pictures (`cid:`) come from `tests/fixtures/cid/`.
//!
//! A developer tool, not part of an app: its own labels are not translated.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{column, container, row, scrollable};
use iced::{Element, Length, Task, window};
use torro_gui::icon::Icon;
use torro_gui::theme::{size, space};
use torro_gui::widgets::{self, Trail};
use torro_gui::{layout, style, typo};
use torro_htmlview::{CidSource, Content, HtmlView, Scheme};

include!("../tests/fixtures/long_mail.rs");

/// The test mails: file stem under `tests/fixtures/` (`long` is built in code) and the title in the list.
const MAILS: [(&str, &str); 6] = [
    ("newsletter", "Newsletter"),
    ("invoice", "Invoice"),
    ("outlook", "Outlook reply"),
    ("styled", "Style block"),
    ("plain", "Plain text"),
    ("long", "Long digest"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1));
    if let Some(dir) = value("--shoot") {
        let scale = value("--scale").and_then(|scale| scale.parse().ok()).unwrap_or(1.0);
        return match shoot(Path::new(dir), scale) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("fixtures: {err}");
                ExitCode::FAILURE
            }
        };
    }
    let result = iced::application(Viewer::new, Viewer::update, Viewer::view)
        .settings(torro_gui::settings())
        .title("torro-htmlview fixtures")
        .theme(|_: &Viewer| torro_gui::theme::theme())
        .window_size(iced::Size::new(1200.0, 900.0))
        .run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("fixtures: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The inline pictures, read from `tests/fixtures/cid/<content id>.png`.
struct FixtureParts;

impl CidSource for FixtureParts {
    fn picture(&self, content_id: &str) -> Option<Vec<u8>> {
        if content_id.contains(['/', '\\']) || content_id.starts_with('.') {
            return None;
        }
        std::fs::read(fixtures().join("cid").join(format!("{content_id}.png"))).ok()
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn html(name: &str) -> Result<String, String> {
    if name == "long" {
        return Ok(long_mail());
    }
    let path = fixtures().join(format!("{name}.html"));
    std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))
}

fn content(name: &str) -> Result<Content, String> {
    Ok(Content::offline(&html(name)?, Some(Arc::new(FixtureParts))))
}

/// `--shoot`: every mail, light and dark, 360 and 700 wide, as PNG files. The long mail is cut at 2400.
fn shoot(dir: &Path, scale: f32) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    for (name, _) in MAILS {
        let content = content(name)?;
        let started = Instant::now();
        while content.is_loading() && started.elapsed() < Duration::from_secs(10) {
            // The snapshot takes the pictures in; this only waits until they are all answered.
            content.snapshot(1.0, 1.0, Scheme::Light, 0.0, 1.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        for scheme in [Scheme::Light, Scheme::Dark] {
            for width in [360.0, 700.0] {
                let started = Instant::now();
                let height = content.height(width, scale, scheme).min(2400.0);
                let snapshot = content.snapshot(width, scale, scheme, 0.0, height);
                let took = started.elapsed();
                let scheme_name = if scheme == Scheme::Light { "light" } else { "dark" };
                let path = dir.join(format!("m96-{name}-{scheme_name}-{width}.png"));
                save_png(&path, snapshot.width, snapshot.height, &snapshot.rgba)?;
                println!("{} ({}x{}, {:.1} ms)", path.display(), snapshot.width, snapshot.height, ms(took));
            }
        }
    }
    Ok(())
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let error = |err: &dyn std::fmt::Display| format!("{}: {err}", path.display());
    let file = std::fs::File::create(path).map_err(|err| error(&err))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| error(&err))?;
    writer.write_image_data(rgba).map_err(|err| error(&err))?;
    writer.finish().map_err(|err| error(&err))
}

/// How wide the reader is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Width {
    Narrow,
    Medium,
    Fill,
}

#[derive(Debug, Clone)]
enum Message {
    Open(usize),
    Scheme(Scheme),
    Width(Width),
    Scale(f32),
    Link(String),
}

struct Viewer {
    chosen: usize,
    content: Option<Content>,
    error: Option<String>,
    scheme: Scheme,
    width: Width,
    scale: f32,
}

impl Viewer {
    fn new() -> (Self, Task<Message>) {
        let mut viewer =
            Self { chosen: 0, content: None, error: None, scheme: Scheme::Light, width: Width::Fill, scale: 1.0 };
        viewer.open(0);
        let scale = window::latest().and_then(window::scale_factor).map(Message::Scale);
        (viewer, scale)
    }

    fn open(&mut self, index: usize) {
        self.chosen = index;
        let Some((name, _)) = MAILS.get(index) else { return };
        match content(name) {
            Ok(content) => {
                self.content = Some(content);
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Open(index) => self.open(index),
            Message::Scheme(scheme) => self.scheme = scheme,
            Message::Width(width) => self.width = width,
            Message::Scale(scale) => self.scale = scale,
            Message::Link(url) => println!("link clicked: {url}"),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let list = MAILS.iter().enumerate().fold(column![].spacing(space::xs()), |list, (index, (_, title))| {
            list.push(widgets::sidebar_item(
                Icon::Mail,
                *title,
                Trail::None,
                index == self.chosen,
                Message::Open(index),
            ))
        });
        let sidebar = container(list).width(size::sidebar()).padding(space::s());
        let scheme = widgets::segmented(
            [("Light", Message::Scheme(Scheme::Light)), ("Dark", Message::Scheme(Scheme::Dark))],
            usize::from(self.scheme == Scheme::Dark),
        );
        let widths = [
            ("360", Message::Width(Width::Narrow)),
            ("700", Message::Width(Width::Medium)),
            ("Fill", Message::Width(Width::Fill)),
        ];
        let active = match self.width {
            Width::Narrow => 0,
            Width::Medium => 1,
            Width::Fill => 2,
        };
        let header = row![widgets::segmented(widths, active), scheme]
            .spacing(space::s())
            .height(size::header())
            .align_y(iced::Alignment::Center);
        let reader: Element<'_, Message> = match (&self.content, &self.error) {
            (_, Some(err)) => typo::text(typo::body(), err.as_str()).into(),
            (Some(content), None) => {
                let view = HtmlView::new(content).scheme(self.scheme).scale(self.scale).on_link(Message::Link);
                let width = match self.width {
                    Width::Narrow => Length::Fixed(360.0),
                    Width::Medium => Length::Fixed(700.0),
                    Width::Fill => Length::Fill,
                };
                scrollable(container(view).width(width)).height(Length::Fill).into()
            }
            (None, None) => typo::text(typo::body(), "No mail").into(),
        };
        let main = container(column![header, reader].padding(layout::panel_padding()))
            .style(style::container::panel)
            .width(Length::Fill)
            .height(Length::Fill);
        row![sidebar, container(main).padding(layout::inset())].into()
    }
}
