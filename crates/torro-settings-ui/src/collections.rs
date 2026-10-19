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
use torro_gui::theme::space;
use torro_gui::widgets;
use torro_gui::{layout, style, typo};
use torro_ipc::{AccountInfo, Request, SettingKey};

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
fn page_in(data: &Data, role: Role, one_line: bool) -> Element<'_, Message> {
    let footer = tr("Hidden calendars and lists stay on the server; the apps just don't show them.");
    let groups: Vec<(&str, Vec<Element<'_, Message>>)> = data
        .accounts
        .iter()
        .filter(|account| !account.id.is_empty())
        .filter_map(|account| {
            let rows = rows_of(account, data, role, one_line);
            (!rows.is_empty()).then_some((account.label.as_str(), rows))
        })
        .collect();
    if groups.is_empty() {
        let (title, hint) = match role {
            Role::Calendar => (tr("No calendars"), tr("Calendars appear here once an account has one.")),
            Role::TaskList => (tr("No lists"), tr("Lists appear here once an account has one.")),
        };
        return widgets::empty_state(title, hint);
    }
    let last = groups.len() - 1;
    let built = groups.into_iter().enumerate().map(|(at, (label, rows))| {
        let foot = (at == last).then(|| footer.to_string());
        widgets::group(label, rows, foot)
    });
    column(built).spacing(layout::section_gap()).into()
}

/// One account's rows: its collections that fit `role`, a switch each and, shown and a calendar,
/// the segmented reminder kind – beside the name with `one_line`, else under it ([`stacked_row`]).
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
            let kind = (role == Role::Calendar && shown)
                .then(|| reminder_control(&collection.id, data.settings.reminders.calendar(&collection.id)));
            match (kind, one_line) {
                (Some(kind), false) => stacked_row(dot, collection.name.as_str(), switch.into(), kind),
                (Some(kind), true) => {
                    let control = row![switch, kind].spacing(space::m()).align_y(Alignment::Center);
                    setting_row(Some(dot), collection.name.as_str(), None, control)
                }
                (None, _) => setting_row(Some(dot), collection.name.as_str(), None, switch),
            }
        })
        .collect()
}

/// A calendar's row where the reminder kind does not fit beside the name: dot, name and switch on
/// the first line, the segmented control on its own under them. The metrics are those of
/// `widgets::segment_row` without a column – 12 above and below, 16 at the sides, 8 between – so a
/// narrow settings page looks the same wherever rights or reminders are set.
fn stacked_row<'a>(
    dot: Element<'a, Message>,
    name: &'a str,
    switch: Element<'a, Message>,
    kind: Element<'a, Message>,
) -> Element<'a, Message> {
    // The name wraps instead of being cut: a line of its own leaves room for it, and the row is
    // already as tall as two lines.
    let head =
        row![dot, typo::text(typo::body(), name).width(Fill), switch].spacing(space::m()).align_y(Alignment::Center);
    container(column![head, kind].spacing(space::s())).width(Fill).padding([space::m(), space::l()]).into()
}

/// Whether a calendar's row keeps name, switch and reminder kind on one line in a page `width`
/// wide: only while the longest name still fits beside them (`widgets::segment_column`, which
/// reckons with the 16 at both sides and before the control). At 360 px it did not, and the name
/// vanished altogether – the design review of task S.22 found it. Rows without a reminder kind
/// (task lists, hidden calendars) always keep their one line.
fn one_line(data: &Data, role: Role, width: f32) -> bool {
    if role != Role::Calendar {
        return true;
    }
    let names = data.accounts.iter().flat_map(|account| {
        account
            .collections
            .iter()
            .filter(|collection| collection.kind.has_events())
            .filter(|collection| !data.settings.hidden_collections.contains(&collection.id))
            .map(|collection| (collection.name.as_str(), None))
    });
    widgets::segment_column(reminder_slot(), names, width).is_some()
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
}
