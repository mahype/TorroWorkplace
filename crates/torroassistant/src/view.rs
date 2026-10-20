//! How the assistant's window looks (design skill `torro-design`): the Linear frame of the suite
//! (`torro_gui::shell`) with the conversations in the sidebar, the chat as the content – its
//! entries in a column of reading width ([`size::content_max`]) that stays at the bottom while an
//! answer grows, the composer below – and the glance of mini and strip tiles.
//!
//! Nothing here changes state: every control sends a [`Message`] that [`App::update`] handles.
//!
//! - **Wide:** sidebar (or the rail) | header "title · engine" with "…" | chat.
//! - **Medium:** the rail; the expanded sidebar opens as a drawer (panel button, search, `[`).
//! - **Narrow:** flush; the drawer holds the conversations, the header a "New chat" button.
//! - **Mini, strip:** the newest answer and a one-line composer; nothing else.
//! - **Chat window:** flush in every size, without sidebar.
//!
//! Bots (task B.3): their rows, the editor that takes the chat's place, a bot's menu and the card of
//! a proposal are drawn in [`editor`]. The cards of a conversation (task WZ.8, a file a tool made)
//! in [`cards`].
//!
//! The settings (task S.18) take header and content area from `torro_settings_ui::Host`
//! ([`App::settings_frame`]) while they are open; the sidebar stays as it is.

use iced::widget::text::Wrapping;
use iced::widget::{
    Space, button, column, container, mouse_area, responsive, row, scrollable, stack, text_editor, text_input,
};
use iced::{Alignment, Background, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, Cover, SidebarMode, Surface, TitleButtons};
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, MenuSection, Placement, Trail};
use torro_gui::{layout, markdown, style, typo};
use torro_ipc::api::Area;
use torro_ipc::assistant::{ConversationSummary, EngineKind, Entry, EntryContent};
use torro_ipc::{ApprovalInfo, ApprovalState};
use torro_settings_ui::assistant as settings;
use torro_settings_ui::{Frame, Page};

use crate::app::{
    Access, App, Dialog, EDITOR, HISTORY, MAIN, MINI_INPUT, Menu, MenuAt, Message, RENAME, SEARCH, composer_binding,
};
use crate::bots;
use crate::chat::{self, Choice};

mod cards;
mod editor;

/// The fewest lines of an answer worth scrolling in a glance with a file card under it; with less
/// room the answer shows as one line.
const GLANCE_LINES: f32 = 3.0;

/// The anchors of the menus that float next to their buttons; new in every `view`.
#[derive(Default)]
struct Anchors {
    switcher: Anchor,
    header: Anchor,
    picker: Anchor,
    /// The effort of the composer (issue #131).
    effort: Anchor,
    /// The permission mode under the composer (issue #132).
    mode: Anchor,
    areas: Anchor,
    row: Anchor,
    /// A bot's row in the sidebar, for its menu.
    bot_row: Anchor,
    /// The engine of the bot editor.
    bot_picker: Anchor,
}

/// The composer's text area: two lines high at first, at most eight, then it scrolls.
fn editor_heights() -> (f32, f32) {
    let line = typo::line_height(typo::body());
    let frame = style::INPUT_PADDING.top + style::INPUT_PADDING.bottom;
    (2.0 * line + frame, 8.0 * line + frame)
}

/// How wide the label of the picker and of the areas may get before it is cut: room for both
/// beside "Send" in a narrow window.
fn label_room(class: SizeClass) -> f32 {
    match class {
        SizeClass::Wide | SizeClass::Medium => size::sidebar(),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => size::props() / 2.0,
    }
}

/// The colours answers are read in: the paragraphs of Markdown in `text`, not the quieter
/// `text_2` of notes, since the answer is what the person asked for.
fn answer_colours(mut colours: Palette) -> Palette {
    colours.text_2 = colours.text;
    colours
}

/// Above and below a tool line's words: one line is then as high as a small control.
fn tool_inset() -> f32 {
    ((size::control_sm() - typo::line_height(typo::meta())) / 2.0).max(0.0)
}

/// A tool's input: for an object one line per field, the name quiet, the value in monospace; any
/// other input as JSON.
fn input_view<'a>(input: &serde_json::Value) -> Element<'a, Message> {
    let mono = |text: String| typo::text(typo::meta(), text).font(markdown::MONO).style(theme::text_2);
    let Some(fields) = input.as_object().filter(|fields| !fields.is_empty()) else {
        return mono(serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string())).into();
    };
    let lines = fields.iter().map(|(name, value)| {
        let value = match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        row![typo::text(typo::meta(), name.clone()).style(theme::text_3), mono(value).wrapping(Wrapping::WordOrGlyph)]
            .spacing(space::s())
            .into()
    });
    column(lines).spacing(space::xs()).into()
}

/// The width a conversation's title has in a sidebar row: the sidebar less its edges and the
/// row's own padding.
fn row_title_room() -> f32 {
    size::sidebar() - 2.0 * space::s() - style::ITEM_PADDING.left - style::ITEM_PADDING.right
}

/// How far what belongs to a tool line (its input, result and hints) is indented: under its words,
/// past the icon.
fn tool_indent() -> f32 {
    icon::SIZE_SM + space::s()
}

/// `which` at `edge` in a colour of the palette.
fn tinted<'a>(which: Icon, edge: f32, colour: fn(&Palette) -> iced::Color) -> Element<'a, Message> {
    icon::icon(which, edge).style(move |t, _| iced::widget::svg::Style { color: Some(colour(&palette(t))) }).into()
}

/// The icon of a tool's area.
fn area_icon(area: Option<Area>) -> Icon {
    match area {
        Some(Area::Calendar) => Icon::Calendar,
        Some(Area::Tasks) => Icon::ListTodo,
        Some(Area::Github) => Icon::Github,
        Some(Area::Inbox) => Icon::Inbox,
        Some(Area::Mail) => Icon::Mail,
        Some(Area::Workspaces) => Icon::LayoutGrid,
        Some(Area::Accounts) => Icon::Users,
        Some(Area::Settings) => Icon::Settings,
        Some(Area::Apps) => Icon::AppWindow,
        Some(Area::Ai) => Icon::Sparkles,
        Some(Area::Bots) => Icon::Users,
        // The assistant's own tools (task WZ.8): the only one makes files.
        Some(Area::Tools) => cards::FILE_ICON,
        _ => Icon::Search,
    }
}

