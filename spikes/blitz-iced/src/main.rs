//! Spike for task M9.4 (docs/tasks/mail.md): can Blitz draw HTML mail directly inside an Iced window?
//!
//! Left a list of sample mails (`fixtures/`), right the chosen mail. Each sample is cleaned with the allow
//! list of M9.1 (`torro_mail_imap::mime::clean_html`), so the spike sees the HTML that later arrives, and
//! drawn by Blitz (Stylo for CSS, Taffy for layout, Parley for text) through Vello on the CPU into a picture
//! that Iced shows (way 2 of the task). Way 1, Vello drawing into Iced's own wgpu through the `shader`
//! widget, does not fit the published crates: Iced 0.14 brings wgpu 27, Blitz 0.3 draws through
//! anyrender_vello 0.15, that is Vello 0.11 on wgpu 30 (see `docs/research.md`). With `--features gpu`
//! Vello draws on the GPU instead, with its own wgpu 30 device, and the picture is read back; only to
//! compare times.
//!
//! Run with `cargo run --release`. `--shoot <dir>` takes a screenshot of every sample (light, some dark,
//! some narrow), then clicks a link, selects text and scrolls in the newsletter by handing the widget's
//! own messages to the app (no system input, the user's mouse stays untouched), measures times, prints
//! them and exits. `--dump <dir>` writes the cleaned samples as HTML files for a browser (with `cid:`
//! pointing at the picture files) and exits. Without the `blitz` feature the same window shows the cleaned
//! HTML as text: the build the binary size "without Blitz" is taken from.
//!
//! Not product code: no `torro-gui`, no translations, no tests (the task asks for none).

// The build without Blitz only exists to measure the binary size; what only Blitz uses lies idle there.
#![cfg_attr(not(feature = "blitz"), allow(dead_code, unused_imports))]

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

#[cfg(not(feature = "blitz"))]
use iced::widget::scrollable;
use iced::widget::{button, column, container, row, text, toggler};
use iced::{Element, Length, Point, Size, Subscription, Task, Theme, keyboard, mouse, window};

/// Width of the sample list on the left, in logical pixels.
const LIST_WIDTH: f32 = 240.0;
/// Size of the window when it opens, and the height the screenshots use.
const WINDOW: Size = Size::new(1200.0, 800.0);

/// The samples: file stem under `fixtures/` (`long` is built in code) and the title in the list.
const SAMPLES: &[(&str, &str)] = &[
    ("newsletter", "Newsletter: nested tables, pictures"),
    ("invoice", "Shop invoice"),
    ("outlook", "Outlook reply (MSO, o:p, font)"),
    ("styled", "Style block, media queries, dark"),
    ("plain", "Plain text mail as <pre>"),
    ("long", "Very long mail (over 1 MiB)"),
];

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(64);
        }
    };
    let samples = match load_samples() {
        Ok(samples) => samples,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(dir) = &args.dump {
        return match dump(&samples, dir) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    let shoot = args.shoot.clone();
    // While shooting the window floats at a fixed size: a tiling window manager (the Torro core on this
    // Mac, for one) would otherwise fit it into its layout and the screenshots would differ in size.
    let resizable = shoot.is_none();

    let result = iced::application(move || Spike::new(samples.clone(), shoot.clone()), Spike::update, Spike::view)
        .title("Blitz in Iced (M9.4)")
        .theme(Spike::theme)
        .subscription(Spike::subscription)
        .window_size(WINDOW)
        .resizable(resizable)
        .run();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// What can go wrong outside the window.
#[derive(Debug)]
enum SpikeError {
    Args(String),
    Io(PathBuf, std::io::Error),
    Png(PathBuf, String),
}

impl fmt::Display for SpikeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Args(message) => write!(f, "{message}\nusage: blitz-iced [--shoot <dir> | --dump <dir>]"),
            Self::Io(path, error) => write!(f, "{}: {error}", path.display()),
            Self::Png(path, error) => write!(f, "{}: {error}", path.display()),
        }
    }
}

/// Command line: `--shoot <dir>` or `--dump <dir>`, nothing else.
#[derive(Debug, Clone, Default)]
struct Args {
    shoot: Option<PathBuf>,
    dump: Option<PathBuf>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, SpikeError> {
        let mut parsed = Self::default();
        while let Some(arg) = args.next() {
            let mut dir =
                || args.next().map(PathBuf::from).ok_or_else(|| SpikeError::Args(format!("{arg} needs a directory")));
            match arg.as_str() {
                "--shoot" => parsed.shoot = Some(dir()?),
                "--dump" => parsed.dump = Some(dir()?),
                _ => return Err(SpikeError::Args(format!("unknown argument {arg}"))),
            }
        }
        Ok(parsed)
    }
}

