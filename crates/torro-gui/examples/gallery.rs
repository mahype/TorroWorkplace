//! Gallery of every style in `torro_gui::style`, the building blocks of `torro_gui::widgets` and
//! an example window built with `torro_gui::shell`.
//!
//! `cargo run -p torro-gui --example gallery` shows the styles (dark), `-- --light` in light mode,
//! `-- --blocks` the building blocks, `-- --parts` the states and feedback of everything that answers
//! a click (choice chips, label choices, segments, the busy primary button; task G.10), the blocks of
//! the compact Appearance page (task L.18: the segment with symbols, the heading with quiet actions,
//! the preview tiles in every state and a live grid), the small
//! parts of the tasks redesign (property
//! lines, label chips, state pills, progress bars) and every bundled icon, `-- --cards` the blocks of the
//! inbox's comfortable density (task E8.2: cards, card heads, chips, source tiles, avatars and
//! reading text), `-- --mailbar` the mail's action rail (task E14.1: collapsed, with an open group
//! at the top and one at the foot, without its button as in the panel, expanded; an item off) and
//! the mail window's parts (task
//! E11.1: the styles `cell`, `pill` and the flat input in every state, the attachment chips; task M12.5:
//! the split chips with their menu, the attachment block with "Save all", a menu with details and tags),
//! `-- --detail` the blocks of a detail page in the settings (tasks DS.3, DS.4: head, tabs, open
//! question, figures, shares with a legend, setting pairs, the save bar, the quiet branch), `-- --window` the example "Tasks" window (sample data from
//! `docs/prototypes/linear/tasks.html`) in the layout that fits the window size – resize it to see
//! wide, medium and narrow. `--open nav|switcher|drawer` opens that menu at start, `--open panel`
//! shows the selected task in the side panel of `torro_gui::panel` instead of the detail: beside
//! the list in the wide layout, over the window in medium and narrow (a click on a task opens it, ✕,
//! Escape or a click on the dimmed window closes it). The "Styles ·
//! Blocks · Window" switch in the header changes the page, "Dark · Light" the mode.
//!
//! `-- --design <id>` draws everything with the design `<id>`: the built-in `compact` and `console`
//! (issue #291), or a design of its own from the designs folder (`TORRO_DATA_HOME/designs`), as
//! `--design` does in the apps; an id that is not a valid design is said on stderr and the standard
//! stays. `--font` and `--color-scheme` do the same for the font (e.g. `plex`) and the colours.
//!
//! `--shot <file.bmp>` saves the window as a picture after three seconds and quits (task L.34): a
//! visual check where no tool may capture other windows (macOS without screen recording rights).
//!
//! States are forced through the style closure (`.style(move |theme, _| f(theme,
//! Status::Hovered))`), so one screenshot shows hover, pressed, focus and disabled at once; the
//! "Live" section has real widgets to try by hand.
//!
//! A developer tool, not part of an app: its own labels are not translated.

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, radio, responsive, row, rule, scrollable, text_editor,
    text_input, toggler,
};
use iced::{Alignment, Background, Color, Element, Fill, Length, Subscription, Theme, border};
use torro_core::AppId;
use torro_gui::icon::{self, Icon, icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::panel;
use torro_gui::shell::{self, Switcher};
use torro_gui::style;
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::typo;
use torro_gui::widgets::{
    self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Status, TileFace, TileSize, Trail,
};

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();
    let start = Start::from_args(&args);
    // Before the settings load the fonts, so the first frame has the pack's. `--design`, `--font` and
    // `--color-scheme` each take an id of their own folder (task L.22).
    use torro_gui::design::{self, Forced, Part};
    let part = |flag: &str, part: Part| {
        let id = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1))?;
        if design::is_choice(part, id) {
            return Some(id.clone());
        }
        eprintln!("gallery: no valid {flag} {id:?}; valid: {}", design::choices(part).join(", "));
        None
    };
    let forced = Forced {
        design: part("--design", Part::Design),
        font: part("--font", Part::Font),
        color_scheme: part("--color-scheme", Part::ColorScheme),
    };
    design::start(None, &forced);
    let size = match start.page {
        Page::Window => iced::Size::new(1440.0, 900.0),
        Page::Parts => iced::Size::new(3200.0, 840.0),
        Page::Cards => iced::Size::new(1720.0, 1000.0),
        Page::Mailbar => iced::Size::new(2160.0, 1180.0),
        Page::Detail => iced::Size::new(1760.0, 1100.0),
        _ => iced::Size::new(3700.0, 1060.0),
    };
    let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    iced::application(move || Gallery::new(start, shot.clone()), Gallery::step, Gallery::view)
        .settings(torro_gui::settings())
        .title("Torro style gallery")
        .theme(Gallery::theme)
        .subscription(Gallery::subscription)
        .window(window(size))
        .run()
}

fn window(size: iced::Size) -> iced::window::Settings {
    iced::window::Settings {
        size,
        min_size: Some(iced::Size::new(200.0, 120.0)),
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "torro-gallery".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Styles,
    Blocks,
    /// The small parts of the tasks redesign (R.1) and every bundled icon.
    Parts,
    /// The blocks of the inbox's comfortable density (task E8.2).
    Cards,
    /// The mail window's large buttons (task E11.1).
    Mailbar,
    /// The pills that say what a mail is (task E12.7).
    Categories,
    /// The blocks of a detail page in the settings (tasks DS.3, DS.4).
    Detail,
    Window,
}

/// The menus of the example window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Switcher,
    Nav,
}

/// What the command line asks for.
#[derive(Debug, Clone, Copy)]
struct Start {
    light: bool,
    page: Page,
    menu: Option<Menu>,
    drawer: bool,
    panel: bool,
}

impl Start {
    fn from_args(args: &[String]) -> Self {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let open = args.iter().position(|a| a == "--open").and_then(|i| args.get(i + 1)).map(String::as_str);
        let page = if has("--window") {
            Page::Window
        } else if has("--blocks") {
            Page::Blocks
        } else if has("--parts") {
            Page::Parts
        } else if has("--cards") {
            Page::Cards
        } else if has("--mailbar") {
            Page::Mailbar
        } else if has("--categories") {
            Page::Categories
        } else if has("--detail") {
            Page::Detail
        } else {
            Page::Styles
        };
        let menu = match open {
            Some("nav") => Some(Menu::Nav),
            Some("switcher") => Some(Menu::Switcher),
            _ => None,
        };
        Self { light: has("--light"), page, menu, drawer: open == Some("drawer"), panel: open == Some("panel") }
    }
}

struct Gallery {
    /// `--shot`: where to save the window as a picture, after which the gallery quits.
    shot: Option<std::path::PathBuf>,
    light: bool,
    page: Page,
    section: usize,
    query: String,
    /// The words in the search field of the menu with search (issue #299).
    search: String,
    notes: text_editor::Content,
    view: Option<&'static str>,
    checked: bool,
    toggled: bool,
    choice: Option<u8>,
    /// Example window: the open menu, the drawer, the selected task and which tasks are done.
    menu: Option<Menu>,
    drawer: bool,
    /// The selected task is shown in the side panel instead of the detail (`--open panel`), and
    /// whether that panel is open.
    side_panel: bool,
    panel: bool,
    selected: usize,
    done: Vec<bool>,
    /// "States and feedback" on the parts page: the chosen template (one of), the chosen labels
    /// (several), the segments and whether "Create list" is busy.
    template: Option<usize>,
    labels: [bool; 4],
    show: usize,
    state: usize,
    busy: bool,
    /// The cards page (task E8.2): the card under the "cursor".
    card: usize,
    /// The Appearance blocks (task L.18): the chosen colour mode, the chosen tile and whether the
    /// reasons of the unusable tile are shown.
    mode: usize,
    tile: usize,
    reasons: bool,
    /// The detail page (tasks DS.3, DS.4): the chosen tab.
    tab: usize,
}

#[derive(Debug, Clone)]
enum Msg {
    Noop,
    Light(bool),
    Page(Page),
    Section(usize),
    Query(String),
    Search(String),
    Notes(text_editor::Action),
    View(&'static str),
    Check(bool),
    Toggle(bool),
    Choice(u8),
    Switcher(Switcher),
    OpenMenu(Menu),
    CloseMenu,
    Drawer,
    /// Escape: closes the menu or drawer, else the side panel.
    Escape,
    ClosePanel,
    Select(usize),
    Done(usize, bool),
    Template(usize),
    Label(usize),
    Show(usize),
    State(usize),
    Busy(bool),
    Card(usize),
    Mode(usize),
    Tile(usize),
    Reasons,
    Tab(usize),
    /// `--shot`: take the picture now, then save it.
    Shoot,
    Shot(iced::window::Screenshot),
}

const SECTIONS: [&str; 5] = ["Buttons", "Rows", "Inputs", "Checks", "Surfaces"];
const VIEWS: [&str; 4] = ["Day", "Week", "Month", "Agenda"];
/// Width of the style names in front of each line of states.
const NAME_WIDTH: f32 = 160.0;

type ButtonStyle = fn(&Theme, button::Status) -> button::Style;

impl Gallery {
    fn new(start: Start, shot: Option<std::path::PathBuf>) -> Self {
        Self {
            shot,
            light: start.light,
            page: start.page,
            section: 0,
            query: String::new(),
            search: String::new(),
            notes: text_editor::Content::with_text("Bring the printed agenda.\nAsk about the budget."),
            view: Some("Week"),
            checked: true,
            toggled: true,
            choice: Some(1),
            menu: start.menu,
            drawer: start.drawer,
            side_panel: start.panel,
            panel: start.panel,
            selected: 2,
            done: TASKS.iter().map(|t| t.done).collect(),
            template: Some(1),
            labels: [true, false, true, false],
            show: 0,
            state: 0,
            busy: false,
            card: 0,
            mode: 0,
            tile: 0,
            reasons: true,
            tab: 0,
        }
    }

    fn theme(&self) -> Theme {
        if self.light { theme::light() } else { theme::theme() }
    }

    fn subscription(&self) -> Subscription<Msg> {
        let shot = match self.shot {
            Some(_) => iced::time::every(std::time::Duration::from_secs(3)).map(|_| Msg::Shoot),
            None => Subscription::none(),
        };
        if self.menu.is_some() || self.drawer || self.panel {
            Subscription::batch([widgets::escape().map(|()| Msg::Escape), shot])
        } else {
            shot
        }
    }

    /// [`Gallery::update`], and `--shot`: the picture of the window is taken as a task.
    fn step(&mut self, msg: Msg) -> iced::Task<Msg> {
        match msg {
            Msg::Shoot => iced::window::latest().and_then(iced::window::screenshot).map(Msg::Shot),
            Msg::Shot(picture) => {
                if let Some(path) = self.shot.take()
                    && let Err(err) = std::fs::write(&path, bmp(&picture))
                {
                    eprintln!("gallery: can't save {}: {err}", path.display());
                }
                iced::exit()
            }
            other => {
                self.update(other);
                iced::Task::none()
            }
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Noop => {}
            Msg::Light(light) => self.light = light,
            Msg::Page(page) => self.page = page,
            Msg::Section(i) => self.section = i,
            Msg::Query(q) => self.query = q,
            Msg::Search(words) => self.search = words,
            Msg::Notes(action) => self.notes.perform(action),
            Msg::View(v) => self.view = Some(v),
            Msg::Check(c) => self.checked = c,
            Msg::Toggle(t) => self.toggled = t,
            Msg::Choice(c) => self.choice = Some(c),
            Msg::Switcher(Switcher::Toggle) => {
                self.menu = if self.menu == Some(Menu::Switcher) { None } else { Some(Menu::Switcher) };
            }
            // The gallery has no core to start apps; it only closes the menu.
            Msg::Switcher(_) | Msg::CloseMenu => {
                self.menu = None;
                self.drawer = false;
            }
            Msg::OpenMenu(menu) => self.menu = Some(menu),
            Msg::Drawer => self.drawer = !self.drawer,
            Msg::Escape if self.menu.is_some() || self.drawer => {
                self.menu = None;
                self.drawer = false;
            }
            Msg::Escape | Msg::ClosePanel => self.panel = false,
            Msg::Select(i) => {
                self.selected = i;
                self.menu = None;
                self.panel = self.side_panel;
            }
            Msg::Done(i, done) => {
                if let Some(d) = self.done.get_mut(i) {
                    *d = done;
                }
            }
            // One of: a second click on the chosen template clears it.
            Msg::Template(i) => self.template = if self.template == Some(i) { None } else { Some(i) },
            Msg::Label(i) => {
                if let Some(on) = self.labels.get_mut(i) {
                    *on = !*on;
                }
            }
            Msg::Show(i) => self.show = i,
            Msg::State(i) => self.state = i,
            Msg::Busy(busy) => self.busy = busy,
            Msg::Card(i) => self.card = i,
            Msg::Mode(i) => self.mode = i,
            Msg::Tile(i) => {
                self.tile = i;
                self.reasons = false;
            }
            Msg::Reasons => self.reasons = !self.reasons,
            Msg::Tab(i) => self.tab = i,
            // Handled in `step`.
            Msg::Shoot | Msg::Shot(_) => {}
        }
    }

    fn palette(&self) -> Palette {
        if self.light { Palette::light() } else { Palette::dark() }
    }

    fn view(&self) -> Element<'_, Msg> {
        match self.page {
            Page::Window => responsive(move |size| self.tasks_window(layout::of(size))).into(),
            Page::Blocks => self.page_frame(self.blocks()),
            Page::Parts => self.page_frame(self.parts()),
            Page::Cards => self.page_frame(self.cards()),
            Page::Mailbar => self.page_frame(mailbar()),
            Page::Categories => self.page_frame(categories()),
            Page::Detail => self.page_frame(self.detail()),
            Page::Styles => self.page_frame(self.styles()),
        }
    }

