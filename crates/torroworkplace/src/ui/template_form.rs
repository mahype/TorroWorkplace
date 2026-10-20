//! The dialog of one workspace template (task WS.6, issue #243; draft
//! `docs/prototypes/workspaces/entwurf.html`): its name, its key (a row "None", 1 … 9, 0 with the
//! name of the template that holds a key, and a hint when saving moves a key), and the section
//! "Startup" (a switch, the screen as a sketch in true proportions from `workspaces.list_screens`,
//! what happens when the screen is missing).
//!
//! Two modes ([`Mode`]): `Edit` changes a template that exists; `Save` follows "Save workspace…",
//! which saved the active workspace already – its title is "Save workspace", it says what was
//! captured, and "Cancel" deletes the template again.
//!
//! Two hosts, one [`Form`]: the control centre lays [`view`] over its window (Workspaces ›
//! Edit… and Save workspace…), and `torroworkplace ui --template=<id> [--saved]` shows [`window_view`]
//! as a small window of its own ([`run`], class `torro_core::app::WORKPLACE_DIALOG_CLASS`), which the tray and
//! `apps.open_app` with the target `template:<id>` start. Neither host reads or writes anything
//! itself: "Save" sends only what changed – `RenameTemplate`, `SetTemplateShortcut`,
//! `SetTemplateStartup`, in this order, each after the core answered the one before ([`Step`]) –
//! and an error stays in the dialog, in words for people ([`refusal`]); the core's details go to
//! stderr.
//!
//! The keyboard reaches everything ([`key_message`]): while the name has no focus, the digits choose
//! the key and Backspace or Delete "None", L switches "Load at startup", ← and → choose the screen,
//! F what happens without it. A layer under the dialog ([`with_keys`]) takes those keys only when no
//! field took them, so they type into the name while it has the focus. In the dialog's own window,
//! where the name takes the focus at the start, Tab leaves the name and comes back to it. The keys
//! stand as `kbd` hints next to what they choose.

use std::collections::VecDeque;
use std::time::Instant;

use iced::keyboard::{self, Key, key::Named};
use iced::theme::Mode as SystemMode;
use iced::widget::{Space, Stack, button, canvas, column, container, operation, row, rule, scrollable, svg, text};
use iced::{Alignment, Element, Fill, Length, Rectangle, Size, Subscription, Task, Theme, mouse};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Settings, ShortcutModifiers};
use torro_gui::dialog;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, Cover, Surface, TitleButtons};
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets;
use torro_gui::zoom::{self, Zoom};
use torro_gui::{style, typo};
use torro_ipc::{Request, Response, ScreenInfo, client};
use torro_setups::{ScreenFallback, Startup, TemplateSummary};

use super::Launch;
use super::workspaces::{relative_time, windows_text};
use crate::workspace_keys;

/// The id of the name field, which takes the focus when the dialog's window opens.
pub const NAME_INPUT: &str = "template-name";

/// What the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// "Edit…" of a template: "Cancel" closes and changes nothing.
    Edit,
    /// After "Save workspace…", which saved the template already: "Cancel" deletes it again.
    Save,
}

/// The digit keys in the order of the keyboard, after "None": the row the dialog shows.
const KEYS: [Option<u8>; 11] =
    [None, Some(1), Some(2), Some(3), Some(4), Some(5), Some(6), Some(7), Some(8), Some(9), Some(0)];

/// What the dialog shows and wants to change. Built with [`Form::new`]; [`Form::update`] and
/// [`Form::answered`] say with a [`Step`] what the host does next.
#[derive(Debug, Clone)]
pub struct Form {
    mode: Mode,
    /// The template as far as the dialog knows: the summary it opened with, plus every change the
    /// core took since. "Save" sends only what differs from it.
    stored: TemplateSummary,
    name: String,
    key: Option<u8>,
    /// The key the mode `Save` suggests: the next free one, for the hint under the keys.
    suggested: Option<u8>,
    startup: bool,
    /// The screen it loads on at login; kept while the switch is off, so switching back keeps it.
    screen: Option<String>,
    fallback: ScreenFallback,
    /// The requests still to send after the one on its way.
    queue: VecDeque<Request>,
    /// The request the core is answering now; `None` while nothing is on its way.
    sending: Option<Request>,
    /// "Save" was pressed once: from then on what is missing shows.
    tried: bool,
    /// What the core refused, until the next "Save".
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Name(String),
    /// A key of the row; `None` is "None", no key.
    Key(Option<u8>),
    /// The switch "Load at startup".
    Startup(bool),
    /// A screen of the sketch or its chip, by its name.
    Screen(String),
    Fallback(ScreenFallback),
    /// "Save" or Enter in the name.
    Save,
    /// "Cancel", ✕ or Escape.
    Cancel,
    /// A click next to the dialog: closes it in the mode `Edit`; in the mode `Save` nothing, since
    /// cancelling there deletes the template, which a stray click must not do.
    Dismiss,
}

/// What the host does after [`Form::update`] or [`Form::answered`].
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a step is handled once and at once; boxing the request would only add an allocation"
)]
pub enum Step {
    /// Nothing; the dialog stays.
    Stay,
    /// Send this request; its answer goes to [`Form::answered`].
    Send(Request),
    /// The dialog is done: close it (the window ends).
    Close,
}

/// What the dialog needs to know besides the template, from the host's data.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// All templates, for the names of those that hold a key.
    pub templates: &'a [TemplateSummary],
    /// The screens of `workspaces.list_screens`; empty when the window manager has none.
    pub screens: &'a [ScreenInfo],
    /// The modifiers of the keys (`Settings::workspaces`), for "Shortcut · Super + Alt +".
    pub modifiers: ShortcutModifiers,
    /// A narrow window (below 600): the keys in two rows, the screens as chips.
    pub narrow: bool,
}

impl Form {
    /// The dialog for `template` in `mode`. `templates` are all templates (the template itself may
    /// be among them or not yet, after a save), `screens` those the window manager has now.
    ///
    /// The mode `Save` chooses the next free key ([`next_free_key`]), so a new template gets a key
    /// with one click; `Edit` starts with what the template has. The screen starts at the one of
    /// its startup, else at [`default_screen`].
    pub fn new(mode: Mode, template: &TemplateSummary, templates: &[TemplateSummary], screens: &[ScreenInfo]) -> Form {
        let suggested = match mode {
            Mode::Save => next_free_key(templates, &template.id),
            Mode::Edit => None,
        };
        let startup = template.startup.as_ref();
        Form {
            mode,
            stored: template.clone(),
            name: template.name.clone(),
            key: template.shortcut.or(suggested),
            suggested,
            startup: startup.is_some(),
            screen: startup.map(|startup| startup.screen.clone()).or_else(|| default_screen(template, screens)),
            fallback: startup.map(|startup| startup.fallback).unwrap_or_default(),
            queue: VecDeque::new(),
            sending: None,
            tried: false,
            error: None,
        }
    }

    /// The mode the dialog opened in; the hosts know it already, the tests ask.
    #[cfg(test)]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The template's id.
    pub fn id(&self) -> &str {
        &self.stored.id
    }

    /// A request of the dialog is on its way: its answer belongs to [`Form::answered`].
    pub fn waits(&self) -> bool {
        self.sending.is_some()
    }

