//! Gallery of every style in `torro_gui::style`, each in every state side by side.
//!
//! `cargo run -p torro-gui --example gallery` (dark) or `… --example gallery -- --light`. The
//! "Dark · Light" switch in the header flips the mode live. States are forced through the style
//! closure (`.style(move |theme, _| f(theme, Status::Hovered))`), so one screenshot shows hover,
//! pressed, focus and disabled at once; the "Live" section has real widgets to try by hand.
//!
//! A developer tool, not part of an app: its labels are not translated, and the "+" stands in for
//! the Lucide icons of task F.4.

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, radio, row, rule, scrollable, text_editor, text_input,
    toggler,
};
use iced::{Alignment, Element, Fill, Length, Theme};
use torro_gui::style;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::typo;

fn main() -> iced::Result {
    let light = std::env::args().any(|arg| arg == "--light");
    iced::application(move || Gallery::new(light), Gallery::update, Gallery::view)
        .settings(torro_gui::settings())
        .title("Torro style gallery")
        .theme(Gallery::theme)
        .window(window())
        .run()
}

fn window() -> iced::window::Settings {
    iced::window::Settings {
        size: iced::Size::new(3700.0, 1060.0),
        #[cfg(target_os = "linux")]
        platform_specific: iced::window::settings::PlatformSpecific {
            application_id: "torro-gallery".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

struct Gallery {
    light: bool,
    section: usize,
    query: String,
    notes: text_editor::Content,
    view: Option<&'static str>,
    checked: bool,
    toggled: bool,
    choice: Option<u8>,
}

#[derive(Debug, Clone)]
enum Msg {
    Noop,
    Light(bool),
    Section(usize),
    Query(String),
    Notes(text_editor::Action),
    View(&'static str),
    Check(bool),
    Toggle(bool),
    Choice(u8),
}

const SECTIONS: [&str; 5] = ["Buttons", "Rows", "Inputs", "Checks", "Surfaces"];
const VIEWS: [&str; 4] = ["Day", "Week", "Month", "Agenda"];
/// Width of the style names in front of each line of states.
const NAME_WIDTH: f32 = 160.0;

type ButtonStyle = fn(&Theme, button::Status) -> button::Style;

impl Gallery {
    fn new(light: bool) -> Self {
        Self {
            light,
            section: 0,
            query: String::new(),
            notes: text_editor::Content::with_text("Bring the printed agenda.\nAsk about the budget."),
            view: Some("Week"),
            checked: true,
            toggled: true,
            choice: Some(1),
        }
    }

    fn theme(&self) -> Theme {
        if self.light { theme::light() } else { theme::theme() }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Noop => {}
            Msg::Light(light) => self.light = light,
            Msg::Section(i) => self.section = i,
            Msg::Query(q) => self.query = q,
            Msg::Notes(action) => self.notes.perform(action),
            Msg::View(v) => self.view = Some(v),
            Msg::Check(c) => self.checked = c,
            Msg::Toggle(t) => self.toggled = t,
            Msg::Choice(c) => self.choice = Some(c),
        }
    }

    fn view(&self) -> Element<'_, Msg> {
        // Three columns, so one screenshot of a wide window (3700 × 1060) shows every style.
        let first = column![section("Buttons", buttons()), section("Checks", checks())].spacing(space::XXL);
        let second = column![section("Rows", rows())].spacing(space::XXL);
        let third = column![
            section("Inputs", inputs(&self.notes)),
            section("Surfaces", surfaces()),
            section("Live", self.live()),
        ]
        .spacing(space::XXL);
        let body = row![first, second, third].spacing(space::XXL).padding(space::XL);
        let content = column![
            self.header(),
            rule::horizontal(1).style(style::rule),
            scrollable(body).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable),
        ];
        let panel = container(content).width(Fill).height(Fill).style(style::container::panel);
        row![self.sidebar(), container(panel).padding(iced::Padding { left: 0.0, ..size::INSET.into() })].into()
    }

    fn sidebar(&self) -> Element<'_, Msg> {
        let items = SECTIONS.iter().enumerate().map(|(i, name)| {
            button(container(typo::text(typo::BODY, *name)).center_y(Fill))
                .width(Fill)
                .height(size::ROW_SIDEBAR)
                .padding(style::ITEM_PADDING)
                .on_press(Msg::Section(i))
                .style(style::button::sidebar_item(i == self.section))
                .into()
        });
        let title = typo::text(typo::CAPTION, "Sections").style(theme::text_3);
        let list =
            column![container(title).padding(style::ITEM_PADDING).height(size::ROW_SIDEBAR)].extend(items).spacing(2);
        container(list).width(size::SIDEBAR).height(Fill).padding(space::S).style(style::container::sidebar).into()
    }

    fn header(&self) -> Element<'_, Msg> {
        let mode = if self.light { "Light" } else { "Dark" };
        let title =
            row![typo::text(typo::BODY_STRONG, "Style gallery"), typo::text(typo::BODY, mode).style(theme::text_3)]
                .spacing(space::S);
        let modes = segmented(&[("Dark", !self.light, Msg::Light(false)), ("Light", self.light, Msg::Light(true))]);
        let bar = row![
            title,
            Space::new().width(Fill),
            modes,
            text_button("Filter", style::button::ghost, true),
            text_button("New event", style::button::primary, true),
        ]
        .spacing(space::S)
        .align_y(Alignment::Center);
        container(bar)
            .height(size::HEADER)
            .center_y(size::HEADER)
            .padding(iced::Padding { left: space::L, right: space::M, ..iced::Padding::ZERO })
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
            .spacing(space::S)
            .align_y(Alignment::Center),
            row![
                text_input("Search", &self.query)
                    .on_input(Msg::Query)
                    .padding(style::INPUT_PADDING)
                    .width(260)
                    .style(style::text_input),
                pick_list(VIEWS, self.view, Msg::View)
                    .padding(style::PICK_LIST_PADDING)
                    .text_size(typo::CAPTION.size)
                    .text_line_height(typo::CAPTION.line_height)
                    .font(typo::CAPTION.font)
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
            .spacing(space::L)
            .align_y(Alignment::Center),
        ]
        .spacing(space::L)
        .into()
    }
}