    fn styles(&self) -> Element<'_, Msg> {
        // Three columns, so one screenshot of a wide window (3700 × 1060) shows every style.
        let first = column![section("Buttons", buttons()), section("Checks", checks())].spacing(space::xxl());
        let second = column![section("Rows", rows())].spacing(space::xxl());
        let third = column![
            section("Inputs", inputs(&self.notes)),
            section("Surfaces", surfaces()),
            section("Live", self.live()),
        ]
        .spacing(space::xxl());
        row![first, second, third].spacing(space::xxl()).padding(space::xl()).into()
    }

    /// A gallery page: sidebar, header and the scrolling `body` beside it, as `torro_gui::shell` lays
    /// out a window – flush with a line when the design's inset is 0 (issue #291), else inset.
    fn page_frame<'a>(&'a self, body: Element<'a, Msg>) -> Element<'a, Msg> {
        let content = column![
            self.header(),
            rule::horizontal(1).style(style::rule),
            scrollable(body).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable),
        ];
        let inset = layout::inset();
        let frame = if inset > 0.0 {
            let panel = container(content).width(Fill).height(Fill).style(style::container::panel);
            row![self.sidebar(), container(panel).padding(iced::Padding { left: 0.0, ..inset.into() })]
        } else {
            let panel = container(content).width(Fill).height(Fill).style(style::container::panel_flush);
            row![self.sidebar(), rule::vertical(1).style(style::rule), panel]
        };
        container(frame).style(style::container::sidebar).into()
    }

    fn sidebar(&self) -> Element<'_, Msg> {
        let items = SECTIONS.iter().enumerate().map(|(i, name)| {
            button(container(typo::text(typo::body(), *name)).center_y(Fill))
                .width(Fill)
                .height(size::row_sidebar())
                .padding(style::ITEM_PADDING)
                .on_press(Msg::Section(i))
                .style(style::button::sidebar_item(i == self.section))
                .into()
        });
        let title = typo::text(typo::caption(), "Sections").style(theme::text_3);
        let list =
            column![container(title).padding(style::ITEM_PADDING).height(size::row_sidebar())].extend(items).spacing(2);
        container(list).width(size::sidebar()).height(Fill).padding(space::s()).style(style::container::sidebar).into()
    }

    fn header(&self) -> Element<'_, Msg> {
        let mode = if self.light { "Light" } else { "Dark" };
        let design = torro_gui::design::active().about.name.clone();
        let title = row![
            typo::text(typo::body_strong(), "Style gallery"),
            typo::text(typo::body(), mode).style(theme::text_3),
            typo::text(typo::body(), design).style(theme::text_3),
        ]
        .spacing(space::s());
        let pages = [Page::Styles, Page::Blocks, Page::Parts, Page::Cards, Page::Mailbar, Page::Detail, Page::Window];
        let active = pages.iter().position(|p| *p == self.page).unwrap_or(0);
        let pages = widgets::segmented(
            [
                ("Styles", Msg::Page(Page::Styles)),
                ("Blocks", Msg::Page(Page::Blocks)),
                ("Parts", Msg::Page(Page::Parts)),
                ("Cards", Msg::Page(Page::Cards)),
                ("Mail", Msg::Page(Page::Mailbar)),
                ("Detail", Msg::Page(Page::Detail)),
                ("Window", Msg::Page(Page::Window)),
            ],
            active,
        );
        let modes =
            widgets::segmented([("Dark", Msg::Light(false)), ("Light", Msg::Light(true))], usize::from(self.light));
        let bar = row![
            title,
            Space::new().width(Fill),
            pages,
            modes,
            text_button("Filter", style::button::ghost, true),
            text_button("New event", style::button::primary, true),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        container(bar)
            .height(size::header())
            .center_y(size::header())
            .padding(iced::Padding { left: space::l(), right: space::m(), ..iced::Padding::ZERO })
            .style(style::container::header)
            .into()
    }

    fn live(&self) -> Element<'_, Msg> {
        let views = segmented(&VIEWS.map(|v| (v, self.view == Some(v), Msg::View(v))));
        column![
            row![
                text_button("Today", style::button::secondary, true),
                text_button("Filter", style::button::ghost, true),
                text_button("Delete event", style::button::danger, true),
                text_button("Save", style::button::primary, true),
                views,
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
            row![
                text_input("Search", &self.query)
                    .on_input(Msg::Query)
                    .padding(style::INPUT_PADDING)
                    .width(260)
                    .style(style::text_input),
                pick_list(VIEWS, self.view, Msg::View)
                    .padding(style::pick_list_padding())
                    .text_size(typo::caption().size)
                    .text_line_height(typo::caption().line_height)
                    .font(typo::caption().font)
                    .style(style::pick_list)
                    .menu_style(style::pick_list_menu),
                checkbox(self.checked)
                    .label("Work")
                    .on_toggle(Msg::Check)
                    .size(style::CHECK_SIZE)
                    .style(|t, s| style::checkbox(palette(t).calendar[0])(t, s)),
                checkbox(self.checked)
                    .label("Buy milk")
                    .on_toggle(Msg::Check)
                    .size(style::CHECK_SIZE)
                    .style(|t, s| style::task_check(palette(t).calendar[1])(t, s)),
                toggler(self.toggled)
                    .label("Start at login")
                    .on_toggle(Msg::Toggle)
                    .size(style::CHECK_SIZE)
                    .style(style::toggler),
                radio("Week starts Monday", 1, self.choice, Msg::Choice).size(style::CHECK_SIZE).style(style::radio),
                radio("Sunday", 0, self.choice, Msg::Choice).size(style::CHECK_SIZE).style(style::radio),
            ]
            .spacing(space::l())
            .align_y(Alignment::Center),
        ]
        .spacing(space::l())
        .into()
    }
}

/// A titled block of the gallery.
fn section<'a>(title: &'a str, content: Element<'a, Msg>) -> Element<'a, Msg> {
    column![typo::text(typo::title(), title), content].spacing(space::l()).into()
}

/// One style: its name, then one cell per state.
fn line<'a>(name: &'a str, cells: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    row![
        typo::text(typo::meta(), name).style(theme::text_2).width(NAME_WIDTH),
        iced::widget::Row::with_children(cells).spacing(space::xl())
    ]
    .align_y(Alignment::Start)
    .into()
}

/// A widget with the state it is drawn in written under it.
fn cell<'a>(state: &'a str, widget: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    column![widget.into(), typo::text(typo::meta(), state).style(theme::text_3)].spacing(space::xs()).into()
}

const BUTTON_STATES: [(&str, button::Status); 4] = [
    ("Normal", button::Status::Active),
    ("Hover", button::Status::Hovered),
    ("Pressed", button::Status::Pressed),
    ("Disabled", button::Status::Disabled),
];

/// A button whose style is drawn in `status` whatever the pointer does.
fn forced<'a>(
    content: impl Into<Element<'a, Msg>>,
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
    status: button::Status,
) -> button::Button<'a, Msg> {
    let b = button(content).style(move |t, _| style(t, status));
    if status == button::Status::Disabled { b } else { b.on_press(Msg::Noop) }
}

impl Gallery {
    /// The blocks of the comfortable density (task E8.2) as the mail and the notifications window
    /// lay them out: mail cards with avatar, chips and their buttons always shown, a column of one
    /// app with card heads and ✕, the source tiles in three sizes, the avatars and reading text
    /// beside body text. A click on a mail card selects it.
    fn cards(&self) -> Element<'_, Msg> {
        let buttons = |words: &[(Icon, &'static str, ButtonStyle)]| -> Element<'static, Msg> {
            let all = words
                .iter()
                .map(|&(which, word, style)| widgets::text_button(Some(which), word, style, Some(Msg::Noop)));
            iced::widget::Row::with_children(all).spacing(space::s()).wrap().vertical_spacing(space::s()).into()
        };
        let mail_card = |i: usize,
                         who: (&'static str, &'static str, &'static str),
                         time: &'static str,
                         subject: &'static str,
                         preview: &'static str,
                         chips: Vec<Element<'static, Msg>>| {
            let (initials, name, domain) = who;
            let sender = row![
                typo::text(typo::body_strong(), name),
                typo::text(typo::meta(), domain).style(theme::text_3),
                Space::new().width(Fill),
                typo::text(typo::meta().tabular(), time).style(theme::text_3),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center);
            let head = row![
                avatar(initials, i, size::avatar()),
                column![sender, typo::text(typo::body_strong(), subject)].width(Fill)
            ]
            .spacing(space::m())
            .align_y(Alignment::Center);
            let mut parts = vec![head.into(), typo::text(typo::body(), preview).style(theme::text_2).into()];
            if !chips.is_empty() {
                parts.push(widgets::chip_row(chips));
            }
            parts.push(buttons(&[
                (Icon::Reply, "Reply", style::button::secondary),
                (Icon::ListTodo, "Task", style::button::ghost),
                (Icon::Check, "Done", style::button::ghost),
                (Icon::Clock, "Later", style::button::ghost),
            ]));
            widgets::card(parts, self.card == i, Some(Msg::Card(i)))
        };
        let attention = self.palette().attention_text;
        let mail = column![
            mail_card(
                0,
                ("AK", "Anna Keller", "kellerbau.de"),
                "09:42",
                "Angebot Dachsanierung – bis Freitag?",
                "Hallo Sven, anbei das überarbeitete Angebot. Kannst du uns bis Freitag Bescheid geben? Dann buchen wir das Gerüst für die Woche danach.",
                vec![
                    widgets::state_pill(Icon::Paperclip, "Angebot_v2.pdf"),
                    widgets::state_pill(Mark::Tinted(Icon::Clock, attention), "Answer by Fri"),
                ],
            ),
            mail_card(
                1,
                ("CL", "Chris Lang", "Steuerbüro Lang"),
                "08:31",
                "Unterlagen Q3",
                "Für die Umsatzsteuer fehlen uns noch drei Belege aus September. Ich habe dir die Liste angehängt.",
                vec![widgets::state_pill(Icon::Paperclip, "3 files")],
            ),
            mail_card(
                2,
                ("MS", "Mara Schulz", "torro.dev"),
                "07:55",
                "Re: Release-Notes 0.2",
                "Ich habe die Liste um die Design-Pakete ergänzt. Magst du einmal drüberschauen?",
                vec![],
            ),
        ]
        .spacing(size::card_gap())
        .width(size::list());

        let app_head = |face: TileFace, tint: Option<Color>, name: &'static str, count: &'static str| {
            row![
                widgets::source_tile(face, TileSize::Medium, tint),
                typo::text(typo::body_strong(), name).width(Fill),
                typo::text(typo::meta().tabular(), count).style(theme::text_3),
                widgets::icon_button(Icon::BellOff, "Mute", Some(Msg::Noop)),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
        };
        let notice =
            |lead: Option<Element<'static, Msg>>, title: &'static str, meta: &'static str, text: &'static str| {
                let head = widgets::card_head(lead, title, meta, Some(Msg::Noop));
                let text = typo::text(typo::body(), text).style(theme::text_2);
                let acts = buttons(&[
                    (Icon::Reply, "Reply", style::button::secondary),
                    (Icon::ListTodo, "Task", style::button::ghost),
                    (Icon::Clock, "Later", style::button::ghost),
                ]);
                widgets::card(vec![head, text.into(), acts], false, None)
            };
        let colours = self.palette().calendar;
        let slack = column![
            app_head(TileFace::letter("Slack"), Some(colours[3]), "Slack", "5"),
            notice(
                None,
                "#dev-torro · Thread",
                "3 new · 10:12",
                "Mara: Kann jemand bis morgen über die Release-Notes schauen? @Sven"
            ),
            notice(
                None,
                "Jonas Brandt",
                "Direct message · 09:58",
                "Hast du heute kurz Zeit für den Workshop-Plan? 15 Minuten reichen."
            ),
            notice(
                None,
                "A channel with a name far too long for its column",
                "Info · 08:40 · and a meta line that runs out of room as well",
                "The title and the meta line are cut with an ellipsis and show the whole as tooltip.",
            ),
        ]
        .spacing(size::card_gap())
        .width(size::props());
        let github = column![
            app_head(TileFace::Icon(Icon::Github), None, "GitHub", "3"),
            notice(
                Some(widgets::source_tile(TileFace::Icon(Icon::GitPullRequest), TileSize::Small, None)),
                "feat(torro-wm): parse hyprland clients",
                "TorroWorkplace #142 · Felix Arnold",
                "Review requested.",
            ),
            widgets::card(
                vec![
                    widgets::card_head(None, "An approval: no ✕, no Later", "Claude Code · 10:20", None),
                    buttons(&[
                        (Icon::Check, "Allow", style::button::secondary),
                        (Icon::X, "Deny", style::button::ghost)
                    ]),
                ],
                false,
                None,
            ),
        ]
        .spacing(size::card_gap())
        .width(size::props());

        // The inbox's services (task E5.15): GitHub, Nextcloud, Talk and ntfy with their Lucide icons.
        let faces = [
            (TileFace::Icon(Icon::Github), None),
            (TileFace::Icon(Icon::Cloud), None),
            (TileFace::Icon(Icon::MessageCircle), None),
            (TileFace::Icon(Icon::RadioTower), None),
            (TileFace::Icon(Icon::Calendar), None),
            (TileFace::letter("whatsapp"), Some(colours[1])),
        ];
        let tiles = column(
            [("Small 20", TileSize::Small), ("Medium 28", TileSize::Medium), ("Large 36", TileSize::Large)].map(
                |(name, size)| {
                    let row = faces.iter().map(|&(face, tint)| widgets::source_tile(face, size, tint));
                    let row = iced::widget::Row::with_children(row).spacing(space::s()).align_y(Alignment::Center);
                    cell(name, row)
                },
            ),
        )
        .spacing(space::m());
        let avatars = row![
            cell("avatar 36", avatar("AK", 0, size::avatar())),
            cell("avatar_lg 40", avatar("BO", 1, size::avatar_lg())),
        ]
        .spacing(space::xl());
        let reading = column![
            cell(
                "read 14/22",
                container(typo::text(typo::read(), "Hallo Sven, anbei das überarbeitete Angebot für die Dachsanierung. Wir haben die Dämmung auf 200 mm angepasst und die Dachrinne komplett neu eingeplant, wie besprochen."))
                    .max_width(size::form_max() / 1.5),
            ),
            cell(
                "body 13/20",
                container(typo::text(typo::body(), "Hallo Sven, anbei das überarbeitete Angebot für die Dachsanierung. Wir haben die Dämmung auf 200 mm angepasst und die Dachrinne komplett neu eingeplant, wie besprochen.").style(theme::text_2))
                    .max_width(size::form_max() / 1.5),
            ),
        ]
        .spacing(space::l());
        let side = column![
            section("Source tiles", tiles.into()),
            section("Avatars", avatars.into()),
            section("Reading text", reading.into()),
        ]
        .spacing(space::xl());

        row![
            section("Mail cards", mail.into()),
            section("A column per app", slack.into()),
            section("Card heads", github.into()),
            side,
        ]
        .spacing(space::xxl())
        .padding(space::xl())
        .into()
    }
}

/// A round avatar with `initials` on the `colour`-th calendar colour as a tinted surface, `edge`
/// wide: what a mail card shows beside its lines (the gallery's own; the inbox has its avatar).
fn avatar<'a>(initials: &'a str, colour: usize, edge: f32) -> Element<'a, Msg> {
    container(typo::text(typo::caption(), initials).style(theme::text))
        .center(edge)
        .style(move |t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(theme::over(
                    p.tinted(p.calendar[colour % p.calendar.len()]),
                    p.panel,
                ))),
                border: border::rounded(edge / 2.0),
                ..container::Style::default()
            }
        })
        .into()
}

fn label(text: &str) -> Element<'_, Msg> {
    container(typo::text(typo::caption(), text)).center_y(Fill).into()
}

