//! The search providers of the tool `web` (issue #305, task WEB.6, concept `docs/assistent.md` "Werkzeug Web"):
//! one group per provider as `tools.list_search_providers` lists them, in the order the core asks them.
//!
//! ```text
//! SearXNG     ● Search words go to your own server              [off]
//!               Address  [https://search.example.org]   Check and save
//!               (the core's reason, when the check failed)
//!             Your own SearXNG instance. Torro checks …           (the provider's notice, in text_2)
//! Brave       ● Search words go to api.search.brave.com · USA   [off]
//!               API key  Key saved   [••••••••]   Save key  Remove key
//!               Terms    api-dashboard.search.brave.com            ↗
//!             Brave allows its results to be used …
//! …
//! ```
//!
//! **Not a section of its own.** Assistant › Settings › Tools shows it under the group of the tool `web`
//! ([`super::tools`]), and the setup's step "Web search" shows the same view (task WEB.7, `docs/einrichtung.md`
//! step 9). So it reads nothing but the providers it is handed and returns a [`crate::Action`] that is generic over
//! the app's part – a page of the control centre maps it as it maps the rights editor's:
//!
//! ```ignore
//! // view: the providers from `tools.list_search_providers`, and the sentence for under the page's own group.
//! let groups = web::view(&state.web, &providers, class).map(Message::Web);
//! let footer = web::summary(&providers);
//! // update: one request to send (`Action::Send`), the terms to open (`Action::OpenUrl`, https only) or nothing.
//! let action: torro_settings_ui::Action<()> = web::update(&mut state.web, message, &providers);
//! // every send of the page came back: `Ok` with SearXNG's address as the providers list it after the reload,
//! // or the error. "Checking…" ends once the address is stored, a refused address shows the core's reason.
//! web::sent(&mut state.web, result.as_ref().map(|providers| web::stored_address(providers)).map_err(String::as_str));
//! // Escape first drops what is typed into a key or the address.
//! web::escape(&state.web)
//! ```
//!
//! **Only a person switches a provider on, sets the address and saves a key** (points 5 and 6 of "Was nie nach
//! außen geht", `docs/api.md`): this view runs with the app key. Each control sends exactly one request – a switch
//! `settings.set_setting search_provider`, the address `searxng_address` (the core checks that the instance answers
//! in JSON before it stores it, and says why not), a key `accounts.set_search_key` – and nothing of `clients.*`.
//!
//! **A key stays in its field.** What is typed into a key's masked field is held only as its value, as a
//! [`Secret`] (whose `Debug` shows `Secret(…)`), in this view's state and in the message that carries it; it goes
//! out once with "Save key" and is dropped right then, also when the core refuses it. Of a stored key the view
//! shows only "Key saved" or "No key" ([`KeyState`]).

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg};
use iced::{Alignment, Element, Fill, Theme};

use torro_core::account::Secret;
use torro_core::i18n::{tr, trc, trf};
use torro_gui::datetime;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status};
use torro_gui::{links, style, typo};
use torro_ipc::web::{KeyState, SearchProviderId, SearchProviderInfo};
use torro_ipc::{Request, SettingKey};

use crate::Action;
use crate::parts::{field, notice, pressable, setting_row, switch};

/// What the view keeps between two loads: what is typed and not yet sent, and whether the address is being checked.
#[derive(Debug, Default)]
pub struct State {
    /// The key typed into one provider's field, until "Save key" sends it; one field at a time.
    key: Option<(SearchProviderId, Secret)>,
    /// The SearXNG address typed and not yet stored; `None` shows the stored one.
    address: Option<String>,
    /// "Check and save" went out and its answer is not back: the button says "Checking…".
    checking: bool,
    /// Why the core refused the address last time (the message of its `unsupported`, `offline` or `busy`), under the
    /// field until the address is typed again.
    problem: Option<String>,
}