/// A titled block of the gallery.
fn section<'a>(title: &'a str, content: Element<'a, Msg>) -> Element<'a, Msg> {
    column![typo::text(typo::TITLE, title), content].spacing(space::L).into()
}

/// One style: its name, then one cell per state.
fn line<'a>(name: &'a str, cells: Vec<Element<'a, Msg>>) -> Element<'a, Msg> {
    row![
        typo::text(typo::META, name).style(theme::text_2).width(NAME_WIDTH),
        iced::widget::Row::with_children(cells).spacing(space::XL)
    ]
    .align_y(Alignment::Start)
    .into()
}

/// A widget with the state it is drawn in written under it.
fn cell<'a>(state: &'a str, widget: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    column![widget.into(), typo::text(typo::META, state).style(theme::text_3)].spacing(space::XS).into()
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

fn label(text: &str) -> Element<'_, Msg> {
    container(typo::text(typo::CAPTION, text)).center_y(Fill).into()
}

fn text_button<'a>(text: &'a str, style: ButtonStyle, enabled: bool) -> Element<'a, Msg> {
    let b = button(label(text)).height(size::CONTROL_SM).padding(style::BUTTON_PADDING).style(style);
    if enabled { b.on_press(Msg::Noop).into() } else { b.into() }
}

fn segmented<'a>(options: &[(&'a str, bool, Msg)]) -> Element<'a, Msg> {
    let segments = options.iter().map(|(name, active, msg)| {
        button(label(name))
            .height(size::CONTROL_SM - 2.0 * style::SEGMENT_INSET - 2.0)
            .padding(style::BUTTON_PADDING)
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
                        forced(label(text), style, status).height(size::CONTROL_SM).padding(style::BUTTON_PADDING),
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
                let plus = container(typo::text(typo::BODY, "+")).center(Fill);
                cell(state, forced(plus, style, status).width(size::CONTROL_SM).height(size::CONTROL_SM).padding(0))
            })
            .collect();
        lines.push(line(name, cells));
    }
    for active in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let seg = forced(label("Week"), style::button::segment(active), status)
                    .height(size::CONTROL_SM - 2.0 * style::SEGMENT_INSET - 2.0)
                    .padding(style::BUTTON_PADDING);
                cell(state, container(seg).padding(style::SEGMENT_INSET).style(style::container::segmented))
            })
            .collect();
        lines.push(line(if active { "segment(active)" } else { "segment" }, cells));
    }
    column(lines).spacing(space::L).into()
}