fn text_button<'a>(text: &'a str, style: ButtonStyle, enabled: bool) -> Element<'a, Msg> {
    let b = button(label(text)).height(size::control_sm()).padding(style::button_padding()).style(style);
    if enabled { b.on_press(Msg::Noop).into() } else { b.into() }
}

fn segmented<'a>(options: &[(&'a str, bool, Msg)]) -> Element<'a, Msg> {
    let segments = options.iter().map(|(name, active, msg)| {
        button(label(name))
            .height(size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0)
            .padding(style::button_padding())
            .on_press(msg.clone())
            .style(style::button::segment(*active))
            .into()
    });
    container(row(segments)).padding(style::SEGMENT_INSET).style(style::container::segmented).into()
}

fn buttons() -> Element<'static, Msg> {
    let text_styles: [(&str, &str, ButtonStyle); 4] = [
        ("primary", "New event", style::button::primary),
        ("secondary", "Today", style::button::secondary),
        ("ghost", "Filter", style::button::ghost),
        ("danger", "Delete event", style::button::danger),
    ];
    let mut lines: Vec<Element<'_, Msg>> = text_styles
        .into_iter()
        .map(|(name, text, style)| {
            let cells = BUTTON_STATES
                .iter()
                .map(|&(state, status)| {
                    cell(
                        state,
                        forced(label(text), style, status).height(size::control_sm()).padding(style::button_padding()),
                    )
                })
                .collect();
            line(name, cells)
        })
        .collect();
    let icon_styles: [(&str, ButtonStyle); 2] = [("icon", style::button::icon), ("boxed", style::button::boxed)];
    for (name, style) in icon_styles {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let plus = container(typo::text(typo::body(), "+")).center(Fill);
                cell(state, forced(plus, style, status).width(size::control_sm()).height(size::control_sm()).padding(0))
            })
            .collect();
        lines.push(line(name, cells));
    }
    for active in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let seg = forced(label("Week"), style::button::segment(active), status)
                    .height(size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0)
                    .padding(style::button_padding());
                cell(state, container(seg).padding(style::SEGMENT_INSET).style(style::container::segmented))
            })
            .collect();
        lines.push(line(if active { "segment(active)" } else { "segment" }, cells));
    }
    column(lines).spacing(space::l()).into()
}

fn rows() -> Element<'static, Msg> {
    let mut lines = Vec::new();
    for active in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let content = row![
                    typo::text(typo::body(), "Today"),
                    Space::new().width(Fill),
                    typo::text(typo::meta().tabular(), "5").style(theme::text_3)
                ]
                .align_y(Alignment::Center);
                let item = forced(container(content).center_y(Fill), style::button::sidebar_item(active), status)
                    .width(size::sidebar() - 2.0 * space::s() - 2.0 * space::xs())
                    .height(size::row_sidebar())
                    .padding(style::ITEM_PADDING);
                cell(state, container(item).padding(space::xs()).style(style::container::sidebar))
            })
            .collect();
        lines.push(line(if active { "sidebar_item(active)" } else { "sidebar_item" }, cells));
    }
    for selected in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let content = row![
                    typo::text(typo::body_strong(), "Review API concept"),
                    Space::new().width(Fill),
                    typo::text(typo::meta().tabular(), "Oct 14").style(theme::text_3)
                ]
                .align_y(Alignment::Center);
                // Issue #291: on the chosen row of a design with `selection = "fill"` (Console) the cells
                // are drawn in `on_accent`.
                let content = widgets::row_content(content, selected);
                let item = forced(container(content).center_y(Fill), style::button::list_row(selected), status)
                    .width(240)
                    .height(size::row())
                    .padding(iced::Padding { left: space::l(), right: space::l(), ..iced::Padding::ZERO });
                cell(state, column![item, rule::horizontal(1).style(style::rule)].width(240))
            })
            .collect();
        lines.push(line(if selected { "list_row(selected)" } else { "list_row" }, cells));
    }
    for highlighted in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let content = row![
                    typo::text(typo::body(), "Week"),
                    Space::new().width(Fill),
                    container(typo::text(typo::caption(), "W"))
                        .padding(style::KBD_PADDING)
                        .style(style::container::kbd)
                ]
                .align_y(Alignment::Center);
                let item = forced(container(content).center_y(Fill), style::button::menu_item(highlighted), status)
                    .width(200)
                    .height(style::MENU_ITEM_HEIGHT)
                    .padding(style::ITEM_PADDING);
                cell(state, container(item).padding(style::MENU_INSET).style(style::container::menu))
            })
            .collect();
        lines.push(line(if highlighted { "menu_item(highlighted)" } else { "menu_item" }, cells));
    }
    column(lines).spacing(space::l()).into()
}

fn inputs(notes: &text_editor::Content) -> Element<'_, Msg> {
    let input_states: [(&str, text_input::Status, &str); 4] = [
        ("Normal", text_input::Status::Active, ""),
        ("Hover", text_input::Status::Hovered, ""),
        ("Focused", text_input::Status::Focused { is_hovered: false }, "Review API concept"),
        ("Disabled", text_input::Status::Disabled, "Review API concept"),
    ];
    let inputs = input_states
        .into_iter()
        .map(|(state, status, value)| {
            let input = text_input("Search", value)
                .padding(style::INPUT_PADDING)
                .width(220)
                .style(move |t, _| style::text_input(t, status));
            let input = if status == text_input::Status::Disabled { input } else { input.on_input(|_| Msg::Noop) };
            cell(state, input)
        })
        .collect();
    let editor_states: [(&str, text_editor::Status); 4] = [
        ("Normal", text_editor::Status::Active),
        ("Hover", text_editor::Status::Hovered),
        ("Focused", text_editor::Status::Focused { is_hovered: false }),
        ("Disabled", text_editor::Status::Disabled),
    ];
    let editors = editor_states
        .into_iter()
        .map(|(state, status)| {
            let editor = text_editor(notes)
                .padding(style::INPUT_PADDING)
                .width(220)
                .height(72)
                .style(move |t, _| style::text_editor(t, status));
            let editor = if status == text_editor::Status::Disabled { editor } else { editor.on_action(Msg::Notes) };
            cell(state, editor)
        })
        .collect();
    let pick_states: [(&str, pick_list::Status); 3] = [
        ("Normal", pick_list::Status::Active),
        ("Hover", pick_list::Status::Hovered),
        ("Open", pick_list::Status::Opened { is_hovered: false }),
    ];
    let mut picks: Vec<Element<'_, Msg>> = pick_states
        .into_iter()
        .map(|(state, status)| {
            let pick = pick_list(VIEWS, Some("Week"), |_| Msg::Noop)
                .padding(style::pick_list_padding())
                .text_size(typo::caption().size)
                .text_line_height(typo::caption().line_height)
                .font(typo::caption().font)
                .style(move |t, _| style::pick_list(t, status));
            cell(state, pick)
        })
        .collect();
    picks.push(cell("Menu (pick_list_menu)", pick_menu()));
    column![line("text_input", inputs), line("text_editor", editors), line("pick_list", picks)]
        .spacing(space::l())
        .into()
}

/// The pick list's menu drawn in place: Iced opens it only on click.
fn pick_menu() -> Element<'static, Msg> {
    let items = VIEWS.iter().map(|v| {
        let hovered = *v == "Week";
        container(typo::text(typo::body(), *v))
            .width(160)
            .padding(style::button_padding())
            .center_y(style::MENU_ITEM_HEIGHT)
            .style(move |t: &Theme| {
                let m = style::pick_list_menu(t);
                container::Style {
                    text_color: Some(if hovered { m.selected_text_color } else { m.text_color }),
                    background: hovered.then_some(m.selected_background),
                    ..container::Style::default()
                }
            })
            .into()
    });
    container(column(items))
        .style(|t: &Theme| {
            let m = style::pick_list_menu(t);
            container::Style {
                background: Some(m.background),
                border: m.border,
                shadow: m.shadow,
                text_color: Some(m.text_color),
                ..container::Style::default()
            }
        })
        .into()
}

fn checks() -> Element<'static, Msg> {
    let check_states: [(&str, checkbox::Status); 6] = [
        ("Empty", checkbox::Status::Active { is_checked: false }),
        ("Empty, hover", checkbox::Status::Hovered { is_checked: false }),
        ("Checked", checkbox::Status::Active { is_checked: true }),
        ("Checked, hover", checkbox::Status::Hovered { is_checked: true }),
        ("Disabled", checkbox::Status::Disabled { is_checked: false }),
        ("Checked, disabled", checkbox::Status::Disabled { is_checked: true }),
    ];
    let is_checked = |s: checkbox::Status| match s {
        checkbox::Status::Active { is_checked }
        | checkbox::Status::Hovered { is_checked }
        | checkbox::Status::Disabled { is_checked } => is_checked,
    };
    let check_line = |name: &'static str, task: bool| {
        let cells = check_states
            .into_iter()
            .map(|(state, status)| {
                let b = checkbox(is_checked(status)).size(style::CHECK_SIZE).style(move |t: &Theme, _| {
                    let colour = palette(t).calendar[0];
                    if task { style::task_check(colour)(t, status) } else { style::checkbox(colour)(t, status) }
                });
                cell(state, b)
            })
            .collect();
        line(name, cells)
    };
    let colours = |task: bool| -> Element<'static, Msg> {
        let boxes = (0..5).map(|i| {
            checkbox(true)
                .size(style::CHECK_SIZE)
                .style(move |t: &Theme, _| {
                    let colour = palette(t).calendar[i];
                    let status = checkbox::Status::Active { is_checked: true };
                    if task { style::task_check(colour)(t, status) } else { style::checkbox(colour)(t, status) }
                })
                .into()
        });
        row(boxes).spacing(space::s()).into()
    };
    let toggle_states: [(&str, toggler::Status); 6] = [
        ("Off", toggler::Status::Active { is_toggled: false }),
        ("Off, hover", toggler::Status::Hovered { is_toggled: false }),
        ("On", toggler::Status::Active { is_toggled: true }),
        ("On, hover", toggler::Status::Hovered { is_toggled: true }),
        ("Off, disabled", toggler::Status::Disabled { is_toggled: false }),
        ("On, disabled", toggler::Status::Disabled { is_toggled: true }),
    ];
    let togglers = toggle_states
        .into_iter()
        .map(|(state, status)| {
            let on = matches!(
                status,
                toggler::Status::Active { is_toggled: true }
                    | toggler::Status::Hovered { is_toggled: true }
                    | toggler::Status::Disabled { is_toggled: true }
            );
            cell(state, toggler(on).size(style::CHECK_SIZE).style(move |t, _| style::toggler(t, status)))
        })
        .collect();
    let radio_states: [(&str, radio::Status); 4] = [
        ("Off", radio::Status::Active { is_selected: false }),
        ("Off, hover", radio::Status::Hovered { is_selected: false }),
        ("On", radio::Status::Active { is_selected: true }),
        ("On, hover", radio::Status::Hovered { is_selected: true }),
    ];
    let radios = radio_states
        .into_iter()
        .map(|(state, status)| {
            let on = matches!(
                status,
                radio::Status::Active { is_selected: true } | radio::Status::Hovered { is_selected: true }
            );
            let r = radio("", 1u8, on.then_some(1u8), |_| Msg::Noop)
                .size(style::CHECK_SIZE)
                .style(move |t, _| style::radio(t, status));
            cell(state, r)
        })
        .collect();
    column![
        check_line("checkbox (calendar)", false),
        check_line("task_check (list)", true),
        line("all calendar colours", vec![cell("checkbox", colours(false)), cell("task_check", colours(true))]),
        line("toggler", togglers),
        line("radio", radios),
    ]
    .spacing(space::l())
    .into()
}

fn surfaces() -> Element<'static, Msg> {
    type ContainerStyle = fn(&Theme) -> container::Style;
    let boxes: [(&str, ContainerStyle); 6] = [
        ("panel", style::container::panel),
        ("panel_flush", style::container::panel_flush),
        ("sidebar", style::container::sidebar),
        ("group", style::container::group),
        ("menu", style::container::menu),
        ("popover", style::container::popover),
    ];
    let boxes = boxes
        .into_iter()
        .map(|(name, s)| {
            container(typo::text(typo::body(), name).style(theme::text_2))
                .width(160)
                .height(96)
                .padding(space::l())
                .style(s)
                .into()
        })
        .collect::<Vec<Element<'_, Msg>>>();
    // Surfaces sit on the window background, as in the app: the panel shows its edge there.
    let well = container(iced::widget::Row::with_children(boxes).spacing(space::l()))
        .padding(space::l())
        .style(style::container::sidebar);
    let chip = container(
        row![typo::text(typo::caption(), "Today"), typo::text(typo::caption(), "Work").style(theme::text_3)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .height(style::CHIP_HEIGHT)
    .center_y(style::CHIP_HEIGHT)
    .padding(style::CHIP_PADDING)
    .style(style::container::chip);
    let kbd = container(typo::text(typo::caption(), "Ctrl K")).padding(style::KBD_PADDING).style(style::container::kbd);
    let header = container(typo::text(typo::body_strong(), "Header"))
        .width(320)
        .center_y(size::header())
        .padding(iced::Padding { left: space::l(), ..iced::Padding::ZERO })
        .style(style::container::header);
    let scroll_states: [(&str, scrollable::Status); 3] = [
        (
            "Normal",
            scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            },
        ),
        (
            "Hover on bar",
            scrollable::Status::Hovered {
                is_horizontal_scrollbar_hovered: false,
                is_vertical_scrollbar_hovered: true,
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            },
        ),
        (
            "Dragged",
            scrollable::Status::Dragged {
                is_horizontal_scrollbar_dragged: false,
                is_vertical_scrollbar_dragged: true,
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            },
        ),
    ];
    let scrolls = scroll_states
        .into_iter()
        .map(|(state, status)| {
            let lines = (1..=12).map(|i| typo::text(typo::body(), format!("Row {i}")).into());
            let list = scrollable(column(lines).width(Fill))
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(move |t, _| style::scrollable(t, status))
                .width(160)
                .height(96);
            cell(state, container(list).padding(space::s()).style(style::container::panel))
        })
        .collect();
    column![
        well,
        line(
            "chip · kbd · header",
            vec![
                cell("chip", chip),
                cell("kbd", kbd),
                cell("header + rule", column![header, rule::horizontal(1).style(style::rule)].width(Length::Shrink))
            ]
        ),
        line("scrollable", scrolls),
    ]
    .spacing(space::l())
    .into()
}

// ---------------------------------------------------------------------------------------------
// Building blocks and the example window (task F.5). Sample data as in
// `docs/prototypes/linear/tasks.html`.
// ---------------------------------------------------------------------------------------------