    /// Applies `message`. While a request is on its way the dialog takes nothing.
    pub fn update(&mut self, message: Message) -> Step {
        if self.waits() {
            return Step::Stay;
        }
        match message {
            Message::Name(name) => self.name = name,
            Message::Key(key) => self.key = key,
            Message::Startup(on) => self.startup = on,
            Message::Screen(screen) => self.screen = Some(screen),
            Message::Fallback(fallback) => self.fallback = fallback,
            Message::Save => return self.save(),
            Message::Cancel => return self.cancel(),
            Message::Dismiss if self.mode == Mode::Edit => return self.cancel(),
            Message::Dismiss => {}
        }
        Step::Stay
    }

    /// The core's answer to the request the dialog sent. `None` when the dialog waits for none:
    /// the answer is not its own. An error ends the round and stays in the dialog in words for
    /// people ([`refusal`]: by its code when the answer has one, `Err` has none); what was taken
    /// before it counts as stored, so the next "Save" does not send it again.
    pub fn answered(&mut self, result: &Result<Response, String>) -> Option<Step> {
        let sent = self.sending.take()?;
        let refused = match result {
            Ok(Response::Error { code, message }) => Some(refusal(Some(code), message)),
            Err(detail) => Some(refusal(None, detail)),
            Ok(_) => None,
        };
        match refused {
            Some(words) => {
                self.queue.clear();
                self.error = Some(words);
                Some(Step::Stay)
            }
            None => {
                self.took(&sent);
                Some(self.next())
            }
        }
    }

    /// "Save": nothing while something is missing (it shows from now on), else the round of
    /// requests for what changed; nothing changed closes the dialog.
    fn save(&mut self) -> Step {
        self.tried = true;
        self.error = None;
        if self.problem().is_some() {
            return Step::Stay;
        }
        self.queue = self.requests().into();
        self.next()
    }

    /// "Cancel": closes; in the mode `Save` it deletes the template first.
    fn cancel(&mut self) -> Step {
        match self.mode {
            Mode::Edit => Step::Close,
            Mode::Save => {
                self.queue = VecDeque::from([Request::DeleteTemplate { id: self.stored.id.clone() }]);
                self.next()
            }
        }
    }

    /// The next request of the round, or closing when it is done.
    fn next(&mut self) -> Step {
        match self.queue.pop_front() {
            Some(request) => {
                self.sending = Some(request.clone());
                Step::Send(request)
            }
            None => Step::Close,
        }
    }

    /// The core took `request`: the template has it now.
    fn took(&mut self, request: &Request) {
        match request {
            Request::RenameTemplate { name, .. } => self.stored.name = name.clone(),
            Request::SetTemplateShortcut { key, .. } => self.stored.shortcut = *key,
            Request::SetTemplateStartup { startup, .. } => self.stored.startup = startup.clone(),
            _ => {}
        }
    }

    /// What keeps "Save" from sending, for people: an empty name, or loading at startup without a
    /// screen.
    pub fn problem(&self) -> Option<&'static str> {
        if self.name.trim().is_empty() {
            Some(tr("Give the template a name."))
        } else if self.startup && self.screen.is_none() {
            Some(tr("Choose the screen it loads on."))
        } else {
            None
        }
    }

    /// Where and how the template should load at login, as the dialog shows it now.
    fn wanted_startup(&self) -> Option<Startup> {
        let screen = self.screen.clone().filter(|_| self.startup)?;
        Some(Startup { screen, fallback: self.fallback })
    }

    /// The requests for what differs from [`Form::stored`], in the order the core gets them: name,
    /// key, startup. One request per property, so nothing the dialog did not change is touched.
    pub fn requests(&self) -> Vec<Request> {
        let id = &self.stored.id;
        let mut requests = Vec::new();
        let name = self.name.trim();
        if name != self.stored.name {
            requests.push(Request::RenameTemplate { id: id.clone(), name: name.to_string() });
        }
        if self.key != self.stored.shortcut {
            requests.push(Request::SetTemplateShortcut { id: id.clone(), key: self.key });
        }
        let startup = self.wanted_startup();
        if startup != self.stored.startup {
            requests.push(Request::SetTemplateStartup { id: id.clone(), startup });
        }
        requests
    }
}

/// What the dialog says when the core refused one of its requests (`code`, as `Response::Error`
/// has it) or gave no answer (`None`): words for people that say what to do. `detail`, the core's
/// message or why the call failed, goes to stderr only: it is English for the log, not for people.
pub fn refusal(code: Option<&str>, detail: &str) -> String {
    eprintln!("torroworkplace: the template dialog's request failed ({}): {detail}", code.unwrap_or("no answer"));
    match code {
        Some("busy") => tr("A template is being loaded or saved. Try again in a moment."),
        Some("not-found") => tr("This template no longer exists. It may have been deleted."),
        _ => tr("The change could not be saved. Try again."),
    }
    .to_string()
}

/// The first key in the order of the keyboard (1 … 9, 0) that no template but `id` holds.
pub fn next_free_key(templates: &[TemplateSummary], id: &str) -> Option<u8> {
    KEYS.into_iter().flatten().find(|key| owner(templates, id, *key).is_none())
}

/// The template other than `id` that holds `key`.
fn owner<'a>(templates: &'a [TemplateSummary], id: &str, key: u8) -> Option<&'a TemplateSummary> {
    templates.iter().find(|template| template.id != id && template.shortcut == Some(key))
}

/// The screen a template loads on when its startup is switched on first: the one it was saved on,
/// if the window manager has it, else the main screen, else the first; without any screen the one
/// it was saved on (the core allows a screen that is not connected now).
pub fn default_screen(template: &TemplateSummary, screens: &[ScreenInfo]) -> Option<String> {
    let saved_on = template.monitor.as_deref();
    let known = saved_on.filter(|name| screens.iter().any(|screen| screen.name == *name));
    known
        .map(str::to_string)
        .or_else(|| screens.iter().find(|screen| screen.main).or(screens.first()).map(|screen| screen.name.clone()))
        .or_else(|| saved_on.map(str::to_string))
}

// ----- Keyboard ---------------------------------------------------------------------------------

/// What a key press does in the dialog when no field took it (the name has no focus): `1` … `9`,
/// `0` choose that key, Backspace and Delete "None", `L` switches "Load at startup"; while it is on,
/// ← and → choose the screen before or after the chosen one in the order the dialog shows them
/// (`screens`, stopping at either end; without a chosen one → takes the first, ← the last) and `F`
/// switches what happens without it. `None` for every other key, for a key held with Ctrl, Alt or
/// Cmd (those are the suite's and the system's: zoom, the template keys themselves) and while a
/// request is on its way.
pub fn key_message(
    form: &Form,
    screens: &[ScreenInfo],
    key: &Key,
    physical: keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> Option<Message> {
    if form.waits() || modifiers.command() || modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    match key {
        Key::Named(Named::Backspace | Named::Delete) => return Some(Message::Key(None)),
        Key::Named(Named::ArrowLeft) if form.startup => return neighbour_screen(form, screens, false),
        Key::Named(Named::ArrowRight) if form.startup => return neighbour_screen(form, screens, true),
        _ => {}
    }
    match key.to_latin(physical)?.to_ascii_lowercase() {
        digit @ '0'..='9' => {
            digit.to_digit(10).and_then(|digit| u8::try_from(digit).ok()).map(|key| Message::Key(Some(key)))
        }
        'l' => Some(Message::Startup(!form.startup)),
        'f' if form.startup => Some(Message::Fallback(match form.fallback {
            ScreenFallback::Main => ScreenFallback::Skip,
            ScreenFallback::Skip => ScreenFallback::Main,
        })),
        _ => None,
    }
}

/// The screen after (`forward`) or before the chosen one among `screens`; at the end it stays.
fn neighbour_screen(form: &Form, screens: &[ScreenInfo], forward: bool) -> Option<Message> {
    let at = form.screen.as_deref().and_then(|chosen| screens.iter().position(|screen| screen.name == chosen));
    let next = match (at, forward) {
        (Some(at), true) => (at + 1).min(screens.len().saturating_sub(1)),
        (Some(at), false) => at.saturating_sub(1),
        (None, true) => 0,
        (None, false) => screens.len().checked_sub(1)?,
    };
    screens.get(next).map(|screen| Message::Screen(screen.name.clone()))
}

/// `content` with the dialog's keys under it ([`key_message`]): a layer that draws nothing and
/// lies *under* the dialog, so it gets a key press only when no widget above took it – a focused
/// name keeps its digits, letters, arrows and Backspace. It takes the keys it acts on, so the host
/// does not act on them too.
fn with_keys<'a>(form: &'a Form, screens: &'a [ScreenInfo], content: Element<'a, Message>) -> Element<'a, Message> {
    Stack::with_children([content]).push_under(canvas(KeyLayer { form, screens }).width(Fill).height(Fill)).into()
}

