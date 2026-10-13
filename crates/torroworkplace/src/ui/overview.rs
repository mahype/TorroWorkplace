//! The overview (task S.4 in `docs/tasks/steuerzentrale.md`): status at a glance –
//! daemon running, window manager detected (`data.status.backend`), accounts and their
//! state, last sync, number of templates – and a "Needs your attention" card listing
//! accounts whose state is `Connected { problem: Some(..) }` or that need a new sign-in,
//! each with a button that jumps to Accounts (`Action::Select(Section::Accounts)`).
//!
//! The tiles have the surface of a settings group; a status is a dot and a word, the colour only
//! in the dot. The tiles share the width of the page evenly, at least [`TILE_MIN_WIDTH`] each, and
//! wrap onto further lines when the window is narrow. "Needs your attention" is a settings group
//! with a row per account.

use iced::widget::{Row, column, container, responsive, row, space as blank};
use iced::{Alignment, Element, Length};
use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, space};
use torro_gui::widgets::{self, Status as Dot};
use torro_gui::{layout, style, typo};
use torro_ipc::{AccountInfo, Status};
use torro_pim::AccountState;

use super::{Action, Data, Section, setting_row};

/// The least width of a status tile; below that the tiles wrap onto further lines.
const TILE_MIN_WIDTH: f32 = 160.0;

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
    column![at_a_glance(data), needs_attention(&items, data.status.is_some())].spacing(layout::section_gap()).into()
}

/// How a status is shown: a dot in its colour next to the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
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
    /// The dot in front of the word; a plain fact has none.
    pub(super) fn dot(self) -> Option<Dot> {
        match self {
            Tone::Ok => Some(Dot::Ok),
            Tone::Attention => Some(Dot::Attention),
            Tone::Error => Some(Dot::Error),
            Tone::Inactive => Some(Dot::Inactive),
            Tone::Plain => None,
        }
    }
}

/// One line of a tile: a status with its dot, or a plain fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Line {
    pub(super) tone: Tone,
    pub(super) text: String,
}