/// A task of the example window. `list` indexes the default list colours (`Palette::calendar`).
struct Task {
    title: &'static str,
    priority: u8,
    urgent: bool,
    list: usize,
    list_name: &'static str,
    due: &'static str,
    late: bool,
    done: bool,
}

const fn task(title: &'static str, priority: u8, list: usize, list_name: &'static str, due: &'static str) -> Task {
    Task { title, priority, urgent: false, list, list_name, due, late: false, done: false }
}

const TASKS: [Task; 9] = [
    Task { urgent: true, late: true, ..task("File invoices for September", 0, 0, "Work", "Fri 9") },
    Task { late: true, ..task("Call the insurance about the bike", 1, 1, "Personal", "Thu 8") },
    task("Review API concept", 2, 0, "Work", "16:00"),
    task("Buy Mum's present", 3, 2, "Family", "18:00"),
    Task { done: true, ..task("Book train tickets", 0, 1, "Personal", "") },
    task("Prepare sprint demo", 2, 0, "Work", "Mon 12"),
    task("Groceries for the weekend", 0, 4, "Groceries", "Fri 16"),
    task("Renew passport", 1, 1, "Personal", "Oct 30"),
    task("Plan Christmas with the family", 0, 2, "Family", "Nov 15"),
];

/// The groups of the list: their tasks are `TASKS[range]`.
const GROUPS: [(&str, std::ops::Range<usize>); 3] = [("Overdue", 0..2), ("Today", 2..5), ("Upcoming", 5..9)];

/// An icon in `text_3` (the quiet icons of the example detail).
fn quiet<'a>(which: Icon, edge: f32) -> Element<'a, Msg> {
    let quiet: Element<'static, Msg> =
        icon(which, edge).style(|t, _| iced::widget::svg::Style { color: Some(palette(t).text_3) }).into();
    quiet
}

/// An icon tinted with a palette colour that follows the mode.
fn coloured_svg(
    colour: impl Fn(&Palette) -> Color + 'static,
) -> impl Fn(&Theme, iced::widget::svg::Status) -> iced::widget::svg::Style {
    move |t, _| iced::widget::svg::Style { color: Some(colour(&palette(t))) }
}

fn coloured(colour: impl Fn(&Palette) -> Color + 'static) -> impl Fn(&Theme) -> iced::widget::text::Style {
    move |t| iced::widget::text::Style { color: Some(colour(&palette(t))) }
}

/// Linear's priority: three bars rising, the first `level` in `text_2`; urgent is an orange square
/// with "!". Task-specific, so it lives with the tasks app; drawn here for the example.
fn priority<'a>(level: u8, urgent: bool) -> Element<'a, Msg> {
    if urgent {
        return container(typo::text(typo::caption(), "!").style(coloured(|_| Palette::dark().bg)))
            .center(icon::SIZE_SM)
            .style(|t| container::Style {
                background: Some(Background::Color(palette(t).attention)),
                border: border::rounded(3),
                ..container::Style::default()
            })
            .into();
    }
    let bars = [5.0, 8.0, 12.0].into_iter().enumerate().map(|(i, height)| {
        let on = (i as u8) < level;
        container(Space::new().width(3).height(height))
            .style(move |t| {
                let p = palette(t);
                container::Style {
                    background: Some(Background::Color(if on { p.text_2 } else { p.line_strong })),
                    border: border::rounded(1),
                    ..container::Style::default()
                }
            })
            .into()
    });
    container(row(bars).spacing(2).align_y(Alignment::End)).width(icon::SIZE_SM).height(12).align_bottom(12).into()
}

/// A ring of `edge` in `colour`, 2 px (list marks outside the building blocks).
fn small_ring<'a>(colour: Color, edge: f32) -> Element<'a, Msg> {
    container(Space::new().width(edge).height(edge))
        .style(move |_| container::Style {
            border: border::rounded(edge / 2.0).color(colour).width(2),
            ..container::Style::default()
        })
        .into()
}

fn task_check<'a>(done: bool, colour: usize, on_toggle: Option<Msg>) -> Element<'a, Msg> {
    let b =
        checkbox(done).size(style::CHECK_SIZE).style(move |t, s| style::task_check(palette(t).calendar[colour])(t, s));
    match on_toggle {
        Some(msg) => b.on_toggle(move |_| msg.clone()).into(),
        None => b.into(),
    }
}

