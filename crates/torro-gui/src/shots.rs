//! The visual check without a window (#492): `--demo --shots <dir>` renders the app's view in memory
//! at each size class and writes one PNG per size, instead of opening a window.
//!
//! The old way (`shoot.sh` under Hyprland, `screencapture -l` on macOS) needed a real window, and
//! macOS brings such a window to the front and takes the focus while someone works. Here the program
//! runs in [`iced_test::Emulator`], the headless runtime of Iced: its tasks and subscriptions run as in
//! the real window, its view is drawn by the same renderer (wgpu, or tiny-skia without a GPU) into
//! a buffer. No window, no Dock icon, no focus change.
//!
//! What the picture does not hold: the window's frame (traffic lights, title bar), hover states,
//! fonts switched while running (the emulator does not load fonts later) and the system's light or
//! dark appearance (pass `--appearance`). Every app wires it in its `main` through [`Shots::from_args`]
//! and [`run`]; `.claude/skills/torro-design/shots.sh` builds an app and calls it for several states.
//!
//! Files: `<dir>/<prefix>-<size>-<W>x<H>.png` for the [`SIZES`] (W×H in logical pixels, the picture
//! at [`SCALE`] times that), `<dir>/<prefix>-<W>x<H>.png` for sizes of `--shot-size WxH`. The prefix
//! is the program's name and the values of the flags that pick a view ([`Shots::from_args`]).

use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use iced::Size;
use iced_test::emulator::{self, Emulator};
use iced_test::futures::futures::channel::mpsc::{self, TryRecvError};

/// The size classes of the visual check, one example each, in logical pixels: the sizes
/// `crate::layout` sorts into its classes (torro-design, section 6).
pub const SIZES: [(&str, Size); 6] = [
    ("mini", Size::new(240.0, 160.0)),
    ("mini-tall", Size::new(320.0, 480.0)),
    ("strip", Size::new(1200.0, 200.0)),
    ("narrow", Size::new(480.0, 900.0)),
    ("medium", Size::new(800.0, 600.0)),
    ("wide", Size::new(1600.0, 1000.0)),
];

/// The pictures are drawn at twice the logical size, like a Retina screen, so small text and hairlines
/// can be judged.
pub const SCALE: f32 = 2.0;

/// The flags whose values name the view in a file, in this order.
const NAMING_FLAGS: [&str; 24] = [
    "--view",
    "--section",
    "--settings",
    "--open",
    "--design",
    "--font",
    "--color-scheme",
    "--appearance",
    // Flags that pick another window or item: without them in the name its pictures would replace
    // those of the main window.
    "--chat",
    "--bot",
    "--lane",
    "--source",
    "--template",
    "--task",
    "--message",
    "--reply",
    "--reply-all",
    "--forward",
    "--account",
    "--step",
    "--mail-view",
    "--mail-colors",
    "--density",
    "--now",
];

/// The largest edge `--shot-size` takes, in logical pixels.
/// At [`SCALE`] that is a picture of 8192 pixels, the largest texture wgpu guarantees.
const MAX_EDGE: u32 = 4096;

/// How long the program must stay quiet after it booted before its picture is taken.
const QUIET: Duration = Duration::from_millis(300);

/// How long the program gets at most; a view that never settles is taken as it is then.
const LIMIT: Duration = Duration::from_secs(10);