/// One sample mail, already cleaned.
#[derive(Debug, Clone)]
struct Sample {
    name: &'static str,
    title: &'static str,
    html: String,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn load_samples() -> Result<Vec<Sample>, SpikeError> {
    let dir = fixtures();
    SAMPLES
        .iter()
        .map(|&(name, title)| {
            let raw = if name == "long" {
                long_mail()
            } else {
                let path = dir.join(format!("{name}.html"));
                fs::read_to_string(&path).map_err(|error| SpikeError::Io(path, error))?
            };
            let html = torro_mail_imap::mime::clean_html(&raw);
            println!("sample {name}: {} bytes raw, {} bytes cleaned", raw.len(), html.len());
            Ok(Sample { name, title, html })
        })
        .collect()
}

/// A digest mail of a bit over 1 MiB, built in code so the repository does not carry a megabyte of
/// filler: headings, paragraphs and a small table per post, as a mailing list digest looks.
fn long_mail() -> String {
    let mut out = String::from(
        "<div style=\"font-family:Helvetica,Arial,sans-serif;font-size:15px;line-height:22px;color:#222;\
         max-width:680px;margin:0 auto;padding:16px\"><h1>Example list digest</h1>",
    );
    let mut post = 0;
    while out.len() < 1_150_000 {
        post += 1;
        out.push_str(&format!(
            "<h3 style=\"margin:24px 0 4px;color:#1f3864\">Post {post}: Re: how do you tile your windows?</h3>\
             <p style=\"margin:0 0 8px\">Sample {post} wrote: I keep the calendar on the left and the inbox \
             in the middle, the tasks on the right. With a wide monitor that leaves room for an editor. \
             The <b>mini tiles</b> are the best part, they show what matters without a click. \
             <a href=\"https://example.com/list/{post}\">Read in the archive</a>.</p>\
             <table width=\"100%\" cellpadding=\"4\" cellspacing=\"0\" border=\"1\" \
             style=\"border-collapse:collapse;border-color:#ddd;font-size:13px\">\
             <tr><td width=\"30%\">Author</td><td>Sample {post}</td></tr>\
             <tr><td>Replies</td><td>{}</td></tr></table>",
            post * 7 % 31
        ));
    }
    out.push_str("</div>");
    out
}

/// `--dump`: the cleaned samples as files a browser can open, `cid:` replaced by the picture file (what
/// `torromailview` does through its own protocol), in standards mode like the page of `torromailview`.
fn dump(samples: &[Sample], dir: &Path) -> Result<(), SpikeError> {
    fs::create_dir_all(dir).map_err(|error| SpikeError::Io(dir.to_path_buf(), error))?;
    let cid = fixtures().join("cid");
    let cid_url = format!("file://{}/", cid.display());
    for sample in samples {
        let path = dir.join(format!("{}.html", sample.name));
        let page = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>{}</body></html>",
            sample.html.replace("cid:logo@torro.example", &format!("{cid_url}logo@torro.example.png"))
        );
        // Headless Chrome cannot be told to prefer dark, so a second file applies the dark rules always.
        if page.contains("prefers-color-scheme: dark") {
            let dark = dir.join(format!("{}-dark.html", sample.name));
            let page = page.replace("@media (prefers-color-scheme: dark)", "@media all");
            fs::write(&dark, page).map_err(|error| SpikeError::Io(dark.clone(), error))?;
            println!("wrote {}", dark.display());
        }
        fs::write(&path, page).map_err(|error| SpikeError::Io(path.clone(), error))?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Prints where a screenshot went, or why it did not.
fn report(result: Result<(), SpikeError>, path: &Path) {
    match result {
        Ok(()) => println!("screenshot {}", path.display()),
        Err(error) => eprintln!("{error}"),
    }
}

/// Saves an RGBA picture as PNG.
fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), SpikeError> {
    let file = fs::File::create(path).map_err(|error| SpikeError::Io(path.to_path_buf(), error))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    // Screenshots go into the repository: smallest files over speed.
    encoder.set_compression(png::Compression::High);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let png_error = |error: png::EncodingError| SpikeError::Png(path.to_path_buf(), error.to_string());
    let mut writer = encoder.write_header().map_err(png_error)?;
    writer.write_image_data(rgba).map_err(png_error)?;
    writer.finish().map_err(png_error)
}

/// What Blitz drew last: the picture and its size in logical pixels.
#[derive(Debug, Clone)]
struct Frame {
    handle: iced::widget::image::Handle,
    size: Size,
}

/// A press, release or move of the mouse over the mail.
#[derive(Debug, Clone, Copy)]
enum PointerKind {
    Down,
    Up,
    Move,
}

#[derive(Debug, Clone)]
enum Message {
    Window(Option<window::Id>),
    Scale(f32),
    Select(usize),
    Dark(bool),
    /// The mail pane got this size (logical pixels).
    Pane(Size),
    Pointer(PointerKind, Point, keyboard::Modifiers),
    Wheel(mouse::ScrollDelta, Point),
    Copy,
    Tick,
    Shot(window::Screenshot),
}

/// One screenshot of `--shoot`.
#[derive(Debug, Clone)]
struct Step {
    sample: usize,
    dark: bool,
    width: f32,
    /// File stem: `<stem>-blitz.png` holds the mail pane alone (to lay next to a browser's picture of the
    /// same mail), `<stem>-window.png` the whole window, when `window` is set.
    stem: String,
    window: bool,
    /// What to do in the mail before the screenshot (the same messages the widget sends).
    act: Act,
}

/// An input `--shoot` gives the mail, at a point of the pane (logical pixels, measured on the screenshot of
/// the newsletter at 960 wide).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    None,
    Click(Point),
    Select(Point, Point),
    Scroll(Point),
}

/// The run of `--shoot`: the steps, where it is, and whether it waits for a screenshot.
#[derive(Debug)]
struct Shoot {
    dir: PathBuf,
    steps: Vec<Step>,
    at: usize,
    applied: bool,
    capturing: bool,
}