/// The layer of [`with_keys`].
struct KeyLayer<'a> {
    form: &'a Form,
    screens: &'a [ScreenInfo],
}

impl canvas::Program<Message> for KeyLayer<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, physical_key, modifiers, .. }) = event else {
            return None;
        };
        key_message(self.form, self.screens, key, *physical_key, *modifiers)
            .map(|message| canvas::Action::publish(message).and_capture())
    }

    fn draw(
        &self,
        _state: &Self::State,
        _renderer: &iced::Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        Vec::new()
    }
}

// ----- View -------------------------------------------------------------------------------------

/// The dialog over the control centre: head, body and foot in a [`dialog::frame`], whose body
/// scrolls when the window is too low. The host lays it into its dialog surface. The keys work as
/// [`key_message`] says.
pub fn view<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let (head, body, foot) = parts(form, facts, false);
    with_keys(form, facts.screens, dialog::frame(head, body, foot, space::xl()))
}

/// The dialog as a window of its own: the foot at the window's bottom, the body scrolling above it.
/// The name has the focus at first, so the name's label shows that Tab leaves it.
pub fn window_view<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let (head, body, foot) = parts(form, facts, true);
    let body = scrollable(body)
        .direction(scrollable::Direction::Vertical(style::scrollbar().spacing(space::xs())))
        .style(style::scrollable);
    with_keys(form, facts.screens, column![head, container(body).height(Fill), foot].spacing(space::xl()).into())
}

/// Head (title, what the template holds, ✕ in the mode `Edit`), body and foot of the dialog; `window`
/// when it is the dialog's own window, where Tab moves the focus.
fn parts<'a>(
    form: &'a Form,
    facts: Facts<'a>,
    window: bool,
) -> (Element<'a, Message>, Element<'a, Message>, Element<'a, Message>) {
    (head(form), body(form, facts, window), foot(form, facts))
}

fn head<'a>(form: &'a Form) -> Element<'a, Message> {
    let template = &form.stored;
    let title = match form.mode {
        Mode::Edit => tr("Edit template"),
        Mode::Save => tr("Save workspace"),
    };
    let mut detail = vec![windows_text(template.windows)];
    if !template.workspace.is_empty() {
        detail.push(match &template.monitor {
            Some(screen) => {
                trf("workspace {workspace} on {screen}", &[("workspace", &template.workspace), ("screen", screen)])
            }
            None => trf("workspace {workspace}", &[("workspace", &template.workspace)]),
        });
    }
    if form.mode == Mode::Edit {
        detail.push(trf("Updated {when}", &[("when", &relative_time(template.updated, chrono::Utc::now()))]));
    }
    let words =
        column![typo::text(typo::title(), title), typo::text(typo::meta(), detail.join(" · ")).style(theme::text_3),]
            .spacing(space::xs())
            .width(Fill);
    // The mode `Save` has no ✕: closing there is "Cancel", which deletes, and says so by its place.
    let close = (form.mode == Mode::Edit)
        .then(|| widgets::icon_button(Icon::X, tr("Close"), (!form.waits()).then_some(Message::Cancel)));
    row![words].push(close).spacing(space::m()).align_y(Alignment::Start).into()
}

/// Name, key and the section "Startup" (its title bound to the switch with `space::s()`), the fields
/// under the switch, and what the core refused; `section_gap()` between the fields.
fn body<'a>(form: &'a Form, facts: Facts<'a>, window: bool) -> Element<'a, Message> {
    let submit = (!form.waits()).then_some(Message::Save);
    let name = super::field(tr("Name"), &form.name).id(NAME_INPUT).on_input(Message::Name).on_submit_maybe(submit);
    // Only the window gives the name the focus and moves it with Tab; over the control centre the
    // name has it only after a click, and leaves it with a click elsewhere.
    let tab = window.then(|| {
        widgets::with_tooltip(widgets::kbd("Tab"), tr("Press Tab to leave the name and choose with the keys"))
    });
    let mut name_field = column![label_row(tr("Name"), tab)].push(name).spacing(space::xs());
    if form.tried && form.name.trim().is_empty() {
        name_field = name_field.push(error_text(tr("Give the template a name.")));
    }
    let startup = column![section_title(tr("Startup")), startup_switch(form)].spacing(space::s());
    let mut body = column![name_field, shortcut_field(form, facts), startup].spacing(layout::section_gap());
    if form.startup {
        body = body.push(container(startup_fields(form, facts)).padding(iced::Padding::ZERO.left(switch_indent())));
    }
    if let Some(error) = &form.error {
        body = body.push(error_text(error.clone()));
    }
    body.into()
}

/// "Shortcut · Super + Alt +" with the digits' hint, the row of keys, and the hint under it: who
/// loses a key, or which key the mode `Save` chose.
fn shortcut_field<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let keys = workspace_keys::modifiers_label(facts.modifiers);
    let title = trf("Shortcut · {keys} +", &[("keys", &keys)]);
    let mut field = column![label_row(title, Some(widgets::kbd("0–9"))), key_slots(form, facts)].spacing(space::xs());
    let id = form.id();
    let moves = form.key.and_then(|key| owner(facts.templates, id, key).map(|owner| (key, owner)));
    if let Some((key, owner)) = moves {
        let text = trf(
            "Key {key} belongs to “{name}”. Saving moves it here; “{name}” keeps no key.",
            &[("key", &key), ("name", &owner.name)],
        );
        field = field.push(warning(text));
    } else if let Some(key) = form.key.filter(|key| form.suggested == Some(*key)) {
        field = field.push(hint(trf("{key} is the next free key.", &[("key", &key)])));
    }
    field.into()
}

