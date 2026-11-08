//! The calendar app's sections "Reminders" and "Night" (task S.19 in
//! `docs/tasks/steuerzentrale.md`), ported from the control centre's General page (task ER2.4,
//! K.27) now that "Calendars" moves into [`crate::collections`] instead, grouped by account like
//! "Lists" in the tasks app. "Working hours" (task K.33) follows the night here, since app
//! settings live in the app's own window.
//!
//! `torrocalendar` builds its sections – "Calendars" from [`crate::collections`], and these – into
//! one `SECTIONS` list of its own: this library has no module that already combines them, since
//! "Calendars" is shared with the tasks app's "Lists" and does not belong only here.
//!
//! The floating card "Night" at the moon on the timeline's canvas (`torrocalendar::app`) is the
//! app's own quick way to the same setting; this section is the full page behind the gear and
//! does not touch it.
//!
//! Every choice is a Torro dropdown (issue #554): the default reminder a plain menu, the quarter
//! hours of night and working hours a menu with search. Each section keeps which one is open in its
//! [`reminders::State`], [`night::State`] or [`working_hours::State`] and shows it in its `overlay`.

use iced::widget::{column, row, toggler};
use iced::{Alignment, Element};

use torro_core::i18n::{tr, trf};
use torro_core::settings::Quarter;
use torro_gui::theme::{self, space};
use torro_gui::widgets::{Align, DROPDOWN_SEARCH, Dropdown, DropdownEvent, MenuItem, MenuSection};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{Request, SettingKey};

use crate::parts::setting_row;
use crate::{Action, Data};

/// Reminders of events: on or off, how long before, every display, the sound, and which events
/// remind at all (task ER2.4).
pub mod reminders {
    use torro_core::settings::{ReminderSound, Reminders};

    use super::*;

