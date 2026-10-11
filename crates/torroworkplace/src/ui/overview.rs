//! The overview (task S.4 in `docs/tasks/steuerzentrale.md`): status at a glance –
//! daemon running, window manager detected (`data.status.backend`), accounts and their
//! state, last sync, number of templates – and a "Needs your attention" card listing
//! accounts whose state is `Connected { problem: Some(..) }` or that need a new sign-in,
//! each with a button that jumps to Accounts (`Action::Select(Section::Accounts)`).

use iced::widget::{Row, button, column, container, row, space, text};
use iced::{Alignment, Background, Color, Element, Length, Theme, border};
use torro_core::i18n::{tr, trf};
use torro_core::theme::{STATUS_ERROR, STATUS_OK};
use torro_gui::{theme, widgets};
use torro_ipc::{AccountInfo, Status};
use torro_pim::AccountState;

use super::{Action, Data, Section};

/// One status tile. Tiles wrap onto further lines when the window is narrow.
const TILE_WIDTH: f32 = 200.0;
/// Between tiles and between attention rows: items of one group sit close together.
const GROUP_GAP: f32 = 8.0;
/// Between the groups of the page.
const SECTION_GAP: f32 = 24.0;

#[derive(Debug, Default)]
pub struct State {}

#[derive(Debug, Clone)]
pub enum Message {
    /// The "Open accounts" button of an attention row.
    OpenAccounts,
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
    match message {
        Message::OpenAccounts => Action::Select(Section::Accounts),
    }
}

pub fn view<'a>(_state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let items = attention(&data.accounts);
    column![at_a_glance(data), needs_attention(&items, data.status.is_some())].spacing(SECTION_GAP).into()
}

/// How a status is shown. The symbol carries the meaning next to the colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    /// Running, connected, fine.
    Ok,
    /// Waiting for the user.
    Attention,
    /// Broken, or the core is not running.
    Error,
    /// Not set up, unknown, not supported.
    Inactive,
    /// A plain fact, not a status: no symbol.
    Plain,
}

impl Tone {
    fn symbol(self) -> &'static str {
        match self {
            Tone::Ok => "●",
            Tone::Attention => "▲",
            Tone::Error => "■",
            Tone::Inactive => "○",
            Tone::Plain => "",
        }
    }

    fn color(self) -> Color {
        match self {
            Tone::Ok => theme::color(STATUS_OK),
            Tone::Attention => theme::attention(),
            Tone::Error => theme::color(STATUS_ERROR),
            Tone::Inactive => theme::MUTED,
            Tone::Plain => theme::TEXT,
        }
    }
}

/// One line of a tile: a status with its symbol, or a plain fact.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    tone: Tone,
    text: String,
}

impl Line {
    fn new(tone: Tone, text: impl Into<String>) -> Line {
        Line { tone, text: text.into() }
    }