/// What the view's controls report.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A provider's switch: on or off, for every client (`search_provider`).
    Switch(SearchProviderId, bool),
    /// What is typed into a provider's key field. A [`Secret`], so no log of messages can show it.
    TypeKey(SearchProviderId, Secret),
    /// "Save key", or Enter in the key field: the typed key goes out and is dropped.
    SaveKey,
    /// "Remove key" at a provider whose key is stored.
    RemoveKey(SearchProviderId),
    /// What is typed into SearXNG's address.
    TypeAddress(String),
    /// "Check and save", or Enter in the address.
    CheckAddress,
    /// "Remove" at a stored SearXNG address.
    RemoveAddress,
    /// "Terms" at a provider: its terms in the browser.
    Terms(SearchProviderId),
    /// Escape while a key or the address is typed: back to what is stored.
    Discard,
}

/// Applies `message`; `providers` are the ones the view shows (`tools.list_search_providers`).
///
/// Returns [`Action::Send`] with exactly one request for a switch, "Save key", "Remove key", "Check and save" and
/// "Remove" (never a `clients.*` request, never two), [`Action::OpenUrl`] with a provider's terms – only an
/// `https` address the core's catalogue named ([`terms_url`]) – and [`Action::None`] for typing, for "Save key"
/// with nothing typed and for "Check and save" while the address is blank or the stored one. The page sends the
/// action and hands every answer of its sends to [`sent`].
pub fn update<A>(state: &mut State, message: Message, providers: &[SearchProviderInfo]) -> Action<A> {
    match message {
        Message::Switch(id, on) => Action::send(switch_request(id, on)),
        Message::TypeKey(id, key) => {
            state.key = Some((id, key));
            Action::None
        }
        // Taken out of the state before it goes, so a refused key is not kept either.
        Message::SaveKey => match state.key.take() {
            Some((id, key)) if !key.is_blank() => Action::send(key_request(id, Some(Secret::new(key.expose().trim())))),
            _ => Action::None,
        },
        Message::RemoveKey(id) => {
            state.key = state.key.take().filter(|(held, _)| *held != id);
            Action::send(key_request(id, None))
        }
        Message::TypeAddress(text) => {
            state.address = Some(text);
            state.problem = None;
            Action::None
        }
        Message::CheckAddress => match checkable(state, providers) {
            Some(address) => {
                let request = address_request(Some(&address));
                state.checking = true;
                state.problem = None;
                Action::send(request)
            }
            None => Action::None,
        },
        Message::RemoveAddress => {
            state.address = None;
            state.problem = None;
            Action::send(address_request(None))
        }
        Message::Terms(id) => match providers.iter().find(|info| info.id == id).and_then(terms_url) {
            Some(url) => Action::OpenUrl(url.to_string()),
            None => Action::None,
        },
        Message::Discard => {
            *state = State { checking: state.checking, ..State::default() };
            Action::None
        }
    }
}

/// A send of the page came back: `Ok` with SearXNG's address as the providers list it after the reload
/// ([`stored_address`]), or the message of its error. While "Check and save" waits: an `Ok` whose stored address is
/// the typed one ends it and the typed address goes; an error ends it and the typed address stays with the core's
/// reason under it. Any other `Ok` is the answer of another send (sends run side by side), so the check still waits.
/// Nothing changes while no check waits. A page whose sends all pass one place (`assistant::received`) hands each
/// one here.
pub fn sent(state: &mut State, result: Result<Option<&str>, &str>) {
    if !state.checking {
        return;
    }
    match result {
        Ok(stored) if stored.is_some() && stored == state.address.as_deref().map(str::trim) => {
            state.checking = false;
            state.address = None;
        }
        Ok(_) => {}
        Err(problem) => {
            state.checking = false;
            state.problem = Some(problem.to_string());
        }
    }
}

/// Whether "Check and save" went out and waits for its answer: a page that shows the core's refusals itself hands
/// an error to [`sent`] while this is `true`, and the shell's notice stays quiet then (the setup, task WEB.7).
pub fn checking(state: &State) -> bool {
    state.checking
}