impl Shoot {
    fn new(dir: PathBuf) -> Self {
        let mut steps = Vec::new();
        let index = |name: &str| SAMPLES.iter().position(|&(sample, _)| sample == name).unwrap_or(0);
        for (sample, &(name, _)) in SAMPLES.iter().enumerate() {
            let window = name == "newsletter";
            steps.push(Step {
                sample,
                dark: false,
                width: WINDOW.width,
                stem: format!("{name}-light"),
                window,
                act: Act::None,
            });
            if name == "newsletter" {
                // Interaction in the newsletter: the link "full story with all setups", the first paragraph, then scroll.
                let acts = [
                    ("probe-link", Act::Click(Point::new(350.0, 508.0))),
                    ("probe-select", Act::Select(Point::new(212.0, 444.0), Point::new(718.0, 468.0))),
                    ("probe-scroll", Act::Scroll(Point::new(400.0, 400.0))),
                ];
                for (stem, act) in acts {
                    steps.push(Step {
                        sample,
                        dark: false,
                        width: WINDOW.width,
                        stem: stem.into(),
                        window: false,
                        act,
                    });
                }
            }
        }
        for name in ["newsletter", "styled", "plain"] {
            let window = name == "styled";
            steps.push(Step {
                sample: index(name),
                dark: true,
                width: WINDOW.width,
                stem: format!("{name}-dark"),
                window,
                act: Act::None,
            });
        }
        // 760 wide leaves a pane of 520: below the 600 px of the media query in `styled`.
        for name in ["styled", "newsletter"] {
            let window = name == "styled";
            steps.push(Step {
                sample: index(name),
                dark: false,
                width: 760.0,
                stem: format!("{name}-narrow"),
                window,
                act: Act::None,
            });
        }
        Self { dir, steps, at: 0, applied: false, capturing: false }
    }
}

struct Spike {
    samples: Vec<Sample>,
    selected: usize,
    dark: bool,
    window: Option<window::Id>,
    scale: f32,
    /// Size of the mail pane as Iced laid it out; zero until the first frame.
    pane: Size,
    #[cfg(feature = "blitz")]
    mail: Option<blitz_view::MailView>,
    /// One font context for all mails: building one reads the system fonts (about 20 ms).
    #[cfg(feature = "blitz")]
    fonts: blitz_dom::FontContext,
    frame: Option<Frame>,
    status: String,
    shoot: Option<Shoot>,
}

impl Spike {
    fn new(samples: Vec<Sample>, shoot: Option<PathBuf>) -> (Self, Task<Message>) {
        let shoot = shoot.map(Shoot::new);
        if let Some(shoot) = &shoot
            && let Err(error) = fs::create_dir_all(&shoot.dir)
        {
            eprintln!("{}: {error}", shoot.dir.display());
        }
        let spike = Self {
            samples,
            selected: 0,
            dark: false,
            window: None,
            scale: 1.0,
            pane: Size::ZERO,
            #[cfg(feature = "blitz")]
            mail: None,
            #[cfg(feature = "blitz")]
            fonts: blitz_view::shared_fonts(),
            frame: None,
            status: String::new(),
            shoot,
        };
        (spike, window::latest().map(Message::Window))
    }

    fn theme(&self) -> Theme {
        if self.dark { Theme::Dark } else { Theme::Light }
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.shoot.is_some() || self.loading() {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    #[cfg(feature = "blitz")]
    fn loading(&self) -> bool {
        self.mail.as_ref().is_some_and(blitz_view::MailView::loading)
    }

    #[cfg(not(feature = "blitz"))]
    fn loading(&self) -> bool {
        false
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Window(id) => {
                self.window = id;
                return match id {
                    Some(id) => window::scale_factor(id).map(Message::Scale),
                    None => Task::none(),
                };
            }
            Message::Scale(scale) => {
                self.scale = scale;
                self.relayout();
            }
            Message::Select(index) => {
                self.selected = index;
                self.open();
            }
            Message::Dark(dark) => {
                self.dark = dark;
                self.relayout();
            }
            Message::Pane(size) => {
                let first = self.pane == Size::ZERO;
                self.pane = size;
                if first { self.open() } else { self.relayout() }
            }
            Message::Pointer(kind, point, modifiers) => self.pointer(kind, point, modifiers),
            Message::Wheel(delta, point) => self.wheel(delta, point),
            Message::Copy => return self.copy(),
            Message::Tick => {
                self.poll();
                return self.shoot_tick();
            }
            Message::Shot(screenshot) => return self.save_shot(screenshot),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let list = self.samples.iter().enumerate().fold(column![].spacing(4).padding(8), |list, (index, sample)| {
            let style = if index == self.selected { button::primary } else { button::text };
            list.push(
                button(text(sample.title).size(13)).width(Length::Fill).style(style).on_press(Message::Select(index)),
            )
        });
        let list = column![list, container(toggler(self.dark).label("Dark").on_toggle(Message::Dark)).padding(12)]
            .width(LIST_WIDTH);
        let status = container(text(&self.status).size(12)).padding([6, 10]).width(Length::Fill);
        row![list, column![self.mail_view(), status]].into()
    }

    #[cfg(feature = "blitz")]
    fn mail_view(&self) -> Element<'_, Message> {
        let interaction = self.mail.as_ref().map_or(mouse::Interaction::None, blitz_view::MailView::interaction);
        pane::MailPane { frame: self.frame.as_ref(), known: self.pane, interaction }.into()
    }

    /// Without Blitz: the cleaned HTML as text, so the window and its size stay comparable.
    #[cfg(not(feature = "blitz"))]
    fn mail_view(&self) -> Element<'_, Message> {
        let html = self.samples.get(self.selected).map_or("", |sample| sample.html.as_str());
        let html: String = html.chars().take(20_000).collect();
        scrollable(text(html).size(12).font(iced::Font::MONOSPACE)).width(Length::Fill).height(Length::Fill).into()
    }