impl App {
    /// The window with the zoom badge and the notice over it; in mini and strip the glance.
    pub fn view(&self) -> Element<'_, Message> {
        let class = self.class();
        if self.is_glance() {
            let surface = if self.listed || self.chat_only { Surface::of(class) } else { Surface::Window };
            return widgets::ctrl_guard(shell::titlebar(
                self.zoom.view(self.glance(class)),
                self.zoom.factor(),
                surface,
                Cover::Nothing,
                TitleButtons::none(),
                Message::DragWindow,
            ));
        }
        // Always two layers, so the window keeps its state when the notice comes and goes.
        let window = stack![self.window(class), self.notice_layer()].width(Fill).height(Fill);
        let surface = if self.chat_only { Surface::Panel } else { Surface::of(class) };
        // The gear is marked while the settings fill the content area and leads back out of them.
        let mut buttons = TitleButtons::none().settings(Message::ToggleSettings).settings_open(self.settings.is_some());
        if !self.chat_only {
            let label = shell::sidebar_button_label(class, self.sidebar, self.drawer);
            buttons = buttons.sidebar(label, Message::ToggleSidebar);
        }
        let content = self.zoom.view(window);
        widgets::ctrl_guard(shell::titlebar(
            content,
            self.zoom.factor(),
            surface,
            self.cover(),
            buttons,
            Message::DragWindow,
        ))
    }

    /// What lies over the top of the window, which the title band repeats.
    fn cover(&self) -> Cover {
        let drawer = if self.chat_only { Cover::Nothing } else { Cover::drawer(self.class(), self.drawer) };
        // A dialog of an open settings section covers the band like the window's own.
        Cover::dialog(self.dialog.is_some() || self.settings_dialog().is_some()).or(drawer)
    }

    /// The dialog of the open settings section ("Forget this note?"), while one is open.
    fn settings_dialog(&self) -> Option<Element<'_, Message>> {
        let pane = self.settings.as_ref()?;
        settings::overlay(&pane.sections, pane.host.data()).map(|dialog| dialog.map(Message::SettingsPage))
    }

    /// The header bar and the content area of the open settings, as the host draws them: the
    /// section list (wide), the menu in the header (medium) or the steps of a narrow window.
    fn settings_frame<'a>(&'a self, pane: &'a crate::app::SettingsPane, class: SizeClass) -> Frame<'a, Message> {
        let page = settings::page(&pane.sections, pane.host.data(), pane.host.current().id, class);
        let page = Page {
            body: page.body.map(Message::SettingsPage),
            sub_page: page.sub_page.map(|(name, back)| (name, Message::SettingsPage(back))),
            actions: page.actions.into_iter().map(|action| action.map(Message::SettingsPage)).collect(),
        };
        pane.host.view(class, page, Message::SettingsHost)
    }

    /// The notice at the bottom centre; empty without one.
    fn notice_layer(&self) -> Element<'_, Message> {
        match &self.notice {
            Some(notice) => widgets::toast(notice.text.as_str(), None),
            None => Space::new().into(),
        }
    }

    /// The whole window in the layout of `class`, with its menus and dialogs.
    fn window(&self, class: SizeClass) -> Element<'_, Message> {
        let anchors = Anchors::default();
        // The settings take header and content area; the sidebar and its menus stay.
        let (header, body, layer) = match &self.settings {
            Some(pane) => {
                let frame = self.settings_frame(pane, class);
                (frame.header, frame.body, self.layer(&anchors).or(frame.overlay))
            }
            None => (self.header(class, &anchors), self.body(class, &anchors), self.layer(&anchors)),
        };
        let window: Element<'_, Message> = if self.chat_only {
            // A chat window has no sidebar: the flush frame of the narrow layout in every size.
            shell::shell(SizeClass::Narrow, Space::new(), header, body, None).overlay(layer).into()
        } else {
            let rail = (shell::sidebar_mode(class, self.sidebar) == Some(SidebarMode::Rail))
                .then(|| self.sidebar(SidebarMode::Rail, &anchors));
            shell::shell(class, self.sidebar(SidebarMode::Expanded, &anchors), header, body, None)
                .rail(rail)
                .drawer(self.drawer, Message::CloseMenu)
                .overlay(layer)
                .into()
        };
        let window = match &self.dialog {
            Some(dialog) => {
                widgets::modal(window, self.dialog_view(dialog), widgets::DIALOG_WIDTH, Message::CloseDialog)
            }
            None => window,
        };
        // A question of the open settings section lies over the window, like the window's own.
        match self.settings_dialog() {
            Some(dialog) => stack![window, dialog].width(Fill).height(Fill).into(),
            None => window,
        }
    }

    /// The open menu, floating at its button.
    fn layer(&self, anchors: &Anchors) -> Option<Element<'_, Message>> {
        let menu = self.menu.as_ref()?;
        Some(match menu {
            Menu::Switcher => widgets::floating(
                &anchors.switcher,
                Placement::Below(Align::Start),
                shell::switcher_menu(MAIN, Message::Switcher),
                Message::CloseMenu,
            ),
            Menu::Picker => widgets::floating(
                &anchors.picker,
                Placement::Below(Align::Start),
                self.picker_menu(self.choice.as_ref(), Message::Pick),
                Message::CloseMenu,
            ),
            Menu::BotPicker => {
                let chosen = self.bot_draft.as_ref().and_then(|draft| draft.choice.as_ref());
                let menu = self.picker_menu(chosen, |choice| Message::Draft(bots::DraftEdit::Engine(choice)));
                widgets::floating(&anchors.bot_picker, Placement::Below(Align::Start), menu, Message::CloseMenu)
            }
            Menu::Bot { id } => widgets::floating(
                &anchors.bot_row,
                Placement::Below(Align::Start),
                self.bot_menu(id),
                Message::CloseMenu,
            ),
            Menu::Mode => {
                widgets::floating(&anchors.mode, Placement::Below(Align::Start), self.mode_menu(), Message::CloseMenu)
            }
            Menu::Effort => widgets::floating(
                &anchors.effort,
                Placement::Below(Align::Start),
                self.effort_menu(),
                Message::CloseMenu,
            ),
            Menu::Areas => {
                widgets::floating(&anchors.areas, Placement::Below(Align::Start), self.areas_menu(), Message::CloseMenu)
            }
            Menu::Conversation { id, at } => {
                let (anchor, align) = match at {
                    MenuAt::Header => (&anchors.header, Align::End),
                    MenuAt::Row => (&anchors.row, Align::Start),
                };
                widgets::floating(anchor, Placement::Below(align), self.conversation_menu(id), Message::CloseMenu)
            }
        })
    }

    // -- Sidebar --------------------------------------------------------------------------------

    /// The sidebar in `mode`: the search and the conversations, pinned ones first, in the expanded
    /// sidebar; the rail has the head alone (the conversations have no icons to tell them apart,
    /// and the drawer lists them).
    fn sidebar<'a>(&'a self, mode: SidebarMode, anchors: &Anchors) -> Element<'a, Message> {
        let head = shell::sidebar_head(mode, MAIN, &anchors.switcher, Message::Switcher, Message::ToggleSidebar);
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        // Before the conversations came there is nothing to search.
        if mode == SidebarMode::Expanded && self.listed {
            let body = typo::body();
            let search = text_input(tr("Search conversations"), &self.search)
                .id(SEARCH)
                .size(body.size)
                .line_height(body.line_height)
                .font(body.font)
                .padding(style::INPUT_PADDING)
                .width(Fill)
                .style(style::text_input)
                .on_input(Message::Search);
            items.push(search.into());
            // The bots (task B.3) before the conversations: few, and each a way into its chat.
            if self.bots_loaded {
                items.push(widgets::sidebar_group_in(mode, tr("Bots"), None));
                items.extend(self.bots.iter().map(|bot| self.bot_row(bot, anchors)));
                let new = self.bot_draft.as_ref().is_some_and(|draft| draft.id.is_none());
                items.push(widgets::sidebar_item_in(
                    mode,
                    Icon::Plus,
                    tr("New bot"),
                    Trail::None,
                    new,
                    Message::NewBot,
                ));
            }
            let (pinned, recent) = chat::sidebar_groups(&self.conversations, &self.search);
            if !pinned.is_empty() {
                items.push(widgets::sidebar_group_in(mode, tr("Pinned"), None));
                items.extend(pinned.into_iter().map(|c| self.conversation_row(c, anchors)));
            }
            if !recent.is_empty() {
                items.push(widgets::sidebar_group_in(mode, tr("Recent"), None));
                items.extend(recent.into_iter().map(|c| self.conversation_row(c, anchors)));
            }
            if self.listed && self.conversations.is_empty() {
                items.push(sidebar_note(tr("No conversations yet")));
            } else if self.listed && !self.search.trim().is_empty() && self.shown_count() == 0 {
                items.push(sidebar_note(tr("No conversation matches")));
            }
        }
        // Where the band has it (macOS), "Settings" sits in its top right corner.
        let footer = (!shell::has_band())
            .then(|| {
                widgets::sidebar_item_in(
                    mode,
                    Icon::Settings,
                    tr("Settings"),
                    Trail::None,
                    self.settings.is_some(),
                    Message::ToggleSettings,
                )
            })
            .into_iter()
            .collect();
        shell::sidebar(head, items, footer)
    }

    /// How many conversations the search leaves.
    fn shown_count(&self) -> usize {
        let (pinned, recent) = chat::sidebar_groups(&self.conversations, &self.search);
        pinned.len() + recent.len()
    }

    /// A conversation in the sidebar: its title on a row of the sidebar's height, the open one
    /// chosen, one that answers with a quiet mark. A click opens it, a double click opens it in a
    /// window of its own, a right click its menu.
    fn conversation_row<'a>(
        &'a self,
        conversation: &'a ConversationSummary,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let id = conversation.id.clone();
        let active = self.current.as_ref() == Some(&id);
        let hovered = self.hover.as_ref() == Some(&id);
        // The title fits the row or ends in "…", with the whole title as tooltip.
        let mark = if conversation.running { icon::SIZE_SM + space::s() } else { 0.0 };
        let room = row_title_room() - mark;
        let shown = typo::ellipsize(&conversation.title, typo::body(), room);
        let cut = shown != conversation.title;
        let title = container(typo::text(typo::body(), shown).wrapping(Wrapping::None)).width(Fill).clip(true);
        let title: Element<'a, Message> =
            if cut { widgets::with_tooltip(title, conversation.title.as_str()) } else { title.into() };
        let mut content = row![title].spacing(space::s()).align_y(Alignment::Center);
        if conversation.running {
            content = content.push(widgets::with_tooltip(
                tinted(Icon::CircleDotDashed, icon::SIZE_SM, |p| p.text_3),
                tr("Answering"),
            ));
        }
        // The row is no button, so the double and right clicks reach it; it takes the button
        // style of a sidebar row for its surface and text.
        let surface = container(content)
            .width(Fill)
            .height(size::row_sidebar())
            .padding(style::ITEM_PADDING)
            .center_y(size::row_sidebar())
            .style(move |t: &Theme| {
                let status = if hovered { button::Status::Hovered } else { button::Status::Active };
                let look = style::button::sidebar_item(active)(t, status);
                container::Style {
                    background: look.background,
                    text_color: Some(look.text_color),
                    border: look.border,
                    ..container::Style::default()
                }
            });
        let row = mouse_area(surface)
            .on_press(Message::Select(id.clone()))
            .on_double_click(Message::OpenInWindow(id.clone()))
            .on_right_press(Message::ToggleMenu(Menu::Conversation { id: id.clone(), at: MenuAt::Row }))
            .on_enter(Message::Hover(Some(id.clone())))
            .on_exit(Message::Hover(None))
            .interaction(iced::mouse::Interaction::Pointer);
        let menu_here = matches!(&self.menu, Some(Menu::Conversation { id: open, at: MenuAt::Row }) if *open == id);
        if menu_here { widgets::anchored(&anchors.row, row) } else { row.into() }
    }

    // -- Header ---------------------------------------------------------------------------------

    /// The header: the conversation's title with its engine and model quietly beside it, and its
    /// "…" menu; in the narrow main window the drawer button and "New chat".
    fn header<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Element<'a, Message> {
        let mut leading = Vec::new();
        // Narrow has neither sidebar nor rail, whose head holds "New".
        let narrow_main = !self.chat_only && class == SizeClass::Narrow;
        if narrow_main && !shell::has_band() {
            leading.push(shell::sidebar_toggle(Message::ToggleSidebar));
        }
        if let Some(draft) = &self.bot_draft {
            return self.editor_header(draft, leading);
        }
        // Who answers: the assistant's or the bot's picture, and its name before the engine (task
        // C.10, B.3).
        let (name, picture) = self.speaker();
        leading.push(avatar(&name, &picture, header_avatar()));
        // A new chat has no engine yet: its picture says who answers, the welcome says the name.
        // Narrow keeps who answers when it is a bot, and drops the engine; a deleted bot has none.
        let note = match (class == SizeClass::Narrow, self.chat_bot().is_some(), self.bot_gone()) {
            (_, true, true) => Some(name.clone()),
            (true, true, false) => Some(name.clone()),
            (true, false, _) => None,
            (false, _, _) => self.engine_note().map(|engine| format!("{name} · {engine}")),
        };
        let title = shell::header_title(None, self.conversation_title(), note);
        let mut trailing = Vec::new();
        if narrow_main {
            trailing.push(widgets::icon_button(Icon::SquarePen, tr("New chat"), Some(Message::NewChat)));
        }
        if let Some(id) = self.chat_bot().filter(|_| !self.bot_gone()) {
            let edit = Message::EditBot(id.to_owned());
            trailing.push(widgets::icon_button(Icon::SlidersHorizontal, tr("Edit bot"), Some(edit)));
        }
        if let Some(id) = self.current.clone().filter(|_| self.transcript.summary.is_some()) {
            let menu = Menu::Conversation { id, at: MenuAt::Header };
            let more = widgets::icon_button(Icon::Ellipsis, tr("More"), Some(Message::ToggleMenu(menu)));
            trailing.push(widgets::anchored(&anchors.header, more));
        }
        shell::header_bar(leading, title, trailing)
    }

    /// "Ollama · qwen3:14b": who answered the open conversation last.
    fn engine_note(&self) -> Option<String> {
        let summary = self.transcript.summary.as_ref()?;
        Some(self.choice_label(&Choice { engine: summary.engine.clone(), model: summary.model.clone() }))
    }

    /// An engine with its model, as the picker names them.
    fn choice_label(&self, choice: &Choice) -> String {
        let engine = self.engines.iter().find(|engine| engine.id == choice.engine);
        let engine_name = engine.map_or(choice.engine.as_str(), |engine| engine.name.as_str());
        match choice.model.as_ref().map(|id| self.model_name(&choice.engine, id)) {
            Some(model) => format!("{engine_name} · {model}"),
            None => engine_name.to_owned(),
        }
    }

    /// What the model `id` of `engine` is called: an agent's own (issue #131), an AI account's as
    /// listed – turned on or not –, else its id.
    fn model_name(&self, engine: &str, id: &str) -> String {
        let fixed = self.engines.iter().find(|known| known.id == engine).map(|known| known.models.as_slice());
        let listed = self.models.get(engine).and_then(|models| models.as_ref().ok());
        fixed
            .into_iter()
            .chain(listed.map(Vec::as_slice))
            .flatten()
            .find(|model| model.id == id)
            .map_or_else(|| id.to_owned(), |model| model.name.clone())
    }

    // -- Body -----------------------------------------------------------------------------------

    /// The chat: the entries above, the composer below; or loading and the error of the first load.
    fn body<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Element<'a, Message> {
        if let Some(draft) = &self.bot_draft {
            return self.editor(draft, class, anchors);
        }
        if !self.chat_only && !self.listed {
            return match &self.list_error {
                Some(_) => widgets::error_state(tr("Couldn't load the conversations."), Message::Retry),
                None => widgets::loading(),
            };
        }
        if self.chat_error.is_some() {
            return widgets::error_state(tr("Couldn't load the conversation."), Message::Retry);
        }
        if self.current.is_some() && self.transcript.summary.is_none() {
            return widgets::loading();
        }
        let side = match class {
            SizeClass::Wide | SizeClass::Medium => space::l(),
            _ => space::m(),
        };
        let history: Element<'a, Message> = if self.current.is_none() {
            container(self.welcome()).width(Fill).height(Fill).into()
        } else {
            let entries = container(widgets::content(size::content_max(), self.entries()))
                .width(Fill)
                .padding(Padding { top: space::xl(), right: side, bottom: space::l(), left: side });
            // Anchored at the top: an answer that grows below moves nothing the person scrolled up
            // to read. At the bottom the history keeps to the newest entry (`App::scroll_to_newest`).
            scrollable(entries)
                .id(HISTORY)
                .direction(scrollable::Direction::Vertical(style::scrollbar()))
                .on_scroll(Message::HistoryScrolled)
                .style(style::scrollable)
                .width(Fill)
                .height(Fill)
                .into()
        };
        let composer: Element<'a, Message> =
            if self.bot_gone() { self.gone_note() } else { self.composer(class, anchors) };
        let composer = container(widgets::content(size::content_max(), composer)).width(Fill).padding(Padding {
            top: 0.0,
            right: side,
            bottom: side,
            left: side,
        });
        column![history, composer].height(Fill).into()
    }

    /// What a new chat shows above the composer: the first start without the assistant's client,
    /// the hint without an AI account, else a short welcome.
    fn welcome(&self) -> Element<'_, Message> {
        if let Some(bot) = self.chat_bot() {
            return self.bot_welcome(bot);
        }
        if self.access == Access::Missing {
            return prompt_state(
                tr("Choose what the assistant may use"),
                tr(
                    "Until then it answers without your calendar, tasks, Notification Hub or mail. You decide area by area, and you can change it any time.",
                ),
                Some((tr("Set up access"), Message::SetUp)),
            );
        }
        // AI accounts, but none with a model on (task S.26): the core lists none of them.
        if self.no_model_on() {
            return prompt_state(
                tr("No model is turned on"),
                tr("Turn on models in Settings › Accounts."),
                Some((tr("Manage models…"), Message::ManageModels)),
            );
        }
        if self.engines_loaded && self.engines.is_empty() {
            return prompt_state(
                tr("No AI account yet"),
                tr("Add a model on this computer with Ollama or LM Studio, or open models over OpenRouter."),
                Some((tr("Add an AI account"), Message::AddAiAccount)),
            );
        }
        self.greeting()
    }

    /// The welcome of a new chat (task C.10): who answers – the assistant's picture, name and
    /// description close together –, then "What can I do for you?" and what it can be asked about.
    fn greeting(&self) -> Element<'_, Message> {
        let mut who = column![
            avatar(self.assistant_name(), self.assistant_avatar(), size::control_lg()),
            typo::text(typo::meta(), self.assistant_name().to_owned()).style(theme::text_2).wrapping(Wrapping::None),
        ]
        .spacing(space::xs())
        .align_x(Alignment::Center);
        if let Some(description) = self.profile.as_ref().map(|p| p.description.as_str()).filter(|d| !d.is_empty()) {
            who = who
                .push(typo::text(typo::meta(), description.to_owned()).style(theme::text_3).align_x(Alignment::Center));
        }
        let ask = column![
            typo::text(typo::body_strong(), tr("What can I do for you?")).align_x(Alignment::Center),
            typo::text(
                typo::body(),
                tr("Ask about your calendar, tasks, Notification Hub or mail. The assistant uses only what you allowed."),
            )
            .style(theme::text_2)
            .align_x(Alignment::Center)
        ]
        .spacing(space::s())
        .align_x(Alignment::Center);
        let content = column![who, ask].spacing(space::l()).align_x(Alignment::Center).max_width(size::panel());
        container(content).center(Fill).padding(space::xl()).into()
    }

    /// The entries in order, the growing answer last; tool lines close together, everything else
    /// with room between. While an answer appears letter by letter, what came after it waits, and
    /// before its first letter it takes no room; so does a card the core has not resolved yet.
    fn entries(&self) -> Element<'_, Message> {
        let colours = answer_colours(palette(&self.theme()));
        let mut list = column![].width(Fill);
        let mut previous_tool: Option<bool> = None;
        let appearing = self.appearing();
        for entry in &self.transcript.entries {
            if appearing.is_some_and(|seq| entry.seq > seq) {
                break;
            }
            let unresolved = matches!(entry.content, EntryContent::Card { .. }) && !self.cards.contains_key(&entry.seq);
            if !self.answer_shows(entry.seq) || unresolved {
                continue;
            }
            let tool = matches!(entry.content, EntryContent::Tool { .. });
            if let Some(was_tool) = previous_tool {
                let gap = if was_tool && tool { space::xs() } else { space::l() };
                list = list.push(Space::new().height(gap));
            }
            previous_tool = Some(tool);
            list = list.push(self.entry(entry, colours));
        }
        // The assistant proposed a bot and waits for the person's word (task B.2).
        if let Some(card) = self.proposal_card() {
            list = list.push(Space::new().height(space::l())).push(card);
        }
        // An answer runs, and nothing shows it yet: a quiet word, no spinner.
        let writing = self.transcript.growing().is_some_and(|entry| self.answer_shows(entry.seq))
            || appearing.is_some_and(|seq| self.answer_shows(seq));
        if self.transcript.running() && !writing && !self.waits_for_approval() {
            list = list.push(Space::new().height(space::l()));
            list = list.push(typo::text(typo::meta(), tr("Working …")).style(theme::text_3));
        }
        list.into()
    }

    fn entry<'a>(&'a self, entry: &'a Entry, colours: Palette) -> Element<'a, Message> {
        match &entry.content {
            EntryContent::Message { text, .. } => {
                let bubble = container(typo::text(typo::body(), text.as_str()).wrapping(Wrapping::WordOrGlyph))
                    .padding([space::s(), space::m()])
                    .max_width(size::form_max())
                    .style(style::container::group);
                container(bubble).width(Fill).align_right(Fill).into()
            }
            EntryContent::Answer { text, .. } => match self.answers.get(&entry.seq) {
                Some((_, items)) => markdown::view(items, colours).map(Message::OpenLink),
                None => typo::text(typo::body(), text.as_str()).into(),
            },
            EntryContent::Tool { name, input, summary, error, lack } => {
                self.tool(entry.seq, name, input, summary, error.as_deref(), *lack)
            }
            EntryContent::Approval { id } => self.approval_card(id, self.approvals.get(id)),
            EntryContent::Error { message, .. } => row![
                tinted(Icon::CircleAlert, icon::SIZE, |p| p.error_text),
                typo::text(typo::body(), message.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
            EntryContent::Stopped => row![
                tinted(Icon::Square, icon::SIZE_SM, |p| p.text_3),
                typo::text(typo::meta(), tr("Stopped")).style(theme::text_3)
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
            // What the core resolved for it (`assistant.get_cards`); [`App::entries`] leaves out a
            // card that is not resolved yet.
            EntryContent::Card { .. } => self.card(entry.seq).unwrap_or_else(|| Space::new().into()),
        }
    }

    /// A tool line: the area's icon, "Searched mail · 12 results" and a chevron; a click unfolds
    /// its input and result, and "Open Calendar" for the app that shows it. A refused tool says so
    /// in the error colour, and for `forbidden` why (`lack`, issue #133) and, where the rights lift
    /// it, how to change them ([`chat::refusal_hint`]).
    fn tool<'a>(
        &'a self,
        seq: u64,
        name: &'a str,
        input: &'a serde_json::Value,
        summary: &'a str,
        error: Option<&'a str>,
        lack: Option<torro_ipc::api::Lack>,
    ) -> Element<'a, Message> {
        let area = chat::area_of(name);
        let open = self.expanded.contains(&seq);
        let asked = bots::asked_bot(name, input);
        // A bot the person has not asked for yet is a proposal, no failure: its mark stays quiet.
        let proposed = asked.is_some() && error == Some("forbidden");
        let mark: Element<'a, Message> = match error {
            Some(_) if !proposed => tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text),
            _ => tinted(area_icon(area), icon::SIZE_SM, |p| p.text_3),
        };
        let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
        let words = match (&asked, error) {
            // "Asked Travel planner · 3 steps" (task B.2): the core's words, the bot by its name.
            (Some(_), None) if !summary.is_empty() => summary.to_owned(),
            // Refused because the person had not asked for it: a proposal, the card follows.
            (Some(asked), Some("forbidden")) => trf("Wants to ask {bot}", &[("bot", &self.bot_name(&asked.bot))]),
            (Some(asked), _) => {
                let tool = trf("Asked {bot}", &[("bot", &self.bot_name(&asked.bot))]);
                match chat::tool_outcome(summary, error) {
                    outcome if outcome.is_empty() => tool,
                    outcome => trf("{tool} · {outcome}", &[("tool", &tool), ("outcome", &outcome)]),
                }
            }
            // The file card right under it names the file and its size: the line only says what
            // was done ("Created PDF"), not the name a second time.
            (None, None) if self.file_card_after(seq) => chat::tool_title(name),
            (None, _) => chat::tool_line(name, summary, error),
        };
        let line = row![
            mark,
            typo::text(typo::meta(), words).wrapping(Wrapping::WordOrGlyph),
            tinted(chevron, icon::SIZE_SM, |p| p.text_3)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        // A long line wraps rather than being cut; one line is as high as a small control.
        let line = button(line)
            .padding(Padding { top: tool_inset(), bottom: tool_inset(), left: 0.0, right: space::s() })
            .style(style::button::ghost)
            .on_press(Message::ToggleTool(seq));
        let mut parts = column![line].spacing(space::xs());
        // A bot the assistant may not ask is the person's to name or confirm (task B.2), not a
        // right to change.
        if error == Some("forbidden") && area != Some(Area::Bots) {
            let who = match self.chat_bot() {
                Some(bot) => chat::Answerer::Bot(self.listed_bot(bot).map(|listed| listed.name.as_str())),
                None => chat::Answerer::Assistant,
            };
            let (what, change) = chat::refusal_hint(area, lack, who);
            let mut hint = row![typo::text(typo::meta(), what).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)]
                .spacing(space::s())
                .align_y(Alignment::Center);
            if change {
                hint = hint.push(widgets::text_button(
                    None,
                    tr("Change access"),
                    style::button::ghost,
                    Some(Message::SetUp),
                ));
            }
            parts = parts.push(container(hint).padding(Padding::new(0.0).left(tool_indent())));
        }
        if let Some(asked) = asked.as_ref().filter(|_| open) {
            // Asking a bot: the question in words and the way to its chat, no ids (task B.2).
            let question = input.get("text").and_then(serde_json::Value::as_str).unwrap_or_default();
            let mut detail = column![
                typo::text(typo::caption(), tr("Question")).style(theme::text_3),
                typo::text(typo::body(), question.to_owned()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
            ]
            .spacing(space::xs());
            if error.is_none() {
                detail = detail.push(widgets::text_button(
                    Some(Icon::ArrowUpRight),
                    tr("Open chat"),
                    style::button::ghost,
                    Some(self.open_asked(asked.clone())),
                ));
            }
            parts = parts.push(container(detail).padding(Padding::new(0.0).left(tool_indent())));
        } else if open {
            let mut result =
                row![typo::text(typo::meta(), summary).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)]
                    .spacing(space::s())
                    .align_y(Alignment::Center);
            if let Some(app) = area.and_then(chat::app_of).filter(|_| error.is_none()) {
                let label = trf("Open {app}", &[("app", &app_short_name(app))]);
                let open_app = Message::OpenApp(app.binary.to_owned());
                result = result.push(widgets::text_button(
                    Some(Icon::ArrowUpRight),
                    label,
                    style::button::ghost,
                    Some(open_app),
                ));
            }
            let detail = column![
                typo::text(typo::caption(), tr("Input")).style(theme::text_3),
                input_view(input),
                typo::text(typo::caption(), tr("Result")).style(theme::text_3),
                result,
            ]
            .spacing(space::xs());
            parts = parts.push(container(detail).padding(Padding::new(0.0).left(tool_indent())));
        }
        parts.into()
    }

    /// The approval card: what the tool would do, in the core's words (its summary and details,
    /// never the model's text), and "Allow" / "Deny" while it waits – disabled while a decision
    /// is on its way. Decided, it says how.
    fn approval_card<'a>(&'a self, id: &'a str, approval: Option<&'a ApprovalInfo>) -> Element<'a, Message> {
        let head = row![
            tinted(Icon::ShieldAlert, icon::SIZE, |p| p.attention_text),
            typo::text(typo::body_strong(), tr("Approval needed"))
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let mut card = column![head].spacing(space::s());
        let Some(approval) = approval else {
            card = card.push(typo::text(typo::meta(), tr("Loading …")).style(theme::text_3));
            return container(card).width(Fill).padding(layout::panel_padding()).style(style::container::group).into();
        };
        card = card.push(typo::text(typo::body(), approval.summary.as_str()).wrapping(Wrapping::WordOrGlyph));
        for line in &approval.details {
            card =
                card.push(typo::text(typo::meta(), line.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
        }
        let footer: Element<'a, Message> = match &approval.state {
            ApprovalState::Pending => {
                let open = self.deciding.is_none();
                let decide = |allow: bool| open.then(|| Message::Decide(id.to_owned(), allow));
                row![
                    widgets::text_button(Some(Icon::Check), tr("Allow"), style::button::secondary, decide(true)),
                    widgets::text_button(Some(Icon::X), tr("Deny"), style::button::secondary, decide(false)),
                ]
                .spacing(space::s())
                .into()
            }
            ApprovalState::Running | ApprovalState::Done { .. } => {
                outcome(tinted(Icon::CircleCheck, icon::SIZE_SM, |p| p.ok), tr("Allowed"))
            }
            ApprovalState::Denied => outcome(tinted(Icon::CircleX, icon::SIZE_SM, |p| p.error_text), tr("Denied")),
            ApprovalState::Expired => outcome(tinted(Icon::Clock, icon::SIZE_SM, |p| p.text_3), tr("Expired")),
        };
        card = card.push(footer);
        container(card).width(Fill).padding(layout::panel_padding()).style(style::container::group).into()
    }

    // -- Composer -------------------------------------------------------------------------------

    /// The composer: the text (`Enter` sends, `Shift + Enter` a new line), below it the engine and
    /// model, the areas, and "Send" – or "Stop" while an answer runs.
    fn composer<'a>(&'a self, class: SizeClass, anchors: &Anchors) -> Element<'a, Message> {
        let body = typo::body();
        let (least, most) = editor_heights();
        let placeholder = self.placeholder();
        let editor = text_editor(&self.editor)
            .id(EDITOR)
            .placeholder(placeholder)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(style::INPUT_PADDING)
            .min_height(least)
            .max_height(most)
            .wrapping(Wrapping::WordOrGlyph)
            .style(style::text_editor)
            .key_binding(composer_binding)
            .on_action(Message::Edit);
        let room = label_room(class);
        let mut controls = row![].spacing(space::s()).align_y(Alignment::Center);
        controls = controls.push(match &self.choice {
            Some(choice) => {
                let label = typo::ellipsize(&self.choice_label(choice), typo::caption(), room);
                widgets::select_button(&anchors.picker, label, Message::ToggleMenu(Menu::Picker))
            }
            None if self.no_model_on() => {
                typo::text(typo::meta(), tr("No model is turned on")).style(theme::text_3).into()
            }
            None if self.engines_loaded => typo::text(typo::meta(), tr("No AI account")).style(theme::text_3).into(),
            None => typo::text(typo::meta(), tr("Loading …")).style(theme::text_3).into(),
        });
        // The effort, only for a model that thinks (issue #131); the word alone where room is short.
        if self.effort_offered() {
            let level = chat::effort_name(self.effort);
            let label = match class {
                SizeClass::Wide | SizeClass::Medium => trf("Effort: {level}", &[("level", &level)]),
                SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => level.to_owned(),
            };
            controls = controls.push(widgets::select_button(&anchors.effort, label, Message::ToggleMenu(Menu::Effort)));
        }
        // A bot answers with its own client's rights: nothing of the assistant's to switch off.
        let areas = if self.chat_bot().is_some() { Vec::new() } else { self.switchable() };
        if !areas.is_empty() {
            let label = if self.without.is_empty() {
                tr("All areas").to_owned()
            } else {
                let names: Vec<&str> = self.without.iter().map(|area| chat::area_name(*area)).collect();
                trf("Without {areas}", &[("areas", &names.join(", "))])
            };
            let label = typo::ellipsize(&label, typo::caption(), room);
            controls = controls.push(widgets::select_button(&anchors.areas, label, Message::ToggleMenu(Menu::Areas)));
        }
        controls = controls.push(Space::new().width(Fill));
        controls = controls.push(if self.transcript.running() {
            widgets::text_button(Some(Icon::Square), tr("Stop"), style::button::secondary, Some(Message::Stop))
        } else if self.sending {
            widgets::busy_button(tr("Sending…"), style::button::primary)
        } else {
            let ready = self.choice.is_some() && !self.editor.text().trim().is_empty();
            widgets::text_button(Some(Icon::Send), tr("Send"), style::button::primary, ready.then_some(Message::Send))
        });
        let mut composer = column![editor, controls].spacing(space::s());
        // What the next message may do (issue #132): the mode, and the access it leaves in words.
        if let (Access::Granted(rights), None) = (&self.access, self.chat_bot()) {
            let mode =
                widgets::select_button(&anchors.mode, chat::mode_name(self.mode), Message::ToggleMenu(Menu::Mode));
            let line = chat::access_line(rights, &self.without, self.mode);
            let line = widgets::ellipsized(typo::meta(), line, theme::text_3);
            let row = row![mode, container(line).width(Fill)].spacing(space::s()).align_y(Alignment::Center);
            composer = composer.push(row);
        }
        composer.into()
    }

    /// The permission mode's menu (issue #132): each mode, the narrowest first, a check at the chosen.
    fn mode_menu(&self) -> Element<'_, Message> {
        let check = palette(&self.theme()).text_2;
        let items = torro_ipc::assistant::Mode::ALL
            .into_iter()
            .map(|mode| {
                MenuItem::new(Mark::Check(check, self.mode == mode), chat::mode_name(mode), Message::PickMode(mode))
            })
            .collect();
        widgets::menu(vec![MenuSection::titled(tr("What the assistant may do here"), items)])
    }

    /// The picker: each engine with the models that can call tools and are turned on (task S.26);
    /// an engine whose models did not come is offered as it is, and the engine picks its model.
    /// `chosen` has the check, a pick sends `pick` (the composer's choice, or the bot editor's
    /// engine). A chosen model that is turned off stands on top of its engine with why "Automatic"
    /// answers; "Manage models…" at the end opens the control centre's accounts.
    fn picker_menu(&self, chosen: Option<&Choice>, pick_message: fn(Choice) -> Message) -> Element<'_, Message> {
        let check = palette(&self.theme()).text_2;
        let mut sections: Vec<MenuSection<Message>> = self
            .engines
            .iter()
            .map(|engine| {
                let pick = |model: Option<String>| Choice { engine: engine.id.clone(), model };
                let item = |label: String, choice: Choice| {
                    let on = chosen == Some(&choice);
                    MenuItem::new(Mark::Check(check, on), label, pick_message(choice))
                };
                let items = match (engine.kind, self.offered_models(&engine.id)) {
                    (EngineKind::Ai, Some(Ok(models))) => {
                        // The model a bot, the default or the conversation holds, turned off since.
                        let off = chosen.filter(|choice| choice.engine == engine.id && self.turned_off(choice));
                        let mut items: Vec<MenuItem<'_, Message>> = off
                            .into_iter()
                            .flat_map(|choice| {
                                let name = choice.model.as_deref().map(|id| self.model_name(&engine.id, id));
                                [
                                    item(name.unwrap_or_default(), choice.clone()),
                                    MenuItem::new(
                                        Icon::Info,
                                        tr("Turned off in Settings › Accounts"),
                                        Message::CloseMenu,
                                    ),
                                ]
                            })
                            .collect();
                        if models.is_empty() {
                            items.push(MenuItem::new(
                                Icon::CircleAlert,
                                self.none_offered(&engine.id),
                                Message::CloseMenu,
                            ));
                        }
                        items.extend(models.into_iter().map(|model| item(model.name.clone(), pick(Some(model.id)))));
                        items
                    }
                    (EngineKind::Ai, Some(Err(_))) => vec![item(tr("Automatic").to_owned(), pick(None))],
                    (EngineKind::Ai, None) => vec![item(tr("Automatic").to_owned(), pick(None))],
                    // An agent picks its model itself or takes one of its own (issue #131), and never
                    // gets mail (issue #133): said where it is picked, not only when a tool is refused.
                    // No agent is offered since issue #219, so this arm is reached only where one is
                    // listed again; the kind stays in the protocol, so the picker keeps showing it.
                    (EngineKind::Agent, offered) => {
                        let models = offered.and_then(Result::ok).unwrap_or_default();
                        std::iter::once(item(tr("Automatic").to_owned(), pick(None)))
                            .chain(models.into_iter().map(|model| item(model.name.clone(), pick(Some(model.id)))))
                            .chain([MenuItem::new(Icon::Info, tr("Never gets your mail"), Message::CloseMenu)])
                            .collect()
                    }
                    (EngineKind::Test, _) => vec![item(engine.name.clone(), pick(None))],
                };
                let title = match &engine.problem {
                    Some(problem) => format!("{} · {problem}", engine.name),
                    None => engine.name.clone(),
                };
                MenuSection::titled(title, items)
            })
            .collect();
        // Which models are offered is set in the control centre (task S.26), not here.
        if self.engines.iter().any(|engine| engine.kind == EngineKind::Ai) {
            let manage = MenuItem::new(Icon::SlidersHorizontal, tr("Manage models…"), Message::ManageModels)
                .trail(Trail::Icon(Icon::ArrowUpRight));
            sections.push(MenuSection::new(vec![manage]));
        }
        widgets::menu(sections)
    }

    /// Why an AI account offers no model (task S.26): none of its models is turned on, or those that
    /// are cannot call tools.
    fn none_offered(&self, engine: &str) -> &'static str {
        let listed = self.models.get(engine).and_then(|models| models.as_ref().ok());
        match listed.is_some_and(|models| models.iter().any(|model| model.enabled)) {
            true => tr("No model here can use tools"),
            false => tr("No model is turned on"),
        }
    }

    /// The effort's menu (issue #131): "Automatic" and each level, with a check at the chosen one.
    fn effort_menu(&self) -> Element<'_, Message> {
        let check = palette(&self.theme()).text_2;
        let items = std::iter::once(None)
            .chain(torro_core::ai::Effort::ALL.into_iter().map(Some))
            .map(|effort| {
                let on = self.effort == effort;
                MenuItem::new(Mark::Check(check, on), chat::effort_name(effort), Message::PickEffort(effort))
            })
            .collect();
        widgets::menu(vec![MenuSection::titled(tr("How hard the model thinks"), items)])
    }

    /// The areas of the composer: each with a check while the assistant may use it here.
    fn areas_menu(&self) -> Element<'_, Message> {
        let check = palette(&self.theme()).text_2;
        let items = self
            .switchable()
            .into_iter()
            .map(|area| {
                let on = !self.without.contains(&area);
                MenuItem::new(Mark::Check(check, on), chat::area_name(area), Message::ToggleArea(area))
            })
            .collect();
        widgets::menu(vec![MenuSection::titled(tr("The assistant may use"), items)])
    }

    /// A conversation's menu: Rename, Pin or Unpin, Open in new window (main window), Delete.
    fn conversation_menu(&self, id: &str) -> Element<'_, Message> {
        let pinned = self.conversations.iter().find(|c| c.id == id).is_some_and(|c| c.pinned);
        let mut items = vec![
            MenuItem::new(Icon::SquarePen, tr("Rename"), Message::AskRename(id.to_owned())),
            if pinned {
                MenuItem::new(Icon::PinOff, tr("Unpin"), Message::Pin(id.to_owned(), false))
            } else {
                MenuItem::new(Icon::Flag, tr("Pin"), Message::Pin(id.to_owned(), true))
            },
        ];
        if !self.chat_only {
            items.push(MenuItem::new(Icon::AppWindow, tr("Open in new window"), Message::OpenInWindow(id.to_owned())));
        }
        let delete = MenuItem::new(Icon::Trash2, tr("Delete"), Message::AskDelete(id.to_owned()));
        widgets::menu(vec![MenuSection::new(items), MenuSection::new(vec![delete])])
    }

    /// The dialog over the window: renaming (the field, "Cancel", "Rename") or the question before
    /// deleting.
    fn dialog_view<'a>(&'a self, dialog: &'a Dialog) -> Element<'a, Message> {
        let body = typo::body();
        let (title, content, confirm): (&str, Element<'a, Message>, Element<'a, Message>) = match dialog {
            Dialog::Rename { title, .. } => {
                let field = text_input(tr("Title"), title)
                    .id(RENAME)
                    .size(body.size)
                    .line_height(body.line_height)
                    .font(body.font)
                    .padding(style::INPUT_PADDING)
                    .width(Fill)
                    .style(style::text_input)
                    .on_input(Message::RenameInput)
                    .on_submit(Message::ConfirmRename);
                let ready = !title.trim().is_empty();
                let confirm = widgets::text_button(
                    None,
                    tr("Rename"),
                    style::button::secondary,
                    ready.then_some(Message::ConfirmRename),
                );
                (tr("Rename conversation"), field.into(), confirm)
            }
            Dialog::Delete { title, .. } => {
                let text = trf("“{title}” and everything in it will be deleted for good.", &[("title", title)]);
                let words = typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
                let confirm = widgets::text_button(
                    Some(Icon::Trash2),
                    tr("Delete conversation"),
                    style::button::danger,
                    Some(Message::ConfirmDelete),
                );
                (tr("Delete this conversation?"), words.into(), confirm)
            }
            Dialog::DeleteBot { name, .. } => {
                let text = trf(
                    "“{name}” and its access will be deleted for good. Its conversations stay, but no one answers there any more.",
                    &[("name", name)],
                );
                let words = typo::text(typo::body(), text).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
                let confirm = widgets::text_button(
                    Some(Icon::Trash2),
                    tr("Delete bot"),
                    style::button::danger,
                    Some(Message::ConfirmDeleteBot),
                );
                (tr("Delete this bot?"), words.into(), confirm)
            }
        };
        let cancel = widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseDialog));
        let buttons = row![Space::new().width(Fill), cancel, confirm].spacing(space::s()).align_y(Alignment::Center);
        let card = column![typo::text(typo::title(), title), content, buttons].spacing(space::l());
        container(card).width(Fill).padding(layout::panel_padding()).style(style::container::popover).into()
    }

    // -- Mini and strip -------------------------------------------------------------------------

    /// The glance of mini and strip tiles: the newest answer, read from a distance, and a one-line
    /// composer – the tile's one purpose. No sidebar, menus or dialogs.
    fn glance(&self, class: SizeClass) -> Element<'_, Message> {
        let strip = class == SizeClass::Strip;
        let padding = if strip { Padding::new(space::s()).horizontal(space::m()) } else { Padding::new(space::m()) };
        let body = typo::body();
        let field = text_input(&self.placeholder(), &self.mini)
            .id(MINI_INPUT)
            .size(body.size)
            .line_height(body.line_height)
            .font(body.font)
            .padding(style::INPUT_PADDING)
            .width(Fill)
            .style(style::text_input)
            .on_input(Message::MiniInput)
            .on_submit(Message::Send);
        // A deleted bot answers no more: no field to type into, the note says why.
        let field: Element<'_, Message> = if self.bot_gone() {
            let note = typo::ellipsize(tr("This bot was deleted."), typo::meta(), self.size.width);
            typo::text(typo::meta(), note).style(theme::text_3).wrapping(Wrapping::None).into()
        } else {
            field.into()
        };
        let mut composer = row![field].spacing(space::s()).align_y(Alignment::Center);
        if self.transcript.running() {
            composer =
                composer.push(widgets::text_button(None, tr("Stop"), style::button::secondary, Some(Message::Stop)));
        }
        let shown = self.glance_answer();
        let content: Element<'_, Message> = if strip {
            // A flat strip: the answer takes the height, the composer stands beside it.
            row![shown, container(composer).width(size::props()).center_y(Fill)].spacing(space::m()).into()
        } else {
            let mut column = column![].spacing(space::s());
            if self.current.is_some() {
                let title = typo::ellipsize(&self.conversation_title(), typo::meta(), self.size.width);
                column = column.push(typo::text(typo::meta(), title).style(theme::text_3).wrapping(Wrapping::None));
            }
            column.push(shown).push(composer).into()
        };
        container(content)
            .width(Fill)
            .height(Fill)
            .padding(padding)
            .style(|t: &Theme| container::Style {
                background: Some(Background::Color(palette(t).panel)),
                ..container::Style::default()
            })
            .into()
    }

    /// What the glance shows above (or beside) its composer: an approval that waits, in the
    /// core's words – the person decides in a larger window –, else the newest answer, scrolled to
    /// its end so nothing is cut off unseen; "Working …" while an answer runs without text yet.
    ///
    /// A file made for the newest message (task WZ.8) stands as one line under the answer, outside
    /// its scrolling, so the scroll bar never lies over it; with room for fewer than three lines the
    /// answer above it shrinks to one line cut with "…" (`chat::answer_line`), without room for one
    /// it is left out.
    fn glance_answer(&self) -> Element<'_, Message> {
        let card = self.newest_card_line();
        let answer = self.transcript.last_answer().filter(|entry| self.answer_shows(entry.seq));
        match (card, answer) {
            (Some(card), Some(entry)) if !self.waits_in_glance() => {
                let answer = responsive(move |room| {
                    let line_height = typo::line_height(typo::body());
                    if room.height >= GLANCE_LINES * line_height {
                        return self.glance_scroll();
                    }
                    // Not even one line: the card alone says what came of it.
                    if room.height < line_height {
                        return Space::new().into();
                    }
                    let line = match &entry.content {
                        EntryContent::Answer { text, .. } => chat::answer_line(text),
                        _ => String::new(),
                    };
                    container(widgets::ellipsized(typo::body(), line, theme::text))
                        .width(Fill)
                        .align_bottom(Fill)
                        .into()
                });
                column![answer, card].spacing(space::s()).into()
            }
            _ => self.glance_scroll(),
        }
    }

    /// Whether an approval waits in the conversation: the glance shows it instead of the answer.
    fn waits_in_glance(&self) -> bool {
        self.transcript
            .approvals()
            .into_iter()
            .any(|id| self.approvals.get(id).is_some_and(|approval| approval.state.is_pending()))
    }

    /// The scrolling part of [`App::glance_answer`]: the approval, the answer, or a quiet word.
    fn glance_scroll(&self) -> Element<'_, Message> {
        let waiting = self
            .transcript
            .approvals()
            .into_iter()
            .find_map(|id| self.approvals.get(id).filter(|approval| approval.state.is_pending()));
        let content: Element<'_, Message> = if let Some(approval) = waiting {
            column![
                row![
                    tinted(Icon::ShieldAlert, icon::SIZE, |p| p.attention_text),
                    typo::text(typo::body_strong(), tr("Approval needed"))
                ]
                .spacing(space::s())
                .align_y(Alignment::Center),
                typo::text(typo::body(), approval.summary.as_str()).wrapping(Wrapping::WordOrGlyph),
                typo::text(typo::meta(), tr("Allow or deny it in a larger window.")).style(theme::text_3),
            ]
            .spacing(space::xs())
            .into()
        } else if let Some(entry) = self.transcript.last_answer().filter(|entry| self.answer_shows(entry.seq)) {
            self.entry(entry, answer_colours(palette(&self.theme())))
        } else if self.transcript.running() {
            typo::text(typo::meta(), tr("Working …")).style(theme::text_3).into()
        } else {
            typo::text(typo::meta(), tr("Ask anything – the answer shows here.")).style(theme::text_3).into()
        };
        scrollable(container(content).width(Fill))
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .anchor_bottom()
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    }
}

