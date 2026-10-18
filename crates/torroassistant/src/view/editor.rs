//! How bots look in the assistant's window (task B.3, design skill `torro-design`): their rows in
//! the sidebar and a bot's menu, who answers in a bot's chat (header, welcome, a deleted bot), the
//! editor that takes the chat's place, and the card of the assistant's proposal to ask a bot.
//!
//! The editor is a form of reading width ([`size::form_max`]) in settings groups: name,
//! description, picture, personality (with the assistant's templates), instructions, skills as
//! named texts, the engine, and one row "Access ↗" that leads to the bot's page in the control
//! centre (task S.18). **What a bot may use is not written here:** its rights are its client's,
//! and `clients.*` is the app key's alone (point 2 of "Was nie nach außen geht" in `docs/api.md`),
//! so the editor saves with `bots.save_bot` and nothing else. "Save" in the header is the window's
//! one primary button while the editor is open. Problems show after the first "Save" only.

use iced::widget::text::Wrapping;
use iced::widget::{Space, TextInput, column, container, mouse_area, row, scrollable, text_editor, text_input};
use iced::{Alignment, Element, Fill, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::SizeClass;
use torro_gui::shell;
use torro_gui::theme::{self, size, space};
use torro_gui::widgets::{self, Mark, MenuItem, MenuSection};
use torro_gui::{layout, style, typo};
use torro_ipc::assistant::AVATARS;
use torro_ipc::bots::{BotSummary, MAX_INSTRUCTIONS_CHARS, MAX_PERSONALITY_CHARS, MAX_SKILL_CHARS, MAX_SKILLS};
use torro_settings_ui::parts::{pressable, setting_row};

use super::{Anchors, avatar, avatar_icon, header_avatar, prompt_state, row_title_room, tinted};
use crate::app::{App, BOT_FORM, BOT_NAME, Menu, Message};
use crate::bots::{self, Asked, BotDraft, DraftEdit, Template, editor_text};

/// The personality's field shows this many lines before it scrolls.
const PERSONALITY_LINES: f32 = 8.0;
/// The instructions' field.
const INSTRUCTIONS_LINES: f32 = 10.0;
/// A skill's field.
const SKILL_LINES: f32 = 6.0;

/// The height of a text field with `lines` lines of body text and the padding of a field.
fn field_height(lines: f32) -> f32 {
    lines * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// A one-line field in the body style.
fn field<'a>(placeholder: &str, value: &str, on_input: impl Fn(String) -> Message + 'a) -> TextInput<'a, Message> {
    let body = typo::body();
    text_input(placeholder, value)
        .size(body.size)
        .line_height(body.line_height)
        .font(body.font)
        .padding(style::INPUT_PADDING)
        .width(Fill)
        .style(style::text_input)
        .on_input(on_input)
        .on_submit(Message::SaveBot)
}

/// A text field of `lines` lines that scrolls beyond them.
fn area<'a>(
    content: &'a text_editor::Content,
    placeholder: &'a str,
    lines: f32,
    on_action: impl Fn(text_editor::Action) -> Message + 'a,
) -> Element<'a, Message> {
    let body = typo::body();
    text_editor(content)
        .placeholder(placeholder)
        .size(body.size)
        .line_height(body.line_height)
        .font(body.font)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(field_height(lines))
        .style(style::text_editor)
        .on_action(on_action)
        .into()
}

/// "120 of 4,000 characters" at the right under a text field, tabular.
fn counter<'a>(content: &text_editor::Content, max: usize) -> Element<'a, Message> {
    let count = editor_text(content).chars().count();
    let words = trf("{count} of {max} characters", &[("count", &count), ("max", &max)]);
    row![Space::new().width(Fill), typo::text(typo::meta().tabular(), words).style(theme::text_3)].into()
}

/// The word and the mark of a picture in the picker.
fn picture_choice(id: &str) -> (&'static str, Option<Mark>) {
    let label = match id {
        "sparkles" => tr("Sparkles"),
        "sun" => tr("Sun"),
        "moon" => tr("Moon"),
        "cloud" => tr("Cloud"),
        "radar" => tr("Radar"),
        "user" => tr("Person"),
        _ => tr("Letter"),
    };
    (label, avatar_icon(id).map(Mark::Icon))
}