    /// `--shoot`: applies the next step, waits until the mail is drawn and its pictures are in, then takes
    /// the screenshot; after the last step measures and exits.
    fn shoot_tick(&mut self) -> Task<Message> {
        let ready = self.ready();
        let window = self.window;
        let Some(shoot) = &mut self.shoot else { return Task::none() };
        let Some(step) = shoot.steps.get(shoot.at).cloned() else { return Task::none() };
        let Some(id) = window else { return Task::none() };
        if !shoot.applied && step.act != Act::None {
            if !ready {
                return Task::none();
            }
            shoot.applied = true;
            self.act(step.act);
            return Task::none();
        }
        if !shoot.applied {
            shoot.applied = true;
            let resize = window::resize(id, Size::new(step.width, WINDOW.height));
            self.dark = step.dark;
            if self.selected != step.sample || self.mail_missing() {
                self.selected = step.sample;
                self.open();
            } else {
                self.relayout();
            }
            return resize;
        }
        let expected = step.width - LIST_WIDTH;
        if !shoot.capturing && ready && (self.pane.width - expected).abs() < 1.0 {
            shoot.capturing = true;
            return window::screenshot(id).map(Message::Shot);
        }
        Task::none()
    }

    fn save_shot(&mut self, screenshot: window::Screenshot) -> Task<Message> {
        let Some(shoot) = &mut self.shoot else { return Task::none() };
        if let Some(step) = shoot.steps.get(shoot.at) {
            let (width, height) = (screenshot.size.width, screenshot.size.height);
            let scale = screenshot.scale_factor;
            if step.window {
                let path = shoot.dir.join(format!("{}-window.png", step.stem));
                report(save_png(&path, width, height, &screenshot.rgba), &path);
            }
            // The mail pane: right of the list, above the status line.
            let left = ((LIST_WIDTH * scale).round() as u32).min(width);
            let bottom = ((self.pane.height * scale).round() as u32).min(height);
            let row = width as usize * 4;
            let pane: Vec<u8> = screenshot
                .rgba
                .chunks(row)
                .take(bottom as usize)
                .flat_map(|line| line.get(left as usize * 4..).unwrap_or_default().iter().copied())
                .collect();
            let path = shoot.dir.join(format!("{}-blitz.png", step.stem));
            report(save_png(&path, width - left, bottom, &pane), &path);
        }
        shoot.at += 1;
        shoot.applied = false;
        shoot.capturing = false;
        if shoot.at < shoot.steps.len() {
            return Task::none();
        }
        self.bench();
        iced::exit()
    }
}

#[cfg(not(feature = "blitz"))]
impl Spike {
    fn open(&mut self) {
        if let Some(sample) = self.samples.get(self.selected) {
            self.status = format!("{} · {} bytes cleaned HTML (built without Blitz)", sample.name, sample.html.len());
        }
    }
    fn relayout(&mut self) {}
    fn poll(&mut self) {}
    fn pointer(&mut self, _kind: PointerKind, _point: Point, _modifiers: keyboard::Modifiers) {}
    fn wheel(&mut self, _delta: mouse::ScrollDelta, _point: Point) {}
    fn copy(&mut self) -> Task<Message> {
        Task::none()
    }
    fn ready(&self) -> bool {
        true
    }
    fn mail_missing(&self) -> bool {
        false
    }
    fn bench(&mut self) {}
    fn act(&mut self, _act: Act) {}
}

#[cfg(feature = "blitz")]
impl Spike {
    /// Parses, lays out and draws the selected sample for the current pane.
    fn open(&mut self) {
        if self.pane.width < 1.0 || self.pane.height < 1.0 {
            return;
        }
        let Some(sample) = self.samples.get(self.selected) else { return };
        let (mut mail, timing) =
            blitz_view::MailView::open(&sample.html, self.pane, self.scale, self.dark, Some(&self.fonts));
        let (frame, paint) = mail.paint();
        let message = format!("{} · first draw {}", sample.name, timing.with_paint(paint));
        println!("{message}");
        self.status = message;
        self.frame = Some(frame);
        self.mail = Some(mail);
    }

    /// New width, height, scale or colour scheme: style and layout again, then draw.
    fn relayout(&mut self) {
        let name = self.samples.get(self.selected).map_or("", |sample| sample.name);
        let Some(mail) = &mut self.mail else { return };
        if self.pane.width < 1.0 || self.pane.height < 1.0 {
            return;
        }
        let layout = mail.relayout(self.pane, self.scale, self.dark);
        let (frame, paint) = mail.paint();
        let message = format!(
            "{name} · relayout to {:.0}x{:.0}: style+layout {} ms, paint {} ms",
            self.pane.width,
            self.pane.height,
            ms(layout),
            ms(paint)
        );
        println!("{message}");
        self.status = message;
        self.frame = Some(frame);
    }