/// The keys "None", 1 … 9, 0 as one row; in a narrow window two rows of six. A key another template
/// holds carries a dot, and its tooltip says whose it is (the hint under the row says it too once it
/// is chosen): a name cut to two letters in the tile would say nothing.
fn key_slots<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let slot = |key: Option<u8>| key_slot(form, facts, key);
    if facts.narrow {
        let (first, second) = KEYS.split_at(KEYS.len().div_ceil(2));
        let line = |keys: &[Option<u8>]| {
            let mut line = row(keys.iter().map(|key| slot(*key))).spacing(space::xs());
            // The second row is one short: an empty cell keeps the columns under each other.
            for _ in keys.len()..first.len() {
                line = line.push(Space::new().width(Fill));
            }
            line
        };
        column![line(first), line(second)].spacing(space::xs()).into()
    } else {
        row(KEYS.iter().map(|key| slot(*key))).spacing(space::xs()).into()
    }
}

/// One key of [`key_slots`], chosen like a chip (`style::button::choice_chip`), `size::control()`
/// high: a check when chosen, the digit, a dot when another template holds it.
fn key_slot<'a>(form: &'a Form, facts: Facts<'a>, key: Option<u8>) -> Element<'a, Message> {
    let on = form.key == key;
    let taken = key.and_then(|key| owner(facts.templates, form.id(), key));
    let digit: Element<'a, Message> = match key {
        Some(key) => typo::text(typo::body_strong(), key.to_string()).into(),
        None => typo::text(typo::caption(), tr("None")).wrapping(text::Wrapping::None).into(),
    };
    // The chosen key carries a check, so the choice does not rest on the fill alone.
    let mut content = row![].spacing(space::xs()).align_y(Alignment::Center);
    if on {
        content = content.push(check());
    }
    content = content.push(digit);
    if taken.is_some() {
        content = content.push(widgets::status_dot(widgets::Status::Inactive));
    }
    let width = if key.is_none() && !facts.narrow { Length::Shrink } else { Length::Fill };
    let slot = button(container(content).center_x(Fill).center_y(Fill))
        .width(width)
        .height(size::control())
        .padding(style::ITEM_PADDING)
        .style(style::button::choice_chip(on))
        .on_press_maybe((!form.waits()).then_some(Message::Key(key)));
    let tip = match (key, taken) {
        (Some(key), Some(owner)) => format!("{} · {}", workspace_keys::label(facts.modifiers, key), owner.name),
        (Some(key), None) => workspace_keys::label(facts.modifiers, key),
        // Key names are not translated in the suite.
        (None, _) => format!("{} · Backspace", tr("No key")),
    };
    widgets::with_tooltip(slot, tip)
}

/// The switch "Load at startup" with what it means under its label, and its key.
fn startup_switch<'a>(form: &'a Form) -> Element<'a, Message> {
    let switch = super::switch(form.startup, Message::Startup);
    let words = column![
        typo::text(typo::body_strong(), tr("Load at startup")),
        typo::text(typo::meta(), tr("When Torro starts after you log in")).style(theme::text_3),
    ]
    .width(Fill);
    row![switch, words, widgets::kbd("L")].spacing(space::m()).align_y(Alignment::Center).into()
}

/// The screen and what happens without it, under the switch, each label with its keys.
fn startup_fields<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let fallback = widgets::chip_row(vec![
        widgets::choice_chip(
            tr("Use the main screen"),
            None,
            form.fallback == ScreenFallback::Main,
            Some(Message::Fallback(ScreenFallback::Main)),
        ),
        widgets::choice_chip(
            tr("Skip"),
            None,
            form.fallback == ScreenFallback::Skip,
            Some(Message::Fallback(ScreenFallback::Skip)),
        ),
    ]);
    let arrows = row![widgets::kbd("←"), widgets::kbd("→")].spacing(space::xs());
    let mut screen = column![label_row(tr("Screen"), Some(arrows.into())), screens(form, facts)].spacing(space::xs());
    if form.tried && form.screen.is_none() {
        screen = screen.push(error_text(tr("Choose the screen it loads on.")));
    }
    let missing = label_row(tr("If the screen is missing"), Some(widgets::kbd("F")));
    column![screen, column![missing, fallback].spacing(space::xs())].spacing(layout::section_gap()).into()
}

/// The screens to choose from: a sketch of each in true proportions (wide) or a chip each
/// (narrow), and a chip for the chosen screen when it is not connected now. Chips carry no icon
/// (the suite has no screen icon); a chosen one adds the check in front.
fn screens<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let chosen = form.screen.as_deref();
    let missing = chosen.filter(|name| facts.screens.iter().all(|screen| screen.name != *name));
    let missing_chip = missing.map(|name| {
        widgets::choice_chip(
            trf("{screen} · not connected", &[("screen", &name)]),
            None,
            true,
            Some(Message::Screen(name.to_string())),
        )
    });
    if facts.screens.is_empty() && missing.is_none() {
        return hint(tr("No screen found. Start the window manager, then open this dialog again.").to_string());
    }
    if facts.narrow {
        let mut chips: Vec<Element<'a, Message>> = facts
            .screens
            .iter()
            .map(|screen| {
                widgets::choice_chip(
                    screen_name(screen),
                    None,
                    chosen == Some(screen.name.as_str()),
                    Some(Message::Screen(screen.name.clone())),
                )
            })
            .collect();
        chips.extend(missing_chip);
        return widgets::chip_row(chips);
    }
    let scale = sketch_scale(facts.screens);
    let sketches = row(facts.screens.iter().map(|screen| sketch(screen, chosen == Some(screen.name.as_str()), scale)))
        .spacing(space::s())
        .align_y(Alignment::End);
    match missing_chip {
        Some(chip) => column![sketches, chip].spacing(space::s()).into(),
        None => sketches.into(),
    }
}

/// "DP-1 · main" for a chip.
fn screen_name(screen: &ScreenInfo) -> String {
    match screen.main {
        true => trf("{screen} · main", &[("screen", &screen.name)]),
        false => screen.name.clone(),
    }
}

/// One screen as a rectangle in its proportions, `scale` logical pixels per pixel of the screen,
/// with its name inside (a check in front when chosen) and its size under it.
fn sketch<'a>(screen: &'a ScreenInfo, on: bool, scale: f32) -> Element<'a, Message> {
    let mut name = row![].spacing(space::xs()).align_y(Alignment::Center);
    if on {
        name = name.push(check());
    }
    name = name.push(typo::text(typo::caption(), screen.name.as_str()).wrapping(text::Wrapping::None));
    let rectangle = button(container(name).center_x(Fill).center_y(Fill).clip(true))
        .width(screen.width as f32 * scale)
        .height(screen.height as f32 * scale)
        .padding(iced::Padding::ZERO)
        .style(style::button::choice_chip(on))
        .on_press(Message::Screen(screen.name.clone()));
    let size = format!("{}×{}", screen.width, screen.height);
    let size = if screen.main { trf("{size} · main", &[("size", &size)]) } else { size };
    column![rectangle, typo::text(typo::meta().tabular(), size).style(theme::text_3)]
        .spacing(space::xs())
        .align_x(Alignment::Center)
        .into()
}

/// How many logical pixels a pixel of a screen gets in the sketch: the tallest screen as high as
/// [`sketch_height`], all of them side by side at most [`sketch_room`] wide.
pub fn sketch_scale(screens: &[ScreenInfo]) -> f32 {
    let tallest = screens.iter().map(|screen| screen.height).max().unwrap_or(1).max(1) as f32;
    let by_height = sketch_height() / tallest;
    let gaps = space::s() * screens.len().saturating_sub(1) as f32;
    let wide: f32 = screens.iter().map(|screen| screen.width as f32).sum();
    if wide <= 0.0 {
        return by_height;
    }
    by_height.min((sketch_room() - gaps).max(0.0) / wide)
}

