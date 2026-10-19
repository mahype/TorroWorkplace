//! Which calendars or task lists the suite shows (task S.19 in `docs/tasks/steuerzentrale.md`):
//! the calendar app's section "Calendars" and the tasks app's section "Lists", grouped by
//! account like the control centre's own account page had it, before S.21 shortens that page to
//! a single line pointing here.
//!
//! One module serves both sections, told apart by [`Role`]: a calendar also carries how it
//! reminds, shown or not (`settings.set_setting reminders`); a task list is only shown or hidden
//! (`settings.set_collection_shown`). An app with a single section built from this module ("Lists")
//! uses [`page`], [`update`] and [`wanted`] straight from its own `Message`, without a module of
//! its own in this library; the calendar app's "Calendars" sits beside `calendar::reminders` and
//! `calendar::night` in `torrocalendar`'s own `SECTIONS`.
//!
//! The sidebar checkmarks of the calendar app stay as they are: flat, in memory only. That much
//! was decided on 2026-10-18 for S.19; whether they should ever hold for the whole suite is open
//! question 3 of `docs/settings.md` and stays open. This section is the suite-wide, lasting choice
//! behind the gear, not a replacement for them.

use iced::widget::{column, container, responsive, row, toggler};
use iced::{Alignment, Element, Fill, Length};

use torro_core::i18n::tr;
use torro_core::settings::CalendarReminders;
use torro_gui::theme::{self, space};
use torro_gui::widgets;
use torro_gui::{layout, style, typo};
use torro_ipc::{AccountInfo, Request, ServerService, SettingKey};
use torro_pim::{Collection, CollectionKind};

use crate::parts::setting_row;
use crate::{Action, Data};

/// Which collections a page shows: a calendar's events ("Calendars"), or a task list's todos
/// ("Lists").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Calendar,
    TaskList,
}

/// What a row of this section sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// The switch of a collection (`settings.set_collection_shown`).
    Shown { collection: String, shown: bool },
    /// A shown calendar's reminder kind (`settings.set_setting reminders`); only in "Calendars".
    Reminder { collection: String, kind: CalendarReminders },
}

/// What one load needs for either section: the suite-wide settings (hidden collections,
/// reminders) and every account with its collections (`accounts.list_accounts`, hidden ones
/// included).
pub fn wanted() -> Vec<Request> {
    vec![Request::Settings, Request::Accounts]
}

/// A row's change, as the request it sends; the host reloads afterwards, so both windows stay in
/// step.
pub fn update<A>(message: Message) -> Action<A> {
    match message {
        Message::Shown { collection, shown } => Action::send(Request::SetCollectionShown { collection, shown }),
        Message::Reminder { collection, kind } => {
            let value = serde_json::json!({ "calendars": { collection: kind } });
            Action::send(Request::SetSetting { key: SettingKey::Reminders, value })
        }
    }
}

/// The page in the width it gets: a calendar's reminder kind stands beside its name only while
/// the name still fits there ([`one_line`]).
pub fn page(data: &Data, role: Role) -> Element<'_, Message> {
    responsive(move |size| page_in(data, role, one_line(data, role, size.width))).height(Length::Shrink).into()
}

/// The page: one group per connected account that owns a collection of `role`, each row a switch
/// (and, for a shown calendar, its reminder kind); a quiet hint while none does.
///
/// An account whose server could hold such collections but has none yet keeps its place with one
/// sentence instead of rows ([`may_hold`], [`waiting_group`]): before the first sync that worked
/// there is nothing to list, and an account that quietly fell out of the page looked like one that
/// was never set up.
fn page_in(data: &Data, role: Role, one_line: bool) -> Element<'_, Message> {
    let footer = tr("Hidden calendars and lists stay on the server; the apps just don't show them.");
    let groups: Vec<(&str, Vec<Element<'_, Message>>)> = data
        .accounts
        .iter()
        .filter(|account| !account.id.is_empty())
        .filter_map(|account| {
            let rows = rows_of(account, data, role, one_line);
            // An account with no collection of this role: kept with its hint where the server
            // could have them, left out where it never could (AI, ntfy, GitHub's filter lists).
            if rows.is_empty() {
                return may_hold(account, role).then(|| (account.label.as_str(), Vec::new()));
            }
            Some((account.label.as_str(), rows))
        })
        .collect();
    if groups.is_empty() {
        let (title, hint) = match role {
            Role::Calendar => (tr("No calendars"), tr("Calendars appear here once an account has one.")),
            Role::TaskList => (tr("No lists"), tr("Lists appear here once an account has one.")),
        };
        return widgets::empty_state(title, hint);
    }
    // The footer belongs to the last group that has rows: on a group that only says "nothing yet"
    // it would explain switches nobody can see.
    let last_with_rows = groups.iter().rposition(|(_, rows)| !rows.is_empty());
    let built = groups.into_iter().enumerate().map(|(at, (label, rows))| match rows.is_empty() {
        true => waiting_group(label, role),
        false => widgets::group(label, rows, (Some(at) == last_with_rows).then(|| footer.to_string())),
    });
    column(built).spacing(layout::section_gap()).into()
}