    /// The dropdowns of the page.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Choice {
        /// "Default reminder".
        DefaultLead,
    }

    /// What the page keeps between frames: which dropdown is open.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct State {
        pub menus: Dropdown<Choice>,
    }

    /// What a row changes (`settings.set_setting reminders`): only its own field, so two windows
    /// changing different fields never overwrite each other. A calendar's own reminder kind is
    /// [`crate::collections::Message::Reminder`], not here. [`Message::Menus`] only opens and
    /// closes the dropdown.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Message {
        /// The dropdown of the default reminder opened, closed or was searched.
        Menus(DropdownEvent<Choice>),
        Enabled(bool),
        DefaultLead(Lead),
        AllDisplays(bool),
        Sound(bool),
        OnlyAccepted(bool),
        AllDay(bool),
        FreeEvents(bool),
        OutsideWorkHours(bool),
        NeverDuringCall(bool),
    }

    impl Message {
        /// The change as the value of `settings.set_setting reminders`: only this field; `None` for
        /// the dropdown's own events, which change nothing.
        fn value(self) -> Option<serde_json::Value> {
            use serde_json::json;
            Some(match self {
                Message::Menus(_) => return None,
                Message::Enabled(on) => json!({"enabled": on}),
                Message::DefaultLead(Lead(minutes)) => json!({"defaultLead": minutes}),
                Message::AllDisplays(on) => json!({"allDisplays": on}),
                Message::Sound(on) => {
                    let sound = if on { ReminderSound::System } else { ReminderSound::None };
                    json!({"sound": sound})
                }
                Message::OnlyAccepted(on) => json!({"onlyAccepted": on}),
                Message::AllDay(on) => json!({"allDay": on}),
                Message::FreeEvents(on) => json!({"freeEvents": on}),
                Message::OutsideWorkHours(on) => json!({"outsideWorkHours": on}),
                Message::NeverDuringCall(on) => json!({"neverDuringCall": on}),
            })
        }
    }

    /// A default reminder of the list: minutes before the start, or none.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Lead(pub Option<u32>);

    impl Lead {
        /// The leads the list offers.
        const ALL: [Lead; 8] = [
            Lead(None),
            Lead(Some(0)),
            Lead(Some(1)),
            Lead(Some(5)),
            Lead(Some(10)),
            Lead(Some(15)),
            Lead(Some(30)),
            Lead(Some(60)),
        ];
    }

    impl std::fmt::Display for Lead {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.0 {
                None => f.write_str(tr("None")),
                Some(0) => f.write_str(tr("At start")),
                Some(60) => f.write_str(&trf("{span} before", &[("span", &tr("1 hour"))])),
                Some(minutes) => f.write_str(&trf("{span} before", &[("span", &trf("{n} min", &[("n", &minutes)]))])),
            }
        }
    }

    /// Opening the dropdown asks for the focus of its search field; a choice closes it.
    pub fn update<A>(state: &mut State, message: Message) -> Action<A> {
        if let Message::Menus(event) = message {
            return if state.menus.update(event) { Action::Focus(DROPDOWN_SEARCH) } else { Action::None };
        }
        state.menus.close();
        match message.value() {
            Some(value) => Action::send(Request::SetSetting { key: SettingKey::Reminders, value }),
            None => Action::None,
        }
    }

    /// "Reminders" (on/off, default, every display, sound) and "Which events" (task ER2.4).
    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let reminders = &data.settings.reminders;
        column(vec![group(state, reminders.clone()), which_events(reminders)]).spacing(layout::section_gap()).into()
    }

    /// The open dropdown of the default reminder, under its button.
    pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
        let key = state.menus.open()?;
        let chosen = Lead(data.settings.reminders.default_lead);
        let items = Lead::ALL
            .into_iter()
            .map(|lead| MenuItem::plain(lead.to_string(), Message::DefaultLead(lead)).current(lead == chosen))
            .collect();
        let sections = vec![MenuSection::new(items)];
        Some(state.menus.view(key, sections, widgets::menu_width(), Align::End, Message::Menus))
    }

    /// Escape closes the open dropdown first.
    pub fn escape(state: &State) -> Option<Message> {
        state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
    }

    /// A switch for one field of the reminders.
    fn switch<'a>(on: bool, change: fn(bool) -> Message) -> Element<'a, Message> {
        toggler(on).size(style::CHECK_SIZE).style(style::toggler).on_toggle(change).into()
    }

    /// The group "Reminders": on or off, the default reminder, every display, the sound.
    fn group(state: &State, reminders: Reminders) -> Element<'static, Message> {
        let lead = state.menus.button(Choice::DefaultLead, Lead(reminders.default_lead).to_string(), Message::Menus);
        widgets::group(
            tr("Reminders"),
            vec![
                setting_row(None, tr("Show reminders"), None, switch(reminders.enabled, Message::Enabled)),
                setting_row(
                    None,
                    tr("Default reminder"),
                    Some(tr("For events without their own reminder").to_string()),
                    lead,
                ),
                setting_row(
                    None,
                    tr("On all displays"),
                    Some(tr("A full-screen reminder covers every display").to_string()),
                    switch(reminders.all_displays, Message::AllDisplays),
                ),
                setting_row(None, tr("Sound"), None, switch(reminders.sound == ReminderSound::System, Message::Sound)),
            ],
            Some(
                tr("Reminders of an event come first; you set them in the event. A full-screen reminder stays until you join, snooze or dismiss it.")
                    .to_string(),
            ),
        )
    }

    /// The group "Which events" (task ER2.4), with the sentence S.19 adds: these switches narrow
    /// what reminds at all, whatever an event's own reminder says.
    fn which_events(reminders: &Reminders) -> Element<'_, Message> {
        let footer = tr("Choose which kinds of events may remind you, whatever their own reminder says.");
        widgets::group(
            tr("Which events"),
            vec![
                setting_row(
                    None,
                    tr("Only events I accepted"),
                    None,
                    switch(reminders.only_accepted, Message::OnlyAccepted),
                ),
                setting_row(None, tr("All-day events"), None, switch(reminders.all_day, Message::AllDay)),
                setting_row(None, tr("Events marked free"), None, switch(reminders.free_events, Message::FreeEvents)),
                setting_row(
                    None,
                    tr("Outside work hours"),
                    None,
                    switch(reminders.outside_work_hours, Message::OutsideWorkHours),
                ),
                setting_row(
                    None,
                    tr("Never full screen during a video call"),
                    Some(tr("Shows a notification instead while an event with a call link runs").to_string()),
                    switch(reminders.never_during_call, Message::NeverDuringCall),
                ),
            ],
            Some(footer.to_string()),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::Action as Act;

        fn sent(action: Action<()>) -> serde_json::Value {
            match action {
                Act::Send(requests) => match requests.as_slice() {
                    [Request::SetSetting { key: SettingKey::Reminders, value }] => value.clone(),
                    other => panic!("expected one SetSetting(Reminders), got {other:?}"),
                },
                other => panic!("expected Send, got {other:?}"),
            }
        }

        fn update(message: Message) -> Action<()> {
            super::update(&mut State::default(), message)
        }

        /// Every row of the reminders changes only its own field (task ER2.4).
        #[test]
        fn each_row_sets_only_its_field() {
            use serde_json::json;
            assert_eq!(sent(update(Message::Enabled(false))), json!({"enabled": false}));
            assert_eq!(sent(update(Message::DefaultLead(Lead(None)))), json!({"defaultLead": null}));
            assert_eq!(sent(update(Message::DefaultLead(Lead(Some(5))))), json!({"defaultLead": 5}));
            assert_eq!(sent(update(Message::Sound(false))), json!({"sound": "none"}));
            assert_eq!(sent(update(Message::NeverDuringCall(false))), json!({"neverDuringCall": false}));
            for message in [
                Message::AllDisplays(false),
                Message::OnlyAccepted(false),
                Message::AllDay(true),
                Message::FreeEvents(true),
                Message::OutsideWorkHours(false),
            ] {
                let value = sent(update(message));
                assert_eq!(value.as_object().map(|o| o.len()), Some(1), "{value}");
                assert!(Reminders::default().merged(&value).is_ok(), "{value}");
            }
        }

        #[test]
        fn leads_read_as_the_task_says() {
            assert_eq!(Lead(None).to_string(), tr("None"));
            assert_eq!(Lead(Some(0)).to_string(), tr("At start"));
            assert_eq!(Lead(Some(60)).to_string(), trf("{span} before", &[("span", &tr("1 hour"))]));
            assert_eq!(Lead(Some(5)).to_string(), trf("{span} before", &[("span", &trf("{n} min", &[("n", &5)]))]));
        }

        #[tokio::test]
        async fn the_page_builds_with_default_and_stored_reminders() {
            let default = Data::default();
            let state = State::default();
            let _: Element<'_, Message> = view(&state, &default);
            let stored = Data { settings: torro_core::settings::Settings::default(), ..Data::default() };
            let _: Element<'_, Message> = view(&state, &stored);
            assert!(overlay(&state, &stored).is_none(), "no dropdown is open");
        }

        /// Issue #554: the default reminder is a Torro dropdown; opening it asks for the focus,
        /// Escape closes it, a choice closes it and saves.
        #[test]
        fn the_dropdown_opens_closes_and_chooses() {
            let (mut state, data) = (State::default(), Data::default());
            let open = super::update::<()>(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::DefaultLead)));
            assert_eq!(open, Act::Focus(DROPDOWN_SEARCH));
            assert!(overlay(&state, &data).is_some());
            assert_eq!(escape(&state), Some(Message::Menus(DropdownEvent::Close)));
            let chosen = super::update::<()>(&mut state, Message::DefaultLead(Lead(Some(5))));
            assert_eq!(sent(chosen), serde_json::json!({"defaultLead": 5}));
            assert_eq!((state.menus.open(), escape(&state)), (None, None), "the choice closes it");
            let _ = super::update::<()>(&mut state, Message::Menus(DropdownEvent::Toggle(Choice::DefaultLead)));
            assert_eq!(super::update::<()>(&mut state, Message::Menus(DropdownEvent::Close)), Act::None);
            assert!(overlay(&state, &data).is_none());
        }
    }
}