impl App {
    /// Who answers in the open chat, by name and picture: the bot's, else the assistant's.
    pub(super) fn speaker(&self) -> (String, String) {
        match self.chat_bot() {
            // A bot that is gone shows as a person without a letter, so no name is made up.
            Some(id) => {
                let picture = self.listed_bot(id).map_or("user", |bot| bot.avatar.as_str());
                (self.bot_name(id), picture.to_owned())
            }
            None => (self.assistant_name().to_owned(), self.assistant_avatar().to_owned()),
        }
    }

    /// The composer's placeholder: "Ask the assistant…", or "Ask Travel planner…".
    pub(super) fn placeholder(&self) -> String {
        match self.chat_bot() {
            Some(id) => trf("Ask {bot}…", &[("bot", &self.bot_name(id))]),
            None => tr("Ask the assistant…").to_owned(),
        }
    }

    /// What "Open chat" under the tool line of an asked bot does: the bot's conversation the tool
    /// names, else the bot's chat; in a chat window, in a window of its own.
    pub(crate) fn open_asked(&self, asked: Asked) -> Message {
        match (asked.conversation, self.chat_only) {
            (Some(conversation), false) => Message::Select(conversation),
            (Some(conversation), true) => Message::OpenInWindow(conversation),
            (None, false) => Message::OpenBot(asked.bot),
            (None, true) => Message::OpenBotWindow(asked.bot),
        }
    }