impl Line {
    fn new(tone: Tone, text: impl Into<String>) -> Line {
        Line { tone, text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tile {
    pub(super) title: &'static str,
    pub(super) headline: Line,
    pub(super) detail: Option<String>,
}

/// Why an account needs the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reason {
    /// Not signed in yet.
    SignIn,
    /// Signed in, but the last sync failed; the server's words.
    Problem(String),
}

impl Reason {
    pub(super) fn line(&self) -> Line {
        match self {
            Reason::SignIn => Line::new(Tone::Attention, tr("Needs sign-in")),
            // The same words and tone as the account list (`accounts::status_line`): a problem is
            // an error everywhere, only a missing sign-in is attention.
            Reason::Problem(problem) => Line::new(Tone::Error, trf("Problem: {problem}", &[("problem", problem)])),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attention {
    pub(super) label: String,
    pub(super) reason: Reason,
}

/// The accounts that need the user: not signed in, or the last sync failed. An account that
/// is signing in right now is not listed, the user is already on it.
pub(super) fn attention(accounts: &[AccountInfo]) -> Vec<Attention> {
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
pub(super) fn tiles(data: &Data) -> Vec<Tile> {
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

/// Counts every account, each once; a sign-in in progress (no id yet) is not an account. It names
/// the worst state among them, so one broken account out of several is not hidden.
fn accounts_tile(all: &[AccountInfo]) -> Tile {
    let accounts: Vec<&AccountInfo> = super::accounts::listed(all).collect();
    let signing_in = super::accounts::pending(all).next().is_some();
    let items = attention(all);
    let problems = items.iter().filter(|item| matches!(item.reason, Reason::Problem(_))).count();
    // The heaviest reason names the tile: a problem outweighs a missing sign-in.
    let headline = if problems > 0 {
        Line::new(Tone::Error, counted(problems, "1 problem", "{n} problems"))
    } else if !items.is_empty() {
        Line::new(Tone::Attention, tr("Needs sign-in"))
    } else if accounts.is_empty() {
        if signing_in {
            Line::new(Tone::Attention, tr("Signing in …"))
        } else {
            Line::new(Tone::Inactive, tr("Not set up"))
        }
    } else {
        Line::new(Tone::Ok, tr("Connected"))
    };
    let detail = if accounts.is_empty() {
        // The first account is on its way: the browser is where the user is needed.
        if signing_in { tr("Waiting for the browser …") } else { tr("Add one in Accounts") }.to_string()
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
    let tiles = tiles(data);
    // Iced's wrapping row lays its children out loose, so they cannot share the width. The page
    // width decides how many tiles fit a line; `responsive` hands it over and takes the height of
    // its content.
    let grid = responsive(move |bounds| tile_grid(tiles.clone(), bounds.width)).height(Length::Shrink);
    column![widgets::section_title(tr("At a glance")), grid].spacing(space::s()).into()
}

/// How many tiles of at least [`TILE_MIN_WIDTH`] fit a line `width` wide with `space::s()` between
/// them: at least one, at most `count`.
fn tiles_per_line(width: f32, count: usize) -> usize {
    let fit = ((width + space::s()) / (TILE_MIN_WIDTH + space::s())).floor();
    (fit as usize).clamp(1, count.max(1))
}

/// The tiles in lines of equal width: each tile takes the same share of a line, and a short last
/// line is filled up with empty shares so its tiles are as wide as the others.
fn tile_grid<'a>(tiles: Vec<Tile>, width: f32) -> Element<'a, Message> {
    let per_line = tiles_per_line(width, tiles.len());
    let mut lines = column![].spacing(space::s());
    let mut tiles = tiles.into_iter().peekable();
    while tiles.peek().is_some() {
        let mut line = Row::new().spacing(space::s()).width(Length::Fill);
        for _ in 0..per_line {
            line = line.push(match tiles.next() {
                Some(tile) => tile_view(tile),
                None => blank::horizontal().width(Length::FillPortion(1)).into(),
            });
        }
        lines = lines.push(line);
    }
    lines.into()
}

/// A tile: its title, the status as dot and word, and a detail line below.
fn tile_view<'a>(tile: Tile) -> Element<'a, Message> {
    let headline = row![]
        .push(tile.headline.tone.dot().map(widgets::status_dot))
        .push(typo::text(typo::body_strong(), tile.headline.text))
        .spacing(space::s())
        .align_y(Alignment::Center);
    // A tile without a detail keeps an empty line, so the tiles of a line are equally tall (Iced
    // cannot stretch tiles that are `Fill` in both directions to the tallest of their line).
    let detail = typo::text(typo::meta(), tile.detail.unwrap_or_default()).style(theme::text_2);
    let body = column![widgets::section_title(tile.title), headline, detail].spacing(space::s());
    container(body).width(Length::FillPortion(1)).padding(space::l()).style(style::container::group).into()
}

/// The attention group, or a calm line when nothing needs the user. Without a running core
/// the list is unknown, so the page says so instead of claiming all is well.
fn needs_attention<'a>(items: &[Attention], core_running: bool) -> Element<'a, Message> {
    if !core_running {
        return typo::text(typo::body(), tr("Start the core to see what needs your attention."))
            .style(theme::text_2)
            .into();
    }
    if items.is_empty() {
        return typo::text(typo::body(), tr("Everything is running.")).style(theme::text_2).into();
    }
    widgets::group(tr("Needs your attention"), items.iter().map(attention_row).collect(), None)
}

/// An account that needs the user: dot, name, the reason below, and the way to Accounts.
fn attention_row<'a>(item: &Attention) -> Element<'a, Message> {
    let line = item.reason.line();
    let open = widgets::text_button(None, tr("Open accounts"), style::button::secondary, Some(Message::OpenAccounts));
    setting_row(line.tone.dot().map(widgets::status_dot), item.label.clone(), Some(line.text), open)
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

    fn signing_in_entry() -> AccountInfo {
        AccountInfo {
            id: String::new(),
            kind: "nextcloud".into(),
            label: "https://cloud.example.test".into(),
            state: AccountState::SigningIn {
                server: "https://cloud.example.test".into(),
                login_url: "https://cloud.example.test/login".into(),
            },
            collections: Vec::new(),
            scopes: None,
            mail: None,
        }
    }

    fn account(label: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: format!("nextcloud:{label}"),
            kind: "nextcloud".into(),
            label: label.into(),
            state,
            collections: Vec::new(),
            scopes: None,
            mail: None,
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
    fn a_reason_looks_like_the_status_in_the_account_list() {
        let problem = Reason::Problem("Server unreachable".into()).line();
        assert_eq!(problem, Line::new(Tone::Error, trf("Problem: {problem}", &[("problem", &"Server unreachable")])));
        assert_eq!(Reason::SignIn.line(), Line::new(Tone::Attention, tr("Needs sign-in")));
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

        // A problem outweighs a missing sign-in, and is an error like in the account list.
        let broken = accounts_tile(&[
            account("ben", AccountState::NotConnected),
            account("carla", connected(Some("Server unreachable"))),
        ]);
        assert_eq!(broken.headline, Line::new(Tone::Error, tr("1 problem")));
        let two = accounts_tile(&[
            account("anna", connected(Some("Server unreachable"))),
            account("carla", connected(Some("Timeout"))),
        ]);
        assert_eq!(two.headline, Line::new(Tone::Error, trf("{n} problems", &[("n", &2)])));

        // A sign-in in progress is no account: the others are still counted and connected.
        let adding =
            accounts_tile(&[account("anna", connected(None)), account("ben", connected(None)), signing_in_entry()]);
        assert_eq!(adding.headline, Line::new(Tone::Ok, tr("Connected")));
        assert_eq!(adding.detail, Some(trf("{n} accounts", &[("n", &2)])));
        let first = accounts_tile(&[signing_in_entry()]);
        assert_eq!(first.headline, Line::new(Tone::Attention, tr("Signing in …")));
        assert_eq!(first.detail.as_deref(), Some(tr("Waiting for the browser …")));

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
    fn a_status_has_a_dot_a_fact_has_none() {
        assert_eq!(Tone::Ok.dot(), Some(Dot::Ok));
        assert_eq!(Tone::Attention.dot(), Some(Dot::Attention));
        assert_eq!(Tone::Error.dot(), Some(Dot::Error));
        assert_eq!(Tone::Inactive.dot(), Some(Dot::Inactive));
        assert_eq!(Tone::Plain.dot(), None);
    }

    #[test]
    fn tiles_wrap_by_width_and_never_below_one_line() {
        let min = TILE_MIN_WIDTH;
        // Four tiles with the gaps between them need 4 × min + 3 × gap.
        assert_eq!(tiles_per_line(4.0 * min + 3.0 * space::s(), 4), 4);
        assert_eq!(tiles_per_line(4.0 * min + 3.0 * space::s() - 1.0, 4), 3);
        assert_eq!(tiles_per_line(2.0 * min + space::s(), 4), 2);
        assert_eq!(tiles_per_line(min, 4), 1);
        assert_eq!(tiles_per_line(100.0, 4), 1, "a window narrower than a tile still shows one");
        assert_eq!(tiles_per_line(0.0, 4), 1);
        assert_eq!(tiles_per_line(2000.0, 4), 4, "never more per line than there are tiles");
        assert_eq!(tiles_per_line(2000.0, 0), 1);
    }

    #[test]
    fn the_grid_builds_in_every_width() {
        let data = Data { status: Some(status("hyprland")), ..Data::default() };
        for width in [0.0, 160.0, 388.0, 720.0, 2000.0] {
            drop(tile_grid(tiles(&data), width));
        }
    }

    #[test]
    fn page_builds_with_and_without_core() {
        let state = State::default();
        drop(view(&state, &Data::default()));
        let data = Data {
            status: Some(status("hyprland")),
            accounts: vec![account("ben@cloud.example.test", AccountState::NotConnected)],
            ..Data::default()
        };
        drop(view(&state, &data));
    }
}