/// The night: from when until when the timeline and the radar fold it away (task K.27). The
/// floating card at the moon on the timeline's canvas sends the same request; this is the full
/// page behind the gear.
pub mod night {
    use torro_core::settings::Night;

    use super::*;

    /// What the page keeps between frames: which of its two dropdowns is open.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct State {
        pub menus: Dropdown<End>,
    }

    /// "From" or "Until" of the night, or the dropdowns opening and closing.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Message {
        Menus(DropdownEvent<End>),
        From(Quarter),
        To(Quarter),
    }

    /// The whole night goes as one value, with the other end as the stored settings have it, so
    /// two windows never leave half of it behind. Opening a dropdown asks for the focus of its
    /// search field; a choice closes it.
    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        let night = data.settings.night;
        let night = match message {
            Message::Menus(event) => return menus(&mut state.menus, event),
            Message::From(from) => Night { from, ..night },
            Message::To(to) => Night { to, ..night },
        };
        state.menus.close();
        Action::send(Request::SetSetting { key: SettingKey::Night, value: night.to_value() })
    }

    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let footer = tr(
            "The timeline folds the night away and the radar shades it; neither counts it as free time. The same time twice means no night.",
        );
        let night = data.settings.night;
        let control = from_until(&state.menus, night.from, night.to, Message::Menus);
        let row = setting_row(None, tr("Night"), None, control);
        widgets::group(tr("Night"), vec![row], Some(footer.to_string()))
    }

    /// The open dropdown of "From" or "Until", with search.
    pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
        let night = data.settings.night;
        quarter_menu(&state.menus, night.from, night.to, Message::From, Message::To, Message::Menus)
    }

    /// Escape closes the open dropdown first.
    pub fn escape(state: &State) -> Option<Message> {
        state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::Action as Act;

        fn sent(action: Action<()>) -> serde_json::Value {
            match action {
                Act::Send(requests) => match requests.as_slice() {
                    [Request::SetSetting { key: SettingKey::Night, value }] => value.clone(),
                    other => panic!("expected one SetSetting(Night), got {other:?}"),
                },
                other => panic!("expected Send, got {other:?}"),
            }
        }

        fn quarter(hour: u32, minute: u32) -> Quarter {
            Quarter::new(hour, minute).expect("quarter")
        }

        fn update(message: Message, data: &Data) -> Action<()> {
            super::update(&mut State::default(), message, data)
        }

        /// A time of the night sets the whole night, with the other end kept.
        #[test]
        fn a_time_sets_the_whole_night() {
            let data = Data::default();
            assert_eq!(
                sent(update(Message::From(quarter(23, 30)), &data)),
                serde_json::json!({"from": "23:30", "to": "06:00"})
            );
            assert_eq!(
                sent(update(Message::To(quarter(7, 0)), &data)),
                serde_json::json!({"from": "22:00", "to": "07:00"})
            );
        }

        #[test]
        fn the_page_builds() {
            let _: Element<'_, Message> = view(&State::default(), &Data::default());
        }

        /// Issue #554: "From" opens a dropdown with search and the focus; Escape closes it; a
        /// quarter hour closes it and sets the night.
        #[test]
        fn the_dropdown_opens_closes_and_chooses() {
            let (mut state, data) = (State::default(), Data::default());
            let open = super::update::<()>(&mut state, Message::Menus(DropdownEvent::Toggle(End::From)), &data);
            assert_eq!(open, Act::Focus(DROPDOWN_SEARCH));
            assert!(overlay(&state, &data).is_some());
            let _ = super::update::<()>(&mut state, Message::Menus(DropdownEvent::Query("23".into())), &data);
            assert_eq!(state.menus.query(), "23");
            assert_eq!(escape(&state), Some(Message::Menus(DropdownEvent::Close)));
            let chosen = super::update::<()>(&mut state, Message::From(quarter(23, 0)), &data);
            assert_eq!(sent(chosen), serde_json::json!({"from": "23:00", "to": "06:00"}));
            assert!(overlay(&state, &data).is_none(), "the choice closes it");
            assert_eq!(escape(&state), None);
        }
    }
}