/// The height of the tallest screen in the sketch: a header and a bit (52 in the standard design).
fn sketch_height() -> f32 {
    size::header() + space::s()
}

/// The widest the sketches get together: what a dialog in a medium window has beside the indent of
/// the switch (460 in the standard design).
fn sketch_room() -> f32 {
    size::list()
}

/// How far the fields under the switch are indented: the switch and its gap, so they stand under
/// its label.
fn switch_indent() -> f32 {
    style::CHECK_SIZE * 2.0 + space::m()
}

/// The foot: in the mode `Edit` a note that the windows stay (not in narrow windows), "Cancel" and
/// "Save"; the button that sent waits.
fn foot<'a>(form: &'a Form, facts: Facts<'a>) -> Element<'a, Message> {
    let deleting = matches!(form.sending, Some(Request::DeleteTemplate { .. }));
    let note = (form.mode == Mode::Edit && !facts.narrow).then(|| {
        container(
            typo::text(typo::meta(), tr("The windows stay as saved. Update captures them again.")).style(theme::text_3),
        )
        .width(Fill)
    });
    let idle = !form.waits();
    let cancel = widgets::text_button(None, tr("Cancel"), style::button::secondary, idle.then_some(Message::Cancel));
    let save = if form.waits() && !deleting {
        widgets::busy_button(tr("Saving…"), style::button::primary)
    } else {
        widgets::text_button(None, tr("Save"), style::button::primary, idle.then_some(Message::Save))
    };
    let mut foot = row![].spacing(space::s()).align_y(Alignment::Center);
    foot = match note {
        Some(note) => foot.push(note),
        None => foot.push(Space::new().width(Fill)),
    };
    foot.push(cancel).push(save).into()
}

/// The check in front of a chosen key or screen: 14, in `text`.
fn check<'a>() -> Element<'a, Message> {
    icon::icon(Icon::Check, icon::SIZE_SM).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
}

/// The label over a field: `CAPTION` in `text_2`.
fn label<'a>(words: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::caption(), words).style(theme::text_2).into()
}

/// A field's [`label`] with the keys that choose in it at the right (`kbd`), as menus show theirs.
fn label_row<'a>(words: impl text::IntoFragment<'a>, keys: Option<Element<'a, Message>>) -> Element<'a, Message> {
    row![container(label(words)).width(Fill)].push(keys).spacing(space::s()).align_y(Alignment::Center).into()
}

/// A section's title with a line to the right ("Startup ———").
fn section_title<'a>(title: &'a str) -> Element<'a, Message> {
    row![widgets::section_title(title), rule::horizontal(size::HAIRLINE).style(style::rule)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// A note under a field: `META` in `text_3`.
fn hint<'a>(words: String) -> Element<'a, Message> {
    typo::text(typo::meta(), words).style(theme::text_3).into()
}

/// A note that warns: the alert in the attention colour, the words in `text_2` (colour only in the
/// icon). The alert stands in a box as high as the first line, so it is centred on that line, whether
/// the words take one line or wrap.
fn warning<'a>(words: String) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE_SM)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention) });
    row![
        container(alert).center_y(typo::line_height(typo::meta())),
        typo::text(typo::meta(), words).style(theme::text_2)
    ]
    .spacing(space::s())
    .align_y(Alignment::Start)
    .into()
}

/// A sentence in the error colour.
fn error_text<'a>(words: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::meta(), words).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }).into()
}

// ----- The dialog's own window ------------------------------------------------------------------

/// What the window loaded from the core.
#[derive(Debug, Clone, Default)]
struct Loaded {
    templates: Vec<TemplateSummary>,
    screens: Vec<ScreenInfo>,
    settings: Settings,
}

/// The window of `torroworkplace ui --template=<id> [--saved]`.
struct Window {
    launch: Launch,
    id: String,
    mode: Mode,
    loaded: Option<Result<Loaded, String>>,
    form: Option<Form>,
    system_mode: SystemMode,
    zoom: Zoom,
    size: Size,
}

#[derive(Debug, Clone)]
#[expect(
    clippy::large_enum_variant,
    reason = "an answer is handled once; boxing would only add an allocation, as in the control centre"
)]
enum Event {
    Loaded(Result<Box<Loaded>, String>),
    Form(Message),
    Sent(Result<Response, String>),
    Retry,
    Escape,
    /// Tab: the name's focus goes or comes back.
    Tab,
    SystemMode(SystemMode),
    Resized(Size),
    Zoom(zoom::Message),
    DragWindow,
}

/// Runs the dialog of the template `launch.template` as a window of its own: the class
/// `torro_core::app::WORKPLACE_DIALOG_CLASS` (Hyprland floats it, captures leave it out), [`size::form_max`]
/// wide. It ends after "Save" or "Cancel" went through. With `--demo` it shows the demo's template
/// and sends nothing.
pub fn run(launch: Launch) -> iced::Result {
    let socket = torro_ipc::socket_path().filter(|_| !launch.demo);
    if let Some(socket) = &socket {
        torro_gui::source::ensure_core(socket);
    }
    torro_gui::design::start(socket.as_deref(), &launch.forced());
    iced::application(move || Window::new(launch.clone()), Window::update, Window::view)
        .settings(torro_gui::settings())
        .title(|window: &Window| match window.mode {
            Mode::Edit => tr("Edit template").to_string(),
            Mode::Save => tr("Save workspace").to_string(),
        })
        .theme(Window::theme)
        .scale_factor(|window: &Window| window.zoom.factor())
        .subscription(Window::subscription)
        .window(window_settings())
        .run()
}

/// The window: [`size::form_max`] square, with the dialog's class as app id on Linux.
pub fn window_settings() -> iced::window::Settings {
    let mut settings = torro_gui::window(AppId::WORKPLACE);
    settings.size = Size::new(size::form_max(), size::form_max());
    // The class is Linux's app id; elsewhere windows have none that a capture or a rule would read.
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = torro_core::app::WORKPLACE_DIALOG_CLASS.to_string();
    }
    settings
}

impl Window {
    fn new(launch: Launch) -> (Window, Task<Event>) {
        let id = launch.template.clone().unwrap_or_default();
        let mode = if launch.saved { Mode::Save } else { Mode::Edit };
        let demo = launch.demo;
        let mut window = Window {
            launch,
            id,
            mode,
            loaded: None,
            form: None,
            system_mode: SystemMode::None,
            zoom: Zoom::new(torro_ipc::socket_path().filter(|_| !demo)),
            size: window_settings().size,
        };
        let first = if demo {
            let data = super::launch::demo_data();
            window.loaded_with(Ok(Loaded { templates: data.templates, screens: data.screens, settings: data.settings }))
        } else {
            first_load()
        };
        let tasks = Task::batch([
            first,
            window.zoom.fetch().map(Event::Zoom),
            iced::system::theme().map(Event::SystemMode),
            iced::window::latest().and_then(iced::window::size).map(Event::Resized),
            torro_gui::place_traffic_lights(),
        ]);
        (window, tasks)
    }

    /// Takes what was loaded and opens the form; the template's name takes the focus.
    fn loaded_with(&mut self, loaded: Result<Loaded, String>) -> Task<Event> {
        if self.form.is_none()
            && let Ok(loaded) = &loaded
            && let Some(template) = loaded.templates.iter().find(|template| template.id == self.id)
        {
            self.form = Some(Form::new(self.mode, template, &loaded.templates, &loaded.screens));
        }
        self.loaded = Some(loaded);
        if self.form.is_some() { operation::focus(NAME_INPUT) } else { Task::none() }
    }

