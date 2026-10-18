//! The calendar app's sections "Reminders" and "Night" (task S.19 in
//! `docs/tasks/steuerzentrale.md`), ported from the control centre's General page (task ER2.4,
//! K.27) now that "Calendars" moves into [`crate::collections`] instead, grouped by account like
//! "Lists" in the tasks app.
//!
//! `torrocalendar` builds its three sections – "Calendars" from [`crate::collections`], and these
//! two – into one `SECTIONS` list of its own: this library has no module that already combines
//! them, since "Calendars" is shared with the tasks app's "Lists" and does not belong only here.
//!
//! The floating card "Night" at the moon on the timeline's canvas (`torrocalendar::app`) is the
//! app's own quick way to the same setting; this section is the full page behind the gear and
//! does not touch it.

use iced::widget::{column, pick_list, row, toggler};
use iced::{Alignment, Element};

use torro_core::i18n::{tr, trf};
use torro_gui::theme::{self, space};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{Request, SettingKey};

use crate::parts::setting_row;
use crate::{Action, Data};

/// Reminders of events: on or off, how long before, every display, the sound, and which events
/// remind at all (task ER2.4).
pub mod reminders {
    use torro_core::settings::{ReminderSound, Reminders};

    use super::*;

    /// What a row changes (`settings.set_setting reminders`): only its own field, so two windows
    /// changing different fields never overwrite each other. A calendar's own reminder kind is
    /// [`crate::collections::Message::Reminder`], not here.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Message {
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
        /// The change as the value of `settings.set_setting reminders`: only this field.
        fn value(self) -> serde_json::Value {
            use serde_json::json;
            match self {
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
            }
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

    pub fn update<A>(message: Message) -> Action<A> {
        Action::send(Request::SetSetting { key: SettingKey::Reminders, value: message.value() })
    }

    /// "Reminders" (on/off, default, every display, sound) and "Which events" (task ER2.4).
    pub fn view(data: &Data) -> Element<'_, Message> {
        let reminders = &data.settings.reminders;
        column(vec![group(reminders.clone()), which_events(reminders)]).spacing(layout::section_gap()).into()
    }

    /// A switch for one field of the reminders.
    fn switch<'a>(on: bool, change: fn(bool) -> Message) -> Element<'a, Message> {
        toggler(on).size(style::CHECK_SIZE).style(style::toggler).on_toggle(change).into()
    }

    /// The group "Reminders": on or off, the default reminder, every display, the sound.
    fn group(reminders: Reminders) -> Element<'static, Message> {
        let lead = pick_list(Lead::ALL, Some(Lead(reminders.default_lead)), Message::DefaultLead)
            .padding(style::pick_list_padding())
            .text_size(typo::caption().size)
            .text_line_height(typo::caption().line_height)
            .font(typo::caption().font)
            .style(style::pick_list)
            .menu_style(style::pick_list_menu);
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
            let _: Element<'_, Message> = view(&default);
            let stored = Data { settings: torro_core::settings::Settings::default(), ..Data::default() };
            let _: Element<'_, Message> = view(&stored);
        }
    }
}

/// The night: from when until when the timeline and the radar fold it away (task K.27). The
/// floating card at the moon on the timeline's canvas sends the same request; this is the full
/// page behind the gear.
pub mod night {
    use torro_core::settings::{Night, Quarter};

    use super::*;

    /// "From" or "Until" of the night.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Message {
        From(Quarter),
        To(Quarter),
    }

    /// The whole night goes as one value, with the other end as the stored settings have it, so
    /// two windows never leave half of it behind.
    pub fn update<A>(message: Message, data: &Data) -> Action<A> {
        let night = data.settings.night;
        let night = match message {
            Message::From(from) => Night { from, ..night },
            Message::To(to) => Night { to, ..night },
        };
        Action::send(Request::SetSetting { key: SettingKey::Night, value: night.to_value() })
    }

    pub fn view(data: &Data) -> Element<'_, Message> {
        let footer = tr(
            "The timeline folds the night away and the radar shades it; neither counts it as free time. The same time twice means no night.",
        );
        let row = setting_row(None, tr("Night"), None, control(data.settings.night));
        widgets::group(tr("Night"), vec![row], Some(footer.to_string()))
    }

    /// "From" and "Until" of the night, each a list of the quarter hours of a day.
    fn control(night: Night) -> Element<'static, Message> {
        let label = |text: &'static str| typo::text(typo::meta(), text).style(theme::text_2);
        row(vec![
            label(tr("From")).into(),
            time_field(night.from, Message::From),
            label(tr("Until")).into(),
            time_field(night.to, Message::To),
        ])
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    }

    /// A time on the quarter hour as a pick list in the look of a secondary button.
    fn time_field(time: Quarter, on_pick: fn(Quarter) -> Message) -> Element<'static, Message> {
        pick_list(Quarter::all().collect::<Vec<_>>(), Some(time), on_pick)
            .padding(style::pick_list_padding())
            .text_size(typo::caption().size)
            .text_line_height(typo::caption().line_height)
            .font(typo::caption().font)
            .style(style::pick_list)
            .menu_style(style::pick_list_menu)
            .into()
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
            let _: Element<'_, Message> = view(&Data::default());
        }
    }
}