/// An account that could hold collections of `role` but has none yet: its name as a group title
/// and one sentence under it, which says what to do about it. No frame – an empty group would be a
/// bare box.
fn waiting_group<'a>(label: &'a str, role: Role) -> Element<'a, Message> {
    let hint = match role {
        Role::Calendar => tr("No calendars yet. Use “Sync now” at this account in the control centre."),
        Role::TaskList => tr("No task lists yet. Use “Sync now” at this account in the control centre."),
    };
    column![widgets::section_title(label), typo::text(typo::body(), hint).style(theme::text_2)]
        .spacing(space::s())
        .into()
}

/// Whether `account` could hold collections of `role`, so an account without any keeps its place
/// with a hint instead of falling out of the page.
///
/// What the server offers where the first sync that worked reported it
/// ([`torro_ipc::AccountInfo::services`]); else the kind of account, since one that never synced is
/// exactly the one the hint is for: Nextcloud and any other CalDAV server, and a mail account whose
/// sign-in also has a CalDAV address in use. An AI or ntfy account has no calendars or lists at
/// all, and GitHub's filter lists are made in the tasks app instead of fetched from a server.
fn may_hold(account: &AccountInfo, role: Role) -> bool {
    let wanted = match role {
        Role::Calendar => ServerService::Calendars,
        Role::TaskList => ServerService::Tasks,
    };
    match &account.services {
        Some(services) => services.contains(&wanted),
        None => match account.kind.as_str() {
            "nextcloud" | "caldav" => true,
            // `caldav` is stored only while the calendars and tasks of that sign-in are in use.
            "mail" => account.mail.as_ref().is_some_and(|mail| mail.caldav.is_some()),
            _ => false,
        },
    }
}

/// One account's rows: its collections that fit `role`, what each one is under its name
/// ([`collection_detail`]), a switch each and, shown and a calendar, the segmented reminder kind –
/// beside the name with `one_line`, else under it ([`stacked_row`]).
fn rows_of<'a>(account: &'a AccountInfo, data: &'a Data, role: Role, one_line: bool) -> Vec<Element<'a, Message>> {
    account
        .collections
        .iter()
        .filter(|collection| match role {
            Role::Calendar => collection.kind.has_events(),
            Role::TaskList => collection.kind.has_todos(),
        })
        .map(|collection| {
            let shown = !data.settings.hidden_collections.contains(&collection.id);
            let id = collection.id.clone();
            let switch = toggler(shown)
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |shown| Message::Shown { collection: id.clone(), shown });
            let dot = widgets::color_dot(widgets::collection_color(collection.color.as_deref()), space::s());
            let detail = collection_detail(collection);
            let kind = (role == Role::Calendar && shown)
                .then(|| reminder_control(&collection.id, data.settings.reminders.calendar(&collection.id)));
            match (kind, one_line) {
                (Some(kind), false) => stacked_row(dot, collection.name.as_str(), &detail, switch.into(), kind),
                (Some(kind), true) => {
                    let control = row![switch, kind].spacing(space::m()).align_y(Alignment::Center);
                    setting_row(Some(dot), collection.name.as_str(), Some(detail), control)
                }
                (None, _) => setting_row(Some(dot), collection.name.as_str(), Some(detail), switch),
            }
        })
        .collect()
}