impl Gallery {
    /// The example window in the layout of `class`.
    fn tasks_window(&self, class: SizeClass) -> Element<'_, Msg> {
        let (switcher, nav) = (Anchor::new(), Anchor::new());
        let sidebar = self.tasks_sidebar(&switcher);
        let header = self.tasks_header(class, &nav);
        let task_panel = self.panel.then(|| self.task_panel());
        // The panel takes the place of the properties column while it is beside the list.
        let props = (shell::has_props(class) && !self.side_panel).then(|| self.tasks_props());
        let layer = self.menu.map(|menu| match menu {
            Menu::Switcher => widgets::floating(
                &switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(AppId::TASKS, Msg::Switcher),
                Msg::CloseMenu,
            ),
            Menu::Nav => widgets::floating(&nav, Placement::Below(Align::Start), self.nav_menu(), Msg::CloseMenu),
        });
        let body = if self.side_panel { self.task_list(class) } else { self.tasks_body(class) };
        let placed = panel::place(class, body, task_panel, layer);
        shell::shell(class, sidebar, header, placed.body, props)
            .drawer(self.drawer, Msg::CloseMenu)
            .overlay(placed.overlay)
            .into()
    }

    fn tasks_sidebar<'a>(&'a self, switcher: &Anchor) -> Element<'a, Msg> {
        let p = self.palette();
        let lists = [("Work", 0, 6), ("Personal", 1, 4), ("Family", 2, 3), ("Groceries", 4, 5)];
        let mut items = vec![
            widgets::sidebar_item(Icon::Inbox, "Inbox", Trail::Count(2), false, Msg::Noop),
            widgets::sidebar_item(Icon::Sun, "Today", Trail::Count(5), true, Msg::Noop),
            widgets::sidebar_item(Icon::CalendarClock, "Upcoming", Trail::Count(9), false, Msg::Noop),
            widgets::sidebar_item(Icon::Layers, "Anytime", Trail::None, false, Msg::Noop),
            widgets::sidebar_item(Icon::CircleCheck, "Completed", Trail::None, false, Msg::Noop),
            widgets::sidebar_group("Lists", Some((true, Msg::Noop))),
        ];
        items.extend(lists.map(|(name, colour, open)| {
            widgets::sidebar_item(Mark::Ring(p.calendar[colour]), name, Trail::Count(open), false, Msg::Noop)
        }));
        // A group that folds its children in and out (issue #300): open with its branch, then closed.
        let mode = shell::SidebarMode::Expanded;
        let line = p.calendar[0];
        items.push(widgets::sidebar_group("Areas", None));
        items.push(widgets::sidebar_fold_in(
            mode,
            Mark::Tinted(Icon::Layers, line),
            "Projects",
            Trail::Count(7),
            false,
            true,
            Msg::Noop,
        ));
        items.push(widgets::sidebar_branch(
            line,
            vec![
                widgets::sidebar_child(Mark::Tinted(Icon::Flag, line), "Launch", Trail::Count(4), true, Msg::Noop),
                widgets::sidebar_child(Mark::Tinted(Icon::Flag, line), "Website", Trail::Count(3), false, Msg::Noop),
            ],
        ));
        items.push(widgets::sidebar_fold_in(
            mode,
            Mark::Tinted(Icon::Archive, p.calendar[1]),
            "Someday",
            Trail::Count(2),
            false,
            false,
            Msg::Noop,
        ));
        items.push(widgets::sidebar_group("Accounts", Some((true, Msg::Noop))));
        items.push(widgets::sidebar_item(Icon::Cloud, "cloud.example.org", Trail::Dot(p.ok), false, Msg::Noop));
        shell::sidebar(
            shell::app_switcher(AppId::TASKS, switcher, Msg::Switcher),
            items,
            vec![
                widgets::sidebar_item(
                    Icon::Calendar,
                    "Open Calendar",
                    Trail::Icon(Icon::ArrowUpRight),
                    false,
                    Msg::Noop,
                ),
                widgets::sidebar_item(Icon::Settings, "Settings", Trail::None, false, Msg::Noop),
            ],
        )
    }

    fn tasks_header<'a>(&'a self, class: SizeClass, nav: &Anchor) -> Element<'a, Msg> {
        let new_task = widgets::text_button(Some(Icon::Plus), "New task", style::button::primary, Some(Msg::Noop));
        let filter = widgets::icon_button(Icon::ListFilter, "Filter", Some(Msg::Noop));
        let menu = widgets::menu_button(nav, Some(Icon::Sun), "Today", Msg::OpenMenu(Menu::Nav));
        match class {
            SizeClass::Wide => shell::header_bar(
                [],
                shell::header_title(Some(Icon::Sun), "Today", Some("5 open".into())),
                [
                    widgets::text_button(Some(Icon::ListFilter), "Filter", style::button::ghost, Some(Msg::Noop)),
                    widgets::text_button(
                        Some(Icon::SlidersHorizontal),
                        "Display",
                        style::button::ghost,
                        Some(Msg::Noop),
                    ),
                    new_task,
                ],
            ),
            SizeClass::Medium => shell::header_bar(
                [shell::sidebar_toggle(Msg::Drawer)],
                row![menu, typo::text(typo::body(), "5 open").style(theme::text_3)]
                    .spacing(space::s())
                    .align_y(Alignment::Center),
                [filter, new_task],
            ),
            _ => shell::header_bar(
                [shell::sidebar_toggle(Msg::Drawer)],
                menu,
                [filter, widgets::compact_button(Icon::Plus, "New task", style::button::primary, Some(Msg::Noop))],
            ),
        }
    }

    fn nav_menu(&self) -> Element<'_, Msg> {
        let p = self.palette();
        let views = vec![
            MenuItem::new(Icon::Inbox, "Inbox", Msg::CloseMenu).trail(Trail::Count(2)),
            MenuItem::new(Icon::Sun, "Today", Msg::CloseMenu).trail(Trail::Count(5)).current(true),
            MenuItem::new(Icon::CalendarClock, "Upcoming", Msg::CloseMenu).trail(Trail::Count(9)),
            MenuItem::new(Icon::Layers, "Anytime", Msg::CloseMenu),
            MenuItem::new(Icon::CircleCheck, "Completed", Msg::CloseMenu),
        ];
        let lists = [("Work", 0, 6), ("Personal", 1, 4), ("Family", 2, 3), ("Groceries", 4, 5)]
            .map(|(name, colour, open)| {
                MenuItem::new(Mark::Ring(p.calendar[colour]), name, Msg::CloseMenu).trail(Trail::Count(open))
            })
            .to_vec();
        let more = vec![
            MenuItem::new(Icon::Calendar, "Open Calendar", Msg::CloseMenu).trail(Trail::Icon(Icon::ArrowUpRight)),
            MenuItem::new(Icon::Settings, "Settings", Msg::CloseMenu),
        ];
        widgets::menu(vec![MenuSection::new(views), MenuSection::titled("Lists", lists), MenuSection::new(more)])
    }

    fn tasks_body(&self, class: SizeClass) -> Element<'_, Msg> {
        let list = self.task_list(class);
        let width = match class {
            SizeClass::Wide => size::list(),
            SizeClass::Medium => size::list_medium(),
            _ => return list,
        };
        row![container(list).width(width).height(Fill), rule::vertical(1).style(style::rule), self.task_detail(class)]
            .into()
    }

    fn task_list(&self, class: SizeClass) -> Element<'_, Msg> {
        let p = self.palette();
        let quick = container(
            row![
                quiet(Icon::Plus, icon::SIZE_SM),
                typo::text(typo::body(), "New task…").style(theme::text_3),
                Space::new().width(Fill),
                widgets::kbd("N"),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
        )
        .center_y(size::control_lg())
        .padding(style::button_padding())
        .style(|t| container::Style {
            border: border::rounded(theme::radius::sm()).color(palette(t).line).width(1),
            ..container::Style::default()
        });
        let mut list = column![container(quick).padding(iced::Padding {
            top: space::s(),
            right: space::s(),
            bottom: space::xs(),
            left: space::s()
        })];
        let marks =
            [Mark::Tinted(Icon::CircleAlert, p.attention), Mark::Icon(Icon::Sun), Mark::Icon(Icon::CalendarClock)];
        for ((name, range), mark) in GROUPS.into_iter().zip(marks) {
            list = list.push(widgets::group_head(mark, name, Some(range.len()), Some(Msg::Noop)));
            for i in range {
                list = list.push(self.task_row(i, class));
            }
        }
        scrollable(list).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable).into()
    }

    fn task_row(&self, i: usize, class: SizeClass) -> Element<'_, Msg> {
        let t = &TASKS[i];
        let done = self.done.get(i).copied().unwrap_or(t.done);
        let title = typo::text(typo::body(), t.title).wrapping(iced::widget::text::Wrapping::None).style(if done {
            theme::text_3
        } else {
            theme::text
        });
        let late = t.late;
        let due = container(
            typo::text(typo::meta().tabular(), t.due)
                .style(coloured(move |p| if late { p.attention } else { p.text_3 })),
        )
        // Room for "Mon 12" in the standard's meta (13) on one line.
        .width(56)
        .align_right(56);
        let mut cells = vec![
            task_check(done, t.list, Some(Msg::Done(i, !done))),
            priority(t.priority, t.urgent),
            container(title).width(Fill).clip(true).into(),
        ];
        if class != SizeClass::Narrow {
            cells.push(widgets::pill(Some(self.palette().calendar[t.list]), t.list_name));
        }
        cells.push(due.into());
        widgets::list_row(cells, i == self.selected, Some(Msg::Select(i)))
    }

    fn task_detail(&self, class: SizeClass) -> Element<'_, Msg> {
        let p = self.palette();
        let crumb = row![
            small_ring(p.calendar[0], 12.0),
            typo::text(typo::meta(), "Work").style(theme::text_3),
            quiet(Icon::ChevronRight, icon::SIZE_SM),
            typo::text(typo::meta(), "Today").style(theme::text_3),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let mut detail = column![
            container(crumb).padding(iced::Padding::ZERO.bottom(space::m())),
            container(typo::text(typo::headline(), "Review API concept"))
                .padding(iced::Padding::ZERO.bottom(space::s())),
        ];
        if class != SizeClass::Wide {
            let chips = widgets::chip_row(vec![
                widgets::chip(Some(Mark::Ring(p.text_3)), "Open", Some(Msg::Noop)),
                widgets::chip(Some(Mark::Icon(Icon::Flag)), "Medium", Some(Msg::Noop)),
                widgets::chip(Some(Mark::Icon(Icon::Calendar)), "Today, 16:00", Some(Msg::Noop)),
                widgets::chip(Some(Mark::Icon(Icon::Bell)), "15:45", Some(Msg::Noop)),
                widgets::chip(Some(Mark::Ring(p.calendar[0])), "Work", Some(Msg::Noop)),
                widgets::chip(Some(Mark::Icon(Icon::Ellipsis)), "", Some(Msg::Noop)),
            ]);
            detail = detail.push(container(chips).padding(iced::Padding::ZERO.top(space::xs()).bottom(space::l())));
        }
        let description = "Read docs/api.md and decide the three open questions: HTTP API now or later, the app key as \
                           a file or in the keychain, and whether agents may change accounts and settings.";
        detail = detail.push(
            container(typo::text(typo::body(), description).style(theme::text_2))
                .max_width(size::content_max() * 0.7)
                .padding(iced::Padding::ZERO.bottom(space::xl())),
        );
        let progress = container(container(Space::new().width(24).height(4)).style(|t| container::Style {
            background: Some(Background::Color(palette(t).ok)),
            border: border::rounded(2),
            ..container::Style::default()
        }))
        .width(48)
        .height(4)
        .style(|t| container::Style {
            background: Some(Background::Color(palette(t).line_strong)),
            border: border::rounded(2),
            ..container::Style::default()
        });
        let sub_head = row![
            quiet(Icon::ChevronDown, icon::SIZE_SM),
            typo::text(typo::caption(), "Sub-tasks").style(theme::text_3),
            progress,
            typo::text(typo::meta().tabular(), "2/4").style(theme::text_3),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let mut subs = column![container(sub_head).padding(iced::Padding::ZERO.bottom(space::xs()))];
        for (title, done) in [
            ("Read the concept", true),
            ("Compare with TorroMail", true),
            ("Decide the open questions", false),
            ("Start task A.0", false),
        ] {
            let line = row![
                task_check(done, 1, Some(Msg::Noop)),
                typo::text(typo::body(), title).style(if done { theme::text_3 } else { theme::text })
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            subs = subs
                .push(column![container(line).center_y(size::control() - 1.0), rule::horizontal(1).style(style::rule)]);
        }
        subs = subs.push(
            container(
                row![quiet(Icon::Plus, icon::SIZE_SM), typo::text(typo::body(), "Add sub-task").style(theme::text_3)]
                    .spacing(space::s())
                    .align_y(Alignment::Center),
            )
            .center_y(size::control()),
        );
        detail = detail.push(container(subs).max_width(size::content_max() * 0.7));
        let mut activity = column![typo::text(typo::caption(), "Activity").style(theme::text_3)].spacing(space::xs());
        for (which, who, what) in [
            (Icon::CirclePlus, "You", "created the task · 2 days ago"),
            (Icon::Sparkles, "Claude Code", "added two sub-tasks · yesterday"),
            (Icon::Calendar, "You", "set the due date to today · 3 h ago"),
        ] {
            activity = activity.push(
                container(
                    row![
                        quiet(which, icon::SIZE_SM),
                        typo::text(typo::caption(), who).style(theme::text_2),
                        typo::text(typo::meta(), what).style(theme::text_3)
                    ]
                    .spacing(space::s())
                    .align_y(Alignment::Center),
                )
                .center_y(size::row_sidebar()),
            );
        }
        detail = detail.push(container(activity).padding(iced::Padding::ZERO.top(space::xxl())));
        let padding = match class {
            SizeClass::Wide => iced::Padding::from([space::xxl(), space::xxl() + space::s()]),
            SizeClass::Medium => iced::Padding::from([space::xl(), space::xl() + space::xs()]),
            _ => iced::Padding::new(space::l()),
        };
        scrollable(container(detail).padding(padding).width(Fill))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .into()
    }

    fn tasks_props(&self) -> Element<'_, Msg> {
        let p = self.palette();
        let value = |text: &'static str| typo::text(typo::body(), text);
        let quiet_value =
            |text: &'static str| row![Space::new().width(Fill), typo::text(typo::body(), text).style(theme::text_3)];
        column![
            widgets::prop_section(
                "Properties",
                vec![
                    widgets::prop_row(Some(Mark::Ring(p.text_3)), value("Open"), Some(Msg::Noop)),
                    widgets::prop_row(
                        None,
                        row![priority(2, false), value("Medium priority")]
                            .spacing(space::s())
                            .align_y(Alignment::Center),
                        Some(Msg::Noop)
                    ),
                    widgets::prop_row(
                        Some(Mark::Icon(Icon::Calendar)),
                        typo::text(typo::body().tabular(), "Today, 16:00"),
                        Some(Msg::Noop)
                    ),
                    widgets::prop_row(
                        Some(Mark::Icon(Icon::Bell)),
                        typo::text(typo::body().tabular(), "15:45"),
                        Some(Msg::Noop)
                    ),
                    widgets::prop_row(Some(Mark::Icon(Icon::Repeat)), quiet_value("No repeat"), Some(Msg::Noop)),
                ],
            ),
            widgets::prop_section(
                "List",
                vec![widgets::prop_row(Some(Mark::Ring(p.calendar[0])), value("Work"), Some(Msg::Noop))]
            ),
            widgets::prop_section(
                "Tags",
                vec![widgets::prop_row(
                    None,
                    row![widgets::pill(None, "torroworkplace"), widgets::pill(None, "review")].spacing(space::s()),
                    None
                )]
            ),
            widgets::prop_section(
                "Calendar",
                vec![widgets::prop_row(
                    Some(Mark::Icon(Icon::CalendarPlus)),
                    typo::text(typo::body(), "Block time in calendar").style(theme::text_2),
                    Some(Msg::Noop)
                )]
            ),
            widgets::prop_section(
                "Account",
                vec![widgets::prop_row(
                    Some(Mark::Icon(Icon::Cloud)),
                    typo::text(typo::body(), "Nextcloud · Tasks").style(theme::text_2),
                    None
                )]
            ),
        ]
        .spacing(space::l())
        .into()
    }

    /// The selected task in the side panel: path, title, properties as rows, the description and
    /// "Delete task" in the footer.
    fn task_panel(&self) -> panel::Panel<'_, Msg> {
        let t = &TASKS[self.selected.min(TASKS.len() - 1)];
        let description = "Read docs/api.md and decide the three open questions: HTTP API now or later, the app key as \
                           a file or in the keychain, and whether agents may change accounts and settings.";
        let body =
            column![panel::inset(typo::text(typo::body(), description).style(theme::text_2)), self.tasks_props(),]
                .spacing(space::l());
        panel::panel(
            panel::path(Some(Mark::Ring(self.palette().calendar[t.list])), [t.list_name, "Today"]),
            panel::title(t.title),
            body,
            Msg::ClosePanel,
        )
        .actions(vec![widgets::icon_button(Icon::Ellipsis, "More", Some(Msg::Noop))])
        .footer(vec![
            widgets::text_button(None, "Delete task", style::button::danger, Some(Msg::Noop)),
            Space::new().width(Fill).into(),
            widgets::text_button(Some(Icon::CalendarPlus), "Block time", style::button::secondary, Some(Msg::Noop)),
        ])
    }

    /// The building blocks, each in place (menus and cards are drawn where they would float).
    /// A menu with search (issue #299): 40 fonts in two sections with a line between, filtered by the
    /// words in its field; titles without a match go, nothing at all says "No matches".
    fn search_menu(&self) -> Element<'_, Msg> {
        let words = self.search.to_lowercase();
        let mut items: Vec<Element<'_, Msg>> = Vec::new();
        for (title, names) in [("Torro fonts", &SEARCH_FONTS[..20]), ("System fonts", &SEARCH_FONTS[20..])] {
            let found: Vec<&str> = names.iter().copied().filter(|name| name.to_lowercase().contains(&words)).collect();
            if found.is_empty() {
                continue;
            }
            if !items.is_empty() {
                items.push(container(rule::horizontal(1).style(style::rule)).padding([space::xs(), 0.0]).into());
            }
            let title = typo::text(typo::caption(), title).style(theme::text_2);
            items.push(container(title).padding(iced::Padding::new(space::s()).bottom(space::xs())).into());
            items.extend(found.into_iter().map(|name| -> Element<'_, Msg> {
                button(container(typo::text(typo::body(), name)).center_y(Fill))
                    .width(Fill)
                    .height(style::MENU_ITEM_HEIGHT)
                    .padding(style::ITEM_PADDING)
                    .style(style::button::menu_item(false))
                    .on_press(Msg::Noop)
                    .into()
            }));
        }
        widgets::search_menu("gallery-search", &self.search, Msg::Search, Msg::Noop, items, size::props())
    }

    fn blocks(&self) -> Element<'_, Msg> {
        let p = self.palette();
        let sidebar = container(self.tasks_sidebar(&Anchor::new()))
            .width(size::sidebar())
            .height(640)
            .padding(space::s())
            .style(style::container::sidebar);
        let mut list = column![];
        for ((name, range), mark) in GROUPS.into_iter().zip([
            Mark::Tinted(Icon::CircleAlert, p.attention),
            Mark::Icon(Icon::Sun),
            Mark::Icon(Icon::CalendarClock),
        ]) {
            list = list.push(widgets::group_head(mark, name, Some(range.len()), Some(Msg::Noop)));
            for i in range {
                list = list.push(self.task_row(i, SizeClass::Wide));
            }
        }
        let list = container(list).width(size::list()).style(style::container::panel);
        let header = container(shell::header_bar(
            [shell::sidebar_toggle(Msg::Noop)],
            shell::header_title(Some(Icon::Sun), "Today", Some("5 open".into())),
            [
                widgets::select_button(&Anchor::new(), "Week", Msg::Noop),
                widgets::text_button(Some(Icon::Plus), "New task", style::button::primary, Some(Msg::Noop)),
            ],
        ))
        .width(size::list() + size::props())
        .center_y(size::header())
        .padding(iced::Padding::ZERO.left(space::l()).right(space::m()))
        .style(style::container::panel);
        let buttons = row![
            widgets::text_button(Some(Icon::Plus), "New task", style::button::primary, Some(Msg::Noop)),
            widgets::text_button(None, "Today", style::button::secondary, Some(Msg::Noop)),
            widgets::text_button(Some(Icon::ListFilter), "Filter", style::button::ghost, Some(Msg::Noop)),
            widgets::text_button(None, "Delete task", style::button::danger, Some(Msg::Noop)),
            widgets::text_button(None, "Save", style::button::secondary, None),
            widgets::icon_button(Icon::RefreshCw, "Refresh", Some(Msg::Noop)),
            widgets::boxed_button(Icon::SquarePen, "New", Some(Msg::Noop)),
            widgets::compact_button(Icon::Plus, "New task", style::button::primary, Some(Msg::Noop)),
            widgets::segmented([("Day", Msg::Noop), ("Week", Msg::Noop), ("Month", Msg::Noop)], 1),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let chips = widgets::chip_row(vec![
            widgets::chip(Some(Mark::Ring(p.text_3)), "Open", Some(Msg::Noop)),
            widgets::chip(Some(Mark::Icon(Icon::Calendar)), "Today, 16:00", Some(Msg::Noop)),
            widgets::chip(Some(Mark::Dot(p.calendar[0])), "Work", None),
            widgets::chip(Some(Mark::Check(p.calendar[1], true)), "Personal", None),
            widgets::chip(Some(Mark::Icon(Icon::Ellipsis)), "", Some(Msg::Noop)),
        ]);
        let small = row![
            widgets::pill(Some(p.calendar[0]), "Work"),
            widgets::pill(None, "review"),
            widgets::kbd("N"),
            widgets::kbd("Ctrl K"),
            row![widgets::status_dot(Status::Ok), typo::text(typo::meta(), "Connected")]
                .spacing(6)
                .align_y(Alignment::Center),
            row![widgets::status_dot(Status::Attention), typo::text(typo::meta(), "Waiting")]
                .spacing(6)
                .align_y(Alignment::Center),
            row![widgets::status_dot(Status::Error), typo::text(typo::meta(), "Refused")]
                .spacing(6)
                .align_y(Alignment::Center),
            row![widgets::status_dot(Status::Inactive), typo::text(typo::meta(), "Not set up")]
                .spacing(6)
                .align_y(Alignment::Center),
            widgets::avatar_stack(&[("AW", p.calendar[0]), ("JK", p.calendar[1]), ("LM", p.calendar[2])], 3),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        let props = container(self.tasks_props())
            .width(size::props())
            .padding(iced::Padding { top: space::l(), right: space::s(), bottom: space::xl(), left: space::s() })
            .style(style::container::panel);
        let card = widgets::popover(
            row![
                widgets::color_dot(p.calendar[0], 8.0),
                typo::text(typo::meta(), "Work").style(theme::text_3),
                Space::new().width(Fill),
                widgets::icon_button(Icon::Ellipsis, "More", Some(Msg::Noop)),
                widgets::icon_button(Icon::X, "Close", Some(Msg::Noop)),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
            "Sprint planning",
            vec![
                widgets::prop_row(
                    Some(Mark::Icon(Icon::Clock)),
                    typo::text(typo::body().tabular(), "Mon, 5 Oct · 10:00 – 11:30"),
                    None,
                ),
                widgets::prop_row(
                    Some(Mark::Icon(Icon::MapPin)),
                    typo::text(typo::body(), "Room 2 · Office Berlin"),
                    None,
                ),
                widgets::prop_row(
                    Some(Mark::Icon(Icon::Users)),
                    typo::text(typo::body(), "6 guests · you accepted"),
                    None,
                ),
            ],
            vec![
                widgets::text_button(None, "Open details", style::button::secondary, Some(Msg::Noop)),
                widgets::text_button(Some(Icon::Video), "Join", style::button::ghost, Some(Msg::Noop)),
            ],
        );
        let view_menu = widgets::menu(vec![
            MenuSection::titled(
                "View",
                vec![
                    MenuItem::new(Icon::CalendarDays, "Day", Msg::Noop).trail(Trail::Kbd("D")),
                    MenuItem::new(Icon::CalendarRange, "Week", Msg::Noop).trail(Trail::Kbd("W")).current(true),
                    MenuItem::new(Icon::Calendar, "Month", Msg::Noop).trail(Trail::Kbd("M")),
                ],
            ),
            MenuSection::titled(
                "Calendars",
                vec![
                    MenuItem::new(Mark::Check(p.calendar[0], true), "Work", Msg::Noop).trail(Trail::Count(14)),
                    MenuItem::new(Mark::Check(p.calendar[3], false), "Birthdays", Msg::Noop).trail(Trail::Count(1)),
                ],
            ),
            MenuSection::new(vec![MenuItem::new(Icon::Settings, "Calendar settings…", Msg::Noop)]),
        ]);
        let settings = container(widgets::group(
            "General",
            vec![
                widgets::group_row(
                    "Start at login",
                    Some("Starts Torro Workplace in the background.".into()),
                    toggler(true).size(style::CHECK_SIZE).style(style::toggler).on_toggle(|_| Msg::Noop),
                ),
                widgets::group_row(
                    "Appearance",
                    None,
                    widgets::segmented([("System", Msg::Noop), ("Light", Msg::Noop), ("Dark", Msg::Noop)], 0),
                ),
            ],
            Some("Changes apply to all apps of the suite.".into()),
        ))
        .width(size::form_max())
        .padding(space::l())
        .style(style::container::panel);
        let state = |content: Element<'static, Msg>| -> Element<'static, Msg> {
            container(content).width(300).height(180).style(style::container::panel).into()
        };
        let states = row![
            state(widgets::empty_state("No tasks today", "Enjoy the free time, or add one with N.")),
            state(widgets::loading()),
            state(widgets::error_state("The server did not answer. Check the connection and try again.", Msg::Noop)),
        ]
        .spacing(space::l());
        let first = column![section("Sidebar", sidebar.into())];
        let second = column![
            section("Header bar", header.into()),
            section("List: group heads and rows", list.into()),
            section("Buttons", buttons.into()),
            section("Chips", chips),
            section("Pills, keys, status, avatars", small.into()),
        ]
        .spacing(space::xl());
        let third = column![
            section("Menu", row![self.nav_menu(), view_menu].spacing(space::l()).into()),
            section("Menu with search", self.search_menu()),
            section("Floating card", card),
            section("Properties", props.into()),
        ]
        .spacing(space::xl());
        let fourth =
            column![section("Settings group", settings.into()), section("States", states.into())].spacing(space::xl());
        row![first, second, third, fourth].spacing(space::xxl()).padding(space::xl()).into()
    }

    /// The small parts of the tasks redesign (R.1) in the places R.2 and R.3 use them, and every
    /// bundled icon with its name.
    fn parts(&self) -> Element<'_, Msg> {
        let p = self.palette();
        let value = |text: &'static str| widgets::prop_value::<Msg>(text);
        let date = |text: &'static str| typo::text(typo::body().tabular(), text);
        let labels = |names: &[(&'static str, &'static str)]| {
            widgets::label_chip_row(names.iter().map(|(hex, name)| widgets::label_chip(hex, *name)).collect())
        };
        let block = |title: &'static str, rows: Vec<Element<'static, Msg>>| widgets::prop_section(title, rows);
        let props = column![
            column(vec![
                widgets::prop_line("Status", Some(Mark::Tinted(Icon::CircleDot, p.ok)), value("Open"), Some(Msg::Noop)),
                widgets::prop_line(
                    "Priority",
                    None,
                    row![priority(3, false), value("High")].spacing(space::s()).align_y(Alignment::Center),
                    Some(Msg::Noop),
                ),
                widgets::prop_line("Due", Some(Mark::Icon(Icon::Calendar)), date("Today, 16:00"), Some(Msg::Noop),),
                widgets::prop_line(
                    "Reminder",
                    Some(Mark::Icon(Icon::Bell)),
                    widgets::prop_placeholder("Add reminder"),
                    Some(Msg::Noop),
                ),
                widgets::prop_line("Repeat", Some(Mark::Icon(Icon::Repeat)), value("Every 3 days"), Some(Msg::Noop),),
            ]),
            rule::horizontal(1).style(style::rule),
            block(
                "Organisation",
                vec![
                    widgets::prop_line("List", Some(Mark::Ring(p.calendar[0])), value("Work"), Some(Msg::Noop)),
                    widgets::prop_line(
                        "Parent",
                        Some(Mark::Icon(Icon::ListTree)),
                        widgets::prop_placeholder("Add parent task"),
                        Some(Msg::Noop),
                    ),
                    widgets::prop_line(
                        "Labels",
                        None,
                        labels(&[
                            ("d73a4a", "bug"),
                            ("#3B82F6", "ui"),
                            ("#D4A72C", "linux"),
                            ("#2DA44E", "enhancement")
                        ]),
                        Some(Msg::Noop),
                    ),
                    widgets::prop_line(
                        "Labels",
                        Some(Mark::Icon(Icon::Tag)),
                        widgets::prop_placeholder("Add labels"),
                        Some(Msg::Noop),
                    ),
                ]
            ),
            rule::horizontal(1).style(style::rule),
            block(
                "Source",
                vec![
                    widgets::prop_line("Account", Some(Mark::Icon(Icon::Github)), value("GitHub · mahype"), None),
                    widgets::prop_line(
                        "Repo",
                        Some(Mark::Icon(Icon::FolderGit2)),
                        value("mahype/TorroWorkplace"),
                        Some(Msg::Noop),
                    ),
                    widgets::prop_line("Milestone", Some(Mark::Icon(Icon::Milestone)), value("v0.4"), Some(Msg::Noop),),
                    widgets::prop_line(
                        "Branch",
                        Some(Mark::Icon(Icon::GitBranch)),
                        value("anna/tray-templates-with-a-very-long-branch-name"),
                        None,
                    ),
                ]
            ),
        ]
        .spacing(space::s());
        let props = container(props)
            .width(size::props())
            .padding(iced::Padding { top: space::l(), right: space::l(), bottom: space::xl(), left: space::l() })
            .style(style::container::panel);

        let cell = |content: Element<'static, Msg>, name: &'static str| -> Element<'static, Msg> {
            row![content, typo::text(typo::meta(), name).style(theme::text_3)]
                .spacing(space::s())
                .align_y(Alignment::Center)
                .into()
        };
        let pills = column![
            row![
                widgets::state_pill(Mark::Tinted(Icon::CircleDot, p.ok), "Open"),
                widgets::state_pill(Mark::Tinted(Icon::GitPullRequest, p.ok), "Open"),
                widgets::state_pill(Icon::GitPullRequestDraft, "Draft"),
                widgets::state_pill(Mark::Tinted(Icon::GitMerge, p.merged), "Merged"),
                widgets::state_pill(Mark::Tinted(Icon::CircleCheck, p.merged), "Closed"),
                widgets::state_pill(Mark::Tinted(Icon::GitPullRequestClosed, p.error), "Closed"),
                widgets::state_pill(Mark::Tinted(Icon::CircleCheck, p.ok), "Completed"),
            ]
            .spacing(space::s()),
            row![
                widgets::label_chip("#D73A4A", "bug"),
                widgets::label_chip("#3B82F6", "ui"),
                widgets::label_chip("#D4A72C", "linux"),
                widgets::label_chip("2da44e", "enhancement"),
                widgets::label_chip("not a colour", "no colour"),
                typo::text(typo::caption().tabular(), "+2").style(theme::text_3),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center),
        ]
        .spacing(space::m());
        let bar = |done: usize, total: usize| -> Element<'static, Msg> {
            row![
                widgets::progress_bar(done, total),
                typo::text(typo::meta().tabular(), format!("{done}/{total}")).style(theme::text_3)
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into()
        };
        let sub_tasks = column![
            row![
                icon(Icon::ChevronDown, 12.0).style(|t, _| iced::widget::svg::Style { color: Some(palette(t).text_3) }),
                typo::text(typo::caption(), "Sub-tasks").style(theme::text_2),
                bar(2, 4),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center),
            row![bar(0, 3), bar(4, 4), bar(0, 0)].spacing(space::xl()),
        ]
        .spacing(space::m());
        let hint = |which: Icon, label: &'static str| -> Element<'static, Msg> {
            let mut r = row![quiet(which, icon::SIZE_SM)].spacing(space::xs()).align_y(Alignment::Center);
            if !label.is_empty() {
                r = r.push(typo::text(typo::meta().tabular(), label).style(theme::text_3));
            }
            r.into()
        };
        let hints = row![
            hint(Icon::ListChecks, "2/4"),
            hint(Icon::AlignLeft, ""),
            hint(Icon::Repeat, ""),
            hint(Icon::Bell, ""),
            hint(Icon::GripVertical, ""),
            widgets::kbd("N"),
            widgets::kbd("Ctrl L"),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        let ci = row![
            cell(icon(Icon::CircleDotDashed, icon::SIZE_SM).style(coloured_svg(|p| p.attention)).into(), "running"),
            cell(icon(Icon::CircleCheck, icon::SIZE_SM).style(coloured_svg(|p| p.ok)).into(), "passed"),
            cell(icon(Icon::CircleX, icon::SIZE_SM).style(coloured_svg(|p| p.error)).into(), "failed"),
            cell(icon(Icon::CircleDashed, icon::SIZE_SM).style(coloured_svg(|p| p.text_3)).into(), "queued"),
            cell(icon(Icon::Eye, icon::SIZE_SM).style(coloured_svg(|p| p.attention)).into(), "review"),
            cell(icon(Icon::GitMerge, icon::SIZE).style(coloured_svg(|p| p.merged)).into(), "merged"),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        // The action bar of a row (task E3.9), shown as under the keyboard cursor: with words, and as
        // icons in a narrow row. Pointing at any other row shows it too.
        let actions = || {
            vec![
                widgets::RowAction::new(Icon::ListTodo, "Task", "T", Msg::Noop),
                widgets::RowAction::new(Icon::Check, "Done", "E", Msg::Noop),
                widgets::RowAction::new(Icon::Clock, "Later", "S", Msg::Noop),
                widgets::RowAction::new(Icon::Ellipsis, "Hide", "M", Msg::Noop),
            ]
        };
        let entry = |title: &'static str, selected: bool| {
            let cells = vec![quiet(Icon::Mail, icon::SIZE), typo::text(typo::body(), title).width(Fill).into()];
            widgets::list_row(cells, selected, Some(Msg::Noop))
        };
        // A row of three lines (task E3.11): the bar sits on the first line, over the time, and
        // leaves the lines below free.
        let three_lines = || {
            let lines = column![
                typo::text(typo::body_strong(), "Jonas Weber"),
                typo::text(typo::body(), "Budget review for Q4"),
                typo::text(typo::body(), "The numbers for Q4 are in.").style(theme::text_2),
            ];
            let content = row![quiet(Icon::Mail, icon::SIZE), lines.width(Fill)].spacing(space::m());
            // A row of a fixed height takes only the sides of the row padding and centres its lines,
            // as the inbox's rows do: 18 above and below would leave three lines of 22 too little.
            let pad = layout::row_padding();
            container(content)
                .center_y(size::row_message())
                .padding(iced::Padding::new(0.0).left(pad.left).right(pad.right))
                .style(|t: &Theme| container::Style {
                    background: Some(palette(t).selected.into()),
                    ..container::Style::default()
                })
        };
        let line = typo::line_height(typo::body());
        let first_line = (size::row_message() - 3.0 * line) / 2.0 + line / 2.0;
        let bars = column![
            container(widgets::row_actions(entry("Budget review for Q4", true), actions(), true, true))
                .width(size::list()),
            container(widgets::row_actions(entry("Weekend plans", false), actions(), true, false)).width(size::props()),
            container(widgets::row_actions(entry("Point at me", false), actions(), false, true)).width(size::list()),
            container(widgets::row_actions_at(three_lines(), actions(), true, true, first_line)).width(size::list()),
        ]
        .spacing(space::s());
        // The toast after a change (task E3.11), drawn in place instead of over the window.
        let toasts = column![
            container(widgets::toast("Marked as done", Some(widgets::ToastAction::undo(Msg::Noop))))
                .width(size::list())
                .height(size::control_lg() + 2.0 * space::xl()),
            container(widgets::toast::<Msg>("Task created in Work", None))
                .width(size::list())
                .height(size::control_lg() + 2.0 * space::xl()),
        ];
        // A state on an entry is a dot on its icon (issue #248), never only a tinted icon: in the
        // sidebar, its rail and as a chip, chosen and not.
        let in_need = Mark::Badged(Icon::Cloud, p.attention_text);
        let failing = Mark::Badged(Icon::Github, p.error_text);
        let badged = row![
            container(column![
                widgets::sidebar_item_in(
                    shell::SidebarMode::Expanded,
                    in_need,
                    "Nextcloud",
                    Trail::Count(2),
                    true,
                    Msg::Noop
                ),
                widgets::sidebar_item_in(
                    shell::SidebarMode::Expanded,
                    failing,
                    "GitHub",
                    Trail::Count(1),
                    false,
                    Msg::Noop
                ),
            ])
            .width(size::sidebar()),
            container(column![
                widgets::sidebar_item_in(
                    shell::SidebarMode::Rail,
                    in_need,
                    "Nextcloud",
                    Trail::Count(2),
                    true,
                    Msg::Noop
                ),
                widgets::sidebar_item_in(
                    shell::SidebarMode::Rail,
                    failing,
                    "GitHub",
                    Trail::Count(1),
                    false,
                    Msg::Noop
                ),
            ])
            .width(shell::rail()),
            widgets::choice_chip("Nextcloud", Some(in_need), true, Some(Msg::Noop)),
            widgets::choice_chip("GitHub", Some(failing), false, Some(Msg::Noop)),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        let small = column![
            section("State on an entry", badged.into()),
            section("Row actions", bars.into()),
            section("Toast", toasts.into()),
            section("State pills and label chips", pills.into()),
            section("Progress bar", sub_tasks.into()),
            section("Hints in a row, key hints", hints.into()),
            section("Check and state symbols", ci.into()),
        ]
        .spacing(space::xl());

        let every_icon = iced::widget::Row::with_children(Icon::ALL.iter().map(|&which| -> Element<'static, Msg> {
            row![quiet(which, icon::SIZE), typo::text(typo::meta(), format!("{which:?}")).style(theme::text_2)]
                .spacing(space::s())
                .align_y(Alignment::Center)
                .width(170)
                .into()
        }))
        .spacing(space::s())
        .wrap()
        .vertical_spacing(space::m());
        let icons = container(every_icon).width(1000);

        row![
            column![
                section("States and feedback", self.feedback()),
                section("Appearance page (task L.18)", self.appearance()),
            ]
            .spacing(space::xxl()),
            section("Properties column", props.into()),
            small,
            section("Every bundled icon (Lucide 0.469.0, stroke 1.75)", icons.into())
        ]
        .spacing(space::xxl())
        .padding(space::xl())
        .into()
    }

    /// The blocks of the compact Appearance page (task L.18): the tile forced into every state, then
    /// live the segment with symbols (with and without words), the heading with quiet actions and a
    /// grid of tiles – a click on the unusable one shows or hides its reasons under the grid.
    fn appearance(&self) -> Element<'_, Msg> {
        let area = widgets::preview_size();
        let swatch = move |colour: fn(&Palette) -> Color| -> Element<'static, Msg> {
            container(Space::new())
                .width(area.width)
                .height(area.height)
                .style(move |t: &Theme| container::Style {
                    background: Some(Background::Color(colour(&palette(t)))),
                    border: border::rounded(theme::radius::xs()),
                    ..container::Style::default()
                })
                .into()
        };
        let looks: [(&str, bool, bool); 4] = [
            ("preview_tile", false, true),
            ("preview_tile (chosen)", true, true),
            ("preview_tile (unusable)", false, false),
            ("preview_tile (unusable, open)", true, false),
        ];
        let forced_tiles = column(looks.into_iter().map(|(name, on, usable)| {
            let cells = BUTTON_STATES
                .iter()
                .map(|&(state, status)| {
                    let top: Element<'static, Msg> = if usable {
                        swatch(|p| p.calendar[0])
                    } else {
                        container(quiet(Icon::CircleAlert, icon::SIZE))
                            .center_x(area.width)
                            .center_y(area.height)
                            .into()
                    };
                    // Not `label`: its centring fills the height, which a tile of its own height must not.
                    let content = column![top, typo::text(typo::caption(), "Harbor")].spacing(space::xs());
                    cell(
                        state,
                        forced(content, style::button::preview_tile(on, usable), status)
                            .width(widgets::preview_tile_width())
                            .padding(space::xs()),
                    )
                })
                .collect();
            line(name, cells)
        }))
        .spacing(space::l());

        let modes = |words: bool| {
            let parts = [
                (Icon::Monitor, "System", Msg::Mode(0)),
                (Icon::Sun, "Light", Msg::Mode(1)),
                (Icon::Moon, "Dark", Msg::Mode(2)),
            ];
            widgets::icon_segmented(parts, self.mode, words)
        };
        let heading = widgets::group_heading(
            "Design",
            vec![
                widgets::icon_button(Icon::RefreshCw, "Reload", Some(Msg::Noop)),
                widgets::text_button(Some(Icon::Folder), "Open designs folder", style::button::ghost, Some(Msg::Noop)),
            ],
        );
        let names = ["Torro", "Harbor", "Paper", "Commitr Dark Extended", "Neon", "Plex", "Nord", "Dracula"];
        let colours: [fn(&Palette) -> Color; 8] = [
            |p| p.bg,
            |p| p.calendar[0],
            |p| p.raised,
            |p| p.calendar[4],
            |p| p.bg,
            |p| p.calendar[1],
            |p| p.calendar[2],
            |p| p.calendar[3],
        ];
        let tiles = names
            .iter()
            .zip(colours)
            .enumerate()
            .map(|(i, (name, colour))| {
                if *name == "Neon" {
                    let state = widgets::PreviewState::Unusable { open: self.reasons };
                    return widgets::preview_tile(
                        None,
                        name,
                        Some("0.1 · Torro samples".into()),
                        state,
                        Some(Msg::Reasons),
                    );
                }
                let state = if i == self.tile { widgets::PreviewState::Chosen } else { widgets::PreviewState::Plain };
                // Plex wears the note a chosen design puts on the font or colour scheme it suggests (task L.23).
                let note = (*name == "Plex").then_some("Suggested");
                widgets::preview_tile_noted(
                    Some(swatch(colour)),
                    name,
                    note,
                    Some("1.0 · Torro samples".into()),
                    state,
                    Some(Msg::Tile(i)),
                )
            })
            .collect();
        let mut grid = column![widgets::preview_grid(tiles)].spacing(layout::item_gap());
        if self.reasons {
            let reasons = column![
                typo::text(typo::body_strong(), "Neon"),
                typo::text(typo::meta(), "Can't be used:").style(theme::text_2),
                typo::text(typo::meta(), "colors.dark.text: contrast 2.1:1 on panel, at least 4.5:1")
                    .style(theme::text_2),
            ]
            .spacing(space::xs());
            grid = grid
                .push(container(reasons).width(Fill).padding(layout::panel_padding()).style(style::container::group));
        }
        let live = column![
            labelled("Segment with symbols and words", modes(true)),
            labelled("Without words (tooltips)", modes(false)),
            column![heading, grid].spacing(layout::item_gap()),
        ]
        .spacing(space::l());
        column![forced_tiles, rule::horizontal(1).style(style::rule), live]
            .spacing(space::xl())
            .width(size::content_max())
            .into()
    }

    /// Everything that answers a click in the filter list dialog (task G.10): first every state
    /// forced, so one screenshot per mode shows them all, then live blocks to try by hand.
    fn feedback(&self) -> Element<'_, Msg> {
        let p = self.palette();
        // A choice chip drawn in a forced state; the content is what `widgets::choice_chip` builds.
        let choice = |on: bool| -> Element<'static, Msg> {
            let cells = BUTTON_STATES
                .iter()
                .map(|&(state, status)| {
                    let lead = if on { Icon::Check } else { Icon::Bug };
                    let tone: fn(&Palette) -> Color = if on { |p| p.text } else { |p| p.text_2 };
                    let content = row![icon(lead, icon::SIZE_SM).style(coloured_svg(tone)), label("Bugs")]
                        .spacing(space::xs() + space::xs() / 2.0)
                        .align_y(Alignment::Center);
                    let chip = forced(content, style::button::choice_chip(on), status)
                        .height(size::control_sm())
                        .padding(style::CHIP_PADDING);
                    cell(state, chip)
                })
                .collect();
            line(if on { "choice_chip (chosen)" } else { "choice_chip" }, cells)
        };
        let property = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let date = typo::text(typo::meta(), "Today, 16:00").wrapping(iced::widget::text::Wrapping::None);
                let content = row![quiet(Icon::Calendar, icon::SIZE_SM), date]
                    .spacing(space::xs() + space::xs() / 2.0)
                    .align_y(Alignment::Center);
                cell(
                    state,
                    forced(content, style::button::chip, status)
                        .height(style::CHIP_HEIGHT)
                        .padding(style::CHIP_PADDING),
                )
            })
            .collect();
        let secondary = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                cell(
                    state,
                    forced(label("Cancel"), style::button::secondary, status)
                        .height(size::control_sm())
                        .padding(style::button_padding()),
                )
            })
            .collect();
        let segments = |active: bool| -> Element<'static, Msg> {
            let cells = BUTTON_STATES
                .iter()
                .map(|&(state, status)| {
                    let seg = forced(label("Open"), style::button::segment(active), status)
                        .height(size::control_sm() - 2.0 * style::SEGMENT_INSET - 2.0)
                        .padding(style::button_padding());
                    cell(state, container(seg).padding(style::SEGMENT_INSET).style(style::container::segmented))
                })
                .collect();
            line(if active { "segment (active)" } else { "segment" }, cells)
        };
        let primary = line(
            "primary",
            vec![
                cell("Normal", widgets::text_button(None, "Create list", style::button::primary, Some(Msg::Noop))),
                cell("Busy", widgets::busy_button("Creating…", style::button::primary)),
                cell("Disabled", widgets::text_button(None, "Create list", style::button::primary, None)),
            ],
        );
        let forced_states = column![
            choice(false),
            choice(true),
            line("chip (property)", property),
            line("secondary", secondary),
            segments(false),
            segments(true),
            primary,
        ]
        .spacing(space::l());

        // Live: one template (a second click clears it), several labels, segments, busy.
        let templates = ["Assigned to me", "Bugs", "Review requested", "My pull requests"];
        let icons = [Icon::UserCheck, Icon::Bug, Icon::Eye, Icon::GitPullRequest];
        let template_chips = templates
            .iter()
            .zip(icons)
            .enumerate()
            .map(|(i, (name, which))| {
                widgets::choice_chip(*name, Some(Mark::Icon(which)), self.template == Some(i), Some(Msg::Template(i)))
            })
            .collect();
        let labels = [("d73a4a", "bug"), ("#3B82F6", "ui"), ("#D4A72C", "linux"), ("not a colour", "no colour")];
        let mut label_chips: Vec<Element<'_, Msg>> = labels
            .iter()
            .enumerate()
            .map(|(i, (hex, name))| {
                widgets::label_choice(hex, *name, self.labels.get(i).copied().unwrap_or(false), Some(Msg::Label(i)))
            })
            .collect();
        label_chips.push(widgets::choice_chip("Work", Some(Mark::Dot(p.calendar[0])), true, None));
        label_chips.push(widgets::choice_chip("Archive", Some(Mark::Icon(Icon::Inbox)), false, None));
        let people = ["Anyone", "Me", "Unassigned"]
            .iter()
            .enumerate()
            .map(|(i, name)| widgets::choice_chip(*name, None, self.show == i, Some(Msg::Show(i))))
            .collect();
        let show = widgets::segmented(
            [("Issues", Msg::State(0)), ("Pull requests", Msg::State(1)), ("Both", Msg::State(2))],
            self.state,
        );
        let create: Element<'_, Msg> = if self.busy {
            widgets::busy_button("Creating…", style::button::primary)
        } else {
            widgets::text_button(None, "Create list", style::button::primary, Some(Msg::Busy(true)))
        };
        let footer = row![
            widgets::text_button(None, "Cancel", style::button::secondary, Some(Msg::Busy(false))),
            create,
            typo::text(typo::meta(), "Cancel ends the busy state").style(theme::text_3),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let live = column![
            labelled("Start from (one of, click again to clear)", widgets::chip_row(template_chips)),
            labelled("Labels (several; the last two are disabled)", widgets::chip_row(label_chips)),
            labelled("Assignee (one of)", widgets::chip_row(people)),
            labelled("Show", show),
            labelled("Footer", footer.into()),
        ]
        .spacing(space::l());

        column![forced_states, rule::horizontal(1).style(style::rule), live]
            .spacing(space::xl())
            .width(size::content_max())
            .into()
    }
}