impl App {
    /// The short form of the newest resolved card after the person's newest message, for the
    /// glance; `None` when that message brought no card.
    fn newest_card_line(&self) -> Option<Element<'_, Message>> {
        let after = self
            .transcript
            .entries
            .iter()
            .rev()
            .find(|entry| matches!(entry.content, EntryContent::Message { .. }))
            .map_or(0, |entry| entry.seq);
        let seq = self.transcript.cards().into_iter().rev().find(|seq| *seq > after && self.cards.contains_key(seq))?;
        self.card_line(seq)
    }
}

/// The short name of an app for "Open …": "Calendar", "Tasks", "Inbox".
fn app_short_name(app: torro_core::AppId) -> &'static str {
    match app.binary {
        "torrocalendar" => tr("Calendar"),
        "torrotasks" => tr("Tasks"),
        "torroinbox" => tr("Notification Hub"),
        _ => app.display_name,
    }
}

/// How a decided approval went: a mark and a word.
fn outcome<'a>(mark: Element<'a, Message>, word: &'a str) -> Element<'a, Message> {
    row![mark, typo::text(typo::meta(), word).style(theme::text_2)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// A quiet line in the sidebar ("No conversations yet").
fn sidebar_note<'a>(text: &'a str) -> Element<'a, Message> {
    container(typo::text(typo::meta(), text).style(theme::text_3)).padding(style::ITEM_PADDING.top(space::m())).into()
}

