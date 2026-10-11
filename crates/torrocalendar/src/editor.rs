//! Creating and editing an event (task K.12 in `docs/tasks/kalender-aufgaben.md`).
//!
//! A card over the calendar with: title, calendar (pick list), all-day toggle, start date
//! and time, end date and time (German input, `torro_gui::datetime::parse_*`), location,
//! notes; buttons Speichern, Abbrechen and (for existing events) Löschen. Invalid input
//! shows a short hint and disables Speichern. Recurring occurrences (`event.recurring`)
//! open read-only with a note that series cannot be edited yet. Escape closes.

use chrono::{NaiveDate, NaiveTime};
use iced::Element;
use iced::widget::{button, column, text};
use torro_pim::{Collection, Event};

/// The editor's state.
#[derive(Debug, Clone)]
pub struct Editor {
    /// The event being edited; `href: None` for a new one.
    pub original: Option<Event>,
    pub calendars: Vec<Collection>,
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Cancel,
}

/// What the app should do after an update.
#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save(Event),
    Delete(Event),
    Close,
}

impl Editor {
    /// A new event on `date` at `time` (one hour long), or all-day with `None`, in the first
    /// of `calendars`.
    pub fn new_event(calendars: &[Collection], date: NaiveDate, time: Option<NaiveTime>) -> Editor {
        Editor { original: None, calendars: calendars.to_vec(), date, time }
    }

    pub fn edit(event: &Event, calendars: &[Collection]) -> Editor {
        Editor {
            original: Some(event.clone()),
            calendars: calendars.to_vec(),
            date: chrono::Local::now().date_naive(),
            time: None,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Cancel => Action::Close,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let title = self.original.as_ref().map_or("Neuer Termin".to_string(), |e| e.summary.clone());
        column![text(title).size(18), button(text("Abbrechen")).on_press(Message::Cancel)]
            .spacing(12)
            .padding(20)
            .into()
    }
}