/// The mail window's large buttons (task E11.1, `docs/prototypes/inbox/mail-buttons.html#d`): the
/// styles `cell` and `pill`, the track and the flat input forced into every state (the hover is the
/// column "Hover"), the command bar in three widths, the strip under a row with and without words,
/// the pills, the attachment chips and the reader's foot with its view switches.
/// Task E12.7: the category pill in all four groups, with and without the suggestion mark.
impl Gallery {
    /// The blocks of a detail page (tasks DS.3, DS.4), each wide and narrow: the head with and
    /// without its pill, the tabs as a row and as a pick list, open questions.
    fn detail(&self) -> Element<'_, Msg> {
        let more = Anchor::new();
        let actions = || widgets::HeadActions {
            action: Some(widgets::PageAction::new("Sync now", Some(Msg::Noop)).tip("Syncs all accounts")),
            more: Some((more.clone(), Msg::Noop)),
        };
        let head = |status: Option<(Status, String)>, width: f32| {
            container(widgets::detail_head(
                TileFace::letter("anna"),
                "anna@example.org",
                "Anna Example · IMAP mail.example.org:993 · SMTP mail.example.org:587",
                status,
                actions(),
            ))
            .width(width)
        };
        let synced = || Some((Status::Ok, "Synced today 15:39".to_string()));
        let heads = column![
            head(synced(), size::content_max()),
            head(Some((Status::Error, "Login failed".to_string())), size::content_max()),
            head(None, size::content_max()),
            container(widgets::detail_head(
                TileFace::Icon(Icon::Github),
                "GitHub",
                "",
                None,
                widgets::HeadActions::<Msg>::default()
            ))
            .width(size::content_max()),
            head(synced(), size::panel()),
        ]
        .spacing(space::xl());
        let labels = || {
            vec![
                ("Overview".to_string(), None),
                ("Writing".to_string(), None),
                ("Folders".to_string(), Some(12)),
                ("Backup".to_string(), None),
            ]
        };
        let tabs = column![
            container(widgets::tabs(labels(), self.tab, Msg::Tab)).width(size::content_max()),
            container(widgets::tabs(labels(), self.tab, Msg::Tab)).width(size::props()),
        ]
        .spacing(space::xl());
        let question = |width: f32| {
            container(widgets::question_card(
                Icon::Folder,
                "Which folder holds sent mail?",
                "The server names no folder for sent mail. Choose one, so replies are kept.",
                vec![
                    widgets::PageAction::new("Use Sent", Some(Msg::Noop)),
                    widgets::PageAction::new("Use Sent Items", Some(Msg::Noop)),
                ],
            ))
            .width(width)
        };
        let problem = container(widgets::question_card(
            Icon::CircleAlert,
            "The last sync failed",
            "Login failed: the server refused the password.",
            vec![widgets::PageAction::new("Server settings", Some(Msg::Noop))],
        ))
        .width(size::content_max());
        let questions =
            column![question(size::content_max()), problem, question(size::panel())].spacing(size::card_gap());
        let figures = |width: f32| {
            container(widgets::stat_grid(vec![
                widgets::StatFigure::new("Mail", "4,796", "59 unread"),
                widgets::StatFigure::new("On the server", "2.36 GB", "of 10 GB · 24%"),
                widgets::StatFigure::new("On this device", "512 MB", "1,204 texts offline"),
                widgets::StatFigure::new("Searchable", "92%", "4,412 with full text"),
            ]))
            .width(width)
        };
        let figures =
            column![figures(size::content_max()), figures(size::panel()), figures(size::props())].spacing(space::xl());
        let part = |label: &str, value: u64, shown: &str, slot: usize| {
            widgets::SharePart::new(label, value, shown, theme::ChartColor::Series(slot))
        };
        let server = vec![
            part("INBOX", 1_200, "1.2 GB", 0),
            part("Archive", 700, "700 MB", 1),
            part("Sent", 300, "300 MB", 2),
            widgets::SharePart::new("9 other folders", 160, "160 MB", theme::ChartColor::Other),
        ];
        let device = vec![
            part("Search index", 210, "210 MB", 0),
            part("Mail texts", 240, "240 MB", 1),
            part("Originals", 40, "40 MB", 2),
            part("Headers & folders", 22, "22 MB", 3),
            part("Fifth part", 5, "5 MB", 0),
        ];
        let strips = column![
            container(widgets::share_strip(server.clone(), Some(("Free".to_string(), 7_880))))
                .width(size::content_max()),
            container(widgets::share_strip(server, None)).width(size::content_max()),
            container(widgets::share_strip(device, None)).width(size::content_max()),
            container(widgets::share_strip(Vec::new(), Some(("Free".to_string(), 0)))).width(size::props()),
        ]
        .spacing(space::xl());
        let pairs = |width: f32| {
            let name = widgets::setting_pair("Sender name", "Shown as the sender of mail from this account.", || {
                text_input("Anna Example", "Anna Example").style(style::text_input).padding(style::INPUT_PADDING).into()
            });
            let format = widgets::setting_pair(
                "Write new mail as",
                "HTML keeps formatting; plain text reads everywhere.",
                || widgets::segmented([("HTML", Msg::Noop), ("Plain text", Msg::Noop)], 0),
            );
            container(widgets::setting_pairs(vec![name, format])).width(width)
        };
        let pairs = column![pairs(size::content_max()), pairs(size::panel())].spacing(space::xxl());
        let page = container(typo::text(typo::body(), "A page that does not save itself.").style(theme::text_2))
            .width(Fill)
            .height(size::row_message() + widgets::floating_bar_clearance(1))
            .padding(space::l())
            .style(style::container::panel);
        let saving = container(widgets::over_bar(
            page,
            widgets::save_bar("Signature changed", Some(Msg::Noop), Some(Msg::Noop), false),
        ))
        .width(size::content_max())
        .height(size::row_message() + widgets::floating_bar_clearance(1));
        let busy = widgets::save_bar::<Msg>("Signature changed", None, None, true);
        let branch = container(column![
            widgets::sidebar_item(Icon::Users, "Accounts", widgets::Trail::None, false, Msg::Noop),
            widgets::sidebar_branch_quiet(vec![widgets::sidebar_child(
                Icon::Mail,
                "anna@example.org",
                widgets::Trail::None,
                true,
                Msg::Noop,
            )]),
            widgets::sidebar_item(Icon::Settings, "Storage & index", widgets::Trail::None, false, Msg::Noop),
        ])
        .width(size::sidebar())
        .padding(space::s())
        .style(style::container::sidebar);
        let first = column![
            section("Head", heads.into()),
            section("Tabs", tabs.into()),
            section("Figures", figures.into()),
            section("Shares", strips.into()),
        ]
        .spacing(space::xxl());
        let second = column![
            section("Open questions", questions.into()),
            section("Setting pairs", pairs.into()),
            section("Save bar", column![saving, busy].spacing(space::l()).into()),
            section("Quiet branch", branch.into()),
        ]
        .spacing(space::xxl());
        row![first, second].spacing(space::xxl()).padding(space::xl()).into()
    }
}