fn rows() -> Element<'static, Msg> {
    let mut lines = Vec::new();
    for active in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let content = row![
                    typo::text(typo::BODY, "Today"),
                    Space::new().width(Fill),
                    typo::text(typo::META.tabular(), "5").style(theme::text_3)
                ]
                .align_y(Alignment::Center);
                let item = forced(container(content).center_y(Fill), style::button::sidebar_item(active), status)
                    .width(size::SIDEBAR - 2.0 * space::S - 2.0 * space::XS)
                    .height(size::ROW_SIDEBAR)
                    .padding(style::ITEM_PADDING);
                cell(state, container(item).padding(space::XS).style(style::container::sidebar))
            })
            .collect();
        lines.push(line(if active { "sidebar_item(active)" } else { "sidebar_item" }, cells));
    }
    for selected in [false, true] {
        let cells = BUTTON_STATES
            .iter()
            .map(|&(state, status)| {
                let content = row![
                    typo::text(typo::BODY_STRONG, "Review API concept"),
                    Space::new().width(Fill),
                    typo::text(typo::META.tabular(), "Oct 14").style(theme::text_3)
                ]
                .align_y(Alignment::Center);
                let item = forced(container(content).center_y(Fill), style::button::list_row(selected), status)
                    .width(240)
                    .height(size::ROW)
                    .padding(iced::Padding { left: space::L, right: space::L, ..iced::Padding::ZERO });
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
                    typo::text(typo::BODY, "Week"),
                    Space::new().width(Fill),
                    container(typo::text(typo::CAPTION, "W")).padding(style::KBD_PADDING).style(style::container::kbd)
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
    column(lines).spacing(space::L).into()
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
                .padding(style::PICK_LIST_PADDING)
                .text_size(typo::CAPTION.size)
                .text_line_height(typo::CAPTION.line_height)
                .font(typo::CAPTION.font)
                .style(move |t, _| style::pick_list(t, status));
            cell(state, pick)
        })
        .collect();
    picks.push(cell("Menu (pick_list_menu)", pick_menu()));
    column![line("text_input", inputs), line("text_editor", editors), line("pick_list", picks)].spacing(space::L).into()
}

/// The pick list's menu drawn in place: Iced opens it only on click.
fn pick_menu() -> Element<'static, Msg> {
    let items = VIEWS.iter().map(|v| {
        let hovered = *v == "Week";
        container(typo::text(typo::BODY, *v))
            .width(160)
            .padding(style::BUTTON_PADDING)
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
        row(boxes).spacing(space::S).into()
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
    .spacing(space::L)
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
            container(typo::text(typo::BODY, name).style(theme::text_2))
                .width(160)
                .height(96)
                .padding(space::L)
                .style(s)
                .into()
        })
        .collect::<Vec<Element<'_, Msg>>>();
    // Surfaces sit on the window background, as in the app: the panel shows its edge there.
    let well = container(iced::widget::Row::with_children(boxes).spacing(space::L))
        .padding(space::L)
        .style(style::container::sidebar);
    let chip = container(
        row![typo::text(typo::CAPTION, "Today"), typo::text(typo::CAPTION, "Work").style(theme::text_3)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .height(style::CHIP_HEIGHT)
    .center_y(style::CHIP_HEIGHT)
    .padding(style::CHIP_PADDING)
    .style(style::container::chip);
    let kbd = container(typo::text(typo::CAPTION, "Ctrl K")).padding(style::KBD_PADDING).style(style::container::kbd);
    let header = container(typo::text(typo::BODY_STRONG, "Header"))
        .width(320)
        .center_y(size::HEADER)
        .padding(iced::Padding { left: space::L, ..iced::Padding::ZERO })
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
            let lines = (1..=12).map(|i| typo::text(typo::BODY, format!("Row {i}")).into());
            let list = scrollable(column(lines).width(Fill))
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .style(move |t, _| style::scrollable(t, status))
                .width(160)
                .height(96);
            cell(state, container(list).padding(space::S).style(style::container::panel))
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
    .spacing(space::L)
    .into()
}