    /// While pictures and style sheets come in: hand them to the document and draw again.
    fn poll(&mut self) {
        let Some(mail) = &mut self.mail else { return };
        if let Some(loaded) = mail.poll() {
            let (frame, paint) = mail.paint();
            self.frame = Some(frame);
            if let Some(after) = loaded {
                let name = self.samples.get(self.selected).map_or("", |sample| sample.name);
                let message = format!("{name} · pictures in after {} ms (last paint {} ms)", ms(after), ms(paint));
                println!("{message}");
                self.status = message;
            }
        }
    }

    fn pointer(&mut self, kind: PointerKind, point: Point, modifiers: keyboard::Modifiers) {
        let Some(mail) = &mut self.mail else { return };
        if !mail.pointer(kind, point, modifiers) {
            return;
        }
        let (frame, _) = mail.paint();
        self.frame = Some(frame);
        if let Some(link) = mail.take_link() {
            // Only shown, never opened: the spike checks that a click on a link is recognised.
            println!("link clicked: {link}");
            self.status = format!("link clicked: {link}");
        }
    }

    fn wheel(&mut self, delta: mouse::ScrollDelta, point: Point) {
        let Some(mail) = &mut self.mail else { return };
        let started = Instant::now();
        mail.wheel(delta, point);
        let (frame, paint) = mail.paint();
        self.frame = Some(frame);
        self.status = format!("scrolled: {} ms in all, paint {} ms", ms(started.elapsed()), ms(paint));
    }

    fn copy(&mut self) -> Task<Message> {
        let Some(text) = self.mail.as_ref().and_then(blitz_view::MailView::selected_text) else {
            self.status = "nothing selected".into();
            return Task::none();
        };
        self.status = format!("copied {} characters", text.chars().count());
        println!("{}", self.status);
        iced::clipboard::write(text)
    }

    fn ready(&self) -> bool {
        self.mail.as_ref().is_some_and(|mail| !mail.loading() && mail.settled())
    }

    fn mail_missing(&self) -> bool {
        self.mail.is_none()
    }

    /// Gives the mail the input of a `--shoot` step, through the same paths as the widget's messages.
    fn act(&mut self, act: Act) {
        let none = keyboard::Modifiers::default();
        match act {
            Act::None => {}
            Act::Click(at) => {
                self.pointer(PointerKind::Move, at, none);
                self.pointer(PointerKind::Down, at, none);
                self.pointer(PointerKind::Up, at, none);
                if !self.status.starts_with("link clicked") {
                    println!("probe: click at {at:?} did not hit a link");
                }
            }
            Act::Select(from, to) => {
                self.pointer(PointerKind::Move, from, none);
                self.pointer(PointerKind::Down, from, none);
                for step in 1..=10 {
                    let t = step as f32 / 10.0;
                    let at = Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
                    self.pointer(PointerKind::Move, at, none);
                }
                self.pointer(PointerKind::Up, to, none);
                // What Cmd+C would put on the clipboard (not written, so the user's clipboard stays).
                let text = self.mail.as_ref().and_then(blitz_view::MailView::selected_text);
                println!("probe: selected text {text:?}");
                self.status = format!("selected {} characters", text.map_or(0, |text| text.chars().count()));
            }
            Act::Scroll(at) => {
                for _ in 0..5 {
                    self.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 }, at);
                }
                println!("probe: {}", self.status);
            }
        }
    }

    /// Times without the window, at scale 1 and 2: first draw (with a fresh and with a shared font context),
    /// relayout at other widths and scrolling. Pictures are not waited for; they come over the network.
    fn bench(&mut self) {
        let height = self.pane.height.max(1.0);
        for scale in [1.0, 2.0] {
            println!("\n== bench: scale {scale}, pane {:.0}x{height:.0} ==", WINDOW.width - LIST_WIDTH);
            for sample in &self.samples {
                let name = sample.name;
                let pane = Size::new(WINDOW.width - LIST_WIDTH, height);
                let (_, fresh) = blitz_view::MailView::open(&sample.html, pane, scale, false, None);
                let (mut mail, timing) =
                    blitz_view::MailView::open(&sample.html, pane, scale, false, Some(&self.fonts));
                let (_, paint) = mail.paint();
                println!(
                    "{name}: first draw {} | with a fresh font context: parse {} ms",
                    timing.with_paint(paint),
                    ms(fresh.parse)
                );
                let mut line = format!("{name}: relayout");
                for width in [700.0, 520.0, 1400.0, 960.0] {
                    let layout = mail.relayout(Size::new(width, height), scale, false);
                    let (_, paint) = mail.paint();
                    line.push_str(&format!(" | {width:.0}: layout {} + paint {} ms", ms(layout), ms(paint)));
                }
                println!("{line}");
                let steps = 20;
                let started = Instant::now();
                for _ in 0..steps {
                    mail.wheel(mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 }, Point::new(200.0, 200.0));
                    let _ = mail.paint();
                }
                println!(
                    "{name}: scroll {} ms per step (wheel, layout, paint; {steps} steps)",
                    ms(started.elapsed() / steps)
                );
            }
        }
    }
}