fn categories() -> Element<'static, Msg> {
    let groups: [(Icon, &str, &str); 5] = [
        (Icon::Inbox, "Waiting for me", "Request"),
        (Icon::Archive, "Paper trail", "Invoice"),
        (Icon::Info, "Info", "Newsletter"),
        (Icon::ShieldAlert, "Unwanted", "Phishing"),
        (Icon::CircleDashed, "Unsorted", "Other"),
    ];
    let mut rows = column![].spacing(space::l());
    for (at, (mark, group, label)) in groups.into_iter().enumerate() {
        let line = row![
            container(typo::text(typo::meta(), group)).width(140),
            widgets::category_pill(mark, at, label, false),
            widgets::category_pill(mark, at, label, true),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        rows = rows.push(line);
    }
    // A row of the list as it reads with a pill beside the subject.
    let in_a_row = row![
        typo::text(typo::body_strong(), "Shop"),
        typo::text(typo::body(), "Your parcel is on its way"),
        widgets::category_pill(Icon::Archive, 1, "Shipping", true),
    ]
    .spacing(space::m())
    .align_y(Alignment::Center);
    column![
        typo::text(typo::title(), "Mail categories"),
        typo::text(typo::meta(), "Group colour and category icon; sparkles mark a model's guess."),
        rows,
        typo::text(typo::meta(), "In a row of the list:"),
        in_a_row,
    ]
    .spacing(space::xl())
    .into()
}