/// The working hours: from when until when the calendar draws its thin line in day, week,
/// timeline and radar (task K.33). The same every day, never over midnight.
pub mod working_hours {
    use torro_core::settings::WorkingHours;

    use super::*;

    /// What the page keeps between frames: which of its two dropdowns is open.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct State {
        pub menus: Dropdown<End>,
    }

    /// "From" or "Until" of the working hours, or the dropdowns opening and closing.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Message {
        Menus(DropdownEvent<End>),
        From(Quarter),
        To(Quarter),
    }

    /// The working hours after `message`: picking "From" after "Until" (or "Until" before
    /// "From") moves the other end along and keeps the length, held to 00:00 and 23:45, so the
    /// page never shows an error. Where the end of the day leaves no room at all, the picked end
    /// gives way by a quarter hour rather than turning the working hours off unasked. The core
    /// still refuses anything over midnight.
    pub(crate) fn changed(hours: WorkingHours, message: Message) -> WorkingHours {
        let length = hours.to.minutes() - hours.from.minutes().min(hours.to.minutes());
        // No working hours stay none: the other end moves along with nothing between.
        let room = if hours.is_none() { 0 } else { 15 };
        let last = Quarter::all().last().map_or(0, Quarter::minutes);
        let at = |minutes: u32| Quarter::new(minutes / 60, minutes % 60).unwrap_or(hours.from);
        match message {
            Message::From(from) if from > hours.to => {
                let to = at((from.minutes() + length).min(last));
                WorkingHours { from: at(from.minutes().min(to.minutes().saturating_sub(room))), to }
            }
            Message::From(from) => WorkingHours { from, ..hours },
            Message::To(to) if to < hours.from => {
                let from = at(to.minutes().saturating_sub(length));
                WorkingHours { from, to: at(to.minutes().max(from.minutes() + room)) }
            }
            Message::To(to) => WorkingHours { to, ..hours },
            // The dropdowns' own events change no time.
            Message::Menus(_) => hours,
        }
    }

    /// Both ends go as one value, with the other as the stored settings have it, so two windows
    /// never leave half of it behind. Opening a dropdown asks for the focus of its search field; a
    /// choice closes it.
    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        if let Message::Menus(event) = message {
            return menus(&mut state.menus, event);
        }
        state.menus.close();
        let hours = changed(data.settings.working_hours, message);
        Action::send(Request::SetSetting { key: SettingKey::WorkingHours, value: hours.to_value() })
    }

    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let footer = tr(
            "The calendar draws your working hours as a thin line in day, week, timeline and radar. The same time twice means no working hours.",
        );
        let hours = data.settings.working_hours;
        let control = from_until(&state.menus, hours.from, hours.to, Message::Menus);
        let row = setting_row(None, tr("Working hours"), None, control);
        widgets::group(tr("Working hours"), vec![row], Some(footer.to_string()))
    }

    /// The open dropdown of "From" or "Until", with search.
    pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
        let hours = data.settings.working_hours;
        quarter_menu(&state.menus, hours.from, hours.to, Message::From, Message::To, Message::Menus)
    }

    /// Escape closes the open dropdown first.
    pub fn escape(state: &State) -> Option<Message> {
        state.menus.open().map(|_| Message::Menus(DropdownEvent::Close))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::Action as Act;

        fn sent(action: Action<()>) -> serde_json::Value {
            match action {
                Act::Send(requests) => match requests.as_slice() {
                    [Request::SetSetting { key: SettingKey::WorkingHours, value }] => value.clone(),
                    other => panic!("expected one SetSetting(WorkingHours), got {other:?}"),
                },
                other => panic!("expected Send, got {other:?}"),
            }
        }

        fn quarter(hour: u32, minute: u32) -> Quarter {
            Quarter::new(hour, minute).expect("quarter")
        }

        fn hours(from: Quarter, to: Quarter) -> WorkingHours {
            WorkingHours { from, to }
        }

        fn update(message: Message, data: &Data) -> Action<()> {
            super::update(&mut State::default(), message, data)
        }

        /// A time sets both ends, with the other kept while it stays in order.
        #[test]
        fn a_time_sets_the_whole_working_hours() {
            let data = Data::default();
            assert_eq!(
                sent(update(Message::From(quarter(7, 30)), &data)),
                serde_json::json!({"from": "07:30", "to": "17:00"})
            );
            assert_eq!(
                sent(update(Message::To(quarter(16, 0)), &data)),
                serde_json::json!({"from": "08:00", "to": "16:00"})
            );
        }

        /// "From" after "Until" moves "Until" along, and the other way round; the length stays.
        #[test]
        fn the_other_end_moves_along() {
            let short = hours(quarter(9, 0), quarter(11, 30));
            assert_eq!(changed(short, Message::From(quarter(12, 0))), hours(quarter(12, 0), quarter(14, 30)));
            assert_eq!(changed(short, Message::To(quarter(8, 15))), hours(quarter(5, 45), quarter(8, 15)));
        }

        /// Moving along stops at 00:00 and 23:45, never over midnight.
        #[test]
        fn moving_along_stops_at_the_ends_of_the_day() {
            let day = hours(quarter(8, 0), quarter(17, 0));
            assert_eq!(changed(day, Message::From(quarter(20, 0))), hours(quarter(20, 0), quarter(23, 45)));
            assert_eq!(changed(day, Message::To(quarter(3, 0))), hours(quarter(0, 0), quarter(3, 0)));
            assert_eq!(changed(day, Message::From(quarter(23, 45))), hours(quarter(23, 30), quarter(23, 45)));
            assert_eq!(changed(day, Message::To(quarter(0, 0))), hours(quarter(0, 0), quarter(0, 15)));
            let none = hours(quarter(9, 0), quarter(9, 0));
            assert!(changed(none, Message::From(quarter(10, 0))).is_none(), "no working hours stay none");
        }

        #[test]
        fn the_page_builds() {
            let _: Element<'_, Message> = view(&State::default(), &Data::default());
        }

        /// Issue #554: "Until" opens a dropdown with the focus; a quarter hour closes it.
        #[test]
        fn the_dropdown_opens_and_a_choice_closes_it() {
            let (mut state, data) = (State::default(), Data::default());
            let open = super::update::<()>(&mut state, Message::Menus(DropdownEvent::Toggle(End::Until)), &data);
            assert_eq!(open, Act::Focus(DROPDOWN_SEARCH));
            assert!(overlay(&state, &data).is_some());
            let chosen = super::update::<()>(&mut state, Message::To(quarter(16, 0)), &data);
            assert_eq!(sent(chosen), serde_json::json!({"from": "08:00", "to": "16:00"}));
            assert!(overlay(&state, &data).is_none());
            let short = hours(quarter(9, 0), quarter(10, 0));
            assert_eq!(changed(short, Message::Menus(DropdownEvent::Close)), short);
        }
    }
}