    /// `● Running`, `▲ Needs sign-in`; a plain fact has no symbol.
    fn label(&self) -> String {
        match self.tone.symbol() {
            "" => self.text.clone(),
            symbol => format!("{symbol} {}", self.text),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tile {
    title: &'static str,
    headline: Line,
    detail: Option<String>,
}

/// Why an account needs the user.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reason {
    /// Not signed in yet.
    SignIn,
    /// Signed in, but the last sync failed; the server's words.
    Problem(String),
}

impl Reason {
    fn line(&self) -> Line {
        match self {
            Reason::SignIn => Line::new(Tone::Attention, tr("Needs sign-in")),
            Reason::Problem(problem) => Line::new(Tone::Attention, problem.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Attention {
    label: String,
    reason: Reason,
}

/// The accounts that need the user: not signed in, or the last sync failed. An account that
/// is signing in right now is not listed, the user is already on it.
fn attention(accounts: &[AccountInfo]) -> Vec<Attention> {
    accounts
        .iter()
        .filter_map(|account| {
            let reason = match &account.state {
                AccountState::NotConnected => Reason::SignIn,
                AccountState::Connected { problem: Some(problem), .. } => Reason::Problem(problem.clone()),
                AccountState::Connected { problem: None, .. } | AccountState::SigningIn { .. } => return None,
            };
            Some(Attention { label: account.label.clone(), reason })
        })
        .collect()
}

/// The four tiles in reading order: core, window manager, accounts, workspaces.
fn tiles(data: &Data) -> Vec<Tile> {
    vec![
        core_tile(data.status.as_ref(), data.error.as_deref()),
        window_manager_tile(data.status.as_ref()),
        accounts_tile(&data.accounts),
        workspaces_tile(data.templates.len()),
    ]
}

fn core_tile(status: Option<&Status>, error: Option<&str>) -> Tile {
    let (headline, detail) = match status {
        Some(status) => {
            (Line::new(Tone::Ok, tr("Running")), Some(trf("Version {version}", &[("version", &status.version)])))
        }
        None => (Line::new(Tone::Error, tr("Not running")), error.map(str::to_string)),
    };
    Tile { title: tr("Core"), headline, detail }
}

fn window_manager_tile(status: Option<&Status>) -> Tile {
    let (headline, detail) = match status {
        None => (Line::new(Tone::Inactive, tr("Unknown")), Some(tr("Core is not running").to_string())),
        Some(status) if status.backend == "unsupported" => (
            Line::new(Tone::Inactive, tr("Not supported yet")),
            Some(tr("Templates can't be saved or loaded.").to_string()),
        ),
        Some(status) => (Line::new(Tone::Ok, tr("Detected")), Some(status.backend.clone())),
    };
    Tile { title: tr("Window manager"), headline, detail }
}

fn accounts_tile(accounts: &[AccountInfo]) -> Tile {
    let headline = if !attention(accounts).is_empty() {
        Line::new(Tone::Attention, tr("Needs sign-in"))
    } else if accounts.is_empty() {
        Line::new(Tone::Inactive, tr("Not set up"))
    } else if accounts.iter().all(|account| matches!(account.state, AccountState::Connected { .. })) {
        Line::new(Tone::Ok, tr("Connected"))
    } else {
        Line::new(Tone::Inactive, tr("Signing in …"))
    };
    let detail = if accounts.is_empty() {
        tr("Add one in Accounts").to_string()
    } else {
        counted(accounts.len(), "1 account", "{n} accounts")
    };
    Tile { title: tr("Accounts"), headline, detail: Some(detail) }
}

fn workspaces_tile(templates: usize) -> Tile {
    let text = if templates == 0 {
        tr("No templates yet").to_string()
    } else {
        counted(templates, "1 template", "{n} templates")
    };
    Tile { title: tr("Workspaces"), headline: Line::new(Tone::Plain, text), detail: None }
}

/// `1 account` for one, the plural text with `{n}` for any other number.
fn counted(n: usize, one: &'static str, many: &'static str) -> String {
    if n == 1 { tr(one).to_string() } else { trf(many, &[("n", &n)]) }
}

fn at_a_glance<'a>(data: &Data) -> Element<'a, Message> {
    let grid = Row::with_children(tiles(data).into_iter().map(tile_view))
        .spacing(GROUP_GAP)
        .width(Length::Fill)
        .wrap()
        .vertical_spacing(GROUP_GAP);
    column![widgets::section_title(tr("At a glance")), grid].spacing(GROUP_GAP).into()
}

fn tile_view<'a>(tile: Tile) -> Element<'a, Message> {
    let color = tile.headline.tone.color();
    let mut body =
        column![widgets::section_title(tile.title), text(tile.headline.label()).size(16).color(color)].spacing(8);
    if let Some(detail) = tile.detail {
        body = body.push(text(detail).size(14).color(theme::MUTED));
    }
    container(body).width(Length::Fixed(TILE_WIDTH)).padding(16).style(card).into()
}

/// The attention group, or a calm line when nothing needs the user. Without a running core
/// the list is unknown, so the page says so instead of claiming all is well.
fn needs_attention<'a>(items: &[Attention], core_running: bool) -> Element<'a, Message> {
    if !core_running {
        return text(tr("Start the core to see what needs your attention.")).size(14).color(theme::MUTED).into();
    }
    if items.is_empty() {
        return text(tr("Everything is running.")).size(14).into();
    }
    column![
        widgets::section_title(tr("Needs your attention")),
        column(items.iter().map(attention_row)).spacing(GROUP_GAP)
    ]
    .spacing(GROUP_GAP)
    .into()
}

fn attention_row<'a>(item: &Attention) -> Element<'a, Message> {
    let info = column![
        text(item.label.clone()).size(14),
        text(item.reason.line().label()).size(14).color(theme::attention()),
    ]
    .spacing(4);
    let content = row![
        info,
        space::horizontal(),
        button(text(tr("Open accounts")).size(14)).style(button::secondary).on_press(Message::OpenAccounts),
    ]
    .spacing(16)
    .align_y(Alignment::Center);
    container(content).padding(16).width(Length::Fill).style(card).into()
}

/// A raised card: the surface colour with a 12 px radius.
fn card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::SURFACE)),
        border: border::rounded(12),
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected(problem: Option<&str>) -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.test".into(),
            login_name: "anna".into(),
            last_sync: None,
            problem: problem.map(str::to_string),
        }
    }

    fn account(label: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: format!("nextcloud:{label}"),
            kind: "nextcloud".into(),
            label: label.into(),
            state,
            collections: Vec::new(),
        }
    }

