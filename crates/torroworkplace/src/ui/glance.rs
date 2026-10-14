//! The control centre as a glance at a small tile (task D.3 in `docs/tasks/design.md`; design skill
//! `.claude/skills/torro-design`, section 6; the pattern is `torrotasks/src/glance.rs`).
//!
//! - **Mini** ([`mini`]): one status in the largest level that fits – a dot and "Running", or
//!   "1 needs attention" – and, when something needs the user, the first message below it: the
//!   account and why. Nothing else.
//! - **Strip** ([`strip`]): the four tiles of the overview as one calm row – title, status with its
//!   dot, detail – sharing the width evenly.
//!
//! Both are displays, not small apps: no header, sidebar, menu or button, because the tile's
//! purpose is to be read from a distance. Statuses and their tones come from the overview
//! ([`super::overview`]), so the tile never says something the page does not. The views are
//! rebuilt with the window's data, which the shell reloads every few seconds.
//!
//! What is said – the status, its tone, the first message – is a plain function ([`summary`]) with
//! tests; the views only lay out its result.

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Fill, Padding, Size};
use torro_core::i18n::{tr, trf};
use torro_gui::style;
use torro_gui::theme::{self, space};
use torro_gui::typo::{self, ellipsize, text_width};
use torro_gui::widgets;

use super::Data;
use super::overview::{Reason, Tile, Tone, attention, tiles};

/// The levels of the mini tile's status, largest first: the first that fits is used.
fn head_levels() -> [typo::Level; 3] {
    [typo::headline(), typo::title(), typo::body_strong()]
}
/// Edge of the dot in front of a status (the same as everywhere in the suite).
fn dot_size() -> f32 {
    space::s()
}
/// The tiles of the strip.
const STRIP_TILES: usize = 4;

/// The one message under the mini tile's status: what needs the user, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub title: String,
    pub detail: Option<String>,
}

/// What the mini tile says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub tone: Tone,
    pub head: String,
    pub note: Option<Note>,
}

/// "1 needs attention", "3 need attention".
fn needs_attention(n: usize) -> String {
    if n == 1 { tr("1 needs attention").to_string() } else { trf("{n} need attention", &[("n", &n)]) }
}

/// The status of the whole control centre in a few words: the core is not running, accounts need
/// the user (the first message is the heaviest: a problem before a sign-in), or all is running.
pub fn summary(data: &Data) -> Summary {
    if data.status.is_none() {
        let note = data.error.clone().map(|error| Note { title: error, detail: None });
        return Summary { tone: Tone::Error, head: tr("Not running").to_string(), note };
    }
    let items = attention(&data.accounts);
    let first = items.iter().find(|item| matches!(item.reason, Reason::Problem(_))).or(items.first());
    match first {
        Some(item) => {
            let reason = item.reason.line();
            Summary {
                tone: reason.tone,
                head: needs_attention(items.len()),
                note: Some(Note { title: item.label.clone(), detail: Some(reason.text) }),
            }
        }
        None => Summary { tone: Tone::Ok, head: tr("Running").to_string(), note: None },
    }
}

/// The first level of `levels` in which `text` fits `width`, else the last one.
fn fit_level(text: &str, levels: &[typo::Level], width: f32) -> typo::Level {
    levels.iter().copied().find(|level| text_width(text, *level) <= width).unwrap_or(levels[levels.len() - 1])
}

/// The tile around a glance: the panel colour, `padding` in from the edge, clipped.
fn tile<'a, M: 'a>(content: impl Into<Element<'a, M>>, padding: Padding) -> Element<'a, M> {
    container(content).padding(padding).width(Fill).height(Fill).clip(true).style(style::container::panel_flush).into()
}

/// A line of text, never wrapped, cut with "…" to `width`.
fn line<'a>(text: &str, level: typo::Level, width: f32) -> iced::widget::Text<'a> {
    typo::text(level, ellipsize(text, level, width)).wrapping(Wrapping::None)
}

/// The dot of a status; a plain fact has none (the strip's "3 templates").
fn dot<'a, M: 'a>(tone: Tone) -> Option<Element<'a, M>> {
    tone.dot().map(widgets::status_dot)
}

/// The mini tile for a window of `size`: the status with its dot, standing in the middle of the
/// tile, and the first message below it when something needs the user.
pub fn mini<'a, M: 'a>(data: &Data, loaded: bool, size: Size) -> Element<'a, M> {
    if !loaded {
        return tile(widgets::loading(), Padding::new(space::m()));
    }
    let width = (size.width - 2.0 * space::m()).max(0.0);
    let summary = summary(data);
    let level = fit_level(&summary.head, &head_levels(), width - dot_size() - space::s());
    let head = row![]
        .push(dot(summary.tone))
        .push(line(&summary.head, level, width - dot_size() - space::s()))
        .spacing(space::s())
        .align_y(Alignment::Center);
    let mut content = column![head].spacing(space::xs());
    if let Some(note) = summary.note {
        content = content.push(Space::new().height(space::xs())).push(line(&note.title, typo::body(), width));
        if let Some(detail) = note.detail {
            content = content.push(line(&detail, typo::meta(), width).style(theme::text_2));
        }
    }
    tile(container(content).center_y(Fill), Padding::new(space::m()))
}