/// A calendar's row where the reminder kind does not fit beside the name: dot, name and switch on
/// the first line, what the calendar is (`detail`) quietly under the name, the segmented control on
/// its own below them. The metrics are those of `widgets::segment_row` without a column – 12 above
/// and below, 16 at the sides, 8 between – so a narrow settings page looks the same wherever rights
/// or reminders are set.
///
/// `detail` stands here as well as in the one-line form: "Read-only" is the one thing a row must
/// not lose in a narrow window, where it would otherwise look like a calendar that takes new
/// events.
fn stacked_row<'a>(
    dot: Element<'a, Message>,
    name: &'a str,
    detail: &str,
    switch: Element<'a, Message>,
    kind: Element<'a, Message>,
) -> Element<'a, Message> {
    // The name wraps instead of being cut: a line of its own leaves room for it, and the row is
    // already as tall as two lines.
    let lines =
        column![typo::text(typo::body(), name), widgets::ellipsized(typo::meta(), detail.to_string(), theme::text_2)];
    let head = row![dot, lines.width(Fill), switch].spacing(space::m()).align_y(Alignment::Center);
    container(column![head, kind].spacing(space::s())).width(Fill).padding([space::m(), space::l()]).into()
}

/// "Calendar", "Task list · Read-only": the line under a collection's name, as the control centre's
/// own account page had it before task S.19.
///
/// "Read-only" is why it is there: a shared calendar and Nextcloud's contact birthdays take nothing
/// new, and a row without the word looks like one that does.
fn collection_detail(collection: &Collection) -> String {
    let kind = kind_name(collection.kind);
    if collection.writable { kind.to_string() } else { format!("{kind} · {}", tr("Read-only")) }
}

/// What a collection holds, in one word.
fn kind_name(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Events => tr("Calendar"),
        CollectionKind::Todos => tr("Task list"),
        CollectionKind::Both => tr("Both"),
    }
}

/// Whether a calendar's row keeps name, switch and reminder kind on one line in a page `width`
/// wide: only while the longest name and the quiet line under it still fit beside them
/// (`widgets::segment_column`, which reckons with the 16 at both sides and before the control). At
/// 360 px they did not, and the name vanished altogether – the design review of task S.22 found
/// it. Rows without a reminder kind (task lists, hidden calendars) always keep their one line.
fn one_line(data: &Data, role: Role, width: f32) -> bool {
    if role != Role::Calendar {
        return true;
    }
    // Owned, because the line under a name is built ([`collection_detail`]) and the measuring
    // borrows it.
    let shown: Vec<(&str, String)> = data
        .accounts
        .iter()
        .flat_map(|account| {
            account
                .collections
                .iter()
                .filter(|collection| collection.kind.has_events())
                .filter(|collection| !data.settings.hidden_collections.contains(&collection.id))
                .map(|collection| (collection.name.as_str(), collection_detail(collection)))
        })
        .collect();
    let texts = shown.iter().map(|(name, detail)| (*name, Some(detail.as_str())));
    widgets::segment_column(reminder_slot(), texts, width).is_some()
}

/// What a shown calendar's row needs beside its name: the colour dot and its gap, the switch (as
/// wide as two of its height, the way Iced draws a toggler) and the segmented reminder kind.
fn reminder_slot() -> f32 {
    let labels = [tr("Off"), tr("Notification"), tr("Full screen")];
    space::s() + space::m() + 2.0 * style::CHECK_SIZE + space::m() + widgets::segmented_width(&labels)
}