/// What `--shots` asked for: where the pictures go, how they are named and at which sizes.
#[derive(Debug, Clone, PartialEq)]
pub struct Shots {
    dir: PathBuf,
    prefix: String,
    /// The sizes with their class name; `None` for a size of `--shot-size`.
    sizes: Vec<(Option<&'static str>, Size)>,
    /// `--shot-index <n>`: only the size at `n`, in a process that [`run`] started for it.
    index: Option<usize>,
}

/// Why `--shots` was refused, or its pictures could not be written.
#[derive(Debug)]
pub enum ShotError {
    /// `--shots` or `--shot-size` without a value.
    MissingValue(&'static str),
    /// `--shots` without `--demo`: pictures are of sample data only.
    NeedsDemo,
    /// A `--shot-size` that is not `WxH` with whole numbers from 1 to 4096.
    BadSize(String),
    /// A `--shot-index` that names none of the sizes.
    BadIndex(String),
    /// The process for one size could not be started.
    Spawn(io::Error),
    /// The process for the size at `index` failed, with its exit code if it has one.
    Size { index: usize, code: Option<i32> },
    /// The renderer's font store could not be reached (a thread panicked while holding it).
    Fonts,
    /// The folder or a picture could not be written.
    Write { path: PathBuf, source: io::Error },
    /// A picture could not be encoded as PNG.
    Encode { path: PathBuf, source: png::EncodingError },
}

impl fmt::Display for ShotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShotError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            ShotError::NeedsDemo => write!(f, "--shots only works together with --demo"),
            ShotError::BadSize(value) => {
                write!(
                    f,
                    "--shot-size expects WxH with whole numbers from 1 to {MAX_EDGE}, e.g. 800x600; got {value:?}"
                )
            }
            ShotError::BadIndex(value) => write!(f, "--shot-index names no size; got {value:?}"),
            ShotError::Spawn(err) => write!(f, "cannot start the process for a picture: {err}"),
            ShotError::Size { index, code: Some(code) } => {
                write!(f, "the picture of size {index} failed with exit code {code}")
            }
            ShotError::Size { index, code: None } => write!(f, "the picture of size {index} failed"),
            ShotError::Fonts => write!(f, "the fonts could not be loaded for the pictures"),
            ShotError::Write { path, source } => write!(f, "cannot write {}: {source}", path.display()),
            ShotError::Encode { path, source } => write!(f, "cannot encode {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ShotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShotError::Write { source, .. } => Some(source),
            ShotError::Encode { source, .. } => Some(source),
            ShotError::Spawn(source) => Some(source),
            _ => None,
        }
    }
}

/// Why [`run`] failed: the window, or the pictures instead of it.
#[derive(Debug)]
pub enum RunError {
    Window(iced::Error),
    Shots(ShotError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Window(err) => err.fmt(f),
            RunError::Shots(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for RunError {}

impl From<iced::Error> for RunError {
    fn from(err: iced::Error) -> Self {
        RunError::Window(err)
    }
}

impl From<ShotError> for RunError {
    fn from(err: ShotError) -> Self {
        RunError::Shots(err)
    }
}

/// The values of `--flag value` and `--flag=value` in `args`, in order; `Err` for a flag without a value.
fn values<'a>(args: &'a [String], flag: &'static str) -> Result<Vec<&'a str>, ShotError> {
    let mut found = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == flag {
            found.push(words.next().ok_or(ShotError::MissingValue(flag))?.as_str());
        } else if let Some(value) = word.strip_prefix(flag).and_then(|rest| rest.strip_prefix('=')) {
            found.push(value);
        }
    }
    Ok(found)
}

/// `WxH` with whole numbers from 1 to [`MAX_EDGE`].
fn parse_size(value: &str) -> Result<Size, ShotError> {
    let edge = |part: &str| part.parse::<u32>().ok().filter(|n| (1..=MAX_EDGE).contains(n));
    let (width, height) = value.split_once('x').ok_or_else(|| ShotError::BadSize(value.to_owned()))?;
    match (edge(width), edge(height)) {
        // Both at most 4096, so exact as f32.
        (Some(width), Some(height)) => Ok(Size::new(width as f32, height as f32)),
        _ => Err(ShotError::BadSize(value.to_owned())),
    }
}

/// `text` with every character outside `[a-z0-9-]` turned into `-`, safe in a file name.
fn file_safe(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else { '-' }).collect()
}

impl Shots {
    /// Reads `--shots <dir>` (or `--shots=<dir>`) from `args`, the words after the program name;
    /// `Ok(None)` without it. It needs `--demo`. `--shot-size WxH` (several times) replaces the
    /// [`SIZES`]. The files start with `name` and the value of each of `--view`, `--section`,
    /// `--settings`, `--open`, `--design`, `--font`, `--color-scheme`, `--appearance`, `--chat`, `--bot`,
    /// `--lane`, `--source`, `--template` and `--task` that is given, in this order, e.g.
    /// `torrocalendar-week-dark`, and further flags that pick a window, an item or a look ([`NAMING_FLAGS`]).
    /// A flag given twice counts with its first value, as in most of the apps' parsers. `--shot-index <n>`
    /// keeps only the size at `n` (what [`run`] starts a process with).
    pub fn from_args(name: &str, args: &[String]) -> Result<Option<Shots>, ShotError> {
        let Some(dir) = values(args, "--shots")?.first().copied() else {
            return Ok(None);
        };
        // An empty folder would put the pictures in the working directory.
        if dir.is_empty() {
            return Err(ShotError::MissingValue("--shots"));
        }
        if !args.iter().any(|arg| arg == "--demo") {
            return Err(ShotError::NeedsDemo);
        }
        let own = values(args, "--shot-size")?.into_iter().map(parse_size).collect::<Result<Vec<_>, _>>()?;
        let sizes: Vec<(Option<&'static str>, Size)> = if own.is_empty() {
            SIZES.iter().map(|&(class, size)| (Some(class), size)).collect()
        } else {
            own.into_iter().map(|size| (None, size)).collect()
        };
        let index = match values(args, "--shot-index")?.first() {
            None => None,
            Some(value) => match value.parse::<usize>() {
                Ok(index) if index < sizes.len() => Some(index),
                _ => return Err(ShotError::BadIndex((*value).to_owned())),
            },
        };
        let mut prefix = name.to_owned();
        for flag in NAMING_FLAGS {
            if let Some(value) = values(args, flag)?.first() {
                prefix.push('-');
                prefix.push_str(value);
            }
        }
        // The setup of the control centre is a switch without a value, and a window of its own.
        if args.iter().any(|arg| arg == "--setup") {
            prefix.push_str("-setup");
        }
        Ok(Some(Shots { dir: PathBuf::from(dir), prefix: file_safe(&prefix), sizes, index }))
    }