    /// Where the composer would be when the chat's bot was deleted: why nothing can be sent.
    pub(super) fn gone_note(&self) -> Element<'_, Message> {
        row![
            tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.text_3),
            typo::text(typo::meta(), tr("This bot was deleted. You can read the conversation, but not continue it."))
                .style(theme::text_2)
                .wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    }

    /// A new chat with a bot: its picture, name and description, and that it uses only what it was
    /// allowed; for a bot that is gone, that it is.
    pub(super) fn bot_welcome(&self, id: &str) -> Element<'_, Message> {
        let Some(bot) = self.listed_bot(id) else {
            if !self.bots_loaded {
                return widgets::loading();
            }
            return prompt_state(
                tr("This bot no longer exists"),
                tr("It was deleted. Its conversations stay in the assistant's list."),
                None,
            );
        };
        // A long name ends in "…" with the whole name as tooltip.
        let shown = typo::ellipsize(&bot.name, typo::meta(), size::panel());
        let name = typo::text(typo::meta(), shown.clone()).style(theme::text_2).wrapping(Wrapping::None);
        let name: Element<'_, Message> =
            if shown == bot.name { name.into() } else { widgets::with_tooltip(name, bot.name.as_str()) };
        let mut who = column![avatar(&bot.name, &bot.avatar, size::control_lg()), name]
            .spacing(space::xs())
            .align_x(Alignment::Center);
        if !bot.description.is_empty() {
            who = who.push(
                typo::text(typo::meta(), bot.description.clone()).style(theme::text_3).align_x(Alignment::Center),
            );
        }
        let ask = column![
            typo::text(typo::body_strong(), tr("What can I do for you?")).align_x(Alignment::Center),
            typo::text(typo::body(), tr("The bot uses only what you allowed it in the control centre."))
                .style(theme::text_2)
                .align_x(Alignment::Center),
            widgets::text_button(None, tr("Access…"), style::button::ghost, Some(Message::BotAccess(bot.id.clone()))),
        ]
        .spacing(space::s())
        .align_x(Alignment::Center);
        let content = column![who, ask].spacing(space::l()).align_x(Alignment::Center).max_width(size::panel());
        container(content).center(Fill).padding(space::xl()).into()
    }

    /// A bot in the sidebar: its picture and name on a row of the sidebar's height, chosen while its
    /// new chat or its editor is open. A click opens its chat, a double click its chat window, a
    /// right click its menu.
    pub(super) fn bot_row<'a>(&'a self, bot: &'a BotSummary, anchors: &Anchors) -> Element<'a, Message> {
        let id = bot.id.clone();
        let editing = self.bot_draft.as_ref().map(|draft| draft.id.as_deref() == Some(id.as_str()));
        let active = editing.unwrap_or(self.current.is_none() && self.chat_bot() == Some(id.as_str()));
        let hovered = self.hover.as_deref() == Some(id.as_str());
        let room = row_title_room() - header_avatar() - space::s();
        let shown = typo::ellipsize(&bot.name, typo::body(), room);
        let name = container(typo::text(typo::body(), shown).wrapping(Wrapping::None)).width(Fill).clip(true);
        let content =
            row![avatar(&bot.name, &bot.avatar, header_avatar()), name].spacing(space::s()).align_y(Alignment::Center);
        let surface = container(content)
            .width(Fill)
            .height(size::row_sidebar())
            .padding(style::ITEM_PADDING)
            .center_y(size::row_sidebar())
            .style(move |t: &Theme| {
                let status =
                    if hovered { iced::widget::button::Status::Hovered } else { iced::widget::button::Status::Active };
                let look = style::button::sidebar_item(active)(t, status);
                container::Style {
                    background: look.background,
                    text_color: Some(look.text_color),
                    border: look.border,
                    ..container::Style::default()
                }
            });
        let mut area = mouse_area(surface)
            .on_press(Message::OpenBot(id.clone()))
            .on_right_press(Message::ToggleMenu(Menu::Bot { id: id.clone() }))
            .on_enter(Message::Hover(Some(id.clone())))
            .on_exit(Message::Hover(None))
            .interaction(iced::mouse::Interaction::Pointer);
        if !self.chat_only {
            area = area.on_double_click(Message::OpenBotWindow(id.clone()));
        }
        let row: Element<'a, Message> = if bot.description.is_empty() {
            area.into()
        } else {
            widgets::with_tooltip(area, bot.description.as_str())
        };
        let menu_here = matches!(&self.menu, Some(Menu::Bot { id: open }) if *open == id);
        if menu_here { widgets::anchored(&anchors.bot_row, row) } else { row }
    }

    /// A bot's menu: New chat, Edit bot, Access…, Open in new window (main window), Delete bot….
    pub(super) fn bot_menu(&self, id: &str) -> Element<'_, Message> {
        let mut items = vec![
            MenuItem::new(Icon::SquarePen, tr("New chat"), Message::NewBotChat(id.to_owned())),
            MenuItem::new(Icon::SlidersHorizontal, tr("Edit bot"), Message::EditBot(id.to_owned())),
            MenuItem::new(Icon::UserCheck, tr("Access…"), Message::BotAccess(id.to_owned())),
        ];
        if !self.chat_only {
            items.push(MenuItem::new(Icon::AppWindow, tr("Open in new window"), Message::OpenBotWindow(id.to_owned())));
        }
        let delete = MenuItem::new(Icon::Trash2, tr("Delete bot…"), Message::AskDeleteBot(id.to_owned()));
        widgets::menu(vec![MenuSection::titled(self.bot_name(id), items), MenuSection::new(vec![delete])])
    }

    /// The header while the editor is open: the bot's picture and name ("New bot" for a new one)
    /// after `leading`, then "Save" – the window's one primary button, always in sight however long
    /// the form – and "Close".
    pub(super) fn editor_header<'a>(
        &'a self,
        draft: &'a BotDraft,
        mut leading: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let name = match draft.name.trim() {
            "" if draft.id.is_none() => tr("New bot").to_owned(),
            "" => tr("Bot").to_owned(),
            name => name.to_owned(),
        };
        leading.push(avatar(&draft.name, &draft.avatar, header_avatar()));
        let note = Some(if draft.id.is_some() { tr("Edit bot") } else { tr("New bot") }.to_owned())
            .filter(|note| *note != name);
        let title = shell::header_title(None, name, note);
        let save = if draft.saving {
            widgets::busy_button(tr("Saving…"), style::button::primary)
        } else {
            widgets::text_button(Some(Icon::Check), tr("Save"), style::button::primary, Some(Message::SaveBot))
        };
        let close = widgets::icon_button(Icon::X, tr("Close"), Some(Message::CloseEditor));
        shell::header_bar(leading, title, [save, close])
    }

    /// The bot editor in place of the chat (see the module's documentation): after a "Save" that
    /// found a problem, the problem first.
    pub(super) fn editor<'a>(
        &'a self,
        draft: &'a BotDraft,
        class: SizeClass,
        anchors: &Anchors,
    ) -> Element<'a, Message> {
        let side = match class {
            SizeClass::Wide | SizeClass::Medium => space::xl(),
            _ => space::l(),
        };
        let name = field(tr("Name of the bot"), &draft.name, |text| Message::Draft(DraftEdit::Name(text))).id(BOT_NAME);
        let description = field(tr("What it does, in a few words"), &draft.description, |text| {
            Message::Draft(DraftEdit::Description(text))
        });
        let instructions = column![
            area(&draft.instructions, tr("What should the bot do, and how?"), INSTRUCTIONS_LINES, |action| {
                Message::Draft(DraftEdit::Instructions(action))
            }),
            counter(&draft.instructions, MAX_INSTRUCTIONS_CHARS),
        ]
        .spacing(space::xs());
        let mut form = column![].spacing(layout::section_gap());
        if let Some(problem) = draft.problem().filter(|_| draft.tried) {
            form = form.push(problem_line(problem));
        }
        form = form
            .push(section(tr("Name"), name, None))
            .push(section(tr("Description"), description, Some(tr("Shown under the name in the chat."))))
            .push(section(tr("Picture"), self.picture(draft), None))
            .push(section(
                tr("Personality"),
                personality(draft),
                Some(tr(
                    "How the bot speaks and behaves. It never changes what the bot may use, and the rules of Torro Workplace always come first.",
                )),
            ))
            .push(section(
                tr("Instructions"),
                instructions,
                Some(tr("What the bot does and how. The rules of Torro Workplace come first where they disagree.")),
            ))
            .push(section(
                tr("Skills"),
                skills(draft),
                Some(tr("What the bot knows, as text it reads in every answer.")),
            ))
            .push(section(
                tr("Engine"),
                self.engine_choice(draft, anchors),
                Some(tr("What the bot answers with when the assistant asks it. A chat with the bot can pick another.")),
            ))
            .push(access_group(draft));
        if let Some(id) = &draft.id {
            let delete = Message::AskDeleteBot(id.clone());
            let button = widgets::text_button(None, tr("Delete bot…"), style::button::danger, Some(delete));
            // Moved left by its own padding, so its word stands on the form's axis.
            form = form.push(row![Space::new(), button].spacing(-style::button_padding().left));
        }
        let page = container(widgets::content(size::form_max(), form)).width(Fill).padding(Padding {
            top: space::xl(),
            right: side,
            bottom: space::xl(),
            left: side,
        });
        scrollable(page)
            .id(BOT_FORM)
            .direction(scrollable::Direction::Vertical(style::scrollbar()))
            .style(style::scrollable)
            .width(Fill)
            .height(Fill)
            .into()
    }

    /// The picture: the preview beside a chip per picture of the suite.
    fn picture<'a>(&'a self, draft: &'a BotDraft) -> Element<'a, Message> {
        let preview = avatar(&draft.name, &draft.avatar, size::control_lg());
        let chips = AVATARS
            .iter()
            .map(|id| {
                let (label, mark) = picture_choice(id);
                let message = Message::Draft(DraftEdit::Avatar((*id).to_owned()));
                widgets::choice_chip(label, mark, draft.avatar == *id, Some(message))
            })
            .collect();
        row![preview, widgets::chip_row(chips)].spacing(space::l()).align_y(Alignment::Center).into()
    }

    /// The engine and model, chosen in the picker; without any engine, where to add one.
    fn engine_choice<'a>(&'a self, draft: &'a BotDraft, anchors: &Anchors) -> Element<'a, Message> {
        match &draft.choice {
            Some(choice) => {
                let label = typo::ellipsize(&self.choice_label(choice), typo::caption(), size::sidebar());
                let picker = widgets::select_button(&anchors.bot_picker, label, Message::ToggleMenu(Menu::BotPicker));
                row![picker].into()
            }
            None => row![
                typo::text(typo::meta(), tr("No AI account")).style(theme::text_3),
                widgets::text_button(None, tr("Add an AI account"), style::button::ghost, Some(Message::AddAiAccount)),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
        }
    }

    /// The card of the assistant's proposal to ask a bot ("Ask Travel planner?") at the end of its
    /// conversation, while the person has not answered it ([`bots::open_proposal`]): "Ask {bot}"
    /// confirms it (`assistant.confirm_bot`) and says so in the chat. Only in the assistant's own
    /// conversation, never while an answer runs.
    pub(crate) fn proposal_card(&self) -> Option<Element<'_, Message>> {
        if self.chat_bot().is_some() || self.transcript.running() {
            return None;
        }
        let (_, proposal) = bots::open_proposal(self.current.as_deref()?, &self.transcript.entries)?;
        let name = self.bot_name(&proposal.bot);
        let head = row![
            tinted(Icon::Users, icon::SIZE, |p| p.text_2),
            typo::text(typo::body_strong(), trf("Ask {bot}?", &[("bot", &name)])).wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let words = typo::text(
            typo::body(),
            tr("The assistant suggests asking this bot. It works only with what both of them may use."),
        )
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph);
        let ask = match self.confirming.is_some() || self.sending {
            true => widgets::busy_button(trf("Ask {bot}", &[("bot", &name)]), style::button::secondary),
            false => widgets::text_button(
                Some(Icon::Check),
                trf("Ask {bot}", &[("bot", &name)]),
                style::button::secondary,
                Some(Message::ConfirmBot(proposal)),
            ),
        };
        let card = column![head, words, ask].spacing(space::s());
        Some(container(card).width(Fill).padding(layout::panel_padding()).style(style::container::group).into())
    }
}