/// Times of one first draw.
#[derive(Debug, Clone, Copy)]
struct Timing {
    parse: Duration,
    layout: Duration,
}

impl Timing {
    fn with_paint(self, paint: Duration) -> String {
        format!(
            "{} ms (parse {} ms, style+layout {} ms, paint {} ms)",
            ms(self.parse + self.layout + paint),
            ms(self.parse),
            ms(self.layout),
            ms(paint)
        )
    }
}

/// Milliseconds with one decimal.
fn ms(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1000.0)
}

#[cfg(feature = "blitz")]
mod blitz_view {
    //! The mail as a Blitz document: parsing, layout, drawing into pixels, mouse and keyboard.

    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use anyrender::{ImageRenderer as _, PaintScene as _};
    use blitz_dom::{BaseDocument, Document as _, DocumentConfig, FontContext};
    use blitz_html::HtmlDocument;
    use blitz_paint::paint_scene;
    use blitz_traits::events::{
        BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton, MouseEventButtons,
        PointerCoords, PointerDetails, UiEvent,
    };
    use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
    use blitz_traits::net::{NetHandler, NetProvider, Request};
    use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
    use cursor_icon::CursorIcon;
    use iced::{Point, Size, keyboard, mouse};
    use peniko::kurbo::{Affine, Rect};
    use peniko::{Color, Fill};

    use super::{Frame, PointerKind, Timing};

    /// The renderer: Vello on the CPU (way 2), or with `gpu` Vello on its own wgpu 30 device, read back.
    #[cfg(not(feature = "gpu"))]
    type Painter = anyrender_vello_cpu::VelloCpuImageRenderer;
    #[cfg(feature = "gpu")]
    type Painter = anyrender_vello::VelloImageRenderer;

    pub struct MailView {
        doc: BaseDocument,
        net: Arc<MailNet>,
        links: Arc<LinkCatcher>,
        shell: Arc<SpikeShell>,
        painter: Painter,
        pixels: Vec<u8>,
        /// Physical size of the picture.
        size: (u32, u32),
        scale: f32,
        buttons: MouseEventButtons,
        /// While pictures come in: when the mail was opened.
        loading_since: Option<Instant>,
        last_paint: Instant,
    }

    impl MailView {
        /// `fonts`: a font context to share; `None` builds a new one (reads the system fonts again).
        pub fn open(html: &str, pane: Size, scale: f32, dark: bool, fonts: Option<&FontContext>) -> (Self, Timing) {
            let net = Arc::new(MailNet { web: blitz_net::Provider::new(None), cid: super::fixtures().join("cid") });
            let links = Arc::new(LinkCatcher::default());
            let shell = Arc::new(SpikeShell::default());
            let size = physical(pane, scale);
            let started = Instant::now();
            let config = DocumentConfig {
                viewport: Some(viewport(size, scale, dark)),
                net_provider: Some(Arc::clone(&net) as _),
                navigation_provider: Some(Arc::clone(&links) as _),
                shell_provider: Some(Arc::clone(&shell) as _),
                font_ctx: fonts.cloned(),
                ..Default::default()
            };
            let mut doc = HtmlDocument::from_html(html, config).into_inner();
            let parse = started.elapsed();
            let started = Instant::now();
            doc.resolve(0.0);
            let layout = started.elapsed();
            let view = Self {
                doc,
                net,
                links,
                shell,
                painter: Painter::new(size.0, size.1),
                pixels: Vec::new(),
                size,
                scale,
                buttons: MouseEventButtons::None,
                loading_since: Some(Instant::now()),
                last_paint: Instant::now(),
            };
            (view, Timing { parse, layout })
        }

        pub fn relayout(&mut self, pane: Size, scale: f32, dark: bool) -> Duration {
            let started = Instant::now();
            let size = physical(pane, scale);
            if size != self.size {
                self.painter.resize(size.0, size.1);
            }
            self.size = size;
            self.scale = scale;
            self.doc.set_viewport(viewport(size, scale, dark));
            self.doc.resolve(0.0);
            started.elapsed()
        }

        /// Draws the visible part of the mail into a picture for Iced.
        pub fn paint(&mut self) -> (Frame, Duration) {
            let started = Instant::now();
            let (width, height) = self.size;
            let scale = f64::from(self.scale);
            let doc = &mut self.doc;
            self.painter.render_to_vec(
                |scene| {
                    // Mail is written for white paper; like Apple Mail, it stays on white in dark mode unless
                    // it brings its own dark colours (prefers-color-scheme), which then paint over this.
                    scene.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        Color::WHITE,
                        None,
                        &Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                    );
                    paint_scene(scene, doc, scale, width, height, 0, 0);
                },
                &mut self.pixels,
            );
            let frame = Frame {
                handle: iced::widget::image::Handle::from_rgba(width, height, self.pixels.clone()),
                size: Size::new(width as f32 / self.scale, height as f32 / self.scale),
            };
            self.last_paint = Instant::now();
            (frame, started.elapsed())
        }

        pub fn loading(&self) -> bool {
            self.loading_since.is_some()
        }

        /// Drawn and quiet for a moment (for screenshots).
        pub fn settled(&self) -> bool {
            self.last_paint.elapsed() > Duration::from_millis(400)
        }

