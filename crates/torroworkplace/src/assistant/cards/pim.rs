//! The event and task cards (issue #797, `docs/inhaltskarten.md` "Termin", "Aufgabe"): an item the assistant saved
//! or found, as the calendar and the task lists hold it now.
//!
//! Both are read from the core's cache with the rights of the conversation's client ([`Lookup::reach`]): a
//! collection outside its selection is not there, and neither are its items. Nothing here logs a title.

use torro_ipc::Request;
use torro_ipc::api::Rights;
use torro_ipc::assistant::Card;
use torro_pim::EventTime;

use super::Lookup;
use crate::pim::{PimService, Reach};

/// What the event `uid` (one occurrence of a series with `recurrence_id`) shows now: [`Card::Event`] with the event,
/// its calendar's name and colour, and at most two other events at the same time in the calendars the conversation's
/// client may read (`overlaps`, the earliest first), the rest counted in `more_overlaps`.
///
/// Must: look only in calendars within [`Lookup::rights`] (a calendar out of reach is not there, its events neither
/// as the event nor as an overlap); without `recurrence_id` for a series the next occurrence from now; answer
/// [`Card::Unavailable`] for an event gone or out of reach. Logs no title.
pub async fn look_event(lookup: &Lookup<'_>, uid: &str, recurrence_id: Option<&EventTime>) -> Card {
    // Strand 2e (issue #797) reads the event here.
    let _ = (lookup.pim, lookup.reach(), uid, recurrence_id);
    Card::Unavailable
}

/// What the task `uid` shows now: [`Card::Todo`] with the task and its list's name and colour.
///
/// Looks only in task lists within [`Lookup::rights`]; answers [`Card::Unavailable`] for a task gone or out of
/// reach, and when the client may not list tasks at all. Logs no title.
pub async fn look_todo(lookup: &Lookup<'_>, uid: &str) -> Card {
    todo_card(lookup.pim, lookup.rights, uid).await
}

/// [`look_todo`] on `pim` with `rights`: `tasks.list_todos` as the client would send it, then the task by its uid.
async fn todo_card(pim: &PimService, rights: &Rights, uid: &str) -> Card {
    if !rights.allows_request(&Request::Todos) {
        return Card::Unavailable;
    }
    let reach = Reach::Client(rights);
    let Some(todo) = pim.todos_within(reach).await.into_iter().find(|todo| todo.uid == uid) else {
        return Card::Unavailable;
    };
    // The list is in reach, or the task would not have been listed; a list gone in between is out of reach too.
    let lists = pim.task_lists_within(reach).await;
    let Some(list) = lists.into_iter().find(|list| list.id == todo.collection) else {
        return Card::Unavailable;
    };
    Card::Todo { todo: Box::new(todo), list: list.name, color: list.color }
}

#[cfg(test)]
mod tests {
    use torro_ipc::api::{Access, AreaRights, Level, Selection};
    use torro_pim::{Collection, CollectionKind, Todo};

    use super::*;
    use crate::settings_service::SettingsStore;

    const HOME: &str = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/home/";
    const WORK: &str = "nextcloud:anna@cloud.example.org/https://cloud.example.org/remote.php/dav/calendars/anna/work/";

    fn list(id: &str, name: &str) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            kind: CollectionKind::Todos,
            color: Some("#3FB984".into()),
            writable: true,
            source: None,
        }
    }

    fn todo(uid: &str, collection: &str) -> Todo {
        Todo {
            uid: uid.into(),
            collection: collection.into(),
            href: Some(format!("{collection}{uid}.ics")),
            etag: Some("\"1\"".into()),
            summary: "Water plants".into(),
            due: None,
            priority: None,
            completed: false,
            parent: None,
            description: None,
            url: None,
            categories: Vec::new(),
            sort_order: None,
            link: None,
            focus: Default::default(),
        }
    }

    /// A service without settings file and network, caching the lists Home and Work with one task each.
    async fn service() -> PimService {
        let pim = PimService::with_settings(SettingsStore::without_file());
        pim.cache_collections(vec![list(HOME, "Home"), list(WORK, "Work")]).await;
        pim.cache_todos(vec![todo("plants", HOME), todo("report", WORK)]).await;
        pim
    }

    /// The assistant's rights (tasks readable) on the task lists `lists` only.
    fn rights(lists: &[&str]) -> Rights {
        let task_lists = Selection::Only(lists.iter().map(|id| id.to_string()).collect());
        Rights { task_lists, ..Rights::assistant() }
    }

    #[tokio::test]
    async fn a_task_shows_with_its_list_while_the_client_reaches_it() {
        let pim = service().await;
        let card = todo_card(&pim, &rights(&[HOME, WORK]), "plants").await;
        let Card::Todo { todo, list, color } = card else { panic!("a task card, got {card:?}") };
        assert_eq!((todo.uid.as_str(), list.as_str(), color.as_deref()), ("plants", "Home", Some("#3FB984")));
    }

    #[tokio::test]
    async fn a_task_out_of_reach_or_gone_is_unavailable() {
        let pim = service().await;
        assert_eq!(todo_card(&pim, &rights(&[HOME]), "report").await, Card::Unavailable, "its list is not selected");
        assert_eq!(todo_card(&pim, &rights(&[HOME, WORK]), "gone").await, Card::Unavailable, "no such task");
        let mut no_tasks = rights(&[HOME, WORK]);
        no_tasks.areas.insert(torro_ipc::api::Area::Tasks, AreaRights { level: Level::NoAccess, admin: false });
        assert!(!no_tasks.area(torro_ipc::api::Area::Tasks).allows(Access::Read));
        assert_eq!(todo_card(&pim, &no_tasks, "plants").await, Card::Unavailable, "tasks may not be read");
        let mut denied = rights(&[HOME, WORK]);
        denied.denied.insert("tasks.list_todos".into());
        assert_eq!(todo_card(&pim, &denied, "plants").await, Card::Unavailable, "the command is switched off");
    }
}