/// The one row in place of the former section "Access" (task S.18): what the bot may use is
/// chosen in the control centre at the bot's page (`--section bot:<id>`), because a bot's rights
/// are its client's and `clients.*` never leaves the app key. A bot without an id has no page yet –
/// the row waits for the first save.
fn access_group<'a>(draft: &'a BotDraft) -> Element<'a, Message> {
    let detail = match draft.id {
        Some(_) => None,
        None => Some(tr("Save the bot first; until then it may use nothing.").to_owned()),
    };
    let row =
        setting_row(None, tr("What the bot may use"), detail, tinted(Icon::ArrowUpRight, icon::SIZE, |p| p.text_3));
    let footer = tr(
        "Calendars, lists, repositories, sources, mail accounts, sending and admin commands are chosen in Torro Workplace.",
    );
    let open = draft.id.clone().map(Message::BotAccess);
    widgets::group(tr("Access"), vec![pressable(row, open)], Some(footer.to_string()))
}

/// The personality: the templates as chips, the field and its counter.
fn personality(draft: &BotDraft) -> Element<'_, Message> {
    let typed = editor_text(&draft.personality);
    let templates = Template::ALL
        .into_iter()
        .map(|template| {
            let message = Message::Draft(DraftEdit::Template(template));
            widgets::choice_chip(template.label(), None, typed == template.text(), Some(message))
        })
        .collect();
    column![
        widgets::chip_row(templates),
        area(&draft.personality, tr("How should the bot speak and behave?"), PERSONALITY_LINES, |action| {
            Message::Draft(DraftEdit::Personality(action))
        }),
        counter(&draft.personality, MAX_PERSONALITY_CHARS),
    ]
    .spacing(space::xs())
    .into()
}