/// The reminder kind of a shown calendar: Off, Notification, Full screen, as a segmented control.
fn reminder_control<'a>(collection: &str, active: CalendarReminders) -> Element<'a, Message> {
    let msg = |kind: CalendarReminders| Message::Reminder { collection: collection.to_string(), kind };
    let options = [
        (tr("Off"), msg(CalendarReminders::Off)),
        (tr("Notification"), msg(CalendarReminders::Notification)),
        (tr("Full screen"), msg(CalendarReminders::FullScreen)),
    ];
    let active = match active {
        CalendarReminders::Off => 0,
        CalendarReminders::Notification => 1,
        CalendarReminders::FullScreen => 2,
    };
    widgets::segmented(options, active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo::Demo;

    /// Every account the demo answers with, so the page builds rows for more than one.
    async fn data() -> Data {
        let backend: std::sync::Arc<dyn crate::Backend> = std::sync::Arc::new(Demo::new());
        crate::load(backend, wanted()).await
    }

    /// The switch sends exactly `settings.set_collection_shown` with the collection it is on.
    #[test]
    fn the_switch_sends_only_set_collection_shown() {
        let on: Action<()> = update(Message::Shown { collection: "a/work".into(), shown: true });
        assert_eq!(on, Action::send(Request::SetCollectionShown { collection: "a/work".into(), shown: true }));
        let off: Action<()> = update(Message::Shown { collection: "a/work".into(), shown: false });
        assert_eq!(off, Action::send(Request::SetCollectionShown { collection: "a/work".into(), shown: false }));
    }

    /// A reminder kind sends `set_setting reminders` with only that calendar's field.
    #[test]
    fn a_reminder_kind_sets_only_its_calendar() {
        let action: Action<()> =
            update(Message::Reminder { collection: "a/work".into(), kind: CalendarReminders::FullScreen });
        assert_eq!(
            action,
            Action::send(Request::SetSetting {
                key: SettingKey::Reminders,
                value: serde_json::json!({"calendars": {"a/work": "full_screen"}})
            })
        );
    }

    /// The demo's accounts build one group per account that owns a calendar, and another for
    /// task lists; an account without either (an AI account, say) builds no group.
    #[tokio::test]
    async fn the_page_groups_by_account_and_role() {
        let data = data().await;
        assert!(data.accounts.len() > 1, "the demo has more than one account to group");
        let _calendars: Element<'_, Message> = page(&data, Role::Calendar);
        let _lists: Element<'_, Message> = page(&data, Role::TaskList);
    }

    /// Without any matching collection the page says so instead of showing nothing.
    #[test]
    fn an_empty_page_says_so() {
        let data = Data::default();
        let _calendars: Element<'_, Message> = page(&data, Role::Calendar);
        let _lists: Element<'_, Message> = page(&data, Role::TaskList);
    }

    /// Found in the review of task S.22: the line under a collection's name says what it is and
    /// whether it is read-only. Without the word a shared calendar looks like one that takes new
    /// events.
    #[test]
    fn a_row_says_what_a_collection_is_and_whether_it_is_read_only() {
        let mut collection = Collection {
            id: "nextcloud:anna/work".into(),
            name: "Work".into(),
            kind: CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        };
        assert_eq!(collection_detail(&collection), tr("Calendar"));
        collection.writable = false;
        assert_eq!(collection_detail(&collection), format!("{} · {}", tr("Calendar"), tr("Read-only")));
        collection.kind = CollectionKind::Todos;
        assert_eq!(collection_detail(&collection), format!("{} · {}", tr("Task list"), tr("Read-only")));
        collection.kind = CollectionKind::Both;
        collection.writable = true;
        assert_eq!(collection_detail(&collection), tr("Both"));
        assert_eq!(kind_name(CollectionKind::Events), tr("Calendar"));
    }

    /// Found in the review of task S.22: an account whose server holds calendars and lists but has
    /// none yet keeps its place with one sentence; one that never could (AI, ntfy, GitHub) stays
    /// out of the page.
    #[tokio::test]
    async fn an_account_without_collections_keeps_its_place_with_a_hint() {
        let loaded = data().await;
        let of = |id: &str| loaded.accounts.iter().find(|a| a.kind == id).expect(id).clone();

        // Nextcloud without collections: the server could have them, so the hint stands.
        let mut empty = of("nextcloud");
        empty.collections = Vec::new();
        assert!(may_hold(&empty, Role::Calendar) && may_hold(&empty, Role::TaskList));

        // What the first sync that worked reported wins over the kind.
        let synced = AccountInfo { services: Some(vec![ServerService::Calendars]), ..empty.clone() };
        assert!(may_hold(&synced, Role::Calendar), "the server has calendars");
        assert!(!may_hold(&synced, Role::TaskList), "and no task lists");

        // GitHub's filter lists are made in the tasks app; ntfy and mail without CalDAV have none.
        assert!(!may_hold(&of("github"), Role::TaskList) && !may_hold(&of("github"), Role::Calendar));
        assert!(!may_hold(&of("ntfy"), Role::Calendar));
        let mail = of("mail");
        assert!(mail.mail.as_ref().is_some_and(|view| view.caldav.is_none()), "the demo's mail is mail only");
        assert!(!may_hold(&mail, Role::Calendar));
        let with_caldav = AccountInfo {
            mail: mail
                .mail
                .clone()
                .map(|view| torro_ipc::MailView { caldav: Some("https://dav.mailbox.org".into()), ..view }),
            ..mail
        };
        assert!(may_hold(&with_caldav, Role::Calendar), "that sign-in's calendars are in use");

        // The page builds with such an account, in both roles and in a narrow window.
        let accounts = vec![empty, of("github")];
        let waiting = Data { accounts, ..loaded.clone() };
        for role in [Role::Calendar, Role::TaskList] {
            for width in [1100.0, 500.0, 360.0] {
                let _: Element<'_, Message> = page_in(&waiting, role, one_line(&waiting, role, width));
            }
        }
    }
}