fn mailbar() -> Element<'static, Msg> {
    let accent_mark = |which: Icon| -> Element<'static, Msg> {
        icon(which, icon::SIZE).style(|t, _| iced::widget::svg::Style { color: Some(palette(t).accent) }).into()
    };
    let cells = BUTTON_STATES
        .iter()
        .map(|&(state, status)| {
            let content = row![icon(Icon::Archive, icon::SIZE_SM), typo::text(typo::body(), "Archive")]
                .spacing(space::s())
                .align_y(Alignment::Center);
            let b =
                forced(container(content).center(Fill), style::button::cell, status).width(120).height(size::command());
            cell(state, b)
        })
        .collect();
    let pills = BUTTON_STATES
        .iter()
        .map(|&(state, status)| {
            let content = row![accent_mark(Icon::ListTodo), typo::text(typo::body(), "Task")]
                .spacing(space::s())
                .align_y(Alignment::Center);
            let b = forced(container(content).center_y(Fill), style::button::pill, status)
                .height(size::pill())
                .padding([0.0, space::l()]);
            cell(state, b)
        })
        .collect();
    let input_states: [(&str, text_input::Status, &str); 4] = [
        ("Normal", text_input::Status::Active, ""),
        ("Hover", text_input::Status::Hovered, ""),
        ("Focused", text_input::Status::Focused { is_hovered: false }, "budget"),
        ("Disabled", text_input::Status::Disabled, "budget"),
    ];
    let flat = input_states
        .into_iter()
        .map(|(state, status, value)| {
            let input = text_input("Search all mail", value)
                .padding(style::INPUT_PADDING)
                .width(160)
                .style(move |t, _| style::text_input_flat(t, status));
            let input = if status == text_input::Status::Disabled { input } else { input.on_input(|_| Msg::Noop) };
            cell(state, input)
        })
        .collect();
    let styles = column![line("cell", cells), line("pill", pills), line("text_input_flat", flat),].spacing(space::l());

    // The attachment block of the reader (M12.5): three split chips, "Save all" in its head, and the
    // menu of a chip beside it with a detail line and both tags.
    let chips = row![
        widgets::split_file_chip(
            "budget-q4.csv",
            "3 KB",
            "Open: budget-q4.csv",
            Some(Msg::Noop),
            Some(Msg::Noop),
            None
        ),
        widgets::split_file_chip(
            "travel-policy.pdf",
            "184 KB",
            "Open: travel-policy.pdf",
            Some(Msg::Noop),
            Some(Msg::Noop),
            None
        ),
        widgets::split_file_chip("no-size.txt", "", "", None, None, None),
    ]
    .spacing(space::s())
    .wrap()
    .vertical_spacing(space::s());
    let tip = "More ways to save all";
    let save_all = widgets::split_action(Icon::Download, "Save all", tip, Some(Msg::Noop), Some(Msg::Noop), None);
    let block = widgets::file_block(widgets::file_block_head("3 attachments", "188 KB", Some(save_all)), chips);
    let framed = widgets::split_action_framed(Icon::Download, "Save all", tip, Some(Msg::Noop), Some(Msg::Noop), None);
    let menu = widgets::menu(vec![MenuSection::new(vec![
        MenuItem::new(Icon::Folder, "Invoices", Msg::Noop)
            .detail("~/Documents/Invoices/2026/10")
            .tag("Click", false)
            .tag("Suggested", true),
        MenuItem::new(Icon::Download, "Save to Downloads", Msg::Noop).detail("~/Downloads"),
        MenuItem::new(Icon::Save, "Save as…", Msg::Noop),
    ])]);
    let reader_end = column![
        rule::horizontal(1).style(style::rule),
        container(column![block, row![framed], menu].spacing(space::m())).padding([space::s(), space::xl()]),
    ]
    .width(720);

    column![
        section("Action rail of a mail (E14.1)", action_rails()),
        section("Styles in every state", styles.into()),
        section("Attachments at the reader's bottom", reader_end.into()),
    ]
    .spacing(space::xxl())
    .padding(space::xl())
    .into()
}

/// The mail's action rail (task E14.1, `docs/prototypes/inbox/mail-aktionsleiste.html#d`) beside a
/// sample mail: collapsed at rest, with "File" open (top edges in line, "Archive" off), with "More"
/// open (a foot group, bottom edges in line), without its button as in the panel of medium and
/// narrow windows, and expanded. The gallery only shows them: every message is `Noop`, so the open
/// groups stay open.
fn action_rails() -> Element<'static, Msg> {
    let frame = |rail: Element<'static, Msg>, wide: bool, height: f32| {
        let mail = column![
            typo::text(typo::headline(), "Budget review for Q4"),
            typo::text(typo::meta(), "Jonas Weber · Tue, 20 Oct · 09:05").style(theme::text_3),
            typo::text(
                typo::read(),
                "Hi Anna, the numbers for Q4 are in. Could you look at the travel budget before Thursday? Two \
                 things stand out: travel is 12 % over plan, and the software line still carries the old \
                 licences we cancelled in August.",
            ),
        ]
        .spacing(space::m())
        .padding(space::xl())
        .width(Fill);
        let mode = if wide { widgets::RailMode::Expanded } else { widgets::RailMode::Collapsed };
        let width = 220.0 + mode.width();
        Element::from(container(row![mail, rail]).width(width).height(height).style(style::container::panel))
    };
    // The open groups are layers over the whole section, as an app lays them over the whole window:
    // laid out in a frame, a layer wider than the frame would be squeezed.
    let mut layers = Vec::new();
    let mut collapsed = |open: Option<usize>, toggle: widgets::RailToggle<Msg>, height: f32| {
        let anchors: Vec<Anchor> = (0..6).map(|_| Anchor::new()).collect();
        let groups = rail_groups(&anchors);
        if let Some((at, group)) = open.and_then(|at| groups.get(at).cloned().map(|group| (at, group))) {
            layers.push(widgets::rail_flyout(group, &anchors[at], Msg::Noop));
        }
        let rail = widgets::action_rail_toggle(groups, widgets::RailMode::Collapsed, open, |_| Msg::Noop, toggle);
        frame(rail, false, height)
    };
    let tall = 760.0;
    let low = 420.0;
    let on = || widgets::RailToggle::On(Msg::Noop);
    let rest = collapsed(None, on(), tall);
    let file = collapsed(Some(1), on(), tall);
    let more = collapsed(Some(5), on(), tall);
    let panel = collapsed(None, widgets::RailToggle::Hidden, low);
    // Issue #291: no room to expand – the toggle is off, the head and the icons stay.
    let off = collapsed(None, widgets::RailToggle::Off("Widen the window to expand actions"), low);
    let expanded = |height: f32| {
        let anchors: Vec<Anchor> = (0..6).map(|_| Anchor::new()).collect();
        let rail = widgets::action_rail(
            rail_groups(&anchors),
            widgets::RailMode::Expanded,
            None,
            |_| Msg::Noop,
            Some(Msg::Noop),
        );
        frame(rail, true, height)
    };
    let first = row![
        labelled("Collapsed", rest),
        labelled("“File” open · Archive off", file),
        labelled("“More” open · at the foot", more),
        labelled("Expanded · Archive off", expanded(tall)),
    ]
    .spacing(space::l())
    // Wraps in a narrow gallery window, so no frame is cut off.
    .wrap()
    .vertical_spacing(space::l());
    let second = row![
        labelled("In the panel · no button", panel),
        labelled("No room to expand · toggle off", off),
        labelled("Expanded in a low window · the top scrolls, the foot stays", expanded(low)),
    ]
    .spacing(space::l())
    .wrap()
    .vertical_spacing(space::l());
    let section: Element<'static, Msg> = column![first, second].spacing(space::xl()).into();
    iced::widget::stack(std::iter::once(section).chain(layers)).into()
}

/// The groups of the rail in the order of task E14 (decision 2), the icon of each registering its
/// anchor in `anchors`; "Archive" is off (no archive folder), Reply in the accent.
fn rail_groups(anchors: &[Anchor]) -> Vec<widgets::RailGroup<'static, Msg>> {
    use widgets::{RailGroup, RailItem};
    let item = |which: Icon, label: &'static str, key: &'static str| RailItem::new(which, label, key, Some(Msg::Noop));
    let groups = vec![
        RailGroup::new(
            Icon::Reply,
            "Reply",
            vec![
                item(Icon::Reply, "Reply", "R").lead(),
                item(Icon::ReplyAll, "Reply all", "A"),
                item(Icon::Forward, "Forward", "F"),
            ],
        )
        .accent(),
        RailGroup::new(
            Icon::Check,
            "File",
            vec![
                item(Icon::Check, "Done", "E"),
                item(Icon::Clock, "Later…", "S"),
                item(Icon::Info, "Move to Info", "L"),
                RailItem::new(Icon::Archive, "Archive", "", None),
                item(Icon::Trash2, "Delete", "#").danger(),
            ],
        ),
        RailGroup::new(
            Icon::ListTodo,
            "Torro",
            vec![
                item(Icon::ListTodo, "Task", "T"),
                item(Icon::CalendarPlus, "Event", "C"),
                item(Icon::Tag, "Category…", ""),
                item(Icon::Sparkles, "Re-categorize", "").accent(),
            ],
        ),
        RailGroup::new(Icon::Flag, "Mark", vec![item(Icon::Flag, "Unflag", ""), item(Icon::Mail, "Mark unread", "")])
            .tinted(iced::Color::from_rgb8(0xE8, 0xA3, 0x3D)),
        RailGroup::new(Icon::BellOff, "Mute", vec![item(Icon::BellOff, "Mute…", "M")]).foot(),
        RailGroup::new(
            Icon::Ellipsis,
            "More",
            vec![
                item(Icon::Copy, "Copy text", ""),
                item(Icon::AlignLeft, "Show as text", "V"),
                item(Icon::Sun, "Light colours", ""),
                item(Icon::X, "Close", "Esc"),
            ],
        )
        .foot(),
    ];
    groups.into_iter().zip(anchors).map(|(group, anchor)| group.at(anchor)).collect()
}

/// `body` under a small `CAPTION` title in `text_3`, as the fields of a dialog.
fn labelled<'a>(title: &'a str, body: Element<'a, Msg>) -> Element<'a, Msg> {
    column![typo::text(typo::caption(), title).style(theme::text_3), body].spacing(space::xs()).into()
}

/// `picture` as an uncompressed 32-bit BMP (top-down, BGRA): the simplest picture file, written
/// without a crate of its own; `sips -s format png` turns it into a PNG.
fn bmp(picture: &iced::window::Screenshot) -> Vec<u8> {
    let (width, height) = (picture.size.width, picture.size.height);
    let pixels = picture.rgba.len() as u32;
    let mut out = Vec::with_capacity(pixels as usize + 54);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + pixels).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    // A negative height: the rows run from the top down, as the screenshot's.
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for [r, g, b, a] in picture.rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&[*b, *g, *r, *a]);
    }
    out
}

/// The entries of the gallery's menu with search: twenty of the suite's fonts, then twenty of a
/// computer's.
const SEARCH_FONTS: [&str; 40] = [
    "Inter",
    "Atkinson Hyperlegible Next",
    "Cantarell",
    "DM Sans",
    "Fira Code",
    "Fira Sans",
    "Geist",
    "Geist Mono",
    "IBM Plex Mono",
    "IBM Plex Sans",
    "Inconsolata",
    "JetBrains Mono",
    "Lato",
    "Lexend",
    "Literata",
    "Lora",
    "Manrope",
    "Merriweather",
    "Mona Sans",
    "Newsreader 16pt",
    "System",
    "Arial",
    "Avenir",
    "Avenir Next",
    "Baskerville",
    "Charter",
    "Courier New",
    "Futura",
    "Georgia",
    "Gill Sans",
    "Helvetica",
    "Helvetica Neue",
    "Iowan Old Style",
    "Menlo",
    "Monaco",
    "Optima",
    "Palatino",
    "SF Mono",
    "Trebuchet MS",
    "Verdana",
];