    fn theme(&self) -> Theme {
        let stored = match &self.loaded {
            Some(Ok(loaded)) => loaded.settings.appearance,
            _ => Appearance::System,
        };
        theme::for_appearance(self.launch.appearance.unwrap_or(stored), self.system_mode)
    }

    fn subscription(&self) -> Subscription<Event> {
        Subscription::batch([
            iced::system::theme_changes().map(Event::SystemMode),
            iced::event::listen_with(keys),
            iced::window::resize_events().map(|(_, size)| Event::Resized(size)),
            self.zoom.subscription().map(Event::Zoom),
        ])
    }

    fn update(&mut self, event: Event) -> Task<Event> {
        let started = Instant::now();
        let task = match event {
            Event::Loaded(loaded) => self.loaded_with(loaded.map(|loaded| *loaded)),
            Event::Retry => {
                self.loaded = None;
                load()
            }
            Event::Form(message) => match self.form.as_mut() {
                Some(form) => {
                    let step = form.update(message);
                    self.step(step)
                }
                None => Task::none(),
            },
            Event::Escape => match self.form.as_mut() {
                Some(form) => {
                    let step = form.update(Message::Cancel);
                    self.step(step)
                }
                None => iced::exit(),
            },
            // The name is the only field: moving on from it leaves the focus nowhere, so the
            // keys of the dialog work ([`key_message`]); from nowhere it goes back to the name.
            Event::Tab => operation::focus_next(),
            Event::Sent(result) => match self.form.as_mut().and_then(|form| form.answered(&result)) {
                Some(step) => self.step(step),
                None => Task::none(),
            },
            Event::SystemMode(mode) => {
                self.system_mode = mode;
                Task::none()
            }
            Event::Resized(size) => {
                self.size = size;
                Task::none()
            }
            Event::Zoom(message) => self.zoom.update(message).map(Event::Zoom),
            Event::DragWindow => torro_gui::drag_window(),
        };
        torro_core::timing::slow("update", started);
        task
    }

    /// Does what the form asks for: sends (the demo answers itself and changes nothing), or ends.
    fn step(&mut self, step: Step) -> Task<Event> {
        match step {
            Step::Stay => Task::none(),
            Step::Close => iced::exit(),
            Step::Send(request) if self.launch.demo => {
                let data = super::Data::default();
                let answer = super::launch::demo_answer(&request, &data).unwrap_or(Response::Ok);
                self.update(Event::Sent(Ok(answer)))
            }
            Step::Send(request) => send(request),
        }
    }

    fn view(&self) -> Element<'_, Event> {
        let body: Element<'_, Event> = match (&self.form, &self.loaded) {
            (Some(form), Some(Ok(loaded))) => {
                let narrow = matches!(layout::of(self.size), SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip);
                let facts = Facts {
                    templates: &loaded.templates,
                    screens: &loaded.screens,
                    modifiers: loaded.settings.workspaces.modifiers,
                    narrow,
                };
                window_view(form, facts).map(Event::Form)
            }
            (_, None) => widgets::loading(),
            (_, Some(Err(problem))) => widgets::error_state(problem.clone(), Event::Retry),
            (None, Some(Ok(_))) => {
                widgets::error_state(tr("This template no longer exists. It may have been deleted."), Event::Retry)
            }
        };
        let content =
            container(body).padding(space::xl()).width(Fill).height(Fill).style(style::container::panel_flush);
        let content = self.zoom.view(content);
        shell::titlebar(
            content,
            self.zoom.factor(),
            Surface::Panel,
            Cover::Nothing,
            TitleButtons::none(),
            Event::DragWindow,
        )
    }
}

/// Escape, and Tab or Shift + Tab, wherever the focus is.
fn keys(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<Event> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else { return None };
    match key {
        Key::Named(Named::Escape) => Some(Event::Escape),
        Key::Named(Named::Tab) if modifiers.is_empty() || modifiers == keyboard::Modifiers::SHIFT => Some(Event::Tab),
        _ => None,
    }
}

/// The first load, once the core answers (one started just now does not listen yet).
fn first_load() -> Task<Event> {
    match torro_ipc::socket_path() {
        Some(socket) => Task::perform(
            async move { torro_gui::source::wait_for_core(&socket, torro_gui::source::CORE_START_WAIT).await },
            |_| (),
        )
        .then(|()| load()),
        None => load(),
    }
}

/// Loads the templates, the screens and the settings (the keys' modifiers, the appearance). What
/// went wrong shows in words for people ([`unexpected`]).
fn load() -> Task<Event> {
    Task::perform(
        async {
            let socket = torro_ipc::socket_path().ok_or_else(|| tr("No home directory").to_string())?;
            let templates = match client::call(&socket, &Request::ListTemplates).await {
                Ok(Response::Templates { templates }) => templates,
                Ok(Response::Error { code, message }) => return Err(unexpected(format!("{code}: {message}"))),
                Ok(other) => return Err(unexpected(format!("{other:?}"))),
                Err(err) => return Err(unexpected(err)),
            };
            // Without a window manager there are none; the dialog says so at the screens.
            let screens = match client::call(&socket, &Request::ListScreens).await {
                Ok(Response::Screens { screens }) => screens,
                _ => Vec::new(),
            };
            let settings = match client::call(&socket, &Request::Settings).await {
                Ok(Response::Settings { settings }) => settings,
                _ => Settings::default(),
            };
            Ok(Box::new(Loaded { templates, screens, settings }))
        },
        Event::Loaded,
    )
}

/// Sends one request. The core's answer comes back as it is, a refusal with its code, so the form
/// can say what it means ([`refusal`]); `Err` only when no answer came, with why for stderr.
fn send(request: Request) -> Task<Event> {
    Task::perform(
        async move {
            let socket = torro_ipc::socket_path().ok_or_else(|| "no home directory".to_string())?;
            client::call(&socket, &request).await.map_err(|err| err.to_string())
        },
        Event::Sent,
    )
}