/// The assistant's picture in the header: smaller than a control, so the title keeps its line.
fn header_avatar() -> f32 {
    size::control_sm() - space::s()
}

/// The symbol of a picture of the profile ([`torro_ipc::assistant::AVATARS`]); `None` for the
/// letter and for a picture this window does not know, which shows as the letter.
pub fn avatar_icon(id: &str) -> Option<Icon> {
    match id {
        "sparkles" => Some(Icon::Sparkles),
        "sun" => Some(Icon::Sun),
        "moon" => Some(Icon::Moon),
        "cloud" => Some(Icon::Cloud),
        "radar" => Some(Icon::Radar),
        "user" => Some(Icon::User),
        _ => None,
    }
}

/// The assistant's picture, round, `edge` across: its symbol, else the first letter of `name`
/// (`?` without one), in `text` on `selected` – neutral, the red is the app logo's. The same look
/// as in the control centre's Assistant › Profile.
fn avatar<'a>(name: &str, avatar: &str, edge: f32) -> Element<'a, Message> {
    // A symbol keeps a ring of room in a small circle.
    let symbol_edge = if edge < size::control_sm() { icon::SIZE_SM } else { icon::SIZE };
    let face: Element<'a, Message> = match avatar_icon(avatar) {
        Some(symbol) => tinted(symbol, symbol_edge, |p| p.text),
        None => {
            let letter = name.trim().chars().next().map_or_else(|| "?".to_owned(), |c| c.to_uppercase().to_string());
            let style = if edge >= size::control_lg() { typo::title() } else { typo::caption() };
            typo::text(style, letter).style(theme::text).wrapping(Wrapping::None).into()
        }
    };
    container(face)
        .center(edge)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).selected)),
            border: iced::border::rounded(edge / 2.0),
            ..container::Style::default()
        })
        .into()
}

/// A state in the middle of the chat: what this is (`title`), what to do (`hint`), and a button
/// that does it – the empty state with a way out.
fn prompt_state<'a>(title: &'a str, hint: &'a str, action: Option<(&'a str, Message)>) -> Element<'a, Message> {
    let mut content = column![
        tinted(Icon::Sparkles, icon::SIZE, |p| p.text_3),
        typo::text(typo::body_strong(), title).align_x(Alignment::Center),
        typo::text(typo::body(), hint).style(theme::text_2).align_x(Alignment::Center)
    ]
    .spacing(space::s())
    .align_x(Alignment::Center)
    .max_width(size::panel());
    if let Some((label, message)) = action {
        content = content.push(Space::new().height(space::xs()));
        content = content.push(widgets::text_button(None, label, style::button::secondary, Some(message)));
    }
    container(content).center(Fill).padding(space::xl()).into()
}