/// One tile of the strip in a cell `width` wide: its title in `text_3`, the status with its dot,
/// the detail in `text_2`. A tile without a detail keeps the empty line, so the cells line up.
fn cell<'a, M: 'a>(tile: Tile, width: f32) -> Element<'a, M> {
    let room = (width - dot_size() - space::s()).max(0.0);
    let head = row![]
        .push(dot(tile.headline.tone))
        .push(line(
            &tile.headline.text,
            typo::body_strong(),
            if tile.headline.tone.dot().is_some() { room } else { width },
        ))
        .spacing(space::s())
        .align_y(Alignment::Center);
    let detail = line(&tile.detail.unwrap_or_default(), typo::meta(), width).style(theme::text_2);
    let title = line(tile.title, typo::meta(), width).style(theme::text_3);
    container(column![title, head, detail].spacing(space::xs())).width(Fill).clip(true).into()
}

/// The strip for a window of `size`: the overview's four tiles side by side in one row, in the
/// middle of the strip's height.
pub fn strip<'a, M: 'a>(data: &Data, loaded: bool, size: Size) -> Element<'a, M> {
    let padding = Padding { top: space::s(), bottom: space::s(), left: space::m(), right: space::m() };
    if !loaded {
        return tile(widgets::loading(), padding);
    }
    let inner = (size.width - padding.left - padding.right).max(0.0);
    let gap = space::l();
    let width = ((inner - (STRIP_TILES as f32 - 1.0) * gap) / STRIP_TILES as f32).max(0.0);
    let mut cells = row![].spacing(gap).align_y(Alignment::Center).height(Fill);
    for tile in tiles(data) {
        cells = cells.push(cell(tile, width));
    }
    tile(cells, padding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::{AccountInfo, Status};
    use torro_pim::AccountState;

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
            scopes: None,
            mail: None,
            ntfy: None,
        }
    }

    fn running(accounts: Vec<AccountInfo>) -> Data {
        let status =
            Status { version: "0.9.0".into(), backend: "hyprland".into(), workspace: None, templates: 2, busy: false };
        Data { status: Some(status), accounts, ..Data::default() }
    }

    #[test]
    fn all_well_says_running_without_a_message() {
        let summary = summary(&running(vec![account("anna", connected(None))]));
        assert_eq!(summary, Summary { tone: Tone::Ok, head: tr("Running").into(), note: None });
    }

    #[test]
    fn a_sign_in_needs_attention_and_names_the_account() {
        let summary = summary(&running(vec![account("ben", AccountState::NotConnected)]));
        assert_eq!(summary.tone, Tone::Attention);
        assert_eq!(summary.head, tr("1 needs attention"));
        assert_eq!(summary.note, Some(Note { title: "ben".into(), detail: Some(tr("Needs sign-in").into()) }));
    }

    #[test]
    fn a_problem_comes_first_and_the_tile_agrees_with_the_overview() {
        let data = running(vec![
            account("ben", AccountState::NotConnected),
            account("carla", connected(Some("Server unreachable"))),
        ]);
        let summary = summary(&data);
        assert_eq!(summary.head, trf("{n} need attention", &[("n", &2)]));
        assert_eq!(summary.note.as_ref().map(|note| note.title.as_str()), Some("carla"));
        assert_eq!(summary.tone, tiles(&data)[2].headline.tone, "the glance and the overview tile agree");
    }

    #[test]
    fn without_the_core_the_tile_says_so_with_the_reason() {
        let down = summary(&Data { error: Some("no socket".into()), ..Data::default() });
        assert_eq!((down.tone, down.head.as_str()), (Tone::Error, tr("Not running")));
        assert_eq!(down.note, Some(Note { title: "no socket".into(), detail: None }));
    }

    #[test]
    fn the_head_takes_the_largest_level_that_fits() {
        let head = tr("Running");
        assert_eq!(fit_level(head, &head_levels(), 1000.0).size, typo::headline().size);
        assert_eq!(fit_level(head, &head_levels(), 0.0).size, typo::body_strong().size, "the last one when none fits");
    }

    #[test]
    fn both_views_build_in_every_size_and_state() {
        let ok = running(vec![account("anna", connected(None))]);
        let bad = running(vec![account("ben", AccountState::NotConnected)]);
        let down = Data::default();
        for size in [(240.0, 160.0), (320.0, 480.0), (1200.0, 200.0), (600.0, 120.0), (0.0, 0.0)] {
            let size = Size::new(size.0, size.1);
            for data in [&ok, &bad, &down] {
                drop(mini::<()>(data, true, size));
                drop(strip::<()>(data, true, size));
            }
            drop(mini::<()>(&ok, false, size));
            drop(strip::<()>(&ok, false, size));
        }
    }
}
