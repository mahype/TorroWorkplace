//! Assistant › History (task S.17; it was the group "Keep conversations" of the control centre's
//! Assistant page, task C.1, decision 8 in `docs/assistent.md`): how long the assistant keeps a
//! conversation after its last message.
//!
//! 30 days, 90 days, a year or forever (`settings.set_setting assistant_retention`), and a number
//! of days set through the API is listed among them in order ([`retention_options`]). Pinned
//! conversations are kept whatever is chosen.

use iced::Element;

use torro_core::i18n::{tr, trf};
use torro_core::settings::Retention;
use torro_gui::widgets;
use torro_ipc::{Request, SettingKey};

use super::Action;
use crate::Data;
use crate::parts::choice;

/// The retention times the section offers, shortest first.
const RETENTIONS: [Retention; 4] = [Retention::Days(30), Retention::Days(90), Retention::Days(365), Retention::Forever];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A row of "Keep conversations".
    Retention(Retention),
}

pub fn update(message: Message, _data: &Data) -> Action {
    match message {
        Message::Retention(retention) => {
            Action::send(Request::SetSetting { key: SettingKey::AssistantRetention, value: retention.to_value() })
        }
    }
}

// ----- Views --------------------------------------------------------------------------------

/// "Keep conversations": a row per retention time, a check at the stored one.
pub fn view(data: &Data) -> Element<'_, Message> {
    if !super::is_on(data) {
        return super::off_group(tr("History"));
    }
    let current = data.settings.assistant_retention;
    let rows = retention_options(current)
        .into_iter()
        .map(|retention| choice(retention_label(retention), retention == current, Message::Retention(retention)))
        .collect();
    let footer = tr("Conversations are deleted this long after their last message. Pinned ones are kept.");
    widgets::group(tr("Keep conversations"), rows, Some(footer.to_string()))
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The rows of "Keep conversations": the four times above, and `current` among them in order when it is
/// another number of days (set through the API).
pub fn retention_options(current: Retention) -> Vec<Retention> {
    let mut options = RETENTIONS.to_vec();
    if !options.contains(&current) {
        let at = options.iter().position(|option| option.days().is_none_or(|days| Some(days) > current.days()));
        options.insert(at.unwrap_or(options.len()), current);
    }
    options
}

/// "30 days", "1 year", "Forever".
pub fn retention_label(retention: Retention) -> String {
    match retention {
        Retention::Days(365) => tr("1 year").to_string(),
        Retention::Days(days) => trf("{n} days", &[("n", &days)]),
        Retention::Forever => tr("Forever").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{data, turned_off};
    use super::*;
    use crate::Action as Act;

    #[tokio::test]
    async fn keeping_conversations_sends_set_setting() {
        let data = data().await;
        for (retention, value) in
            [(Retention::Days(90), serde_json::json!(90)), (Retention::Forever, serde_json::json!("forever"))]
        {
            let action = update(Message::Retention(retention), &data);
            let Act::Send(requests) = action else { panic!("expected a request") };
            assert_eq!(
                requests,
                vec![Request::SetSetting { key: SettingKey::AssistantRetention, value }],
                "{retention:?}"
            );
        }
    }

    #[test]
    fn retention_rows_include_a_number_set_elsewhere_in_order() {
        assert_eq!(retention_options(Retention::Days(30)), RETENTIONS);
        let custom = retention_options(Retention::Days(14));
        assert_eq!(custom[0], Retention::Days(14));
        let long = retention_options(Retention::Days(500));
        assert_eq!(long[3], Retention::Days(500), "after a year, before forever: {long:?}");
        assert_eq!(long[4], Retention::Forever);
        assert_eq!(retention_label(Retention::Days(30)), trf("{n} days", &[("n", &30)]));
        assert_eq!(retention_label(Retention::Days(365)), tr("1 year"));
        assert_eq!(retention_label(Retention::Forever), tr("Forever"));
    }

    #[tokio::test]
    async fn the_section_builds_on_and_off() {
        let data = data().await;
        let _ = view(&data);
        let _ = view(&Data {
            settings: torro_core::settings::Settings {
                assistant_retention: Retention::Days(14),
                ..data.settings.clone()
            },
            ..data.clone()
        });
        let _ = view(&turned_off(&data));
    }
}