/// The skills – each its name with "Remove skill", its text and counter – and "Add skill" with how
/// many there may be.
fn skills(draft: &BotDraft) -> Element<'_, Message> {
    let mut list = column![].spacing(space::l());
    for (at, skill) in draft.skills.iter().enumerate() {
        let name = field(tr("Skill name, such as Packing lists"), &skill.name, move |text| {
            Message::Draft(DraftEdit::SkillName(at, text))
        });
        let remove =
            widgets::icon_button(Icon::Trash2, tr("Remove skill"), Some(Message::Draft(DraftEdit::RemoveSkill(at))));
        let head = row![name, remove].spacing(space::s()).align_y(Alignment::Center);
        let text = area(&skill.text, tr("What the bot should know"), SKILL_LINES, move |action| {
            Message::Draft(DraftEdit::SkillText(at, action))
        });
        list = list.push(column![head, text, counter(&skill.text, MAX_SKILL_CHARS)].spacing(space::xs()));
    }
    let add = widgets::text_button(
        Some(Icon::Plus),
        tr("Add skill"),
        style::button::secondary,
        draft.can_add_skill().then_some(Message::Draft(DraftEdit::AddSkill)),
    );
    let count = trf("{count} of {max} skills", &[("count", &draft.skills.len()), ("max", &MAX_SKILLS)]);
    let last = row![add, Space::new().width(Fill), typo::text(typo::meta().tabular(), count).style(theme::text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    list.push(last).into()
}

/// A part of the form: its title in `caption` and `text_3`, 4 below it the field, 4 below that
/// what it is for in `meta` and `text_2` – label close to its field, no card around it.
fn section<'a>(
    title: &'a str,
    content: impl Into<Element<'a, Message>>,
    footer: Option<&'a str>,
) -> Element<'a, Message> {
    let mut part =
        column![typo::text(typo::caption(), title).style(theme::text_3), content.into()].spacing(space::xs());
    if let Some(footer) = footer {
        part = part.push(typo::text(typo::meta(), footer).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    part.into()
}

/// What keeps "Save" back, at the top of the form: an alert in the error colour and the words.
fn problem_line<'a>(problem: String) -> Element<'a, Message> {
    let alert = tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error_text);
    row![alert, typo::text(typo::body(), problem).wrapping(Wrapping::WordOrGlyph)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}