/// SearXNG's stored address among `providers`, for [`sent`]; `None` while it has none.
pub fn stored_address(providers: &[SearchProviderInfo]) -> Option<&str> {
    providers.iter().find(|info| info.id == SearchProviderId::Searxng).and_then(|info| info.address.as_deref())
}

/// What Escape means here: drop a key or an address being typed. `None` while nothing is typed.
pub fn escape(state: &State) -> Option<Message> {
    (state.key.is_some() || state.address.is_some() || state.problem.is_some()).then_some(Message::Discard)
}

// ----- Requests -----------------------------------------------------------------------------

/// `settings.set_setting search_provider` for `id`: exactly one request per flip of its switch.
pub fn switch_request(id: SearchProviderId, on: bool) -> Request {
    Request::SetSetting {
        key: SettingKey::SearchProvider,
        value: serde_json::json!({ "provider": id.name(), "enabled": on }),
    }
}

/// `accounts.set_search_key` for `id`: `Some` stores the key in the system keychain, `None` removes it.
pub fn key_request(id: SearchProviderId, key: Option<Secret>) -> Request {
    Request::SetSearchKey { provider: id, key }
}

/// `settings.set_setting searxng_address`: `Some` with the address the core checks and stores, `None` (`null`)
/// for none.
pub fn address_request(address: Option<&str>) -> Request {
    let value = address.map_or(serde_json::Value::Null, |address| serde_json::Value::String(address.to_string()));
    Request::SetSetting { key: SettingKey::SearxngAddress, value }
}