/// The two dropdowns of a span of the day: "From" and "Until" of the night and of the working hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    From,
    Until,
}

/// An event of the dropdowns of [`End`]: opening one asks for the focus of its search field.
fn menus<A>(menus: &mut Dropdown<End>, event: DropdownEvent<End>) -> Action<A> {
    if menus.update(event) { Action::Focus(DROPDOWN_SEARCH) } else { Action::None }
}

/// "From" and "Until", each a dropdown of the quarter hours of a day: the control of the night and
/// of the working hours.
fn from_until<'a, M: Clone + 'a>(
    menus: &Dropdown<End>,
    from: Quarter,
    to: Quarter,
    on_menus: fn(DropdownEvent<End>) -> M,
) -> Element<'a, M> {
    let label = |text: &'static str| typo::text(typo::meta(), text).style(theme::text_2);
    row(vec![
        label(tr("From")).into(),
        menus.button(End::From, from.to_string(), on_menus),
        label(tr("Until")).into(),
        menus.button(End::Until, to.to_string(), on_menus),
    ])
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
}

/// The open dropdown of [`from_until`]: the 96 quarter hours with search (issue #554), the
/// chosen one marked.
fn quarter_menu<'a, M: Clone + 'a>(
    menus: &'a Dropdown<End>,
    from: Quarter,
    to: Quarter,
    on_from: fn(Quarter) -> M,
    on_to: fn(Quarter) -> M,
    on_menus: fn(DropdownEvent<End>) -> M,
) -> Option<Element<'a, M>> {
    let key = menus.open()?;
    let (chosen, on_pick) = match key {
        End::From => (from, on_from),
        End::Until => (to, on_to),
    };
    let items = Quarter::all()
        .map(|quarter| MenuItem::plain(quarter.to_string(), on_pick(quarter)).current(quarter == chosen))
        .collect();
    Some(menus.view(key, vec![MenuSection::new(items)], widgets::menu_width(), Align::End, on_menus))
}