/// The words for a load that went wrong, which the window shows with "Retry"; `detail` goes to stderr.
fn unexpected(detail: impl std::fmt::Display) -> String {
    eprintln!("torroworkplace: the template dialog could not load: {detail}");
    tr("Torro Workplace did not answer as expected. Try again.").to_string()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn template(id: &str, name: &str, key: Option<u8>) -> TemplateSummary {
        TemplateSummary {
            id: id.into(),
            name: name.into(),
            windows: 3,
            updated: Utc.with_ymd_and_hms(2026, 10, 1, 9, 0, 0).unwrap(),
            shortcut: key,
            startup: None,
            workspace: "2".into(),
            monitor: Some("HDMI-A-1".into()),
        }
    }

    fn templates() -> Vec<TemplateSummary> {
        vec![
            template("deep", "Deep work", Some(1)),
            template("mail", "Mail & chat", Some(2)),
            template("dev", "Dev", None),
        ]
    }

    fn screens() -> Vec<ScreenInfo> {
        vec![
            ScreenInfo { name: "DP-1".into(), width: 3840, height: 1080, scale: 2.0, main: true },
            ScreenInfo { name: "HDMI-A-1".into(), width: 1920, height: 1080, scale: 1.0, main: false },
        ]
    }

    fn edit(id: &str) -> Form {
        let all = templates();
        let template = all.iter().find(|template| template.id == id).expect("a template");
        Form::new(Mode::Edit, template, &all, &screens())
    }

    #[test]
    fn saving_without_changes_closes_without_a_request() {
        let mut form = edit("dev");
        assert!(form.requests().is_empty());
        assert_eq!(form.update(Message::Save), Step::Close);
    }

    /// Task WS.6: only what changed, in the order name, key, startup, each after the answer to the
    /// one before; the round ends in closing.
    #[test]
    fn saving_sends_what_changed_one_after_the_other() {
        let mut form = edit("dev");
        form.update(Message::Name("  Development ".into()));
        form.update(Message::Key(Some(3)));
        form.update(Message::Startup(true));
        let rename = Request::RenameTemplate { id: "dev".into(), name: "Development".into() };
        assert_eq!(form.update(Message::Save), Step::Send(rename));
        assert!(form.waits());
        assert_eq!(form.update(Message::Key(Some(4))), Step::Stay, "nothing is taken while it sends");
        let key = Request::SetTemplateShortcut { id: "dev".into(), key: Some(3) };
        assert_eq!(form.answered(&Ok(Response::Ok)), Some(Step::Send(key)));
        // The template was saved on HDMI-A-1, which is connected: that is its screen.
        let startup = Startup { screen: "HDMI-A-1".into(), fallback: ScreenFallback::Main };
        let startup = Request::SetTemplateStartup { id: "dev".into(), startup: Some(startup) };
        assert_eq!(form.answered(&Ok(Response::Ok)), Some(Step::Send(startup)));
        assert_eq!(form.answered(&Ok(Response::Ok)), Some(Step::Close));
        assert_eq!(form.answered(&Ok(Response::Ok)), None, "an answer it does not wait for is not its own");
    }

    /// Task WS.6: choosing a key another template holds says that it moves; saving sends only the
    /// key – the core takes it from the other template.
    #[test]
    fn a_key_another_template_holds_moves_here() {
        let mut form = edit("dev");
        form.update(Message::Key(Some(2)));
        let all = templates();
        assert_eq!(owner(&all, form.id(), 2).map(|t| t.name.as_str()), Some("Mail & chat"));
        assert_eq!(form.requests(), [Request::SetTemplateShortcut { id: "dev".into(), key: Some(2) }]);
        drop(view(
            &form,
            Facts { templates: &all, screens: &screens(), modifiers: ShortcutModifiers::SuperAlt, narrow: false },
        ));
        // Its own key is no other template's.
        let mail = edit("mail");
        assert!(owner(&all, mail.id(), 2).is_none());
        assert_eq!(mail.key, Some(2));
    }

    #[test]
    fn none_takes_the_key_away_and_startup_off_is_sent_as_null() {
        let mut all = templates();
        all[0].startup = Some(Startup { screen: "DP-1".into(), fallback: ScreenFallback::Skip });
        let mut form = Form::new(Mode::Edit, &all[0], &all, &screens());
        assert!(form.startup && form.fallback == ScreenFallback::Skip);
        form.update(Message::Key(None));
        form.update(Message::Startup(false));
        assert_eq!(
            form.requests(),
            [
                Request::SetTemplateShortcut { id: "deep".into(), key: None },
                Request::SetTemplateStartup { id: "deep".into(), startup: None },
            ]
        );
    }

    /// An error ends the round and stays in the dialog; what the core took before counts as stored.
    #[test]
    fn an_error_stays_in_the_dialog_and_the_next_save_sends_only_what_is_left() {
        let mut form = edit("dev");
        form.update(Message::Name("Development".into()));
        form.update(Message::Key(Some(5)));
        assert!(matches!(form.update(Message::Save), Step::Send(Request::RenameTemplate { .. })));
        assert!(matches!(form.answered(&Ok(Response::Ok)), Some(Step::Send(Request::SetTemplateShortcut { .. }))));
        assert_eq!(form.answered(&Err("connection refused".into())), Some(Step::Stay));
        assert_eq!(form.error.as_deref(), Some(tr("The change could not be saved. Try again.")), "no raw text");
        assert!(!form.waits());
        assert_eq!(form.requests(), [Request::SetTemplateShortcut { id: "dev".into(), key: Some(5) }]);
        // A refusal with a code says what it means; the core's message stays out of the dialog.
        form.update(Message::Save);
        let refused = Ok(Response::Error { code: "busy".into(), message: "a save or load runs".into() });
        assert_eq!(form.answered(&refused), Some(Step::Stay));
        assert_eq!(form.error.as_deref(), Some(tr("A template is being loaded or saved. Try again in a moment.")));
        assert_eq!(refusal(Some("not-found"), "x"), tr("This template no longer exists. It may have been deleted."));
        assert_eq!(refusal(Some("error"), "a template needs a name"), tr("The change could not be saved. Try again."));
    }

    #[test]
    fn an_empty_name_or_a_missing_screen_keeps_save_from_sending() {
        let mut form = edit("dev");
        form.update(Message::Name("   ".into()));
        assert!(!form.tried);
        assert_eq!(form.update(Message::Save), Step::Stay);
        assert!(form.tried && form.problem() == Some(tr("Give the template a name.")));
        let mut lost = Form::new(Mode::Edit, &TemplateSummary { monitor: None, ..template("x", "X", None) }, &[], &[]);
        lost.update(Message::Startup(true));
        assert_eq!(lost.update(Message::Save), Step::Stay);
        assert_eq!(lost.problem(), Some(tr("Choose the screen it loads on.")));
    }

    /// Task WS.6: Cancel and Escape close the mode `Edit` without a request; in the mode `Save` they
    /// delete the template that was saved just now, and a click next to the dialog does nothing.
    #[test]
    fn cancel_in_the_mode_save_deletes_and_a_stray_click_does_not() {
        assert_eq!(edit("dev").update(Message::Cancel), Step::Close);
        assert_eq!(edit("dev").update(Message::Dismiss), Step::Close);
        let all = templates();
        let new = template("ws-3", "Workspace 3, 20.10. 10:41", None);
        let mut save = Form::new(Mode::Save, &new, &all, &screens());
        assert_eq!(save.update(Message::Dismiss), Step::Stay);
        assert_eq!(save.update(Message::Cancel), Step::Send(Request::DeleteTemplate { id: "ws-3".into() }));
        assert_eq!(save.answered(&Ok(Response::Ok)), Some(Step::Close));
    }

    #[test]
    fn the_mode_save_suggests_the_next_free_key() {
        let all = templates();
        assert_eq!(next_free_key(&all, "ws-3"), Some(3));
        assert_eq!(next_free_key(&all, "mail"), Some(2), "its own key is free for it");
        let full: Vec<TemplateSummary> =
            KEYS.iter().flatten().map(|k| template(&format!("t{k}"), "T", Some(*k))).collect();
        assert_eq!(next_free_key(&full, "new"), None);
        let new = template("ws-3", "Workspace 3", None);
        let save = Form::new(Mode::Save, &new, &all, &screens());
        assert_eq!((save.key, save.suggested), (Some(3), Some(3)));
        assert_eq!(save.requests(), [Request::SetTemplateShortcut { id: "ws-3".into(), key: Some(3) }]);
        assert_eq!(Form::new(Mode::Edit, &new, &all, &screens()).key, None, "editing suggests nothing");
    }

    #[test]
    fn the_first_screen_is_the_one_it_was_saved_on_else_the_main_one() {
        let saved_on = |monitor: Option<&str>| TemplateSummary {
            monitor: monitor.map(str::to_string),
            ..template("x", "X", None)
        };
        assert_eq!(default_screen(&saved_on(Some("HDMI-A-1")), &screens()).as_deref(), Some("HDMI-A-1"));
        assert_eq!(default_screen(&saved_on(Some("DP-9")), &screens()).as_deref(), Some("DP-1"), "the main screen");
        assert_eq!(default_screen(&saved_on(None), &screens()).as_deref(), Some("DP-1"));
        assert_eq!(default_screen(&saved_on(Some("DP-9")), &[]).as_deref(), Some("DP-9"), "no screens known");
        assert_eq!(default_screen(&saved_on(None), &[]), None);
    }

    #[test]
    fn the_sketch_keeps_proportions_and_its_room() {
        let scale = sketch_scale(&screens());
        assert!((1080.0 * scale - sketch_height()).abs() < 0.01, "the tallest is as high as the sketch");
        let many: Vec<ScreenInfo> =
            (0..6).map(|i| ScreenInfo { name: format!("S{i}"), ..screens()[0].clone() }).collect();
        let scale = sketch_scale(&many);
        let wide = 6.0 * 3840.0 * scale + 5.0 * space::s();
        assert!(wide <= sketch_room() + 0.01, "{wide}");
        assert!(sketch_scale(&[]) > 0.0);
    }

    #[test]
    fn the_dialog_builds_wide_and_narrow_in_both_modes() {
        let all = templates();
        let screens = screens();
        for narrow in [false, true] {
            let facts = Facts { templates: &all, screens: &screens, modifiers: ShortcutModifiers::CtrlAlt, narrow };
            let mut form = edit("dev");
            form.update(Message::Startup(true));
            form.update(Message::Screen("DP-7".into()));
            drop(view(&form, facts));
            drop(window_view(&form, facts));
            let save = Form::new(Mode::Save, &template("ws", "W", None), &all, &screens);
            drop(view(&save, facts));
            let none = Facts { screens: &[], ..facts };
            drop(view(&edit("deep"), none));
        }
    }

    #[test]
    fn the_window_is_the_dialog_class_and_a_form_wide() {
        let settings = window_settings();
        assert_eq!(settings.size.width, size::form_max());
        #[cfg(target_os = "linux")]
        assert_eq!(settings.platform_specific.application_id, torro_core::app::WORKPLACE_DIALOG_CLASS);
    }

    /// The demo's window opens the demo template's form, and its "Save" goes through without a core.
    #[test]
    fn the_demo_window_opens_the_form_and_closes_after_save() {
        let launch = Launch::parse(&["--demo".to_string(), "--template=writing".to_string()]).expect("valid");
        let (mut window, _) = Window::new(launch);
        let form = window.form.as_ref().expect("the demo template's form");
        assert_eq!((form.id(), form.mode()), ("writing", Mode::Edit));
        let _ = window.update(Event::Form(Message::Name("Writing time".into())));
        let _ = window.update(Event::Form(Message::Save));
        assert!(!window.form.as_ref().expect("still there until the window ends").waits());
        drop(window.view());
        let gone = Launch::parse(&["--demo".to_string(), "--template=nope".to_string(), "--saved".to_string()])
            .expect("valid");
        let (window, _) = Window::new(gone);
        assert!(window.form.is_none());
        drop(window.view());
    }

    /// Review fix D1: while no field has the focus, the keyboard reaches every choice of the dialog;
    /// keys held with Ctrl, Alt or Cmd stay the suite's, and nothing is taken while it sends.
    #[test]
    fn keys_choose_the_key_the_switch_the_screen_and_the_fallback() {
        use keyboard::Modifiers;
        use keyboard::key::{Code, Physical};
        let screens = screens();
        let with = |form: &Form, key: Key, modifiers: Modifiers| {
            key_message(form, &screens, &key, Physical::Code(Code::KeyA), modifiers)
        };
        let press = |form: &Form, key: Key| with(form, key, Modifiers::empty());
        let char = |c: &str| Key::Character(c.into());
        let mut form = edit("dev");

        assert!(matches!(press(&form, char("3")), Some(Message::Key(Some(3)))));
        assert!(matches!(press(&form, char("0")), Some(Message::Key(Some(0)))));
        assert!(matches!(press(&form, Key::Named(Named::Backspace)), Some(Message::Key(None))));
        assert!(matches!(press(&form, Key::Named(Named::Delete)), Some(Message::Key(None))));
        for held in [Modifiers::CTRL, Modifiers::ALT, Modifiers::LOGO] {
            assert!(with(&form, char("3"), held).is_none(), "{held:?} + 3 is not the dialog's");
        }
        assert!(press(&form, char("x")).is_none());
        // Off: the screen and the fallback are not shown, so their keys do nothing.
        assert!(press(&form, Key::Named(Named::ArrowRight)).is_none());
        assert!(press(&form, char("f")).is_none());

        let switch = press(&form, char("L")).expect("L switches");
        assert!(matches!(switch, Message::Startup(true)));
        form.update(switch);
        // Saved on HDMI-A-1, the second screen: → stays at the end, ← goes to DP-1.
        assert!(matches!(press(&form, Key::Named(Named::ArrowRight)), Some(Message::Screen(s)) if s == "HDMI-A-1"));
        let left = press(&form, Key::Named(Named::ArrowLeft)).expect("←");
        assert!(matches!(&left, Message::Screen(s) if s == "DP-1"));
        form.update(left);
        assert!(matches!(press(&form, Key::Named(Named::ArrowLeft)), Some(Message::Screen(s)) if s == "DP-1"));
        let fallback = press(&form, char("f")).expect("F switches the fallback");
        assert!(matches!(fallback, Message::Fallback(ScreenFallback::Skip)));
        form.update(fallback);
        assert!(matches!(press(&form, char("f")), Some(Message::Fallback(ScreenFallback::Main))));
        assert!(matches!(press(&form, char("l")), Some(Message::Startup(false))));

        form.update(Message::Key(Some(7)));
        assert!(matches!(form.update(Message::Save), Step::Send(_)));
        assert!(press(&form, char("3")).is_none(), "nothing while a request is on its way");
    }

    /// A screen not among the connected ones: → takes the first, ← the last.
    #[test]
    fn arrows_from_a_screen_not_connected_start_at_an_end() {
        use keyboard::key::{Code, Physical};
        let mut form = edit("dev");
        form.update(Message::Startup(true));
        form.update(Message::Screen("DP-9".into()));
        let press = |key: Named| {
            key_message(&form, &screens(), &Key::Named(key), Physical::Code(Code::KeyA), keyboard::Modifiers::empty())
        };
        assert!(matches!(press(Named::ArrowRight), Some(Message::Screen(s)) if s == "DP-1"));
        assert!(matches!(press(Named::ArrowLeft), Some(Message::Screen(s)) if s == "HDMI-A-1"));
        assert!(
            key_message(
                &form,
                &[],
                &Key::Named(Named::ArrowLeft),
                Physical::Code(Code::KeyA),
                keyboard::Modifiers::empty()
            )
            .is_none()
        );
    }

    #[test]
    fn templates_have_keys_up_to_the_limit() {
        assert_eq!(KEYS.iter().flatten().count(), usize::from(torro_setups::MAX_SHORTCUT) + 1);
    }
}