/// The typed address, trimmed, when "Check and save" has something to send: not blank and not the stored one.
fn checkable(state: &State, providers: &[SearchProviderInfo]) -> Option<String> {
    let typed = state.address.as_deref()?.trim();
    (!typed.is_empty() && Some(typed) != stored_address(providers)).then(|| typed.to_string())
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The sentence about the order and the approval, for under the page's own group: "Asked in this order: SearXNG,
/// DuckDuckGo. Wikipedia is asked as well. The assistant asks once in each conversation before it searches." – the
/// switched-on providers as listed (the core's order; one alone is just named), Wikipedia apart since it is asked
/// as well and is no fallback; with none on, that the web search is off.
pub fn summary(providers: &[SearchProviderInfo]) -> String {
    let wikipedia = providers.iter().any(|info| info.enabled && info.id == SearchProviderId::Wikipedia);
    let asked: Vec<&str> = providers
        .iter()
        .filter(|info| info.enabled && info.id != SearchProviderId::Wikipedia)
        .map(|info| info.name.as_str())
        .collect();
    let names = asked.join(", ");
    let order = match (asked.as_slice(), wikipedia) {
        ([], false) => tr("No provider is on, so the web search is off.").to_string(),
        ([], true) => tr("Only Wikipedia is asked.").to_string(),
        ([one], _) => trf("{provider} is asked.", &[("provider", one)]),
        _ => trf("Asked in this order: {providers}.", &[("providers", &names)]),
    };
    let order = match (asked.is_empty(), wikipedia) {
        (false, true) => format!("{order} {}", tr("Wikipedia is asked as well.")),
        _ => order,
    };
    format!("{order} {}", tr("The assistant asks once in each conversation before it searches."))
}

/// The address of `info`'s terms, when it may be opened: `https` only, as the core's catalogue names it, never a
/// scheme of its own or plain `http`.
pub fn terms_url(info: &SearchProviderInfo) -> Option<&str> {
    let url = info.terms.as_str();
    (url.starts_with("https://") && links::openable(url)).then_some(url)
}

/// Where the search words go: "Search words go to html.duckduckgo.com · USA"; SearXNG's own server while it has
/// no address.
pub fn sends_to(info: &SearchProviderInfo) -> String {
    match (info.sends_to.is_empty(), info.seat.is_empty()) {
        (true, _) => tr("Search words go to your own server").to_string(),
        (false, true) => trf("Search words go to {host}", &[("host", &info.sends_to)]),
        (false, false) => trf("Search words go to {host} · {seat}", &[("host", &info.sends_to), ("seat", &info.seat)]),
    }
}

/// Why a switched-on provider is not asked now, for under its switch: it rests after a block or a limit, or it
/// lacks its key or address. `None` for one that is off or is asked.
pub fn standing(info: &SearchProviderInfo) -> Option<String> {
    if !info.enabled {
        return None;
    }
    if let Some(until) = info.paused_until {
        let time = datetime::format_time(datetime::to_local(until).time());
        return Some(trf("Paused until {time} after a block or a limit", &[("time", &time)]));
    }
    if info.needs_key && info.key == KeyState::Missing {
        return Some(tr("Skipped until a key is saved").to_string());
    }
    if info.id == SearchProviderId::Searxng && info.address.is_none() {
        return Some(tr("Skipped until an address is saved").to_string());
    }
    None
}

/// The dot before a provider's switch: grey while off, green while it is asked, orange while it is on and not
/// asked ([`standing`] says why, so the colour is never the only sign).
fn status(info: &SearchProviderInfo) -> Status {
    match (info.enabled, standing(info)) {
        (false, _) => Status::Inactive,
        (true, None) => Status::Ok,
        (true, Some(_)) => Status::Attention,
    }
}

// ----- Views --------------------------------------------------------------------------------

/// A group per provider, in the order of `providers`. In a narrow window where the words go wraps instead of being
/// cut, a field stands on a line of its own under its label and the buttons are icons with a tooltip, so the
/// fields keep their room (design skill, section 6). Without providers (an answer that did not come) one row
/// says there is nothing to choose from.
pub fn view<'a>(state: &'a State, providers: &'a [SearchProviderInfo], class: SizeClass) -> Element<'a, Message> {
    if providers.is_empty() {
        let row = setting_row(None, tr("Nothing to choose from yet."), None, Space::new());
        return widgets::group(tr("Search providers"), vec![row], None);
    }
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    column(providers.iter().map(|info| provider_group(state, info, words))).spacing(layout::section_gap()).into()
}

/// One provider: its switch with where the words go, its address or key, its terms; its notice under the group.
fn provider_group<'a>(state: &'a State, info: &'a SearchProviderInfo, words: bool) -> Element<'a, Message> {
    let id = info.id;
    let dot = widgets::status_dot(status(info));
    let on = switch(info.enabled, move |on| Message::Switch(id, on));
    let mut rows = vec![match words {
        true => setting_row(Some(dot), sends_to(info), standing(info), on),
        false => wrapping_row(dot, sends_to(info), standing(info), on),
    }];
    if id == SearchProviderId::Searxng {
        rows.push(address_row(state, info, words));
        if let Some(problem) = &state.problem {
            rows.push(container(notice(problem.clone())).width(Fill).padding([space::m(), space::l()]).into());
        }
    }
    if info.needs_key {
        rows.push(key_row(state, info, words));
    }
    if let Some(url) = terms_url(info) {
        rows.push(terms_row(id, url));
    }
    widgets::group(info.name.as_str(), rows, info.notice.clone())
}

/// SearXNG's address: the field with what is stored or typed, "Check and save" – always there, so the field keeps
/// its width; it can be pressed while the typed address differs – and "Remove" while one is stored.
fn address_row<'a>(state: &'a State, info: &'a SearchProviderInfo, words: bool) -> Element<'a, Message> {
    let stored = info.address.as_deref();
    let value = state.address.as_deref().or(stored).unwrap_or_default();
    let input =
        field("https://search.example.org", value).on_input(Message::TypeAddress).on_submit(Message::CheckAddress);
    let check = state.address.as_deref().map(str::trim).is_some_and(|typed| !typed.is_empty() && Some(typed) != stored);
    let check = check.then_some(Message::CheckAddress);
    let mut buttons = row![].spacing(space::s()).align_y(Alignment::Center);
    buttons = buttons.push(match (state.checking, words) {
        (true, _) => widgets::busy_button(tr("Checking…"), style::button::secondary),
        (false, true) => widgets::text_button(None, tr("Check and save"), style::button::secondary, check),
        (false, false) => widgets::icon_button(Icon::Check, tr("Check and save"), check),
    });
    if stored.is_some() {
        buttons = buttons.push(remove_button(tr("Remove"), Message::RemoveAddress));
    }
    let control = row![input, buttons].spacing(space::s()).align_y(Alignment::Center);
    field_row(tr("Address"), None, control, words)
}

/// A provider's key: "Key saved" or "No key" under the label, the masked field, "Save key" (it can be pressed once
/// something is typed) and "Remove key" while one is stored.
fn key_row<'a>(state: &'a State, info: &'a SearchProviderInfo, words: bool) -> Element<'a, Message> {
    let id = info.id;
    let typed = state.key.as_ref().filter(|(held, _)| *held == id).map_or("", |(_, key)| key.expose());
    let input = field(tr("Paste the key"), typed)
        .secure(true)
        .on_input(move |text| Message::TypeKey(id, Secret::new(text)))
        .on_submit(Message::SaveKey);
    let save = (!typed.trim().is_empty()).then_some(Message::SaveKey);
    let mut buttons = row![match words {
        true => widgets::text_button(None, tr("Save key"), style::button::secondary, save),
        false => widgets::icon_button(Icon::Check, tr("Save key"), save),
    }]
    .spacing(space::s())
    .align_y(Alignment::Center);
    if info.key == KeyState::Stored {
        buttons = buttons.push(remove_button(tr("Remove key"), Message::RemoveKey(id)));
    }
    // "No key" is also a keyboard's, said otherwise in German.
    let detail = match info.key {
        KeyState::Stored => tr("Key saved"),
        KeyState::Missing | KeyState::NotNeeded => trc("api key", "No key"),
    };
    let control = row![input, buttons].spacing(space::s()).align_y(Alignment::Center);
    field_row(tr("API key"), Some(detail.to_string()), control, words)
}

/// A row with a field: beside its label where there is room (`wide`), else under it on the row's whole width, so
/// what is typed is not squeezed into half of a narrow window.
fn field_row<'a>(
    label: &'a str,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
    wide: bool,
) -> Element<'a, Message> {
    if wide {
        return setting_row(None, label, detail, control);
    }
    let mut lines = column![widgets::ellipsized(typo::body(), label, theme::text)];
    if let Some(detail) = detail {
        lines = lines.push(widgets::ellipsized(typo::meta(), detail, theme::text_2));
    }
    let content = column![lines, control.into()].spacing(space::s());
    container(content).width(Fill).padding([space::m(), space::l()]).into()
}