    /// The folder the pictures go to.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file of the picture at `size` with the class name `class`.
    fn file(&self, class: Option<&str>, size: Size) -> PathBuf {
        let (width, height) = (size.width.round(), size.height.round());
        let name = match class {
            Some(class) => format!("{}-{class}-{width}x{height}.png", self.prefix),
            None => format!("{}-{width}x{height}.png", self.prefix),
        };
        self.dir.join(name)
    }
}

/// Runs `application` as a window, or with `shots` renders it to pictures instead ([`write`]).
///
/// With more than one size each size gets a process of its own: this program again, with the same
/// words and `--shot-index <n>`. A demo source lives in the process, and what one boot changes in it
/// (a mail marked read when it opens) would otherwise show in the pictures of the next sizes.
pub fn run<P>(application: iced::Application<P>, shots: Option<&Shots>) -> Result<(), RunError>
where
    P: iced::Program<Theme = iced::Theme> + 'static,
    P::Message: iced::message::MaybeDebug + iced::message::MaybeClone,
{
    match shots {
        None => Ok(application.run()?),
        Some(shots) if shots.index.is_none() && shots.sizes.len() > 1 => Ok(each_size(shots.sizes.len())?),
        Some(shots) => write(&application, shots).map(|_| ()).map_err(RunError::from),
    }
}

/// Starts this program once per size, one after the other, with the words it got and
/// `--shot-index <n>`; stops at the first that fails.
fn each_size(count: usize) -> Result<(), ShotError> {
    let program = std::env::current_exe().map_err(ShotError::Spawn)?;
    let words: Vec<OsString> = std::env::args_os().skip(1).collect();
    each_size_with(count, &words, |words| {
        let status = std::process::Command::new(&program).args(words).status()?;
        Ok(if status.success() { Ok(()) } else { Err(status.code()) })
    })
}

/// [`each_size`] with the way a process is started given as `start`: it gets the words of one size and
/// answers `Ok(())` for a process that succeeded, `Err(exit code)` for one that failed.
fn each_size_with(
    count: usize,
    words: &[OsString],
    mut start: impl FnMut(&[OsString]) -> io::Result<Result<(), Option<i32>>>,
) -> Result<(), ShotError> {
    for index in 0..count {
        let mut line = words.to_vec();
        line.push(format!("--shot-index={index}").into());
        if let Err(code) = start(&line).map_err(ShotError::Spawn)? {
            return Err(ShotError::Size { index, code });
        }
    }
    Ok(())
}

/// The fonts of the program's settings are loaded once per process into the renderer's font store.
static FONTS_LOADED: AtomicBool = AtomicBool::new(false);

/// Renders `program` at each size of `shots` and writes the pictures, replacing older ones; prints
/// each file on its own line on stdout and returns them.
///
/// Each size boots the program afresh, runs its tasks and subscriptions until it has been quiet for
/// [`QUIET`] after booting (at most [`LIMIT`]; then a note goes to stderr and the picture is taken
/// anyway) and draws it in the theme it asks for (dark if it names none).
pub fn write<P>(program: &P, shots: &Shots) -> Result<Vec<PathBuf>, ShotError>
where
    P: iced::Program<Theme = iced::Theme> + 'static,
{
    std::fs::create_dir_all(&shots.dir).map_err(|source| ShotError::Write { path: shots.dir.clone(), source })?;
    // The emulator draws with the settings' default font but does not load the settings' fonts the way
    // a window does; without this every text would be Fira Sans.
    if !FONTS_LOADED.swap(true, Ordering::SeqCst) {
        let mut fonts = iced_test::renderer::graphics::text::font_system().write().map_err(|_| ShotError::Fonts)?;
        for font in program.settings().fonts {
            fonts.load_font(font);
        }
    }
    let mut written = Vec::with_capacity(shots.sizes.len());
    let sizes = shots.sizes.iter().enumerate().filter(|(index, _)| shots.index.is_none_or(|only| only == *index));
    for (_, &(class, size)) in sizes {
        let path = shots.file(class, size);
        let (sender, mut receiver) = mpsc::channel(100);
        let mut emulator = Emulator::new(sender, program, emulator::Mode::Patient, size);
        settle(program, &mut emulator, &mut receiver, &path);
        let theme = emulator.theme(program).unwrap_or(iced::Theme::Dark);
        let screenshot = emulator.screenshot(program, &theme, SCALE);
        save(&path, screenshot.size.width, screenshot.size.height, &screenshot.rgba)?;
        println!("{}", path.display());
        written.push(path);
    }
    Ok(written)
}

/// Runs the emulator's events until the program has been quiet for [`QUIET`] after booting, or
/// [`LIMIT`] has passed (noted on stderr for the picture `path`).
fn settle<P>(program: &P, emulator: &mut Emulator<P>, receiver: &mut mpsc::Receiver<emulator::Event<P>>, path: &Path)
where
    P: iced::Program + 'static,
{
    let started = Instant::now();
    let mut ready = false;
    let mut last = Instant::now();
    loop {
        if started.elapsed() >= LIMIT {
            eprintln!("{}: not settled after {} s", path.display(), LIMIT.as_secs());
            return;
        }
        match receiver.try_recv() {
            Ok(event) => {
                match event {
                    emulator::Event::Action(action) => emulator.perform(program, action),
                    emulator::Event::Ready => ready = true,
                    // No instructions are run, so none can fail.
                    emulator::Event::Failed(_) => {}
                }
                last = Instant::now();
            }
            // The runtime is gone: nothing more will come.
            Err(TryRecvError::Closed) => return,
            Err(TryRecvError::Empty) if ready && last.elapsed() >= QUIET => return,
            Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

/// Writes the RGBA pixels `rgba` of a `width` × `height` picture as a PNG at `path`.
fn save(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), ShotError> {
    let file = File::create(path).map_err(|source| ShotError::Write { path: path.to_owned(), source })?;
    let encode = |source| ShotError::Encode { path: path.to_owned(), source };
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(encode)?;
    writer.write_image_data(rgba).map_err(encode)?;
    writer.finish().map_err(encode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|word| (*word).to_string()).collect()
    }

    fn shots(line: &[&str]) -> Result<Option<Shots>, ShotError> {
        Shots::from_args("torrocalendar", &args(line))
    }

    #[test]
    fn without_the_flag_there_are_no_pictures() {
        assert!(matches!(shots(&["--demo", "--open", "week"]), Ok(None)));
        assert!(matches!(shots(&[]), Ok(None)));
    }

    #[test]
    fn the_flag_needs_a_folder_and_the_demo() {
        assert!(matches!(shots(&["--demo", "--shots"]), Err(ShotError::MissingValue("--shots"))));
        assert!(matches!(shots(&["--shots", "/tmp/x"]), Err(ShotError::NeedsDemo)));
        assert!(matches!(shots(&["--shots=/tmp/x"]), Err(ShotError::NeedsDemo)));
        assert!(matches!(shots(&["--demo", "--shots", "/tmp/x", "--shot-size"]), Err(ShotError::MissingValue(_))));
    }

    #[test]
    fn both_spellings_give_the_folder_and_the_six_classes() {
        for line in [&["--demo", "--shots", "/tmp/x"][..], &["--shots=/tmp/x", "--demo"]] {
            let shots = shots(line).expect("valid").expect("asked for");
            assert_eq!(shots.dir(), Path::new("/tmp/x"));
            assert_eq!(shots.prefix, "torrocalendar");
            let classes: Vec<_> = shots.sizes.iter().map(|(class, _)| class.expect("a class")).collect();
            assert_eq!(classes, ["mini", "mini-tall", "strip", "narrow", "medium", "wide"]);
        }
    }

    #[test]
    fn own_sizes_replace_the_classes() {
        let shots = shots(&["--demo", "--shots", "d", "--shot-size", "40x30", "--shot-size=4096x1"])
            .expect("valid")
            .expect("asked for");
        assert_eq!(shots.sizes, [(None, Size::new(40.0, 30.0)), (None, Size::new(4096.0, 1.0))]);
        assert_eq!(shots.file(None, Size::new(40.0, 30.0)), Path::new("d/torrocalendar-40x30.png"));
    }

    #[test]
    fn bad_sizes_are_refused() {
        for bad in ["0x10", "10x0", "4097x10", "10", "10x", "x10", "1.5x2", "-1x2", "10X10", "axb"] {
            let result = shots(&["--demo", "--shots", "d", "--shot-size", bad]);
            assert!(matches!(&result, Err(ShotError::BadSize(value)) if value == bad), "{bad}: {result:?}");
        }
    }

    #[test]
    fn the_prefix_names_the_view_in_a_fixed_order() {
        let line = [
            "--appearance",
            "dark",
            "--demo",
            "--open=week",
            "--color-scheme",
            "nord",
            "--font",
            "inter",
            "--design",
            "linear",
            "--settings",
            "night",
            "--section=accounts",
            "--view",
            "mail",
            "--shots",
            "d",
        ];
        let shots = shots(&line).expect("valid").expect("asked for");
        assert_eq!(shots.prefix, "torrocalendar-mail-accounts-night-week-linear-inter-nord-dark");
        assert_eq!(shots.file(Some("wide"), Size::new(1600.0, 1000.0)), {
            Path::new("d/torrocalendar-mail-accounts-night-week-linear-inter-nord-dark-wide-1600x1000.png")
        });
    }

    /// Review of #492: the first value counts, as for the apps; windows of their own are named apart.
    #[test]
    fn the_first_value_and_the_window_name_the_files() {
        let line = ["--demo", "--shots", "a", "--open", "detail", "--shots", "b", "--open", "pr", "--chat=new"];
        let shots = Shots::from_args("torroassistant", &args(&line)).expect("valid").expect("asked for");
        assert_eq!((shots.dir(), shots.prefix.as_str()), (Path::new("a"), "torroassistant-detail-new"));
    }

    /// PR review of #492: `--setup` of the control centre is a switch without a value; it is named as
    /// one and must not take the next word as its value.
    #[test]
    fn a_switch_is_no_naming_flag() {
        for line in [&["--demo", "--shots", "d", "--setup"][..], &["--demo", "--setup", "--shots", "d"]] {
            let named = Shots::from_args("torroworkplace", &args(line)).expect("valid").expect("asked for");
            assert_eq!((named.dir(), named.prefix.as_str()), (Path::new("d"), "torroworkplace-setup"), "{line:?}");
        }
    }

    /// PR review of #492: each size gets a process with the same words and its index; the first that
    /// fails stops the rest with its exit code.
    #[test]
    fn each_size_runs_in_a_process_of_its_own() {
        let words: Vec<OsString> = ["ui", "--demo", "--shots", "d"].into_iter().map(OsString::from).collect();
        let mut started = Vec::new();
        each_size_with(3, &words, |line| {
            started.push(line.to_vec());
            Ok(Ok(()))
        })
        .expect("all succeed");
        let want = |index: usize| {
            let mut line = words.clone();
            line.push(format!("--shot-index={index}").into());
            line
        };
        assert_eq!(started, [want(0), want(1), want(2)]);

        let mut count = 0;
        let failed = each_size_with(3, &words, |_| {
            count += 1;
            Ok(if count == 2 { Err(Some(3)) } else { Ok(()) })
        });
        assert!(matches!(failed, Err(ShotError::Size { index: 1, code: Some(3) })), "{failed:?}");
        assert_eq!(count, 2, "the third size is not started");
    }

    /// Review of #492: one size per process, an empty folder is no folder.
    #[test]
    fn an_index_keeps_one_size_and_an_empty_folder_is_refused() {
        let one = shots(&["--demo", "--shots", "d", "--shot-index=2"]).expect("valid").expect("asked for");
        assert_eq!(one.index, Some(2));
        for bad in ["6", "-1", "x"] {
            let result = shots(&["--demo", "--shots", "d", "--shot-index", bad]);
            assert!(matches!(&result, Err(ShotError::BadIndex(value)) if value == bad), "{bad}: {result:?}");
        }
        assert!(matches!(shots(&["--demo", "--shots="]), Err(ShotError::MissingValue("--shots"))));
    }

    #[test]
    fn the_prefix_is_safe_in_a_file_name() {
        let shots = Shots::from_args("torroassistant", &args(&["--demo", "--shots", "d", "--open", "bot:Ab/c d"]))
            .expect("valid")
            .expect("asked for");
        assert_eq!(shots.prefix, "torroassistant-bot--b-c-d");
    }

    #[derive(Debug, Clone)]
    enum Never {}

    /// A program that is one colour, rendered without a window: one picture at twice the size, the
    /// colour in its middle.
    #[test]
    fn a_program_is_rendered_to_a_picture_without_a_window() {
        use iced::widget::container;

        const FILL: iced::Color = iced::Color::from_rgb8(0x33, 0x99, 0xcc);
        fn view(_: &()) -> iced::Element<'_, Never> {
            container(iced::widget::Space::new())
                .width(iced::Fill)
                .height(iced::Fill)
                .style(|_| container::Style::default().background(FILL))
                .into()
        }
        let application = iced::application(|| (), |_: &mut (), _: Never| iced::Task::none(), view);
        let dir = std::env::temp_dir().join(format!("torro-gui-shots-{}", std::process::id()));
        let line = args(&["--demo", "--shots", dir.to_str().expect("utf-8 temp dir"), "--shot-size", "40x30"]);
        let shots = Shots::from_args("test", &line).expect("valid").expect("asked for");

        let written = write(&application, &shots).expect("written");

        assert_eq!(written, [dir.join("test-40x30.png")]);
        let decoder = png::Decoder::new(io::BufReader::new(File::open(&written[0]).expect("open")));
        let mut reader = decoder.read_info().expect("a png");
        let mut pixels = vec![0; reader.output_buffer_size().expect("a size")];
        let info = reader.next_frame(&mut pixels).expect("a frame");
        assert_eq!((info.width, info.height, info.color_type), (80, 60, png::ColorType::Rgba));
        let middle = ((30 * 80 + 40) * 4) as usize;
        let want = [0x33, 0x99, 0xcc, 0xff];
        for (channel, (&got, &want)) in pixels[middle..middle + 4].iter().zip(&want).enumerate() {
            assert!(got.abs_diff(want) <= 2, "channel {channel}: {got} instead of {want}");
        }
        std::fs::remove_dir_all(&dir).expect("clean up");
    }
}