    fn status(backend: &str) -> Status {
        Status { version: "0.9.0".into(), backend: backend.into(), workspace: None, templates: 2, busy: false }
    }

    #[test]
    fn lists_accounts_that_need_the_user() {
        let signing_in = AccountState::SigningIn {
            server: "https://cloud.example.test".into(),
            login_url: "https://cloud.example.test/login".into(),
        };
        let accounts = [
            account("anna@cloud.example.test", connected(None)),
            account("ben@cloud.example.test", AccountState::NotConnected),
            account("carla@cloud.example.test", connected(Some("Server unreachable"))),
            account("dora@cloud.example.test", signing_in),
        ];
        assert_eq!(
            attention(&accounts),
            vec![
                Attention { label: "ben@cloud.example.test".into(), reason: Reason::SignIn },
                Attention {
                    label: "carla@cloud.example.test".into(),
                    reason: Reason::Problem("Server unreachable".into()),
                },
            ]
        );
    }

    #[test]
    fn nothing_to_report_when_all_is_connected() {
        let accounts = [account("anna@cloud.example.test", connected(None))];
        assert!(attention(&accounts).is_empty());
    }

    #[test]
    fn core_tile_shows_version_or_error() {
        let running = core_tile(Some(&status("hyprland")), None);
        assert_eq!(running.title, tr("Core"));
        assert_eq!(running.headline, Line::new(Tone::Ok, tr("Running")));
        assert_eq!(running.detail, Some(trf("Version {version}", &[("version", &"0.9.0")])));

        let down = core_tile(None, Some("no socket"));
        assert_eq!(down.headline, Line::new(Tone::Error, tr("Not running")));
        assert_eq!(down.detail.as_deref(), Some("no socket"));
    }

    #[test]
    fn window_manager_tile_marks_unsupported() {
        let unsupported = window_manager_tile(Some(&status("unsupported")));
        assert_eq!(unsupported.headline, Line::new(Tone::Inactive, tr("Not supported yet")));
        let detected = window_manager_tile(Some(&status("hyprland")));
        assert_eq!(detected.headline, Line::new(Tone::Ok, tr("Detected")));
        assert_eq!(detected.detail.as_deref(), Some("hyprland"));
    }

    #[test]
    fn accounts_tile_follows_attention() {
        let ok = accounts_tile(&[account("anna", connected(None))]);
        assert_eq!(ok.headline, Line::new(Tone::Ok, tr("Connected")));
        assert_eq!(ok.detail, Some(counted(1, "1 account", "{n} accounts")));

        let needs = accounts_tile(&[account("anna", connected(None)), account("ben", AccountState::NotConnected)]);
        assert_eq!(needs.headline, Line::new(Tone::Attention, tr("Needs sign-in")));
        assert_eq!(needs.detail, Some(trf("{n} accounts", &[("n", &2)])));

        let none = accounts_tile(&[]);
        assert_eq!(none.headline, Line::new(Tone::Inactive, tr("Not set up")));
        assert_eq!(none.detail.as_deref(), Some(tr("Add one in Accounts")));
    }

    #[test]
    fn workspaces_tile_counts_templates() {
        assert_eq!(workspaces_tile(0).headline.text, tr("No templates yet"));
        assert_eq!(workspaces_tile(1).headline.text, tr("1 template"));
        assert_eq!(workspaces_tile(3).headline.text, trf("{n} templates", &[("n", &3)]));
    }

    #[test]
    fn status_is_symbol_and_word() {
        assert_eq!(Line::new(Tone::Attention, tr("Needs sign-in")).label(), format!("▲ {}", tr("Needs sign-in")));
        assert_eq!(Line::new(Tone::Plain, "3").label(), "3");
    }
}