/// The switch's row in a narrow window: where the words go wraps instead of being cut ("… · USA" would be lost
/// behind the "…"). With one line of text as high as a [`setting_row`]; it grows with each further line.
fn wrapping_row<'a>(
    mark: Element<'a, Message>,
    label: String,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    // A strut as high as the row inside its padding (2 × 8 = 16), so one line gives the height of the rows around.
    let tall = detail.is_some();
    let strut =
        Space::new().height(if tall { size::header() + space::m() - space::l() } else { size::header() - space::l() });
    let mut lines = column![typo::text(typo::body(), label).style(theme::text).wrapping(Wrapping::WordOrGlyph)];
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let marked = row![mark, lines.width(Fill)].spacing(space::m()).align_y(Alignment::Center);
    let text = row![strut, marked].align_y(Alignment::Center);
    let content = row![text.width(Fill), control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding([space::s(), space::l()]).into()
}

/// "Remove" or "Remove key", always with its word: a ✕ beside the field would read as "clear the field" and remove
/// what is stored.
fn remove_button<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    widgets::text_button(None, label, style::button::ghost, Some(message))
}

/// "Terms" with the host they are on, and the arrow of a page outside the suite; the whole row opens them.
fn terms_row<'a>(id: SearchProviderId, url: &str) -> Element<'a, Message> {
    let host = url.trim_start_matches("https://").split('/').next().unwrap_or_default().to_string();
    let arrow =
        icon::icon(Icon::ArrowUpRight, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    pressable(setting_row(None, tr("Terms"), Some(host), arrow), Some(Message::Terms(id)))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use torro_core::settings::SearchProviderSwitch;
    use torro_ipc::api::Area;

    use super::*;

    type Act = Action<()>;

    /// A provider as the core lists it, off, with the facts of its catalogue entry.
    fn provider(id: SearchProviderId) -> SearchProviderInfo {
        let (sends_to, seat, terms, needs_key) = match id {
            SearchProviderId::Searxng => ("", "", "", false),
            SearchProviderId::Brave => ("api.search.brave.com", "USA", "https://example.org/brave-terms", true),
            SearchProviderId::Duckduckgo => ("html.duckduckgo.com", "USA", "https://duckduckgo.com/terms", false),
            _ => ("example.org", "USA", "https://example.org/terms", false),
        };
        SearchProviderInfo {
            id,
            name: id.title().to_string(),
            sends_to: sends_to.into(),
            seat: seat.into(),
            terms: terms.into(),
            notice: None,
            needs_key,
            key: if needs_key { KeyState::Missing } else { KeyState::NotNeeded },
            address: None,
            enabled: false,
            paused_until: None,
        }
    }

    fn all() -> Vec<SearchProviderInfo> {
        SearchProviderId::ORDER.into_iter().map(provider).collect()
    }

    /// The one request of an action.
    fn one(action: Act) -> Request {
        match action {
            Action::Send(mut requests) if requests.len() == 1 => requests.remove(0),
            other => panic!("expected exactly one request, got {other:?}"),
        }
    }

    /// A switch sends exactly one `set_setting search_provider`, both ways, which the core reads back.
    #[test]
    fn a_switch_sends_exactly_one_set_setting() {
        let mut state = State::default();
        for on in [true, false] {
            let request = one(update(&mut state, Message::Switch(SearchProviderId::Duckduckgo, on), &all()));
            let Request::SetSetting { key: SettingKey::SearchProvider, value } = &request else {
                panic!("expected search_provider, got {request:?}")
            };
            let switch: SearchProviderSwitch = serde_json::from_value(value.clone()).expect("the core reads it");
            assert_eq!(switch, SearchProviderSwitch { provider: SearchProviderId::Duckduckgo, enabled: on });
        }
    }

    /// A key goes out once as `accounts.set_search_key` – never as a setting – and is dropped as it goes; nothing
    /// typed sends nothing; "Remove key" sends `null`. The state and the messages never show it.
    #[test]
    fn a_key_goes_out_once_and_is_never_kept() {
        let providers = all();
        let mut state = State::default();
        let typed = Message::TypeKey(SearchProviderId::Brave, Secret::new(" BSA-secret-123 "));
        assert_eq!(update::<()>(&mut state, typed.clone(), &providers), Action::None, "typing sends nothing");
        assert!(!format!("{state:?}{typed:?}").contains("BSA-secret"), "no Debug shows the key");
        assert_eq!(escape(&state), Some(Message::Discard));
        let request = one(update(&mut state, Message::SaveKey, &providers));
        assert_eq!(
            request,
            Request::SetSearchKey { provider: SearchProviderId::Brave, key: Some(Secret::new("BSA-secret-123")) }
        );
        assert_eq!(request.operation().area(), Area::Accounts);
        assert!(!format!("{request:?}").contains("BSA-secret"));
        assert!(state.key.is_none() && escape(&state).is_none(), "dropped as it goes");
        assert_eq!(update::<()>(&mut state, Message::SaveKey, &providers), Action::None, "nothing left to save");
        let blank = Message::TypeKey(SearchProviderId::Brave, Secret::new("   "));
        let _ = update::<()>(&mut state, blank, &providers);
        assert_eq!(update::<()>(&mut state, Message::SaveKey, &providers), Action::None, "a blank key is not sent");
        assert_eq!(
            one(update(&mut state, Message::RemoveKey(SearchProviderId::Brave), &providers)),
            Request::SetSearchKey { provider: SearchProviderId::Brave, key: None }
        );
    }

    /// "Check and save" sends the trimmed address once; while it waits the button says so; a refusal keeps the
    /// address with the core's reason, a success drops the typed one. Blank or stored sends nothing; "Remove"
    /// sends `null`.
    #[test]
    fn the_address_is_checked_by_the_core_and_its_reason_shown() {
        let mut providers = all();
        let mut state = State::default();
        let _ = update::<()>(&mut state, Message::TypeAddress("  https://search.example.org ".into()), &providers);
        let request = one(update(&mut state, Message::CheckAddress, &providers));
        assert_eq!(
            request,
            Request::SetSetting {
                key: SettingKey::SearxngAddress,
                value: serde_json::json!("https://search.example.org")
            }
        );
        assert!(state.checking);
        sent(&mut state, Ok(None));
        assert!(state.checking, "the answer of another send: the check still waits");
        sent(&mut state, Err("This SearXNG instance does not answer in JSON."));
        assert_eq!(state.problem.as_deref(), Some("This SearXNG instance does not answer in JSON."));
        assert!(state.address.is_some() && !state.checking, "kept to be corrected");
        sent(&mut state, Err("another send"));
        assert_eq!(state.problem.as_deref(), Some("This SearXNG instance does not answer in JSON."), "not the check's");
        let _ = update::<()>(&mut state, Message::TypeAddress("https://search.example.org".into()), &providers);
        assert!(state.problem.is_none(), "typing again clears the reason");
        let _ = one(update(&mut state, Message::CheckAddress, &providers));
        sent(&mut state, Ok(Some("https://search.example.org")));
        assert!(state.address.is_none() && state.problem.is_none() && !state.checking);

        providers[0].address = Some("https://search.example.org".into());
        for typed in ["   ", "https://search.example.org"] {
            let _ = update::<()>(&mut state, Message::TypeAddress(typed.into()), &providers);
            assert_eq!(update::<()>(&mut state, Message::CheckAddress, &providers), Action::None, "{typed:?}");
        }
        assert_eq!(
            one(update(&mut state, Message::RemoveAddress, &providers)),
            Request::SetSetting { key: SettingKey::SearxngAddress, value: serde_json::Value::Null }
        );
        let _ = update::<()>(&mut state, Message::TypeAddress("x".into()), &providers);
        assert_eq!(update::<()>(&mut state, Message::Discard, &providers), Action::None);
        assert!(escape(&state).is_none());
    }

    /// No control of the view reaches the clients: switches and the address are settings, keys are accounts, the
    /// terms open the browser – an `https` address of the catalogue only.
    #[test]
    fn nothing_reaches_the_clients_and_only_https_terms_open() {
        let mut providers = all();
        let mut state = State::default();
        let messages = [
            Message::Switch(SearchProviderId::Searxng, true),
            Message::TypeKey(SearchProviderId::Exa, Secret::new("k")),
            Message::SaveKey,
            Message::RemoveKey(SearchProviderId::Exa),
            Message::TypeAddress("https://s.example.org".into()),
            Message::CheckAddress,
            Message::RemoveAddress,
            Message::Terms(SearchProviderId::Duckduckgo),
            Message::Discard,
        ];
        for message in messages {
            match update::<()>(&mut state, message.clone(), &providers) {
                Action::Send(requests) => {
                    assert_eq!(requests.len(), 1, "{message:?}");
                    assert!(matches!(requests[0].operation().area(), Area::Settings | Area::Accounts), "{message:?}");
                }
                Action::OpenUrl(url) => assert_eq!(url, "https://duckduckgo.com/terms"),
                Action::None => {}
                other => panic!("{message:?} did {other:?}"),
            }
        }
        providers[5].terms = "http://duckduckgo.com/terms".into();
        assert_eq!(update::<()>(&mut state, Message::Terms(SearchProviderId::Duckduckgo), &providers), Action::None);
        providers[5].terms = "javascript:alert(1)".into();
        assert!(terms_url(&providers[5]).is_none());
        assert!(terms_url(&providers[0]).is_none(), "SearXNG has none");
    }

    /// The sentence under the group names the order of the switched-on ones, Wikipedia apart, and the approval.
    #[test]
    fn the_summary_says_the_order_and_the_approval() {
        let ask = tr("The assistant asks once in each conversation before it searches.");
        let mut providers = all();
        assert_eq!(summary(&providers), format!("{} {ask}", tr("No provider is on, so the web search is off.")));
        providers[6].enabled = true;
        assert_eq!(summary(&providers), format!("{} {ask}", tr("Only Wikipedia is asked.")));
        providers[0].enabled = true;
        providers[5].enabled = true;
        let names = "SearXNG, DuckDuckGo";
        let order = trf("Asked in this order: {providers}.", &[("providers", &names)]);
        let also = tr("Wikipedia is asked as well.");
        assert_eq!(summary(&providers), format!("{order} {also} {ask}"));
        providers[6].enabled = false;
        assert_eq!(summary(&providers), format!("{order} {ask}"));
        providers[0].enabled = false;
        let one = trf("{provider} is asked.", &[("provider", &"DuckDuckGo")]);
        assert_eq!(summary(&providers), format!("{one} {ask}"));
    }

    /// Where the words go and why a switched-on provider is passed over, in words beside the dot's colour.
    #[test]
    fn each_provider_says_where_the_words_go_and_how_it_stands() {
        let mut providers = all();
        assert_eq!(sends_to(&providers[0]), tr("Search words go to your own server"));
        assert_eq!(
            sends_to(&providers[5]),
            trf("Search words go to {host} · {seat}", &[("host", &"html.duckduckgo.com"), ("seat", &"USA")])
        );
        providers[0].sends_to = "192.168.1.20".into();
        assert_eq!(sends_to(&providers[0]), trf("Search words go to {host}", &[("host", &"192.168.1.20")]));
        assert!(providers.iter().all(|info| standing(info).is_none()), "off says nothing");
        assert!(providers.iter().all(|info| status(info) == Status::Inactive));
        for info in &mut providers {
            info.enabled = true;
        }
        assert_eq!(standing(&providers[0]).as_deref(), Some(tr("Skipped until an address is saved")));
        assert_eq!(standing(&providers[1]).as_deref(), Some(tr("Skipped until a key is saved")));
        assert_eq!((standing(&providers[5]), status(&providers[5])), (None, Status::Ok));
        providers[5].paused_until = Utc.with_ymd_and_hms(2026, 10, 24, 14, 30, 0).single();
        let paused = standing(&providers[5]).expect("paused");
        assert!(
            paused.contains(&datetime::format_time(datetime::to_local(providers[5].paused_until.expect("set")).time()))
        );
        assert_eq!(status(&providers[5]), Status::Attention);
        providers[1].key = KeyState::Stored;
        assert_eq!(status(&providers[1]), Status::Ok);
    }

    /// Every provider builds in every size, with a key and an address typed, while checking and with a refusal.
    #[test]
    fn the_view_builds_in_every_state() {
        let mut providers = all();
        providers[0].address = Some("https://search.example.org".into());
        providers[1].key = KeyState::Stored;
        providers[5].enabled = true;
        providers[5].paused_until = Some(Utc::now());
        let mut typing = State::default();
        let _ = update::<()>(&mut typing, Message::TypeKey(SearchProviderId::Brave, Secret::new("k")), &providers);
        let _ = update::<()>(&mut typing, Message::TypeAddress("https://other.example.org".into()), &providers);
        let _ = update::<()>(&mut typing, Message::CheckAddress, &providers);
        let problem = Some("This SearXNG instance cannot be reached. Check the address.".to_string());
        let refused = State { problem, ..State::default() };
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&State::default(), &typing, &refused] {
                let _ = view(state, &providers, class);
                let _ = view(state, &[], class);
            }
        }
    }
}