        /// Hands what came over the network to the document. `None`: nothing to draw; `Some(None)`: draw,
        /// still loading; `Some(Some(t))`: draw, everything is in after `t`.
        pub fn poll(&mut self) -> Option<Option<Duration>> {
            let since = self.loading_since?;
            // Check first, then resolve: bytes that land between the two would otherwise never be
            // handed to the document.
            let done = self.net.web.is_empty();
            self.doc.resolve(0.0);
            if done {
                self.loading_since = None;
                return Some(Some(since.elapsed()));
            }
            Some(None)
        }

        /// Gives the document a mouse event; `true` when the picture should be drawn again.
        pub fn pointer(&mut self, kind: PointerKind, point: Point, modifiers: keyboard::Modifiers) -> bool {
            match kind {
                PointerKind::Down => self.buttons = MouseEventButtons::Primary,
                PointerKind::Up => self.buttons = MouseEventButtons::None,
                PointerKind::Move => {}
            }
            let event = self.pointer_event(point, modifiers);
            let event = match kind {
                PointerKind::Down => UiEvent::PointerDown(event),
                PointerKind::Up => UiEvent::PointerUp(event),
                PointerKind::Move => UiEvent::PointerMove(event),
            };
            self.doc.handle_ui_event(event);
            self.doc.resolve(0.0);
            // Moving without a button only changes the cursor (hover styles are rare in mail); dragging
            // changes the selection, and a press or release may select or click.
            !matches!(kind, PointerKind::Move) || self.buttons != MouseEventButtons::None
        }

        pub fn wheel(&mut self, delta: mouse::ScrollDelta, point: Point) {
            let delta = match delta {
                mouse::ScrollDelta::Lines { x, y } => BlitzWheelDelta::Lines(f64::from(x), f64::from(y)),
                mouse::ScrollDelta::Pixels { x, y } => BlitzWheelDelta::Pixels(f64::from(x), f64::from(y)),
            };
            let coords = self.coords(point);
            let event = BlitzWheelEvent {
                delta,
                coords,
                buttons: self.buttons,
                mods: keyboard_types::Modifiers::empty(),
                element: Default::default(),
            };
            self.doc.handle_ui_event(UiEvent::Wheel(event));
            self.doc.resolve(0.0);
        }

        pub fn selected_text(&self) -> Option<String> {
            self.doc.get_selected_text().filter(|text| !text.is_empty())
        }

        pub fn take_link(&self) -> Option<String> {
            self.links.0.lock().ok().and_then(|mut link| link.take())
        }

        pub fn interaction(&self) -> mouse::Interaction {
            match self.shell.cursor.lock().ok().and_then(|cursor| *cursor) {
                Some(CursorIcon::Pointer) => mouse::Interaction::Pointer,
                Some(CursorIcon::Text) => mouse::Interaction::Text,
                _ => mouse::Interaction::Idle,
            }
        }

        fn coords(&self, point: Point) -> PointerCoords {
            let scroll = self.doc.viewport_scroll();
            PointerCoords {
                page_x: point.x + scroll.x as f32,
                page_y: point.y + scroll.y as f32,
                screen_x: point.x,
                screen_y: point.y,
                client_x: point.x,
                client_y: point.y,
            }
        }

        fn pointer_event(&self, point: Point, modifiers: keyboard::Modifiers) -> BlitzPointerEvent {
            let mut mods = keyboard_types::Modifiers::empty();
            mods.set(keyboard_types::Modifiers::SHIFT, modifiers.shift());
            mods.set(keyboard_types::Modifiers::CONTROL, modifiers.control());
            mods.set(keyboard_types::Modifiers::ALT, modifiers.alt());
            mods.set(keyboard_types::Modifiers::META, modifiers.logo());
            BlitzPointerEvent {
                id: BlitzPointerId::Mouse,
                is_primary: true,
                coords: self.coords(point),
                button: MouseEventButton::Main,
                buttons: self.buttons,
                mods,
                details: PointerDetails::default(),
                element: Default::default(),
                active_pointers: Default::default(),
            }
        }
    }

    /// A font context with the system fonts and Blitz's bullet font, as Blitz builds one per document, but
    /// shared, so that a second mail does not read the system fonts again.
    pub fn shared_fonts() -> FontContext {
        use parley::fontique::{Collection, CollectionOptions, SourceCache};
        let mut fonts = FontContext {
            source_cache: SourceCache::new_shared(),
            collection: Collection::new(CollectionOptions { shared: true, system_fonts: true }),
        };
        fonts.collection.register_fonts(peniko::Blob::new(std::sync::Arc::new(blitz_dom::BULLET_FONT) as _), None);
        fonts
    }

    fn physical(pane: Size, scale: f32) -> (u32, u32) {
        ((pane.width * scale).round().max(1.0) as u32, (pane.height * scale).round().max(1.0) as u32)
    }

    fn viewport(size: (u32, u32), scale: f32, dark: bool) -> Viewport {
        Viewport::new(size.0, size.1, scale, if dark { ColorScheme::Dark } else { ColorScheme::Light })
    }

    /// Fetches for the mail: `cid:` from the picture files next to the samples (what M9.2 gets through
    /// `mail.get_attachment`), `http`, `https` and `data` through blitz-net. Nothing else.
    struct MailNet {
        web: blitz_net::Provider,
        cid: PathBuf,
    }

    impl NetProvider for MailNet {
        fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
            match request.url.scheme() {
                "cid" => match cid_picture(&self.cid, request.url.path()) {
                    Some(bytes) => handler.bytes(request.url.to_string(), bytes.into()),
                    None => println!("cid picture missing"),
                },
                "http" | "https" | "data" => {
                    let url = request.url.to_string();
                    println!("fetch {url}");
                    self.web.fetch(doc_id, request, Box::new(Logged { url, handler }));
                }
                scheme => println!("refused to fetch a {scheme}: URL"),
            }
        }
    }

    /// Passes bytes on under the URL that was asked for, and notes that they came (a failed fetch never
    /// calls the handler). blitz-net reports the URL after redirects, but Blitz files waiting pictures under
    /// the URL in `src`; behind a redirect (picsum.photos, most mail trackers and CDNs) the picture would
    /// never show.
    struct Logged {
        url: String,
        handler: Box<dyn NetHandler>,
    }

    impl NetHandler for Logged {
        fn bytes(self: Box<Self>, resolved_url: String, bytes: bytes::Bytes) {
            if resolved_url != self.url {
                println!("fetched {} via a redirect ({} bytes)", self.url, bytes.len());
            } else {
                println!("fetched {} ({} bytes)", self.url, bytes.len());
            }
            self.handler.bytes(self.url, bytes);
        }
    }

    /// The file for a content id: only plain names, only picture types.
    fn cid_picture(dir: &Path, id: &str) -> Option<Vec<u8>> {
        let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '-');
        if id.is_empty() || id.starts_with('.') || !id.chars().all(plain) {
            return None;
        }
        ["png", "jpg", "gif", "webp"].iter().find_map(|ext| std::fs::read(dir.join(format!("{id}.{ext}"))).ok())
    }

    /// Catches link clicks: Blitz asks to navigate, the spike only remembers the URL.
    #[derive(Default)]
    struct LinkCatcher(Mutex<Option<String>>);

    impl NavigationProvider for LinkCatcher {
        fn navigate_to(&self, options: NavigationOptions) {
            if let Ok(mut link) = self.0.lock() {
                *link = Some(options.url.to_string());
            }
        }
    }

    /// The cursor Blitz asks for (pointer over links, text over text).
    #[derive(Default)]
    struct SpikeShell {
        cursor: Mutex<Option<CursorIcon>>,
    }

    impl ShellProvider for SpikeShell {
        fn set_cursor(&self, icon: Option<CursorIcon>) {
            if let Ok(mut cursor) = self.cursor.lock() {
                *cursor = icon;
            }
        }
    }
}

#[cfg(feature = "blitz")]
mod pane {
    //! The Iced widget that shows Blitz's picture and passes mouse, wheel and copy on as messages.

    use iced::advanced::image::Image;
    use iced::advanced::image::Renderer as _;
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::renderer::{self, Renderer as _};
    use iced::advanced::widget::{Tree, Widget};
    use iced::advanced::{Clipboard, Shell};
    use iced::{Color, Element, Event, Length, Rectangle, Size, Theme, keyboard, mouse, window};

    use super::{Frame, Message, PointerKind};

    pub(super) struct MailPane<'a> {
        pub(super) frame: Option<&'a Frame>,
        /// The pane size the app knows; a different size here is reported as [`Message::Pane`].
        pub(super) known: Size,
        pub(super) interaction: mouse::Interaction,
    }

    impl Widget<Message, Theme, iced::Renderer> for MailPane<'_> {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(&mut self, _tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
            layout::Node::new(limits.max())
        }

        fn update(
            &mut self,
            _tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            _renderer: &iced::Renderer,
            _clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            _viewport: &Rectangle,
        ) {
            let bounds = layout.bounds();
            let inside = cursor.position_in(bounds);
            let message = match event {
                Event::Window(window::Event::RedrawRequested(_)) if bounds.size() != self.known => {
                    Some(Message::Pane(bounds.size()))
                }
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    inside.map(|point| Message::Pointer(PointerKind::Down, point, keyboard::Modifiers::default()))
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    inside.map(|point| Message::Pointer(PointerKind::Up, point, keyboard::Modifiers::default()))
                }
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    inside.map(|point| Message::Pointer(PointerKind::Move, point, keyboard::Modifiers::default()))
                }
                Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    inside.map(|point| Message::Wheel(*delta, point))
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Character(c), modifiers, ..
                }) if c.as_str() == "c" && modifiers.command() => Some(Message::Copy),
                _ => None,
            };
            if let Some(message) = message {
                shell.publish(message);
            }
        }

        fn draw(
            &self,
            _tree: &Tree,
            renderer: &mut iced::Renderer,
            _theme: &Theme,
            _style: &renderer::Style,
            layout: Layout<'_>,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
            let bounds = layout.bounds();
            renderer.fill_quad(renderer::Quad { bounds, ..renderer::Quad::default() }, Color::WHITE);
            if let Some(frame) = self.frame {
                let image = Image { snap: true, ..Image::new(frame.handle.clone()) };
                renderer.draw_image(image, Rectangle::new(bounds.position(), frame.size), bounds);
            }
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            if cursor.is_over(layout.bounds()) { self.interaction } else { mouse::Interaction::None }
        }
    }

    impl<'a> From<MailPane<'a>> for Element<'a, Message> {
        fn from(pane: MailPane<'a>) -> Self {
            Element::new(pane)
        }
    }
}
