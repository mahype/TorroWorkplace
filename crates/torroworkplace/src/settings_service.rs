//! The core's side of the settings: reading, saving and applying them.
//!
//! Task S.1 in `docs/tasks/steuerzentrale.md`. `torro_core::settings` holds the type and the
//! file; this module applies changes: start at login (task X.3). It is one entry per system, all
//! starting the running program with `daemon`: an XDG autostart entry on Linux
//! (`~/.config/autostart/torro-workplace.desktop`), a LaunchAgent on macOS
//! (`~/Library/LaunchAgents/io.github.mahype.torro-workplace.plist`) and the value
//! `TorroWorkplace` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows.
//! Hidden collections are applied by `pim.rs`, which reads [`SettingsStore::current`] on every
//! request. A design (task L.3), a font and a colour scheme (task L.22) are checked against the rules
//! of `torro_core::design` before they are stored, each in its own folder; the apps follow the
//! settings themselves. Choosing another design with `set_setting` also sets the font and the mail
//! view it suggests, in the same write ([`DesignSuggestions`], task L.33); whole settings never do.
//!
//! The file is not looked up here but handed in ([`SettingsStore`]): the daemon uses the user's
//! `settings.json`, tests a file in a temp directory, so no test reads or writes the real one
//! (task A.0d). Likewise only the daemon's store applies "start at login" to the system: a test
//! store keeps the setting in its file only, so no test points the user's login item at a test
//! program (task X.12).
//!
//! Changes go through one lock: requests run concurrently, and two targeted changes
//! ([`SettingsStore::set`], [`SettingsStore::set_collection_shown`]) must not lose each other in
//! read, change, write.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use torro_core::design::{self, DesignError, Part};
use torro_core::settings::{
    self, AssistantDefault, AttachmentClick, ComponentSwitch, FolderChange, Language, ListOrderEntry, MailAppearance,
    MailAttachments, MailFormat, Maps, Night, PdfSettings, PdfTemplate, PdfTemplateId, Reminders, Retention,
    SearchProviderSwitch, Settings, SetupState, ToolSwitch, Voice, WebSearch, WorkingHours, Zoom,
};
use torro_core::theme::Appearance;
use torro_ipc::SettingKey;

/// Held while the settings file is read, changed and written. One lock for every store: in
/// production there is only the one file, and serialising test stores costs nothing.
static WRITING: Mutex<()> = Mutex::new(());

/// The settings file the core reads and changes. Cheap to clone; the core and the PIM service
/// hold one each for the same file (`Core::new` takes the PIM service's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsStore {
    /// `None` without a home directory: reads give the defaults, changes fail.
    file: Option<PathBuf>,
    /// The folder of the design packs (`torro_core::design::dir`), where a chosen design other than
    /// the built-in ones (`torro_core::design::BUILT_IN_IDS`) must be found and valid; `None` without
    /// a home directory, then only the built-in designs can be chosen.
    designs: Option<PathBuf>,
    /// The folder of the fonts (`torro_core::design::font_dir`), where a chosen font must be found and
    /// valid; `None` without a home directory, then only the standard can be chosen.
    fonts: Option<PathBuf>,
    /// The folder of the colour schemes (`torro_core::design::color_scheme_dir`), like
    /// [`SettingsStore::fonts`].
    color_schemes: Option<PathBuf>,
    /// Whether a change of `start_at_login` is applied to the system's login item. Only the
    /// daemon's store does: the entry points at the running program, and from a test that is the
    /// test binary, which the next login would start instead of the daemon (task X.12).
    applies_login_item: bool,
}

impl SettingsStore {
    /// The user's `settings.json` (`torro_core::settings::settings_file`), as the daemon uses it.
    /// Changing `start_at_login` here writes or removes the system's login item.
    pub fn system() -> SettingsStore {
        SettingsStore {
            file: settings::settings_file(),
            designs: design::dir(),
            fonts: design::font_dir(),
            color_schemes: design::color_scheme_dir(),
            applies_login_item: true,
        }
    }

    /// The settings in `file`, for tests. `start_at_login` is only stored in `file`; the
    /// system's login item is never touched, so tests may switch it freely. The designs, fonts and
    /// colour schemes are the folders `designs`, `fonts` and `color-schemes` next to `file`.
    #[cfg(test)]
    pub fn at(file: PathBuf) -> SettingsStore {
        let beside = |name: &str| file.parent().map(|dir| dir.join(name));
        SettingsStore {
            designs: beside("designs"),
            fonts: beside("fonts"),
            color_schemes: beside("color-schemes"),
            file: Some(file),
            applies_login_item: false,
        }
    }

    /// No file at all, for tests that do not look at the settings: reads give the defaults,
    /// changes fail. Like [`SettingsStore::at`], it never touches the system's login item.
    #[cfg(test)]
    pub fn without_file() -> SettingsStore {
        SettingsStore { file: None, designs: None, fonts: None, color_schemes: None, applies_login_item: false }
    }

    /// The folder of the design packs, for `settings.list_designs`; `None` without a home
    /// directory (only the built-in designs exist then).
    pub fn designs(&self) -> Option<&Path> {
        self.designs.as_deref()
    }

    /// The folder of the fonts, for `settings.list_fonts` (task L.22); `None` without a home
    /// directory (only the standard exists then).
    pub fn fonts(&self) -> Option<&Path> {
        self.fonts.as_deref()
    }

    /// The folder of the colour schemes, for `settings.list_color_schemes` (task L.22); `None`
    /// without a home directory (only the standard exists then).
    pub fn color_schemes(&self) -> Option<&Path> {
        self.color_schemes.as_deref()
    }

    /// The folder of `part`.
    fn folder(&self, part: Part) -> Option<&Path> {
        match part {
            Part::Design => self.designs(),
            Part::Font => self.fonts(),
            Part::ColorScheme => self.color_schemes(),
        }
    }

    /// The settings as stored now; the defaults when there is no file or it cannot be read.
    pub fn current(&self) -> Settings {
        self.file.as_deref().map(load_from).unwrap_or_default()
    }

    /// Saves `new` and applies what changed compared to the stored settings.
    ///
    /// A hidden collection named by a bare URL, as before several accounts existed, is refused
    /// and nothing is written (task M1.4): the old ids were rewritten once when the core started,
    /// and one coming back later would otherwise belong to no account, or by chance to the first.
    /// So is a PDF template or folder that changes and does not fit, as `set_setting` refuses it
    /// (`torro_core::settings::PdfSettings::check_changes`).
    ///
    /// `decide_mail_to_cloud` (issue #325) is never stored unset: `new` without it – from an app
    /// before the slots – keeps it as stored, since unset would take the old AI values over again at
    /// the next load (`Settings::take_over_old_ai`).
    pub fn update(&self, new: Settings) -> Result<(), String> {
        let ids = new.hidden_collections.iter().chain(new.list_orders.keys());
        if let Some(bare) = ids.into_iter().find(|id| torro_pim::account::is_legacy_id(id)) {
            return Err(format!("{bare:?} is not a collection id; use the ids from accounts.list_accounts"));
        }
        self.change(|settings| {
            let stored = settings.mail_to_cloud_in(torro_core::ai::AiSlot::Decide);
            let decide_mail_to_cloud = Some(new.decide_mail_to_cloud.unwrap_or(stored));
            *settings = Settings { decide_mail_to_cloud, ..new }
        })
    }

    /// Like [`SettingsStore::update`], but the hidden collections, the orders of the task lists and
    /// the reminder styles of calendars (task ER1.2) stay as stored, read under the same lock as the write, so a collection hidden
    /// meanwhile is not shown again. For clients, which never see the hidden collections nor the
    /// orders of lists outside their selection, and so cannot send them back; they change those
    /// with `set_collection_shown` and `set_setting`.
    ///
    /// Nor does a client switch the microphone on (point 6 of "Was nie nach außen geht",
    /// task C.9): voice stays off when it is off as stored now, whatever `new` says. The core
    /// refuses such a request before (`forbidden`); this keeps it so when a person switched voice
    /// off between that check and the write. The same holds for letting mail go to cloud models
    /// (`mail_to_cloud`, task C.17): a client may keep it local, never let it out; and for the ways
    /// in (`allow_api_keys`, `allow_mcp`, #231): a client may close them, never open them.
    ///
    /// Nor which model does which function (`ai_assignments`, task E12.5): those are checked
    /// against the accounts, the models and the data rule when they are set
    /// (`Core::set_ai_assignment`), and a whole-settings write would go round that check. They
    /// change only through [`SettingsStore::set_ai_assignment`]. Nor the model of a slot
    /// (`ai_slots`, issue #325): `Core::set_ai_slot` checks it, and it changes only through
    /// [`SettingsStore::set_ai_slot`]. The permission of the slot Sorting (`decide_mail_to_cloud`)
    /// stays on only when it is on as stored and in `new`, like `mail_to_cloud`; a client from before
    /// the slots, which sends none, switches it off – never back to unset, which would take the old
    /// AI values over again at the next load (`Settings::take_over_old_ai`).
    ///
    /// Nor does a client switch on one of the assistant's own tools (point 9, issue #225): a tool
    /// stays on only when it is on as stored and in `new`, the PDF folder and the logos of the
    /// templates stay as stored, and so do the allowed folders (issue #304,
    /// `torro_core::settings::Tools::from_client`); the other fields of the templates come from `new`,
    /// refused like in [`SettingsStore::update`] when they do not fit.
    ///
    /// Nor does a client turn maps on or name another search or tile service (task TE.1): maps stay
    /// on only when on as stored and in `new`, both addresses stay as stored; `new` without `maps`
    /// keeps them all (`torro_core::settings::Maps::from_client`).
    ///
    /// Nor does a client set the folder of "Save to Downloads" (point 9, issue #290): it stays as
    /// stored unless `new` removes it; the click on an attachment comes from `new`
    /// (`torro_core::settings::MailAttachments::from_client`).
    /// Nor does a client switch a web search provider on or set another SearXNG address (issue #305):
    /// a provider stays on only when on as stored and in `new`, the address stays as stored
    /// (`torro_core::settings::WebSearch::from_client`).
    pub fn update_keeping_hidden(&self, new: Settings) -> Result<(), String> {
        self.change(|settings| {
            let hidden = std::mem::take(&mut settings.hidden_collections);
            let assignments = std::mem::take(&mut settings.ai_assignments);
            let list_orders = std::mem::take(&mut settings.list_orders);
            // Nor the reminder styles of calendars outside its selection (task ER1.2): it changes
            // them with `set_setting reminders`, which names each calendar.
            let calendars = std::mem::take(&mut settings.reminders.calendars);
            let enabled = settings.voice.enabled && new.voice.enabled;
            let voice = Voice { enabled, ..new.voice.clone() };
            let mail_to_cloud = settings.mail_to_cloud && new.mail_to_cloud;
            let ai_slots = std::mem::take(&mut settings.ai_slots);
            let decide_out = settings.mail_to_cloud_in(torro_core::ai::AiSlot::Decide);
            let decide_mail_to_cloud = Some(decide_out && new.decide_mail_to_cloud.unwrap_or(false));
            let allow_api_keys = settings.allow_api_keys && new.allow_api_keys;
            let allow_mcp = settings.allow_mcp && new.allow_mcp;
            let reminders = Reminders { calendars, ..new.reminders.clone() };
            let tools = settings.tools.from_client(&new.tools);
            let maps = settings.maps.from_client(&new.maps);
            let mail_attachments = settings.mail_attachments.from_client(&new.mail_attachments);
            let web_search = settings.web_search.from_client(&new.web_search);
            *settings = Settings {
                hidden_collections: hidden,
                ai_assignments: assignments,
                list_orders,
                voice,
                mail_to_cloud,
                ai_slots,
                decide_mail_to_cloud,
                allow_api_keys,
                allow_mcp,
                reminders,
                tools,
                maps,
                mail_attachments,
                web_search,
                ..new
            };
        })
    }

    /// Sets which account and model `function` uses, or takes the assignment back with `None`
    /// (task E12.5). The one way `ai_assignments` changes: `Core::set_ai_assignment` checks the
    /// account, the model and the data rule before it calls this.
    pub fn set_ai_assignment(
        &self,
        function: torro_core::ai::AiFunction,
        assignment: Option<torro_core::ai::AiAssignment>,
    ) -> Result<(), String> {
        self.change(|settings| match &assignment {
            Some(held) => {
                settings.ai_assignments.insert(function, held.clone());
            }
            None => {
                settings.ai_assignments.remove(&function);
            }
        })
    }

    /// Puts `chosen` in `slot`, or empties it with `None` (issue #325, `ai.set_slot`). The one way
    /// `ai_slots` changes besides the old key `assistant_default` (the slot Chat): `Core::set_ai_slot`
    /// checks the account, the model and who asks before it calls this.
    ///
    /// For [`torro_core::ai::AiSlot::Decide`] the same write sets `decide_mail_to_cloud` to
    /// `elsewhere` – whether the model chosen computes elsewhere –, so the permission follows the
    /// choice: a model elsewhere lets sorted mail out, one here or none keeps it here (`elsewhere`
    /// is then `false`; for `None` it is taken as `false` whatever is passed). For
    /// [`torro_core::ai::AiSlot::Chat`] `elsewhere` is ignored: the chat's permission is
    /// `mail_to_cloud`, which only its own switch changes.
    pub fn set_ai_slot(
        &self,
        slot: torro_core::ai::AiSlot,
        chosen: Option<torro_core::ai::SlotModel>,
        elsewhere: bool,
    ) -> Result<(), String> {
        self.change(|settings| match slot {
            torro_core::ai::AiSlot::Chat => settings.ai_slots.chat = chosen.clone(),
            torro_core::ai::AiSlot::Decide => {
                settings.decide_mail_to_cloud = Some(elsewhere && chosen.is_some());
                settings.ai_slots.decide = chosen.clone();
            }
        })
    }

    /// Sets one setting (`Request::SetSetting`). The value is checked against the key before the
    /// file is read; a value that does not fit is an error and changes nothing.
    ///
    /// A design that differs from the stored one also sets what it suggests, in the same write under
    /// the same lock ([`SettingsStore::design_suggestions`], task L.33): the font when it loads valid,
    /// the mail view when the design names one. The same design again changes nothing.
    pub fn set(&self, key: SettingKey, value: &serde_json::Value) -> Result<(), String> {
        let change = Change::parse(key, value)?;
        self.change(|settings| self.apply_with_suggestions(change, settings))
    }

    /// Like [`SettingsStore::set`], for clients: a PDF folder, a template's logo or the Downloads
    /// folder of attachments (issue #290) is written only when it is removed or equals the stored
    /// one, read under the same lock as the write. The core refuses a new path before (point 9,
    /// `Core::switches_on_for_people`); this keeps a path a person removed between that check and
    /// the write removed. A design sets its suggestions as in [`SettingsStore::set`]. The allowed
    /// folders are never a client's (point 9, issue #304): the core refuses them before
    /// (`Request::kept_for_people`), and this refuses them too.
    pub fn set_by_client(&self, key: SettingKey, value: &serde_json::Value) -> Result<(), String> {
        if key == SettingKey::AllowedFolder {
            return Err("only a person manages the allowed folders".into());
        }
        let change = Change::parse(key, value)?;
        self.change(|settings| {
            let change = change.limited_for_clients(settings);
            self.apply_with_suggestions(change, settings)
        })
    }

    /// What choosing the design `id` with `set_setting` would set besides the design, against the
    /// settings as stored now (task L.33): for the summary of an approval (`approvals::Facts`), which
    /// must name what really changes. The run decides again under the lock ([`SettingsStore::set`]),
    /// with the same rule.
    pub fn design_suggestions(&self, id: &str) -> DesignSuggestions {
        self.suggestions_against(&self.current(), id)
    }

    /// The rule of [`SettingsStore::design_suggestions`] against `settings`: nothing when `id` is the
    /// stored design or does not load valid; else the design's font when that font loads valid
    /// (the same check as choosing it, [`SettingsStore::check_part`]) and its mail view, each only
    /// when it differs from the stored one. Only these two: never the colour scheme, never words
    /// from the design's folder. A built-in design suggests without a designs folder too (it loads
    /// without the disk); its font then only when that font loads, which without a fonts folder only
    /// the standard does.
    fn suggestions_against(&self, settings: &Settings, id: &str) -> DesignSuggestions {
        if settings.design == id {
            return DesignSuggestions::default();
        }
        let chosen = match self.designs() {
            Some(dir) => design::load(dir, id).ok(),
            // `design::load` gives a built-in design without touching the disk; any other id would be
            // read relative to the working directory, so it is not looked up.
            None if design::BUILT_IN_IDS.contains(&id) => design::load(Path::new(""), id).ok(),
            None => None,
        };
        let Some(chosen) = chosen else {
            return DesignSuggestions::default();
        };
        let font = chosen.about.font.filter(|font| *font != settings.font && self.check_part(Part::Font, font).is_ok());
        let mail_view = chosen.about.mail_view.filter(|view| *view != settings.mail_view);
        DesignSuggestions { font, mail_view }
    }

    /// Applies `change` to `settings` and, for a design that differs from the one in `settings`, what
    /// it suggests ([`SettingsStore::suggestions_against`], judged before the change).
    fn apply_with_suggestions(&self, change: Change, settings: &mut Settings) {
        let suggested = match &change {
            Change::Design(id) => self.suggestions_against(settings, id),
            _ => DesignSuggestions::default(),
        };
        change.apply(settings);
        suggested.apply(settings);
    }

    /// Shows or hides one collection in all apps (`Request::SetCollectionShown`). Whether the id
    /// belongs to an account is the caller's check.
    pub fn set_collection_shown(&self, collection: &str, shown: bool) -> Result<(), String> {
        self.change(|settings| show_in(settings, collection, shown))
    }

    /// Takes `collections` out of the hidden ones, the list orders and the reminder styles, for an
    /// account that is gone: its ids mean nothing any more, and an account added later must not
    /// start with them.
    /// Writes nothing when none of them was named.
    pub fn forget_collections(&self, collections: &[String]) -> Result<(), String> {
        self.change(|settings| {
            settings.hidden_collections.retain(|id| !collections.contains(id));
            settings.list_orders.retain(|id, _| !collections.contains(id));
            settings.reminders.calendars.retain(|id, _| !collections.contains(id));
        })
    }

    /// Hides and orders every collection of the account `from` under the id it has under the
    /// account `to` as well (`<from>/<URL>` → `<to>/<URL>`), keeping the old ids: the first step
    /// when a mail account takes over the CalDAV account of the same sign-in (task M2.7), taken
    /// before the takeover is stored. Hidden collections keep agents out too, so a calendar the
    /// person hid must never show under its new id, not even for a moment; the old ids go once
    /// the takeover is stored ([`SettingsStore::forget_account`]). One atomic write, only when
    /// something changed.
    pub fn copy_collections(&self, from: &str, to: &str) -> Result<(), String> {
        let renamed =
            |id: &str| torro_pim::account::local_id(id, from).map(|url| torro_pim::account::collection_id(to, url));
        self.change(|settings| {
            let hidden: Vec<String> = settings.hidden_collections.iter().filter_map(|id| renamed(id)).collect();
            for id in hidden {
                if !settings.hidden_collections.contains(&id) {
                    settings.hidden_collections.push(id);
                }
            }
            let orders: Vec<_> =
                settings.list_orders.iter().filter_map(|(id, order)| Some((renamed(id)?, *order))).collect();
            for (id, order) in orders {
                settings.list_orders.entry(id).or_insert(order);
            }
            // A calendar shown over the whole screen must stay so under its new id (task ER1.2).
            let styles: Vec<_> =
                settings.reminders.calendars.iter().filter_map(|(id, shown)| Some((renamed(id)?, *shown))).collect();
            for (id, shown) in styles {
                settings.reminders.calendars.entry(id).or_insert(shown);
            }
        })
    }

    /// Takes every collection of the account `from` out of the hidden ones and the list orders
    /// ([`SettingsStore::forget_collections`] for all ids `<from>/…`): for an account that is gone,
    /// such as a CalDAV account a mail account took over (task M2.7) after
    /// [`SettingsStore::copy_collections`].
    pub fn forget_account(&self, from: &str) -> Result<(), String> {
        let settings = self.current();
        let of_account = |id: &&String| torro_pim::account::local_id(id, from).is_some();
        let ids: Vec<String> = settings
            .hidden_collections
            .iter()
            .chain(settings.list_orders.keys())
            .chain(settings.reminders.calendars.keys())
            .filter(of_account)
            .cloned()
            .collect();
        self.forget_collections(&ids)
    }

    /// Puts `owner` in front of every hidden collection whose id is still a bare URL from before
    /// several accounts existed (task M1.1, [`torro_pim::account::qualify_legacy_id`]): back then
    /// there was one account, `owner`. Runs once when the core starts; writes the file (atomically,
    /// like every change) only when something changed.
    pub fn qualify_legacy_collections(&self, owner: &str) -> Result<(), String> {
        self.change(|settings| {
            for id in &mut settings.hidden_collections {
                if let Some(qualified) = torro_pim::account::qualify_legacy_id(id, owner) {
                    *id = qualified;
                }
            }
        })
    }

    /// Sets the click on an attachment back to `open` when it names the save target `id`, which was
    /// just removed (issue #290): compared and written under the same lock, so a click a person
    /// chose meanwhile stays. Writes nothing when the click names another.
    pub fn release_attachment_click(&self, id: &str) -> Result<(), String> {
        let named = torro_core::settings::AttachmentClick::Target(id.to_owned());
        self.change(|settings| {
            if settings.mail_attachments.click == named {
                settings.mail_attachments.click = torro_core::settings::AttachmentClick::Open;
            }
        })
    }

    /// Reads the stored settings, lets `edit` change them, saves and applies the result. An edit
    /// that changes nothing writes nothing.
    fn change(&self, edit: impl FnOnce(&mut Settings)) -> Result<(), String> {
        // A panic while holding the lock leaves no half-written state behind (the file is written
        // atomically), so a poisoned lock is still good to use.
        let _writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
        let old = self.current();
        let mut new = old.clone();
        edit(&mut new);
        // Settings sent without a font or colour scheme (a client that does not know them, task
        // L.22) keep the stored ones.
        new.fill_unnamed_parts(&old);
        if new == old {
            return Ok(());
        }
        // Checked on every way in (set_setting, update_settings), but only when it changes: a part
        // that broke after it was chosen must not block changing anything else.
        for (part, new_id, old_id) in [
            (Part::Design, &new.design, &old.design),
            (Part::Font, &new.font, &old.font),
            (Part::ColorScheme, &new.color_scheme, &old.color_scheme),
        ] {
            if new_id != old_id {
                self.check_part(part, new_id)?;
            }
        }
        // The same for the PDF templates and folder: `set_setting` checks them when it parses them,
        // whole settings (`update_settings`) carry them unchecked. A header thousands of characters
        // long would make every PDF slow (`tools::pdf`), margins out of bounds leave no page.
        if new.tools.pdf != old.tools.pdf {
            new.tools.pdf.check_changes(&old.tools.pdf)?;
        }
        // And the allowed folders (issue #304): at most 50, absolute, each once – on both ways in.
        if new.tools.folders != old.tools.folders {
            new.tools.check_folders()?;
        }
        // The addresses of maps too (task TE.1): one with a key in its query must not get in through
        // `update_settings` either.
        new.maps.check_changes(&old.maps)?;
        // And the SearXNG address (issue #305), on the same rule.
        new.web_search.check_changes(&old.web_search)?;
        let file = self.file.as_deref().ok_or("no home directory, so no settings file")?;
        save_to(file, &new)?;
        if let Some(enabled) = self.login_item_change(&old, &new) {
            apply_start_at_login(enabled)?;
        }
        Ok(())
    }

    /// Whether the design, font or colour scheme `id` can be chosen: the standard, a built-in design
    /// (`design::BUILT_IN_IDS`, also without a home directory), or one in the folder of `part` that
    /// passes every rule. Otherwise every reason, for the person or agent who chose it; an id the
    /// folder lacks but another holds also names the key that chooses it there (`"nord" is a colour
    /// scheme: choose it with the key color_scheme.`, task L.22).
    fn check_part(&self, part: Part, id: &str) -> Result<(), String> {
        // Whole settings (`update_settings`) come here without `part_id`: the same limit, and the id is
        // not repeated, so a long or invisible text never runs back into the message.
        if bad_family(id) {
            return Err(format!("the {} can't be used: {BAD_FAMILY}", part_name(part)));
        }
        let refused = |reasons: String| format!("the {} {id:?} can't be used: {reasons}", part_name(part));
        // The fonts of the computer (issue #299) need no folder either: they are checked where they are.
        let Some(dir) = self.folder(part).or_else(|| system_font(part, id).then_some(Path::new(""))) else {
            // The built-in designs need no folder, as `settings.list_designs` shows them valid without one.
            let built_in = match part {
                Part::Design => design::BUILT_IN_IDS.contains(&id),
                Part::Font | Part::ColorScheme => id == design::STANDARD_ID,
            };
            return match built_in {
                true => Ok(()),
                false => Err(refused(format!("there is no {} folder without a home directory", folder_name(part)))),
            };
        };
        let Err(problems) = load_part(part, dir, id) else { return Ok(()) };
        let mut reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
        // An installed font is in no folder, so no other part can have its id.
        if matches!(problems[..], [DesignError::NotFound { .. }])
            && !design::is_system_id(id)
            && let Some(hint) = self.found_elsewhere(part, id)
        {
            reasons.push(hint);
        }
        Err(refused(reasons.join(" ")))
    }

    /// Which other parts have a folder `id` (valid or not), as a hint naming the key that chooses
    /// each; `None` when no other folder has it.
    fn found_elsewhere(&self, part: Part, id: &str) -> Option<String> {
        let found: Vec<Part> = [Part::Design, Part::Font, Part::ColorScheme]
            .into_iter()
            .filter(|other| *other != part)
            .filter(|other| {
                let Some(dir) = self.folder(*other) else { return false };
                !matches!(
                    load_part(*other, dir, id).as_ref().map_err(Vec::as_slice),
                    Err([DesignError::NotFound { .. }])
                )
            })
            .collect();
        let names: Vec<&str> = found.iter().map(|part| part_name(*part)).collect();
        let keys: Vec<&str> = found.iter().map(|part| part_key(*part)).collect();
        match found.len() {
            0 => None,
            1 => Some(format!("{id:?} is a {}: choose it with the key {}.", names[0], keys[0])),
            _ => Some(format!(
                "{id:?} is a {}: choose them with the keys {}.",
                names.join(" and a "),
                keys.join(" and ")
            )),
        }
    }

    /// The state the system's login item must be switched to after `old` became `new`, or
    /// `None` when it stays as it is: the setting did not change, or this store must not touch
    /// the system (a test store, see [`SettingsStore::at`]). Kept apart from [`Self::change`] so
    /// tests can check the decision without the system call.
    fn login_item_change(&self, old: &Settings, new: &Settings) -> Option<bool> {
        (self.applies_login_item && old.start_at_login != new.start_at_login).then_some(new.start_at_login)
    }
}

/// What choosing another design with `set_setting design` sets besides the design (task L.33): the
/// font and the mail view the design suggests (`[about] font`, `[about] mail_view`), each only when
/// it is set and would change. Built only by [`SettingsStore::design_suggestions`], which looks them
/// up in the design and checks the font; never from a client's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesignSuggestions {
    /// The id of the font the design suggests, set only when that font loads valid now.
    pub font: Option<String>,
    /// How a mail with HTML shows, as the design suggests.
    pub mail_view: Option<MailFormat>,
}

impl DesignSuggestions {
    /// Puts what is suggested into `settings`; leaves the rest.
    fn apply(self, settings: &mut Settings) {
        if let Some(font) = self.font {
            settings.font = font;
        }
        if let Some(view) = self.mail_view {
            settings.mail_view = view;
        }
    }
}

/// One setting with its value, checked against its key.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    Zoom(Zoom),
    Night(Night),
    WorkingHours(WorkingHours),
    /// Whether the list is known is the caller's check (`Core`), like for hidden collections.
    ListOrder(ListOrderEntry),
    QuietWhenBusy(bool),
    /// Whether the pack exists and is valid is [`SettingsStore::check_part`]'s check.
    Design(String),
    /// The font (task L.22), checked like [`Change::Design`].
    Font(String),
    /// The colour scheme (task L.22), checked like [`Change::Design`].
    ColorScheme(String),
    /// How long the assistant keeps conversations (task C.1).
    AssistantRetention(Retention),
    /// Voice on or off (task C.9). Who may switch it on is the core's check
    /// (`Request::kept_for_people`).
    VoiceEnabled(bool),
    WakeWord(String),
    Microphone(Option<String>),
    SpeakAnswers(bool),
    SttModel(Option<String>),
    Voice(Option<String>),
    /// What a new chat of the assistant starts with (issue #131): the slot Chat (issue #325), written
    /// without the checks of `ai.set_slot`; `None` for the one used last.
    AssistantDefault(Option<AssistantDefault>),
    /// Where the first-run setup stands (task W.1); the version is recorded on the way in.
    Setup(SetupState),
    /// One component on or off (task W.1).
    Component(ComponentSwitch),
    /// How a mail with HTML first shows in the reader (task M9.5).
    MailView(MailFormat),
    /// The colours of a mail's content (task M9.5).
    MailAppearance(MailAppearance),
    /// What a click on an attachment's name does (issue #290). Whether a named target exists is not
    /// checked: one that is gone opens the file.
    MailAttachmentClick(AttachmentClick),
    /// The folder of "Save to Downloads"; `None` for the system's. Who may set one is the core's
    /// check (`Core::switches_on_for_people`), as for the PDF folder.
    MailDownloadsFolder(Option<String>),
    /// The data rule for mail (task C.17). Who may switch it on is the core's check
    /// (`Request::kept_for_people`).
    MailToCloud(bool),
    /// The data rule for the mail the slot Sorting reads (issue #325), checked like
    /// [`Change::MailToCloud`].
    DecideMailToCloud(bool),
    /// Which mail categories the sidebar leaves out (task E12.10).
    HiddenCategories(Vec<torro_mail::Category>),
    /// "Allow API keys" (#231). Who may switch it on is the core's check
    /// (`Request::kept_for_people`).
    AllowApiKeys(bool),
    /// "Allow MCP" (#231), checked like [`Change::AllowApiKeys`].
    AllowMcp(bool),
    /// A change of the reminders of events (task ER1.2): the fields it names, checked against the
    /// defaults when parsed and merged onto the stored ones when applied ([`Reminders::merged`]).
    Reminders(serde_json::Value),
    /// One of the assistant's own tools on or off (issue #225). Who may switch it on is the core's
    /// check (`Request::kept_for_people`).
    ToolEnabled(ToolSwitch),
    /// The folder of new PDFs; `None` for the desktop. Who may set one is the core's check
    /// (`Core::switches_on_for_people`).
    PdfFolder(Option<String>),
    /// One PDF template, replaced whole. Who may set its logo is the core's check, as for the folder.
    PdfTemplate(PdfTemplateId, PdfTemplate),
    /// One allowed folder added, given another access or removed (issue #304). Only a person's
    /// (`Request::kept_for_people`, [`SettingsStore::set_by_client`]); the list's limits are checked
    /// after it is applied ([`torro_core::settings::Tools::check_folders`]).
    AllowedFolder(FolderChange),
    /// The modifiers of the template keys (issue #243).
    WorkspaceShortcuts(torro_core::settings::ShortcutModifiers),
    /// A change of maps and the place search (task TE.1): the fields it names, checked against the
    /// defaults when parsed and merged onto the stored ones when applied ([`Maps::merged`]). Who may
    /// turn them on or name a service is the core's check (`Request::kept_for_people`).
    Maps(serde_json::Value),
    /// One web search provider on or off (issue #305). Who may switch one on is the core's check
    /// (`Request::kept_for_people`).
    SearchProvider(SearchProviderSwitch),
    /// The SearXNG address; `None` for none. Who may set one is the core's check
    /// (`Core::switches_on_for_people`).
    SearxngAddress(Option<String>),
}

impl Change {
    /// `value` as the type `key` takes, or why it does not fit.
    fn parse(key: SettingKey, value: &serde_json::Value) -> Result<Change, String> {
        let invalid = |err: serde_json::Error| format!("{value} is not a value for {key:?}: {err}");
        Ok(match key {
            SettingKey::Language => Change::Language(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Appearance => Change::Appearance(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::StartAtLogin => Change::StartAtLogin(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Not through `Zoom`'s own reading, which clamps any number for the sake of old files:
            // a value from outside must be in range.
            SettingKey::Zoom => Change::Zoom(
                value
                    .as_f64()
                    .and_then(Zoom::from_factor)
                    .ok_or_else(|| format!("{value} is not a zoom: send a number from 0.67 to 2.0"))?,
            ),
            // Strict as well: the file's reading falls back to 22:00–06:00 instead.
            SettingKey::Night => Change::Night(Night::from_value(value)?),
            SettingKey::WorkingHours => Change::WorkingHours(WorkingHours::from_value(value)?),
            SettingKey::ListOrder => Change::ListOrder(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::QuietWhenBusy => Change::QuietWhenBusy(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Design => Change::Design(part_id(key, value)?),
            SettingKey::Font => Change::Font(part_id(key, value)?),
            SettingKey::ColorScheme => Change::ColorScheme(part_id(key, value)?),
            // Strict, like the night: the file's reading falls back to 30 days instead.
            SettingKey::AssistantRetention => Change::AssistantRetention(Retention::from_value(value)?),
            SettingKey::VoiceEnabled => Change::VoiceEnabled(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::WakeWord => Change::WakeWord(Voice::wake_word_from(value)?),
            SettingKey::Microphone => Change::Microphone(Voice::microphone_from(value)?),
            SettingKey::SpeakAnswers => Change::SpeakAnswers(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::SttModel => Change::SttModel(Voice::file_from(value, "bin")?),
            SettingKey::Voice => Change::Voice(Voice::file_from(value, "onnx")?),
            SettingKey::AssistantDefault => Change::AssistantDefault(AssistantDefault::from_value(value)?),
            SettingKey::Setup => Change::Setup(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::Component => Change::Component(serde_json::from_value(value.clone()).map_err(invalid)?),
            // Strict: only the file's reading falls back to the default.
            SettingKey::MailView => Change::MailView(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::MailAppearance => {
                Change::MailAppearance(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::MailAttachmentClick => {
                Change::MailAttachmentClick(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::MailDownloadsFolder => Change::MailDownloadsFolder(MailAttachments::downloads_from(value)?),
            SettingKey::MailToCloud => Change::MailToCloud(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::DecideMailToCloud => {
                Change::DecideMailToCloud(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            // Strict, like the night: the file's reading falls back to the defaults instead.
            SettingKey::HiddenCategories => {
                Change::HiddenCategories(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::Reminders => {
                Reminders::default().merged(value)?;
                Change::Reminders(value.clone())
            }
            SettingKey::AllowApiKeys => Change::AllowApiKeys(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::AllowMcp => Change::AllowMcp(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::ToolEnabled => Change::ToolEnabled(serde_json::from_value(value.clone()).map_err(invalid)?),
            SettingKey::PdfFolder => Change::PdfFolder(PdfSettings::folder_from(value)?),
            SettingKey::PdfTemplate => {
                let (id, template) = PdfTemplate::from_value(value)?;
                Change::PdfTemplate(id, template)
            }
            SettingKey::AllowedFolder => Change::AllowedFolder(FolderChange::from_value(value)?),
            SettingKey::WorkspaceShortcuts => {
                Change::WorkspaceShortcuts(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            // Strict, like the reminders: the file's reading falls back to the defaults (off) instead.
            SettingKey::Maps => {
                Maps::default().merged(value)?;
                Change::Maps(value.clone())
            }
            SettingKey::SearchProvider => {
                Change::SearchProvider(serde_json::from_value(value.clone()).map_err(invalid)?)
            }
            SettingKey::SearxngAddress => Change::SearxngAddress(WebSearch::address_from(value)?),
        })
    }

    /// The change as a client may make it against `settings` (as stored): its paths only removed or
    /// kept ([`torro_core::settings::kept_or_removed`]), everything else as sent.
    fn limited_for_clients(self, settings: &Settings) -> Change {
        use torro_core::settings::kept_or_removed;
        match self {
            Change::PdfFolder(folder) => Change::PdfFolder(kept_or_removed(&settings.tools.pdf.folder, &folder)),
            Change::MailDownloadsFolder(folder) => {
                Change::MailDownloadsFolder(kept_or_removed(&settings.mail_attachments.downloads, &folder))
            }
            Change::PdfTemplate(id, mut template) => {
                template.logo = kept_or_removed(&settings.tools.pdf.template(id).logo, &template.logo);
                Change::PdfTemplate(id, template)
            }
            // The address as the folder: removed or kept, never set by a client (issue #305).
            Change::SearxngAddress(address) => {
                Change::SearxngAddress(kept_or_removed(&settings.web_search.searxng, &address))
            }
            other => other,
        }
    }

    /// Puts the value into its field.
    fn apply(self, settings: &mut Settings) {
        match self {
            Change::Language(language) => settings.language = language,
            Change::Appearance(appearance) => settings.appearance = appearance,
            Change::StartAtLogin(enabled) => settings.start_at_login = enabled,
            Change::Zoom(zoom) => settings.zoom = zoom,
            Change::Night(night) => settings.night = night,
            Change::WorkingHours(hours) => settings.working_hours = hours,
            Change::ListOrder(entry) => settings.set_list_order(&entry.list, entry.order),
            Change::QuietWhenBusy(quiet) => settings.quiet_when_busy = quiet,
            Change::Design(id) => settings.design = id,
            Change::Font(id) => settings.font = id,
            Change::ColorScheme(id) => settings.color_scheme = id,
            Change::AssistantRetention(retention) => settings.assistant_retention = retention,
            Change::VoiceEnabled(enabled) => settings.voice.enabled = enabled,
            Change::WakeWord(phrase) => settings.voice.wake_word = phrase,
            Change::Microphone(name) => settings.voice.microphone = name,
            Change::SpeakAnswers(speak) => settings.voice.speak_answers = speak,
            Change::SttModel(path) => settings.voice.stt_model = path,
            Change::Voice(path) => settings.voice.voice = path,
            Change::AssistantDefault(default) => settings.ai_slots.chat = default,
            Change::Setup(state) => settings.setup = settings.setup.moved_to(state),
            Change::Component(switch) => settings.set_component(switch.component, switch.enabled),
            Change::MailView(format) => settings.mail_view = format,
            Change::MailAppearance(appearance) => settings.mail_appearance = appearance,
            Change::MailAttachmentClick(click) => settings.mail_attachments.click = click,
            Change::MailDownloadsFolder(folder) => settings.mail_attachments.downloads = folder,
            Change::MailToCloud(allowed) => settings.mail_to_cloud = allowed,
            Change::DecideMailToCloud(allowed) => settings.decide_mail_to_cloud = Some(allowed),
            Change::HiddenCategories(hidden) => settings.hidden_categories = hidden,
            Change::AllowApiKeys(allowed) => settings.allow_api_keys = allowed,
            Change::AllowMcp(allowed) => settings.allow_mcp = allowed,
            // Checked when parsed: what the stored ones hold cannot make the fields it names unfit.
            Change::Reminders(change) => {
                if let Ok(merged) = settings.reminders.merged(&change) {
                    settings.reminders = merged;
                }
            }
            Change::ToolEnabled(switch) => settings.tools.set_on(switch.tool, switch.enabled),
            Change::PdfFolder(folder) => settings.tools.pdf.folder = folder,
            Change::PdfTemplate(id, template) => *settings.tools.pdf.template_mut(id) = template,
            Change::AllowedFolder(change) => settings.tools.change_folder(change),
            Change::WorkspaceShortcuts(modifiers) => settings.workspaces.modifiers = modifiers,
            // Checked when parsed; an address only when it changes, so one edited into the file by
            // hand does not keep maps from being turned off.
            Change::Maps(change) => {
                if let Ok(merged) = settings.maps.merged(&change) {
                    settings.maps = merged;
                }
            }
            Change::SearchProvider(switch) => settings.web_search.set_on(switch.provider, switch.enabled),
            Change::SearxngAddress(address) => settings.web_search.searxng = address,
        }
    }
}

/// The id `value` names for the design, font or colour scheme `key`: a string that is not empty (an
/// empty one would mean "left out", [`Settings::fill_unnamed_parts`]). Whether the folder has it is
/// [`SettingsStore::check_part`]'s check.
fn part_id(key: SettingKey, value: &serde_json::Value) -> Result<String, String> {
    match value.as_str() {
        // The family of an installed font is held to the rules its listing follows (issue #299)
        // before anything is loaded, so no long or invisible text runs into a message or an approval.
        Some(id) if bad_family(id) => Err(format!("not a font of this computer for {key:?}: {BAD_FAMILY}")),
        Some(id) if !id.is_empty() => Ok(id.to_string()),
        _ => Err(format!("{value} is not a value for {key:?}: send the id as a string, \"torro\" for the standard")),
    }
}

/// Why a `system:<family>` id is refused before anything is loaded ([`bad_family`]).
const BAD_FAMILY: &str = "after \"system:\" comes a family name of 1 to 64 characters, not starting with \".\", \
     without control, format or line-separator characters";

/// Whether `id` is `system:` with a family name no listing would show (issue #299): too long, empty,
/// hidden or with control, format or line-separator characters.
fn bad_family(id: &str) -> bool {
    id.strip_prefix(design::SYSTEM_PREFIX).is_some_and(|family| !design::is_family_name(family))
}

/// Whether `id` names a font of the computer for `part` (issue #299): only fonts have them.
fn system_font(part: Part, id: &str) -> bool {
    part == Part::Font && design::is_system_id(id)
}

/// Reads the design, font or colour scheme `id` from `dir`, only for its reasons.
fn load_part(part: Part, dir: &Path, id: &str) -> Result<(), Vec<DesignError>> {
    match part {
        Part::Design => design::load(dir, id).map(drop),
        Part::Font => design::load_font(dir, id).map(drop),
        Part::ColorScheme => design::load_color_scheme(dir, id).map(drop),
    }
}

/// What a part is called in messages.
fn part_name(part: Part) -> &'static str {
    match part {
        Part::Design => "design",
        Part::Font => "font",
        Part::ColorScheme => "colour scheme",
    }
}

/// The folder of a part, in messages.
fn folder_name(part: Part) -> &'static str {
    match part {
        Part::Design => "designs",
        Part::Font => "fonts",
        Part::ColorScheme => "colour schemes",
    }
}

/// The key of `settings.set_setting` that chooses a part.
fn part_key(part: Part) -> &'static str {
    match part {
        Part::Design => "design",
        Part::Font => "font",
        Part::ColorScheme => "color_scheme",
    }
}

/// Removes `collection` from the hidden ones, or adds it once.
fn show_in(settings: &mut Settings, collection: &str, shown: bool) {
    let hidden = &mut settings.hidden_collections;
    hidden.retain(|id| id != collection);
    if !shown {
        hidden.push(collection.to_string());
    }
}

// `load_from` and `save_to` follow the private helpers of `torro_core::settings`, which only work
// on the user's file. Same format, same rules: a missing or broken file gives the defaults, writes
// are atomic.

/// Reads `path`. A missing file is normal (first start); any other problem is reported and the
/// defaults are used, so a broken file never keeps the suite from starting.
fn load_from(path: &Path) -> Settings {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("torroworkplace: cannot read {}: {err}", path.display());
            }
            return Settings::default();
        }
    };
    let mut settings: Settings = serde_json::from_str(&text).unwrap_or_else(|err| {
        eprintln!("torroworkplace: ignoring {}, it is not valid settings JSON: {err}", path.display());
        Settings::default()
    });
    // A file without a font or colour scheme draws with the standard (task L.22).
    settings.fill_unnamed_parts(&Settings::default());
    // The AI choices of a file from before the slots (issue #325).
    settings.take_over_old_ai();
    settings
}

/// Writes `settings` to a temporary file next to `path` and renames it over `path`, so a crash
/// never leaves half a file behind. The process id keeps two writers from sharing the temporary.
fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no directory", path.display()))?;
    fs::create_dir_all(dir).map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
    let mut json = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    json.push('\n');
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = write_synced(&tmp, &json).and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn write_synced(path: &Path, content: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

/// Turns the start-at-login entry of this system on or off (see the module docs for where it is).
/// The entry points at the running program, so switching the setting off and on again after the
/// program moved updates it.
#[cfg(target_os = "linux")]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled)
}

#[cfg(target_os = "macos")]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    launch_agent::set_enabled(enabled)
}

#[cfg(windows)]
fn apply_start_at_login(enabled: bool) -> Result<(), String> {
    run_key::set_enabled(enabled)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn apply_start_at_login(_enabled: bool) -> Result<(), String> {
    eprintln!("start at login is not supported on this system yet");
    Ok(())
}

/// The XDG autostart entry (Linux): desktop environments and Hyprland's session start every
/// `.desktop` file found in `autostart/` at login.
#[cfg(target_os = "linux")]
mod autostart {
    use std::env;
    use std::ffi::OsStr;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    const ENTRY_FILE: &str = "torro-workplace.desktop";

    /// Creates the entry when `enabled`, removes it otherwise. Uses the real environment.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let xdg = env::var_os("XDG_CONFIG_HOME");
        let home = env::var_os("HOME").map(PathBuf::from);
        let path = entry_path(xdg.as_deref(), home.as_deref()).ok_or("no home directory, so no autostart entry")?;
        if !enabled {
            return remove_entry(&path);
        }
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        write_entry(&path, &desktop_entry(&exe)).map_err(|err| format!("cannot write {}: {err}", path.display()))
    }

    /// `$XDG_CONFIG_HOME/autostart/torro-workplace.desktop`, else `~/.config/autostart/…`. The XDG
    /// spec requires `XDG_CONFIG_HOME` to be absolute, so an empty or relative value counts as unset.
    fn entry_path(xdg_config_home: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
        let config = xdg_config_home
            .map(Path::new)
            .filter(|dir| dir.is_absolute())
            .map(Path::to_path_buf)
            .or_else(|| home.map(|home| home.join(".config")))?;
        Some(config.join("autostart").join(ENTRY_FILE))
    }

    /// The `.desktop` file: starts `<exe> daemon` at login and stays out of the launcher menus.
    fn desktop_entry(exe: &Path) -> String {
        let exec = exec_arg(exe);
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Torro Workplace\n\
             Exec={exec} daemon\n\
             X-GNOME-Autostart-enabled=true\n\
             NoDisplay=true\n"
        )
    }

    /// The program part of `Exec=`. A path with spaces or characters the spec reserves is quoted.
    /// Inside the quotes `"`, `` ` ``, `$`, `\` and `%` need escaping, and because the value is a
    /// string as well, every backslash is doubled once more.
    fn exec_arg(exe: &Path) -> String {
        const RESERVED: &str = " \t\n\"'\\><~|&;*?#()$`%";
        let path = exe.to_string_lossy();
        if !path.chars().any(|c| RESERVED.contains(c)) {
            return path.into_owned();
        }
        let mut out = String::from("\"");
        for c in path.chars() {
            match c {
                '%' => out.push_str("%%"),
                '"' | '`' | '$' => {
                    out.push_str("\\\\");
                    out.push(c);
                }
                '\\' => out.push_str("\\\\\\\\"),
                _ => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn write_entry(path: &Path, content: &str) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| io::Error::other("autostart entry has no directory"))?;
        fs::create_dir_all(dir)?;
        fs::write(path, content)
    }

    /// Removing an entry that is not there is fine: the goal is that it is gone.
    fn remove_entry(path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("cannot remove {}: {err}", path.display())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn path_follows_xdg_config_home() {
            let path = entry_path(Some(OsStr::new("/xdg")), Some(Path::new("/home/anna")));
            assert_eq!(path, Some(PathBuf::from("/xdg/autostart/torro-workplace.desktop")));
        }

        #[test]
        fn path_falls_back_to_home_config() {
            let path = entry_path(None, Some(Path::new("/home/anna")));
            assert_eq!(path, Some(PathBuf::from("/home/anna/.config/autostart/torro-workplace.desktop")));
        }

        #[test]
        fn relative_or_empty_xdg_config_home_counts_as_unset() {
            let home = Some(Path::new("/home/anna"));
            let expected = Some(PathBuf::from("/home/anna/.config/autostart/torro-workplace.desktop"));
            assert_eq!(entry_path(Some(OsStr::new("relative")), home), expected);
            assert_eq!(entry_path(Some(OsStr::new("")), home), expected);
        }

        #[test]
        fn no_xdg_and_no_home_gives_no_path() {
            assert_eq!(entry_path(None, None), None);
        }

        #[test]
        fn desktop_entry_has_the_expected_content() {
            let content = desktop_entry(Path::new("/usr/bin/torroworkplace"));
            let expected = "[Desktop Entry]\n\
                Type=Application\n\
                Name=Torro Workplace\n\
                Exec=/usr/bin/torroworkplace daemon\n\
                X-GNOME-Autostart-enabled=true\n\
                NoDisplay=true\n";
            assert_eq!(content, expected);
        }

        #[test]
        fn exec_quotes_paths_with_spaces() {
            assert_eq!(exec_arg(Path::new("/opt/Torro Apps/torroworkplace")), "\"/opt/Torro Apps/torroworkplace\"");
        }

        #[test]
        fn exec_escapes_quotes_dollar_percent_and_backslashes() {
            assert_eq!(exec_arg(Path::new("/a\"b")), "\"/a\\\\\"b\"");
            assert_eq!(exec_arg(Path::new("/a b$c")), "\"/a b\\\\$c\"");
            assert_eq!(exec_arg(Path::new("/a b%c")), "\"/a b%%c\"");
            assert_eq!(exec_arg(Path::new("/a b\\c")), "\"/a b\\\\\\\\c\"");
        }

        #[test]
        fn entry_is_created_and_removed() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = entry_path(Some(dir.path().as_os_str()), None).expect("path");
            let content = desktop_entry(Path::new("/usr/bin/torroworkplace"));

            write_entry(&path, &content).expect("create entry");
            assert_eq!(fs::read_to_string(&path).expect("read entry"), content);

            remove_entry(&path).expect("remove entry");
            assert!(!path.exists());
            remove_entry(&path).expect("removing a missing entry is fine");
        }
    }
}

/// The LaunchAgent (macOS): `launchd` starts every agent in `~/Library/LaunchAgents` at login.
///
/// Switching it off only deletes the file, without `launchctl bootout`: the running daemon is the
/// one the user is talking to right now and should not be killed by a settings change, and
/// `launchd` reads the file only at the next login, so a registration left over until then cannot
/// start anything. The same goes for switching on: no `launchctl bootstrap`, the daemon is
/// already running.
#[cfg(target_os = "macos")]
mod launch_agent {
    use std::env;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    const LABEL: &str = "io.github.mahype.torro-workplace";

    /// Creates the agent when `enabled`, removes it otherwise. Uses the real home directory and
    /// the running program, which inside an `.app` bundle is `Contents/MacOS/torroworkplace`.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let path = plist_path(home.as_deref()).ok_or("no home directory, so no login item")?;
        if !enabled {
            return remove_plist(&path);
        }
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        let content = plist(&exe)?;
        write_plist(&path, &content).map_err(|err| format!("cannot write {}: {err}", path.display()))
    }

    /// `~/Library/LaunchAgents/io.github.mahype.torro-workplace.plist`. An empty or relative home
    /// counts as none: a relative path would land wherever the process happens to run.
    fn plist_path(home: Option<&Path>) -> Option<PathBuf> {
        let home = home.filter(|home| home.is_absolute())?;
        Some(home.join("Library").join("LaunchAgents").join(format!("{LABEL}.plist")))
    }

    /// The property list: runs `<exe> daemon` at login. `ProcessType` Interactive keeps the
    /// system from throttling the daemon like a background job. Fails for a path that is not
    /// UTF-8, which a property list cannot hold.
    fn plist(exe: &Path) -> Result<String, String> {
        let exe = exe.to_str().ok_or_else(|| format!("{} is not valid UTF-8", exe.display()))?;
        let exe = xml_escape(exe);
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{exe}</string>\n\
             \t\t<string>daemon</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>ProcessType</key>\n\
             \t<string>Interactive</string>\n\
             </dict>\n\
             </plist>\n"
        ))
    }

    /// `text` as XML character data: the characters XML reserves are replaced.
    fn xml_escape(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
        out
    }

    fn write_plist(path: &Path, content: &str) -> io::Result<()> {
        let dir = path.parent().ok_or_else(|| io::Error::other("login item has no directory"))?;
        fs::create_dir_all(dir)?;
        fs::write(path, content)
    }

    /// Removing a file that is not there is fine: the goal is that it is gone.
    fn remove_plist(path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("cannot remove {}: {err}", path.display())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn path_is_in_the_users_launch_agents() {
            let path = plist_path(Some(Path::new("/Users/anna")));
            assert_eq!(
                path,
                Some(PathBuf::from("/Users/anna/Library/LaunchAgents/io.github.mahype.torro-workplace.plist"))
            );
        }

        #[test]
        fn no_home_or_a_relative_one_gives_no_path() {
            assert_eq!(plist_path(None), None);
            assert_eq!(plist_path(Some(Path::new(""))), None);
            assert_eq!(plist_path(Some(Path::new("anna"))), None);
        }

        #[test]
        fn plist_has_the_expected_content() {
            let content = plist(Path::new("/Applications/Torro.app/Contents/MacOS/torroworkplace")).expect("plist");
            let expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                <plist version=\"1.0\">\n\
                <dict>\n\
                \t<key>Label</key>\n\
                \t<string>io.github.mahype.torro-workplace</string>\n\
                \t<key>ProgramArguments</key>\n\
                \t<array>\n\
                \t\t<string>/Applications/Torro.app/Contents/MacOS/torroworkplace</string>\n\
                \t\t<string>daemon</string>\n\
                \t</array>\n\
                \t<key>RunAtLoad</key>\n\
                \t<true/>\n\
                \t<key>ProcessType</key>\n\
                \t<string>Interactive</string>\n\
                </dict>\n\
                </plist>\n";
            assert_eq!(content, expected);
        }

        #[test]
        fn special_characters_in_the_path_are_escaped() {
            let content = plist(Path::new("/Users/a&b/<x>/\"y\"/it's/torroworkplace")).expect("plist");
            assert!(
                content.contains("<string>/Users/a&amp;b/&lt;x&gt;/&quot;y&quot;/it&apos;s/torroworkplace</string>"),
                "{content}"
            );
        }

        #[test]
        fn a_path_with_spaces_stays_one_argument() {
            let content = plist(Path::new("/Users/Anna Meyer/Torro Apps/torroworkplace")).expect("plist");
            assert!(content.contains("<string>/Users/Anna Meyer/Torro Apps/torroworkplace</string>"));
        }

        #[test]
        fn a_path_that_is_not_utf8_is_refused() {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let path = Path::new(OsStr::from_bytes(b"/tmp/\xff/torroworkplace"));
            assert!(plist(path).is_err());
        }

        #[test]
        fn plist_is_created_and_removed() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = plist_path(Some(dir.path())).expect("path");
            let content = plist(Path::new("/usr/local/bin/torroworkplace")).expect("plist");

            write_plist(&path, &content).expect("create plist, with the missing directories");
            assert_eq!(fs::read_to_string(&path).expect("read plist"), content);

            remove_plist(&path).expect("remove plist");
            assert!(!path.exists());
            remove_plist(&path).expect("removing a missing plist is fine");
        }
    }
}

/// The `Run` key (Windows): Explorer starts every value under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` as a command line at login. Per user,
/// so it needs no administrator rights.
#[cfg(windows)]
mod run_key {
    use std::env;
    use std::io;
    use std::path::Path;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "TorroWorkplace";

    /// Sets the value when `enabled`, deletes it otherwise. Uses the real `Run` key and the
    /// running program.
    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        let exe = env::current_exe().map_err(|err| format!("cannot find the executable: {err}"))?;
        set_in(RUN_KEY, enabled, &exe).map_err(|err| format!(r"cannot change HKCU\{RUN_KEY}: {err}"))
    }

    /// `set_enabled` on the key `subkey` of `HKEY_CURRENT_USER`, which tests point at a key of
    /// their own so they never touch `Run`.
    fn set_in(subkey: &str, enabled: bool, exe: &Path) -> io::Result<()> {
        let user = RegKey::predef(HKEY_CURRENT_USER);
        if enabled {
            // Opens the key, or creates it (the test keys do not exist yet).
            let (key, _) = user.create_subkey(subkey)?;
            return key.set_value(VALUE_NAME, &command_line(exe));
        }
        // Removing what is not there is fine: the goal is that it is gone.
        let key = match user.open_subkey_with_flags(subkey, KEY_SET_VALUE) {
            Ok(key) => key,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        match key.delete_value(VALUE_NAME) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    /// `"<exe>" daemon`. The path is quoted so one with spaces (`C:\Program Files\…`) stays the
    /// program; Windows paths cannot contain a quote, so there is nothing to escape.
    fn command_line(exe: &Path) -> String {
        format!("\"{}\" daemon", exe.display())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A key of its own for one test, removed again when the test ends, even by a failure.
        struct TestKey(String);

        impl TestKey {
            fn new(name: &str) -> TestKey {
                TestKey(format!(r"Software\TorroWorkplaceTest\{name}-{}", std::process::id()))
            }

            fn value(&self) -> io::Result<String> {
                let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&self.0)?;
                key.get_value(VALUE_NAME)
            }
        }

        impl Drop for TestKey {
            fn drop(&mut self) {
                let user = RegKey::predef(HKEY_CURRENT_USER);
                let _ = user.delete_subkey_all(&self.0);
                // Only succeeds when no other test key is left in it; the parent stays otherwise.
                let _ = user.delete_subkey(r"Software\TorroWorkplaceTest");
            }
        }

        #[test]
        fn command_line_quotes_the_path() {
            assert_eq!(
                command_line(Path::new(r"C:\Program Files\Torro Workplace\torroworkplace.exe")),
                r#""C:\Program Files\Torro Workplace\torroworkplace.exe" daemon"#
            );
        }

        #[test]
        fn value_is_set_replaced_and_deleted() {
            let key = TestKey::new("set");
            set_in(&key.0, true, Path::new(r"C:\one\torroworkplace.exe")).expect("set");
            assert_eq!(key.value().expect("value"), r#""C:\one\torroworkplace.exe" daemon"#);

            set_in(&key.0, true, Path::new(r"C:\two words\torroworkplace.exe")).expect("replace");
            assert_eq!(key.value().expect("value"), r#""C:\two words\torroworkplace.exe" daemon"#);

            set_in(&key.0, false, Path::new(r"C:\ignored.exe")).expect("delete");
            assert!(key.value().is_err());
        }

        #[test]
        fn deleting_what_is_not_there_is_fine() {
            let missing_key = TestKey::new("missing");
            set_in(&missing_key.0, false, Path::new(r"C:\x.exe")).expect("no key");

            let missing_value = TestKey::new("novalue");
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(&missing_value.0).expect("create key");
            set_in(&missing_value.0, false, Path::new(r"C:\x.exe")).expect("no value");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;
    use torro_core::settings::{Component, Setup};

    use super::*;

    /// A store on `settings.json` in a fresh temp directory, which the caller keeps alive.
    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        (dir, store)
    }

    fn file_exists(store: &SettingsStore) -> bool {
        store.file.as_deref().expect("a test store has a file").exists()
    }

    /// Writes a design pack `id` with `toml` as its `design.toml` into the store's designs folder.
    fn pack(store: &SettingsStore, id: &str, toml: &str) {
        let folder = store.designs().expect("a test store has a designs folder").join(id);
        fs::create_dir_all(&folder).expect("pack folder");
        fs::write(folder.join("design.toml"), toml).expect("design.toml");
    }

    /// Issue #325: the core takes a file from before the slots over like the apps do, and its next
    /// write leaves `assistantDefault` out; the old key writes the slot Chat.
    #[test]
    fn a_file_from_before_the_slots_is_taken_over_and_written_without_the_old_field() {
        let (dir, store) = store();
        let file = dir.path().join("settings.json");
        let old = json!({
            "assistantDefault": {"engine": "claude-code"},
            "aiAssignments": {"inbox.categorize": {"function": "inbox.categorize", "engine": "ai:a", "enabled": true}}
        });
        fs::write(&file, old.to_string()).expect("write");
        let read = store.current();
        assert_eq!(read.ai_slots.chat.map(|chosen| chosen.engine).as_deref(), Some("claude-code"));
        assert_eq!(read.ai_slots.decide.map(|chosen| chosen.engine).as_deref(), Some("ai:a"));
        assert!(read.ai_assignments.is_empty() && read.decide_mail_to_cloud == Some(false));

        store.set(SettingKey::AssistantDefault, &json!({"engine": "ai:b", "model": "qwen3:8b"})).expect("a default");
        let written: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).expect("read")).expect("json");
        assert!(written.get("assistantDefault").is_none(), "{written}");
        assert_eq!(written["aiSlots"]["chat"], json!({"engine": "ai:b", "model": "qwen3:8b"}));
        assert_eq!(written["aiSlots"]["decide"], json!({"engine": "ai:a"}));
        store.set(SettingKey::DecideMailToCloud, &json!(true)).expect("a flag");
        assert!(store.current().mail_to_cloud_in(torro_core::ai::AiSlot::Decide));
    }

    #[test]
    fn a_valid_design_is_stored_and_read_back() {
        let (_dir, store) = store();
        pack(
            &store,
            "teal",
            "format = 1\n[about]\nname = \"Teal\"\nversion = \"1.0.0\"\n[radius]\nscale = [3, 5, 7, 9]\n",
        );
        assert_eq!(store.current().design, "torro", "the standard until one is chosen");
        store.set(SettingKey::Design, &json!("teal")).expect("a valid pack");
        assert_eq!(store.current().design, "teal");
        store.set(SettingKey::Design, &json!("torro")).expect("the standard is always there");
        assert_eq!(store.current().design, "torro");
    }

    #[test]
    fn a_missing_or_broken_design_is_refused_with_its_reasons() {
        let (_dir, store) = store();
        pack(&store, "pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[size]\nrow = 20\n");
        let broken = store.set(SettingKey::Design, &json!("pale")).expect_err("rows too low");
        assert!(broken.contains("\"pale\" can't be used"), "{broken}");
        assert!(broken.contains("size.row"), "names the broken key: {broken}");
        let missing = store.set(SettingKey::Design, &json!("plex")).expect_err("no such pack");
        assert!(missing.contains("\"plex\""), "{missing}");
        assert!(store.set(SettingKey::Design, &json!(7)).is_err(), "an id is a string");
        assert!(!file_exists(&store), "nothing was written");
        // update_settings takes the same check.
        let all = Settings { design: "plex".into(), ..Settings::default() };
        assert!(store.update(all).is_err());
        assert_eq!(store.current().design, "torro");
    }

    /// Writes `toml` after `format` and `[about]` as the file `file` of `id` in the store's folder `folder`.
    fn part(store: &SettingsStore, folder: Part, id: &str, file: &str, toml: &str) {
        let folder = store.folder(folder).expect("a test store has every folder").join(id);
        fs::create_dir_all(&folder).expect("part folder");
        let text = format!("format = 1\n[about]\nname = \"Sample\"\nversion = \"1.0.0\"\n{toml}");
        fs::write(folder.join(file), text).expect("toml");
    }

    /// Task L.22: font and colour scheme are chosen apart from the design, each checked in its own
    /// folder; an id that only another folder has is refused with the key that chooses it.
    #[test]
    fn font_and_colour_scheme_are_chosen_on_their_own_and_misplaced_ids_name_their_key() {
        let (_dir, store) = store();
        part(&store, Part::Font, "plain", "font.toml", "");
        part(&store, Part::ColorScheme, "nord", "color-scheme.toml", "");
        part(&store, Part::ColorScheme, "murky", "color-scheme.toml", "[colors.dark]\ntext = \"#151515\"\n");
        store.set(SettingKey::Font, &json!("plain")).expect("a valid font");
        store.set(SettingKey::ColorScheme, &json!("nord")).expect("a valid colour scheme");
        let current = store.current();
        assert_eq!(
            (current.design.as_str(), current.font.as_str(), current.color_scheme.as_str()),
            ("torro", "plain", "nord")
        );

        let wrong = store.set(SettingKey::Design, &json!("nord")).expect_err("nord is no design");
        assert!(wrong.contains("the design \"nord\" can't be used"), "{wrong}");
        assert!(wrong.contains("is a colour scheme: choose it with the key color_scheme."), "{wrong}");
        let wrong = store.set(SettingKey::ColorScheme, &json!("plain")).expect_err("plain is no colour scheme");
        assert!(wrong.contains("choose it with the key font."), "{wrong}");
        let missing = store.set(SettingKey::Font, &json!("gone")).expect_err("no such font");
        assert!(missing.contains("\"gone\"") && !missing.contains("the key"), "{missing}");
        let broken = store.set(SettingKey::ColorScheme, &json!("murky")).expect_err("text too dark");
        assert!(broken.contains("can't be used") && !broken.contains("the key"), "{broken}");
        for empty in [json!(""), json!(null), json!(3)] {
            assert!(store.set(SettingKey::Font, &empty).is_err(), "{empty}");
        }
        assert_eq!(store.current(), current, "nothing changed");
        store.set(SettingKey::Font, &json!("torro")).expect("the standard is always there");
        assert_eq!(store.current().font, "torro");
    }

    /// Issue #299: a font of the computer is chosen by `system:<family>`; one that isn't installed is
    /// refused with its reason and no hint at other folders, and a family name the listing would never
    /// show (too long, control or format characters) is refused before anything is loaded. Without a
    /// home directory such a font is still checked where it is, not refused for want of a folder.
    #[test]
    fn fonts_of_the_computer_are_checked_by_their_family() {
        let (_dir, store) = store();
        let gone = "system:Torro Test Font That Is Never Installed";
        let missing = store.set(SettingKey::Font, &json!(gone)).expect_err("not installed");
        let reason = DesignError::NotFound { part: Part::Font, id: gone.into() }.to_string();
        assert!(missing.contains(&reason) && !missing.contains("the key"), "{missing}");
        let long = format!("system:{}", "x".repeat(65));
        for bad in ["system:", "system:.SF NS", "system:Evil\u{202E}Name", "system:Tab\tName", long.as_str()] {
            let refused = store.set(SettingKey::Font, &json!(bad)).expect_err("no such family name");
            let family = &bad[design::SYSTEM_PREFIX.len()..];
            assert!(refused.contains("1 to 64 characters"), "{refused}");
            assert!(family.is_empty() || !refused.contains(family), "the name never runs into the message: {refused}");
        }
        assert_eq!(store.current().font, "torro", "nothing changed");

        // Whole settings take the same limit (they skip `part_id`).
        let whole = store.check_part(Part::Font, &long).expect_err("too long");
        assert!(whole.contains("1 to 64 characters") && !whole.contains("xxxxxxxx"), "{whole}");

        let homeless = SettingsStore::without_file();
        let refused = homeless.check_part(Part::Font, gone).expect_err("not installed");
        assert!(refused.contains(&reason) && !refused.contains("folder"), "checked, not refused: {refused}");
        assert!(homeless.check_part(Part::ColorScheme, gone).is_err(), "only fonts are installed");
    }

    /// Task L.22: whole settings without the font and colour scheme (a client that does not know
    /// them) keep the stored ones; with them, they are checked like set_setting.
    #[test]
    fn whole_settings_without_font_and_colour_scheme_keep_them() {
        let (_dir, store) = store();
        part(&store, Part::Font, "plain", "font.toml", "");
        part(&store, Part::ColorScheme, "nord", "color-scheme.toml", "");
        store.set(SettingKey::Font, &json!("plain")).expect("valid");
        store.set(SettingKey::ColorScheme, &json!("nord")).expect("valid");
        let sent: Settings = serde_json::from_value(json!({"appearance": "dark"})).expect("a client's settings");
        store.update_keeping_hidden(sent.clone()).expect("written");
        let current = store.current();
        assert_eq!(current.appearance, Appearance::Dark);
        assert_eq!((current.font.as_str(), current.color_scheme.as_str()), ("plain", "nord"), "kept");
        store.update(sent).expect("the app key's way keeps them too");
        assert_eq!(store.current().font, "plain");
        let chosen = Settings { font: "gone".into(), ..store.current() };
        assert!(store.update_keeping_hidden(chosen).is_err(), "a font that is not there is refused");
        let back = Settings { font: "torro".into(), color_scheme: "torro".into(), ..store.current() };
        store.update_keeping_hidden(back).expect("named: the standard");
        assert_eq!((store.current().font.as_str(), store.current().color_scheme.as_str()), ("torro", "torro"));
    }

    #[test]
    fn a_design_that_broke_later_does_not_block_other_changes() {
        let (_dir, store) = store();
        pack(&store, "teal", "format = 1\n[about]\nname = \"Teal\"\nversion = \"1.0.0\"\n");
        store.set(SettingKey::Design, &json!("teal")).expect("valid");
        fs::remove_dir_all(store.designs().expect("folder").join("teal")).expect("remove the pack");
        store.set(SettingKey::Appearance, &json!("dark")).expect("the design is not checked again");
        assert_eq!(store.current().design, "teal", "kept as chosen; the apps fall back to the standard");
    }

    /// Task L.33: choosing Console sets its mail view at once, its font only while a valid font of
    /// that id is there; the standard suggests nothing, so going back keeps both.
    #[test]
    fn choosing_a_design_sets_its_mail_view_and_its_font_only_when_it_loads() {
        let (_dir, store) = store();
        store.set(SettingKey::Design, &json!("console")).expect("built in");
        let chosen = store.current();
        assert_eq!((chosen.design.as_str(), chosen.mail_view), ("console", MailFormat::Text));
        assert_eq!(chosen.font, "torro", "jetbrains-mono is not there: the font stays");
        store.set(SettingKey::Design, &json!("torro")).expect("the standard");
        let back = store.current();
        assert_eq!((back.font.as_str(), back.mail_view), ("torro", MailFormat::Text), "the standard suggests nothing");

        part(&store, Part::Font, "jetbrains-mono", "font.toml", "");
        store.set_by_client(SettingKey::Design, &json!("console")).expect("built in");
        let chosen = store.current();
        assert_eq!((chosen.font.as_str(), chosen.mail_view), ("jetbrains-mono", MailFormat::Text), "a client too");
        assert_eq!(chosen.color_scheme, "torro", "never the colour scheme");

        // A broken font of that id is not set.
        store.set(SettingKey::Design, &json!("torro")).expect("the standard");
        store.set(SettingKey::Font, &json!("torro")).expect("the standard font");
        part(
            &store,
            Part::Font,
            "jetbrains-mono",
            "font.toml",
            "[fonts.text]\nfamily = \"Gone\"\nregular = \"Gone.ttf\"\n",
        );
        store.set(SettingKey::Design, &json!("console")).expect("the design is valid");
        assert_eq!(store.current().font, "torro", "a font that breaks a rule stays unchosen");
    }

    /// Task L.33: after the design was chosen, font and mail view are free; choosing the same design
    /// again changes nothing, and whole settings set exactly what they carry.
    #[test]
    fn the_same_design_again_and_whole_settings_apply_no_suggestions() {
        let (_dir, store) = store();
        part(&store, Part::Font, "jetbrains-mono", "font.toml", "");
        store.set(SettingKey::Design, &json!("console")).expect("built in");
        store.set(SettingKey::Font, &json!("torro")).expect("chosen freely");
        store.set(SettingKey::MailView, &json!("html")).expect("chosen freely");
        let before = store.current();
        store.set(SettingKey::Design, &json!("console")).expect("the same design");
        assert_eq!(store.current(), before, "nothing changed");

        store.set(SettingKey::Design, &json!("torro")).expect("the standard");
        let whole = Settings { design: "console".into(), ..store.current() };
        store.update(whole.clone()).expect("written");
        assert_eq!(store.current(), whole, "update_settings sets exactly what it sends");
        store.set(SettingKey::Design, &json!("torro")).expect("the standard");
        store.update_keeping_hidden(Settings { design: "console".into(), ..store.current() }).expect("written");
        let kept = store.current();
        assert_eq!((kept.font.as_str(), kept.mail_view), ("torro", MailFormat::Html), "a client's way neither");
    }

    /// Task L.33: what an approval's summary names, looked up in the design and the fonts folder.
    #[test]
    fn the_suggestions_name_only_what_would_change() {
        let (_dir, store) = store();
        let text_only = DesignSuggestions { font: None, mail_view: Some(MailFormat::Text) };
        assert_eq!(store.design_suggestions("console"), text_only, "no such font: the mail view alone");
        part(&store, Part::Font, "jetbrains-mono", "font.toml", "");
        let both = DesignSuggestions { font: Some("jetbrains-mono".into()), mail_view: Some(MailFormat::Text) };
        assert_eq!(store.design_suggestions("console"), both);
        let nothing = DesignSuggestions::default();
        assert_eq!(store.design_suggestions("torro"), nothing, "the chosen design changes nothing");
        assert_eq!(store.design_suggestions("compact"), nothing, "Compact suggests nothing");
        assert_eq!(store.design_suggestions("gone"), nothing, "a design that is not there");
        store.set(SettingKey::MailView, &json!("text")).expect("valid");
        let font_only = DesignSuggestions { font: Some("jetbrains-mono".into()), mail_view: None };
        assert_eq!(store.design_suggestions("console"), font_only, "the mail view is text already");
        assert_eq!(store.current().design, "torro", "looking up changes nothing");
    }

    /// Issue #291 (review of the three designs): without a home directory there are no folders, but
    /// the built-in designs need none – `settings.list_designs` shows Compact and Console as valid, so
    /// they can be chosen and suggest what they suggest; a font they suggest only when it loads, which
    /// without a fonts folder it never does. Other designs, fonts and colour schemes stay refused.
    #[test]
    fn built_in_designs_can_be_chosen_without_a_home_folder() {
        let store = SettingsStore::without_file();
        for id in design::BUILT_IN_IDS {
            assert_eq!(store.check_part(Part::Design, id), Ok(()), "{id} is built in");
        }
        assert!(store.check_part(Part::Design, "nord").is_err(), "a design of a folder needs the folder");
        assert!(store.check_part(Part::Font, "jetbrains-mono").is_err(), "no fonts folder");
        assert!(store.check_part(Part::ColorScheme, "compact").is_err(), "not a colour scheme");
        let text_only = DesignSuggestions { font: None, mail_view: Some(MailFormat::Text) };
        assert_eq!(store.design_suggestions("console"), text_only, "Console's mail view, never its font");
        assert_eq!(store.design_suggestions("compact"), DesignSuggestions::default());
        assert_eq!(store.design_suggestions("nord"), DesignSuggestions::default(), "not looked up without a folder");

        // A settings file but no folders (the folders come from another directory than the file): the
        // choice is stored with its mail view.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore { file: Some(dir.path().join("settings.json")), ..SettingsStore::without_file() };
        store.set(SettingKey::Design, &json!("console")).expect("built in");
        let chosen = store.current();
        assert_eq!(
            (chosen.design.as_str(), chosen.font.as_str(), chosen.mail_view),
            ("console", "torro", MailFormat::Text)
        );
        assert!(store.set(SettingKey::Design, &json!("nord")).is_err(), "no designs folder");
    }

    #[test]
    fn set_changes_only_the_named_setting() {
        let mut settings = Settings { hidden_collections: vec!["work".into()], ..Settings::default() };
        Change::parse(SettingKey::Language, &json!("german")).expect("valid language").apply(&mut settings);
        Change::parse(SettingKey::Appearance, &json!("dark")).expect("valid appearance").apply(&mut settings);
        Change::parse(SettingKey::StartAtLogin, &json!(true)).expect("valid flag").apply(&mut settings);
        Change::parse(SettingKey::Zoom, &json!(1.25)).expect("valid zoom").apply(&mut settings);
        let night = json!({"from": "23:30", "to": "07:00"});
        Change::parse(SettingKey::Night, &night).expect("valid night").apply(&mut settings);
        let working = json!({"from": "07:30", "to": "16:00"});
        Change::parse(SettingKey::WorkingHours, &working).expect("valid working hours").apply(&mut settings);
        Change::parse(SettingKey::QuietWhenBusy, &json!(false)).expect("valid flag").apply(&mut settings);
        Change::parse(SettingKey::Design, &json!("plex")).expect("an id").apply(&mut settings);
        Change::parse(SettingKey::Font, &json!("channelr")).expect("an id").apply(&mut settings);
        Change::parse(SettingKey::ColorScheme, &json!("nord")).expect("an id").apply(&mut settings);
        Change::parse(SettingKey::AssistantRetention, &json!("forever")).expect("forever").apply(&mut settings);
        let default = json!({"engine": "claude-code", "model": "opus", "effort": "medium"});
        Change::parse(SettingKey::AssistantDefault, &default).expect("a default").apply(&mut settings);
        Change::parse(SettingKey::Setup, &json!("skipped")).expect("a state").apply(&mut settings);
        let mail_off = json!({"component": "mail", "enabled": false});
        Change::parse(SettingKey::Component, &mail_off).expect("a switch").apply(&mut settings);
        let reminders = json!({"allDay": true, "calendars": {"a/work": "full_screen"}});
        Change::parse(SettingKey::Reminders, &reminders).expect("valid reminders").apply(&mut settings);
        let keys = json!("ctrl_shift");
        Change::parse(SettingKey::WorkspaceShortcuts, &keys).expect("valid modifiers").apply(&mut settings);
        Change::parse(SettingKey::Maps, &json!({"openWith": "apple"})).expect("valid maps").apply(&mut settings);
        assert!(Change::parse(SettingKey::Maps, &json!({"tiles": "https://t.org/{z}/{x}/{y}?key=1"})).is_err());
        assert!(Change::parse(SettingKey::WorkspaceShortcuts, &json!("hyper")).is_err(), "only the four choices");
        assert_eq!(
            settings,
            Settings {
                language: Language::German,
                appearance: Appearance::Dark,
                start_at_login: true,
                hidden_collections: vec!["work".into()],
                zoom: Zoom::nearest(1.25),
                night: Night::from_value(&night).expect("valid night"),
                working_hours: WorkingHours::from_value(&working).expect("valid working hours"),
                list_orders: Default::default(),
                quiet_when_busy: false,
                design: "plex".into(),
                font: "channelr".into(),
                color_scheme: "nord".into(),
                assistant_retention: Retention::Forever,
                voice: Voice::default(),
                ai_slots: torro_core::ai::AiSlots {
                    chat: AssistantDefault::from_value(&default).expect("a default"),
                    decide: None,
                },
                legacy_assistant_default: None,
                setup: Setup { state: SetupState::Skipped, version: Setup::VERSION },
                components_off: BTreeSet::from([Component::Mail]),
                mail_view: MailFormat::Html,
                mail_appearance: MailAppearance::App,
                mail_attachments: MailAttachments::default(),
                mail_to_cloud: false,
                decide_mail_to_cloud: Some(false),
                reminders: Reminders::from_value(&reminders).expect("valid reminders"),
                maps: Maps { open_with: torro_core::settings::MapsApp::Apple, ..Maps::default() },
                ai_assignments: std::collections::BTreeMap::new(),
                hidden_categories: Vec::new(),
                allow_api_keys: true,
                allow_mcp: true,
                tools: Default::default(),
                web_search: Default::default(),
                workspaces: torro_core::settings::WorkspaceSettings {
                    modifiers: torro_core::settings::ShortcutModifiers::CtrlShift,
                },
            }
        );
        Change::parse(SettingKey::MailToCloud, &json!(true)).expect("valid flag").apply(&mut settings);
        assert!(settings.mail_to_cloud);
        assert!(Change::parse(SettingKey::MailToCloud, &json!("yes")).is_err(), "only true or false");
        assert!(!settings.mail_to_cloud_in(torro_core::ai::AiSlot::Decide), "the chat's rule is not Sorting's");
        Change::parse(SettingKey::DecideMailToCloud, &json!(true)).expect("valid flag").apply(&mut settings);
        assert_eq!(settings.decide_mail_to_cloud, Some(true));
        assert!(Change::parse(SettingKey::DecideMailToCloud, &json!(1)).is_err(), "only true or false");
        assert!(settings.reminders.all_day && settings.reminders.enabled, "the rest keeps its defaults");
        Change::parse(SettingKey::Reminders, &json!({"defaultLead": 5})).expect("valid").apply(&mut settings);
        assert!(settings.reminders.all_day, "a later change keeps what it does not name");
        assert_eq!(settings.reminders.default_lead, Some(5));
        assert!(Change::parse(SettingKey::Reminders, &json!({"defaultLead": -1})).is_err(), "a lead of minutes");
        Change::parse(SettingKey::AssistantDefault, &json!(null)).expect("none").apply(&mut settings);
        assert_eq!(settings.ai_slots.chat, None, "null goes back to the one used last");
        assert!(Change::parse(SettingKey::AssistantDefault, &json!({"engine": " "})).is_err(), "an engine");
        assert!(Change::parse(SettingKey::QuietWhenBusy, &json!("off")).is_err(), "only true or false");
        assert!(Change::parse(SettingKey::AssistantRetention, &json!(0)).is_err(), "at least a day");
        assert!(Change::parse(SettingKey::Setup, &json!("later")).is_err(), "pending, skipped or done");
        let unknown = json!({"component": "whiteboard", "enabled": false});
        assert!(Change::parse(SettingKey::Component, &unknown).is_err(), "a component this version knows");
        let extra = json!({"component": "mail", "enabled": true, "why": "x"});
        assert!(Change::parse(SettingKey::Component, &extra).is_err(), "nothing else in the switch");
    }

    #[test]
    fn components_go_off_and_on_again_through_the_store() {
        let (_dir, store) = store();
        store.set(SettingKey::Component, &json!({"component": "inbox", "enabled": false})).expect("off");
        assert!(!store.current().component_on(Component::Inbox));
        store.set(SettingKey::Component, &json!({"component": "inbox", "enabled": true})).expect("on");
        assert!(store.current().component_on(Component::Inbox));
        store.set(SettingKey::Setup, &json!("done")).expect("done");
        assert_eq!(store.current().setup.state, SetupState::Done);
    }

    /// Task M9.5: how mail shows is set and read back by its key; a value that does not fit is
    /// refused and changes nothing.
    #[test]
    fn mail_view_and_appearance_are_set_and_checked() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Html, MailAppearance::App));
        store.set(SettingKey::MailView, &json!("text")).expect("text");
        store.set(SettingKey::MailAppearance, &json!("dark")).expect("dark");
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Text, MailAppearance::Dark));
        for (key, value) in [
            (SettingKey::MailView, json!("rich")),
            (SettingKey::MailView, json!("HTML")),
            (SettingKey::MailView, json!(true)),
            (SettingKey::MailAppearance, json!("system")),
            (SettingKey::MailAppearance, json!(null)),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Text, MailAppearance::Dark));
        store.set(SettingKey::MailView, &json!("html")).expect("html");
        store.set(SettingKey::MailAppearance, &json!("light")).expect("light");
        let current = store.current();
        assert_eq!((current.mail_view, current.mail_appearance), (MailFormat::Html, MailAppearance::Light));
    }

    /// Issue #290: removing a save target sets the click on attachments back to `open` only when it
    /// named that target.
    #[test]
    fn a_removed_target_releases_only_its_own_click() {
        use torro_core::settings::AttachmentClick;
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        store.set(SettingKey::MailAttachmentClick, &json!({ "target": "t1" })).expect("a target");
        store.release_attachment_click("t2").expect("another target");
        assert_eq!(store.current().mail_attachments.click, AttachmentClick::Target("t1".into()));
        store.release_attachment_click("t1").expect("its target");
        assert_eq!(store.current().mail_attachments.click, AttachmentClick::Open);
    }

    /// Task C.9: the voice settings are set one by one and checked; a client's whole settings
    /// never switch the microphone on, but may switch it off.
    #[test]
    fn voice_settings_are_set_and_a_client_never_switches_the_microphone_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::at(dir.path().join("settings.json"));
        let model = if cfg!(windows) { r"C:\voice\ggml-base.bin" } else { "/voice/ggml-base.bin" };
        for (key, value) in [
            (SettingKey::WakeWord, json!("Hey Torro")),
            (SettingKey::Microphone, json!("alsa_input.usb")),
            (SettingKey::SpeakAnswers, json!(false)),
            (SettingKey::SttModel, json!(model)),
        ] {
            store.set(key, &value).expect("a valid value");
        }
        let voice = store.current().voice;
        assert_eq!(
            voice,
            Voice {
                microphone: Some("alsa_input.usb".into()),
                speak_answers: false,
                stt_model: Some(model.into()),
                ..Voice::default()
            }
        );
        for (key, value) in [
            (SettingKey::WakeWord, json!("ok computer")),
            (SettingKey::Microphone, json!("--help")),
            (SettingKey::SttModel, json!("ggml-base.bin")),
            (SettingKey::Voice, json!("/voices/thorsten.bin")),
            (SettingKey::VoiceEnabled, json!("yes")),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        store.set(SettingKey::Microphone, &json!(null)).expect("back to the default");
        assert_eq!(store.current().voice.microphone, None);

        // A client sends the settings with voice on: it stays off.
        let mut sent = store.current();
        sent.voice.enabled = true;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().voice.enabled);
        // Switched on by a person, a client may switch it off.
        store.set(SettingKey::VoiceEnabled, &json!(true)).expect("the person");
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(store.current().voice.enabled, "on stays on");
        sent.voice.enabled = false;
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().voice.enabled);
    }

    /// Task C.17: a client's whole settings never let mail go to the cloud, but may keep it local.
    #[test]
    fn a_client_never_lets_mail_out_but_may_keep_it_local() {
        let (_dir, store) = store();
        let mut sent = store.current();
        sent.mail_to_cloud = true;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().mail_to_cloud);
        store.set(SettingKey::MailToCloud, &json!(true)).expect("the person");
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(store.current().mail_to_cloud, "on stays on");
        sent.mail_to_cloud = false;
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().mail_to_cloud);
    }

    /// Issue #325 (task MW.2): a client's whole settings never change a slot – `ai.set_slot` checks
    /// those – and never let sorted mail out, but may keep it here. A client from before the slots
    /// sends no `decideMailToCloud`: that keeps it off, never unset (which would take the old values
    /// over again at the next load).
    #[test]
    fn a_client_keeps_the_slots_and_never_lets_sorted_mail_out() {
        use torro_core::ai::{AiSlot, SlotModel};
        let (_dir, store) = store();
        let decide = SlotModel { engine: "ai:a".into(), model: Some("nimble".into()), effort: None };
        store.set_ai_slot(AiSlot::Decide, Some(decide.clone()), true).expect("a slot");
        assert!(store.current().mail_to_cloud_in(AiSlot::Decide), "the slot elsewhere let it out");

        let mut sent = store.current();
        sent.ai_slots.decide = Some(SlotModel { engine: "ai:b".into(), model: None, effort: None });
        sent.ai_slots.chat = Some(SlotModel { engine: "ai:c".into(), model: None, effort: None });
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert_eq!(store.current().ai_slots.decide, Some(decide.clone()), "as stored");
        assert_eq!(store.current().ai_slots.chat, None, "as stored");
        assert!(store.current().mail_to_cloud_in(AiSlot::Decide), "on stays on");

        sent.decide_mail_to_cloud = None;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert_eq!(
            store.current().decide_mail_to_cloud,
            Some(false),
            "an older client switches it off, never unsets it"
        );
        sent.decide_mail_to_cloud = Some(true);
        store.update_keeping_hidden(sent).expect("written");
        assert_eq!(store.current().decide_mail_to_cloud, Some(false), "a client never lets it out");

        // The slot written here takes its permission along; emptied, nothing goes out.
        store.set_ai_slot(AiSlot::Decide, Some(decide), false).expect("here");
        assert_eq!(store.current().decide_mail_to_cloud, Some(false));
        store.set_ai_slot(AiSlot::Decide, None, true).expect("emptied");
        assert_eq!((store.current().ai_slots.decide, store.current().decide_mail_to_cloud), (None, Some(false)));
        // The chat's slot leaves both permissions alone.
        store.set(SettingKey::DecideMailToCloud, &json!(true)).expect("the person");
        let chat = SlotModel { engine: "claude-code".into(), model: None, effort: None };
        store.set_ai_slot(AiSlot::Chat, Some(chat.clone()), true).expect("chat");
        let now = store.current();
        assert_eq!((now.ai_slots.chat, now.decide_mail_to_cloud, now.mail_to_cloud), (Some(chat), Some(true), false));
    }

    /// Issue #325 (task MW.3): the person's whole settings without `decideMailToCloud` – an app from
    /// before the slots – keep it as stored, so the file never holds it unset (which would take the old
    /// AI values over again); with it, it is taken.
    #[test]
    fn the_person_s_whole_settings_never_unset_the_permission_of_sorting() {
        use torro_core::ai::AiSlot;
        let (dir, store) = store();
        store.set(SettingKey::DecideMailToCloud, &json!(true)).expect("the person");
        let mut sent = store.current();
        sent.decide_mail_to_cloud = None;
        store.update(sent.clone()).expect("written");
        assert_eq!(store.current().decide_mail_to_cloud, Some(true), "kept as stored");
        let file = std::fs::read_to_string(dir.path().join("settings.json")).expect("the file");
        assert!(file.contains("\"decideMailToCloud\": true"), "{file}");

        sent.decide_mail_to_cloud = Some(false);
        store.update(sent.clone()).expect("written");
        assert!(!store.current().mail_to_cloud_in(AiSlot::Decide), "taken when it is there");
        sent.decide_mail_to_cloud = None;
        store.update(sent).expect("written");
        assert_eq!(store.current().decide_mail_to_cloud, Some(false));
    }

    /// #231: a client's whole settings never open the API or MCP again, but may close them; the
    /// person sets them one by one.
    #[test]
    fn a_client_never_opens_the_api_or_mcp_but_may_close_them() {
        let (_dir, store) = store();
        let mut sent = store.current();
        sent.allow_api_keys = false;
        sent.allow_mcp = false;
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().allow_api_keys && !store.current().allow_mcp, "closed");
        sent.allow_api_keys = true;
        sent.allow_mcp = true;
        store.update_keeping_hidden(sent).expect("written");
        assert!(!store.current().allow_api_keys && !store.current().allow_mcp, "a client does not open them");
        store.set(SettingKey::AllowApiKeys, &json!(true)).expect("the person");
        store.set(SettingKey::AllowMcp, &json!(true)).expect("the person");
        assert!(store.current().allow_api_keys && store.current().allow_mcp);
        assert!(store.set(SettingKey::AllowMcp, &json!("on")).is_err(), "only true or false");
    }

    /// An absolute path on this system, for folders and logos.
    fn absolute(path: &str) -> String {
        match cfg!(windows) {
            true => format!(r"C:{}", path.replace('/', r"\")),
            false => path.to_string(),
        }
    }

    /// Issue #225: a tool is switched by its key, the PDF folder and a template are set and checked;
    /// a value that does not fit is refused and changes nothing.
    #[test]
    fn tools_are_switched_and_the_pdf_settings_set_and_checked() {
        use torro_core::settings::{Paper, ToolId};
        let (_dir, store) = store();
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": true})).expect("on");
        assert!(store.current().tools.is_on(ToolId::Pdf));
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": false})).expect("off");
        assert!(!store.current().tools.is_on(ToolId::Pdf));
        let folder = absolute("/Users/anna/PDFs");
        store.set(SettingKey::PdfFolder, &json!(folder)).expect("a folder");
        assert_eq!(store.current().tools.pdf.folder, Some(folder.clone()));
        let mut letter = PdfTemplate::standard(PdfTemplateId::Letter);
        (letter.paper, letter.footer) = (Paper::Letter, "ACME".into());
        store.set(SettingKey::PdfTemplate, &letter.to_value(PdfTemplateId::Letter)).expect("a template");
        let stored = store.current().tools;
        assert_eq!(stored.pdf.letter, letter);
        assert_eq!(stored.pdf.report, PdfTemplate::standard(PdfTemplateId::Report), "the others stay");

        let mut too_small = letter.to_value(PdfTemplateId::Letter);
        too_small["fontSizePt"] = json!(7);
        let mut relative_logo = letter.to_value(PdfTemplateId::Letter);
        relative_logo["logo"] = json!("logo.png");
        let mut unknown = letter.to_value(PdfTemplateId::Letter);
        unknown["template"] = json!("invoice");
        for (key, value) in [
            (SettingKey::ToolEnabled, json!({"tool": "whiteboard", "enabled": true})),
            (SettingKey::ToolEnabled, json!({"tool": "pdf", "enabled": true, "why": "x"})),
            (SettingKey::ToolEnabled, json!(true)),
            (SettingKey::PdfFolder, json!("PDFs")),
            (SettingKey::PdfTemplate, too_small),
            (SettingKey::PdfTemplate, relative_logo),
            (SettingKey::PdfTemplate, unknown),
        ] {
            assert!(store.set(key, &value).is_err(), "{key:?} {value}");
        }
        assert_eq!(store.current().tools, stored, "a refused value changes nothing");
        store.set(SettingKey::PdfFolder, &json!(null)).expect("back to the desktop");
        assert_eq!(store.current().tools.pdf.folder, None);
    }

    /// Issue #225, point 9: a client's whole settings never switch a tool on, set a folder or a
    /// logo; they may switch a tool off and change the other fields of a template, and the paths a
    /// person set stay.
    #[test]
    fn a_client_never_switches_a_tool_on_nor_sets_its_paths() {
        use torro_core::settings::ToolId;
        let (_dir, store) = store();
        let folder = absolute("/sync/Out");
        let logo = absolute("/pictures/logo.png");
        let mut sent = store.current();
        sent.tools.set_on(ToolId::Pdf, true);
        sent.tools.pdf.folder = Some(folder.clone());
        sent.tools.pdf.note.logo = Some(logo.clone());
        sent.tools.pdf.note.font_size_pt = 12;
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().tools;
        assert!(!kept.is_on(ToolId::Pdf), "off stays off");
        assert_eq!((kept.pdf.folder, kept.pdf.note.logo), (None, None), "no path of a client");
        assert_eq!(kept.pdf.note.font_size_pt, 12, "the other fields are taken");

        // The person switches it on and sets the paths: on stays on, the paths stay as they are.
        store.set(SettingKey::ToolEnabled, &json!({"tool": "pdf", "enabled": true})).expect("the person");
        store.set(SettingKey::PdfFolder, &json!(absolute("/Users/anna/PDFs"))).expect("the person");
        let mut note = PdfTemplate::standard(PdfTemplateId::Note);
        note.logo = Some(absolute("/pictures/anna.png"));
        store.set(SettingKey::PdfTemplate, &note.to_value(PdfTemplateId::Note)).expect("the person");
        let person = store.current().tools;
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().tools;
        assert!(kept.is_on(ToolId::Pdf), "on stays on");
        assert_eq!((&kept.pdf.folder, &kept.pdf.note.logo), (&person.pdf.folder, &person.pdf.note.logo));
        sent.tools.set_on(ToolId::Pdf, false);
        store.update_keeping_hidden(sent.clone()).expect("written");
        assert!(!store.current().tools.is_on(ToolId::Pdf), "a client may switch it off");
        // Removing a path is an admin change like any other.
        (sent.tools.pdf.folder, sent.tools.pdf.note.logo) = (None, None);
        store.update_keeping_hidden(sent).expect("written");
        let removed = store.current().tools;
        assert_eq!((removed.pdf.folder, removed.pdf.note.logo), (None, None), "a client may remove the paths");
    }

    /// Issue #305: a provider is switched on and off one at a time, the SearXNG address set, removed
    /// and checked; a value that does not fit changes nothing. One setting at a time a client removes
    /// the address but never sets one.
    #[test]
    fn search_providers_are_switched_and_the_searxng_address_set_and_checked() {
        use torro_core::settings::SearchProviderId;
        let (_dir, store) = store();
        store.set(SettingKey::SearchProvider, &json!({"provider": "duckduckgo", "enabled": true})).expect("on");
        store.set(SettingKey::SearchProvider, &json!({"provider": "wikipedia", "enabled": true})).expect("on");
        store.set(SettingKey::SearchProvider, &json!({"provider": "wikipedia", "enabled": false})).expect("off");
        let web = store.current().web_search;
        assert_eq!(web.providers, BTreeSet::from([SearchProviderId::Duckduckgo]));
        store.set(SettingKey::SearxngAddress, &json!("http://192.168.1.20:8080")).expect("the local network");
        assert_eq!(store.current().web_search.searxng.as_deref(), Some("http://192.168.1.20:8080"));
        for refused in [
            (SettingKey::SearchProvider, json!({"provider": "google", "enabled": true})),
            (SettingKey::SearchProvider, json!({"provider": "brave", "enabled": true, "why": "x"})),
            (SettingKey::SearchProvider, json!(true)),
            (SettingKey::SearxngAddress, json!("https://search.example.org/?key=1")),
            (SettingKey::SearxngAddress, json!("https://anna:secret@search.example.org")),
            (SettingKey::SearxngAddress, json!("search.example.org")),
            (SettingKey::SearxngAddress, json!(8080)),
        ] {
            assert!(store.set(refused.0, &refused.1).is_err(), "{refused:?}");
        }
        assert_eq!(store.current().web_search.searxng.as_deref(), Some("http://192.168.1.20:8080"), "unchanged");

        store.set_by_client(SettingKey::SearxngAddress, &json!("https://other.example.org")).expect("written");
        assert_eq!(store.current().web_search.searxng.as_deref(), Some("http://192.168.1.20:8080"), "kept");
        store.set_by_client(SettingKey::SearxngAddress, &json!(null)).expect("removed");
        assert_eq!(store.current().web_search.searxng, None, "a client may remove it");
        store.set_by_client(SettingKey::SearxngAddress, &json!("https://other.example.org")).expect("written");
        assert_eq!(store.current().web_search.searxng, None, "and never sets one");
        store.set(SettingKey::SearxngAddress, &json!("")).expect("blank is none");
    }

    /// Issue #305: a client's whole settings never switch a provider on nor change the SearXNG
    /// address; they may switch providers off, and the address a person set stays. Whole settings
    /// with an address that does not fit are refused, as with `set_setting`.
    #[test]
    fn a_client_never_switches_a_search_provider_on_nor_sets_the_address() {
        use torro_core::settings::SearchProviderId;
        let (_dir, store) = store();
        let mut sent = store.current();
        sent.web_search.set_on(SearchProviderId::Brave, true);
        sent.web_search.searxng = Some("https://search.example.org".into());
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().web_search;
        assert!(kept.providers.is_empty() && kept.searxng.is_none(), "off stays off, no address of a client");

        store.set(SettingKey::SearchProvider, &json!({"provider": "brave", "enabled": true})).expect("the person");
        store.set(SettingKey::SearchProvider, &json!({"provider": "exa", "enabled": true})).expect("the person");
        store.set(SettingKey::SearxngAddress, &json!("https://anna.example.org")).expect("the person");
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().web_search;
        assert_eq!(kept.providers, BTreeSet::from([SearchProviderId::Brave]), "on in both stays on");
        assert_eq!(kept.searxng.as_deref(), Some("https://anna.example.org"), "the person's address stays");
        sent.web_search = torro_core::settings::WebSearch::default();
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current().web_search;
        assert!(kept.providers.is_empty(), "a client may switch them off");
        assert_eq!(kept.searxng.as_deref(), Some("https://anna.example.org"), "the address stays");

        let mut person = store.current();
        person.web_search.searxng = Some("https://search.example.org/?key=1".into());
        assert!(store.update(person.clone()).is_err(), "an address with a query");
        person.web_search.searxng = Some("http://10.0.0.5:8888".into());
        store.update(person).expect("the person's whole settings");
        assert_eq!(store.current().web_search.searxng.as_deref(), Some("http://10.0.0.5:8888"));
    }

    /// Issue #225: whole settings are checked like `set_setting`: a template out of bounds or a
    /// folder that is no absolute path is refused and nothing is written. Only what changes is
    /// checked, so a hand-edited file, read as it is, does not block changing anything else.
    #[test]
    fn whole_settings_with_a_template_out_of_bounds_are_refused() {
        let (_dir, store) = store();
        let stored = store.current();
        let changed = |edit: &dyn Fn(&mut PdfTemplate)| {
            let mut settings = stored.clone();
            edit(settings.tools.pdf.template_mut(PdfTemplateId::Report));
            settings
        };
        let refused = [
            changed(&|template| template.margins_mm = [0; 4]),
            changed(&|template| template.margins_mm[2] = 200),
            changed(&|template| template.font_size_pt = 40),
            changed(&|template| template.header = "a".repeat(100_000)),
            changed(&|template| template.footer = "two\nlines".into()),
        ];
        for sent in refused {
            assert!(store.update(sent.clone()).is_err(), "update: {:?}", sent.tools.pdf.report);
            assert!(store.update_keeping_hidden(sent.clone()).is_err(), "client: {:?}", sent.tools.pdf.report);
        }
        let mut relative = stored.clone();
        relative.tools.pdf.folder = Some("PDFs".into());
        relative.tools.pdf.note.logo = Some("logo.png".into());
        assert!(store.update(relative).is_err(), "a folder and a logo are absolute paths");
        assert!(!file_exists(&store), "nothing was written");

        // A file edited by hand is read as it is; changing something else still works.
        let mut by_hand = stored.clone();
        by_hand.tools.pdf.letter.margins_mm = [99; 4];
        save_to(store.file.as_deref().expect("a file"), &by_hand).expect("written by hand");
        let mut sent = store.current();
        assert_eq!(sent.tools.pdf.letter.margins_mm, [99; 4], "read as it is");
        sent.quiet_when_busy = !sent.quiet_when_busy;
        store.update(sent.clone()).expect("the template did not change");
        store.update_keeping_hidden(sent.clone()).expect("nor for a client");
        sent.tools.pdf.note.font_size_pt = 12;
        store.update(sent).expect("a template within bounds");
        assert_eq!(store.current().tools.pdf.note.font_size_pt, 12);
    }

    /// Issue #304: a person adds a folder, changes its access and removes it, one at a time; a value that
    /// does not fit, a 51st folder and a path twice are refused and change nothing. A client changes
    /// none of it, not one at a time and not with whole settings, where the stored ones stay.
    #[test]
    fn allowed_folders_are_a_persons_one_at_a_time() {
        use torro_core::settings::{AllowedFolder, FolderAccess};
        let (_dir, store) = store();
        let docs = absolute("/Users/anna/Documents");
        let out = absolute("/Users/anna/Out");
        store.set(SettingKey::AllowedFolder, &json!({"path": docs, "access": "read"})).expect("added");
        store.set(SettingKey::AllowedFolder, &json!({"path": out, "access": "read_write"})).expect("added");
        store.set(SettingKey::AllowedFolder, &json!({"path": docs, "access": "read_write"})).expect("wider");
        let both = vec![
            AllowedFolder { path: docs.clone(), access: FolderAccess::ReadWrite },
            AllowedFolder { path: out.clone(), access: FolderAccess::ReadWrite },
        ];
        assert_eq!(store.current().tools.folders, both);
        for value in [
            json!({"path": "Documents", "access": "read"}),
            json!({"path": docs, "access": "write"}),
            json!({"path": docs}),
            json!(docs),
        ] {
            assert!(store.set(SettingKey::AllowedFolder, &value).is_err(), "{value}");
        }
        assert_eq!(store.current().tools.folders, both, "a refused value changes nothing");

        // A client: neither one at a time nor with whole settings, and the stored ones stay.
        let removal = json!({"path": docs, "access": null});
        assert!(store.set_by_client(SettingKey::AllowedFolder, &removal).is_err());
        let mut sent = store.current();
        sent.tools.folders = vec![AllowedFolder { path: absolute("/"), access: FolderAccess::ReadWrite }];
        sent.quiet_when_busy = !sent.quiet_when_busy;
        store.update_keeping_hidden(sent.clone()).expect("written");
        let kept = store.current();
        assert_eq!(kept.tools.folders, both, "the stored ones stay");
        assert_eq!(kept.quiet_when_busy, sent.quiet_when_busy, "the rest is taken");
        sent.tools.folders.clear();
        store.update_keeping_hidden(sent).expect("written");
        assert_eq!(store.current().tools.folders, both, "and are not removed");

        store.set(SettingKey::AllowedFolder, &removal).expect("removed");
        assert_eq!(store.current().tools.folders, both[1..]);
        for at in 1..MAX_ALLOWED {
            let path = absolute(&format!("/f{at}"));
            store.set(SettingKey::AllowedFolder, &json!({"path": path, "access": "read"})).expect("fits");
        }
        assert_eq!(store.current().tools.folders.len(), MAX_ALLOWED);
        let one_more = json!({"path": absolute("/f-more"), "access": "read"});
        assert!(store.set(SettingKey::AllowedFolder, &one_more).is_err_and(|why| why.contains("50")));
        assert_eq!(store.current().tools.folders.len(), MAX_ALLOWED, "nothing written");

        // The person's whole settings are checked the same way.
        let mut twice = store.current();
        twice.tools.folders.push(twice.tools.folders[0].clone());
        assert!(store.update(twice).is_err());
        let mut relative = store.current();
        relative.tools.folders[0].path = "Documents".into();
        assert!(store.update(relative).is_err());
        assert_eq!(store.current().tools.folders.len(), MAX_ALLOWED);
    }

    /// The most allowed folders (issue #304).
    const MAX_ALLOWED: usize = torro_core::settings::MAX_ALLOWED_FOLDERS;

    /// Issue #225, point 9: one setting at a time, a client keeps or removes a path but never sets
    /// a new one, even when the core's check saw the stored one and a person changed it meanwhile.
    #[test]
    fn a_client_sets_no_path_one_setting_at_a_time() {
        let (_dir, store) = store();
        let folder = absolute("/sync/Out");
        let logo = absolute("/pictures/logo.png");
        store.set_by_client(SettingKey::PdfFolder, &json!(folder)).expect("written");
        let mut note = PdfTemplate::standard(PdfTemplateId::Note);
        (note.logo, note.font_size_pt) = (Some(logo.clone()), 12);
        store.set_by_client(SettingKey::PdfTemplate, &note.to_value(PdfTemplateId::Note)).expect("written");
        let kept = store.current().tools.pdf;
        assert_eq!((kept.folder, kept.note.logo), (None, None), "no path of a client");
        assert_eq!(kept.note.font_size_pt, 12, "the other fields are taken");

        store.set(SettingKey::PdfFolder, &json!(folder)).expect("the person");
        store.set(SettingKey::PdfTemplate, &note.to_value(PdfTemplateId::Note)).expect("the person");
        store.set_by_client(SettingKey::PdfFolder, &json!(folder)).expect("the same folder");
        assert_eq!(store.current().tools.pdf.folder, Some(folder), "the stored one stays");
        store.set_by_client(SettingKey::PdfFolder, &json!(null)).expect("removed");
        note.logo = None;
        store.set_by_client(SettingKey::PdfTemplate, &note.to_value(PdfTemplateId::Note)).expect("removed");
        let removed = store.current().tools.pdf;
        assert_eq!((removed.folder, removed.note.logo), (None, None), "a client may remove the paths");
    }

    /// Issue #290, point 9: the click on an attachment is set like any setting; the Downloads folder
    /// only by a person, and a client keeps or removes it, one setting at a time and with the whole
    /// settings, even when a person changed it after the core's check.
    #[test]
    fn a_client_keeps_or_removes_the_downloads_folder_and_sets_the_click() {
        use torro_core::settings::AttachmentClick;
        let (_dir, store) = store();
        let folder = absolute("/Users/anna/Ablage");
        store.set_by_client(SettingKey::MailAttachmentClick, &json!({"target": "0a1b2c3d"})).expect("a click");
        store.set_by_client(SettingKey::MailDownloadsFolder, &json!(folder)).expect("written");
        let kept = store.current().mail_attachments;
        assert_eq!(kept.click, AttachmentClick::Target("0a1b2c3d".into()));
        assert_eq!(kept.downloads, None, "no path of a client");
        assert!(store.set(SettingKey::MailDownloadsFolder, &json!("Ablage")).is_err(), "absolute only");
        assert!(store.set(SettingKey::MailAttachmentClick, &json!("print")).is_err(), "only the five clicks");

        store.set(SettingKey::MailDownloadsFolder, &json!(folder)).expect("the person");
        assert_eq!(store.current().mail_attachments.downloads.as_deref(), Some(&*folder));
        let mut sent = store.current();
        sent.mail_attachments.downloads = Some(absolute("/Users/anna/Dropbox"));
        sent.mail_attachments.click = AttachmentClick::SaveAs;
        store.update_keeping_hidden(sent).expect("written");
        let kept = store.current().mail_attachments;
        assert_eq!((kept.click, kept.downloads.as_deref()), (AttachmentClick::SaveAs, Some(&*folder)));
        store.set_by_client(SettingKey::MailDownloadsFolder, &json!(null)).expect("removed");
        assert_eq!(store.current().mail_attachments.downloads, None, "a client may remove it");
    }

    #[test]
    fn a_list_order_is_set_and_reset_and_kept_from_clients() {
        use torro_core::settings::ListOrder;
        let (_dir, store) = store();
        store.set(SettingKey::ListOrder, &json!({"list": "work", "order": "manual"})).expect("manual");
        assert_eq!(store.current().list_order("work"), ListOrder::Manual);
        assert!(store.set(SettingKey::ListOrder, &json!({"list": "work", "order": "random"})).is_err());
        assert!(store.set(SettingKey::ListOrder, &json!("manual")).is_err(), "the list is missing");
        // A client sends back what it saw, without the orders: they stay.
        store.update_keeping_hidden(Settings::default()).expect("updated");
        assert_eq!(store.current().list_order("work"), ListOrder::Manual);
        store.forget_collections(&["work".into()]).expect("account gone");
        assert!(store.current().list_orders.is_empty());
        store.set(SettingKey::ListOrder, &json!({"list": "home", "order": "manual"})).expect("manual");
        store.set(SettingKey::ListOrder, &json!({"list": "home", "order": "due_date"})).expect("back");
        assert!(store.current().list_orders.is_empty(), "the default is not stored");
    }

    #[test]
    fn a_list_order_with_a_bare_url_is_refused() {
        use torro_core::settings::ListOrder;
        let (_dir, store) = store();
        let mut settings = Settings::default();
        settings.set_list_order("https://cloud.example.org/remote.php/dav/calendars/anna/tasks/", ListOrder::Manual);
        assert!(store.update(settings).is_err());
        assert!(!file_exists(&store));
    }

    #[test]
    fn an_update_from_a_client_keeps_the_hidden_collections() {
        let (_dir, store) = store();
        store.set_collection_shown("secret", false).expect("hidden");
        let new = Settings { language: Language::German, hidden_collections: vec![], ..Settings::default() };
        store.update_keeping_hidden(new).expect("updated");
        let stored = store.current();
        assert_eq!((stored.language, stored.hidden_collections), (Language::German, vec!["secret".to_string()]));
    }

    #[test]
    fn a_value_that_does_not_fit_is_refused() {
        assert!(Change::parse(SettingKey::Language, &json!("klingon")).is_err());
        assert!(Change::parse(SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(Change::parse(SettingKey::Appearance, &json!(1)).is_err());
        for bad in [json!(0.5), json!(2.5), json!("1.1"), json!(true), json!(null)] {
            assert!(Change::parse(SettingKey::Zoom, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_zoom_is_rounded_to_the_nearest_step_and_stored() {
        let (_dir, store) = store();
        store.set(SettingKey::Zoom, &json!(1.3)).expect("in range");
        assert_eq!(store.current().zoom.percent(), 125);
        store.set(SettingKey::Zoom, &json!(2)).expect("an integer is a number too");
        assert_eq!(store.current().zoom, Zoom::MAX);
        assert!(store.set(SettingKey::Zoom, &json!(3.0)).is_err());
        assert_eq!(store.current().zoom, Zoom::MAX, "a refused value changes nothing");
    }

    #[test]
    fn a_night_is_stored_and_a_bad_one_refused_with_its_reason() {
        let (_dir, store) = store();
        store.set(SettingKey::Night, &json!({"from": "23:30", "to": "07:00"})).expect("valid night");
        let stored = store.current().night;
        assert_eq!((stored.from.to_string(), stored.to.to_string()), ("23:30".into(), "07:00".into()));
        store.set(SettingKey::Night, &json!({"from": "06:00", "to": "06:00"})).expect("no night");
        assert!(store.current().night.is_none());
        let refused = store.set(SettingKey::Night, &json!({"from": "22:10", "to": "06:00"}));
        assert!(refused.as_ref().is_err_and(|reason| reason.contains("quarter hour")), "{refused:?}");
        for bad in [json!("22:00-06:00"), json!(22), json!({"from": "22:00"})] {
            assert!(store.set(SettingKey::Night, &bad).is_err(), "{bad}");
        }
        assert!(store.current().night.is_none(), "a refused value changes nothing");
    }

    #[test]
    fn working_hours_are_stored_and_bad_ones_refused_with_their_reason() {
        let (_dir, store) = store();
        assert_eq!(store.current().working_hours, WorkingHours::DEFAULT);
        store.set(SettingKey::WorkingHours, &json!({"from": "07:30", "to": "16:00"})).expect("valid hours");
        let stored = store.current().working_hours;
        assert_eq!((stored.from.to_string(), stored.to.to_string()), ("07:30".into(), "16:00".into()));
        store.set(SettingKey::WorkingHours, &json!({"from": "09:00", "to": "09:00"})).expect("no working hours");
        assert!(store.current().working_hours.is_none());
        let refused = store.set(SettingKey::WorkingHours, &json!({"from": "17:00", "to": "08:00"}));
        assert!(refused.as_ref().is_err_and(|reason| reason.contains("midnight")), "{refused:?}");
        let refused = store.set(SettingKey::WorkingHours, &json!({"from": "08:10", "to": "17:00"}));
        assert!(refused.as_ref().is_err_and(|reason| reason.contains("quarter hour")), "{refused:?}");
        for bad in [json!("08:00-17:00"), json!(8), json!({"from": "08:00"})] {
            assert!(store.set(SettingKey::WorkingHours, &bad).is_err(), "{bad}");
        }
        assert!(store.current().working_hours.is_none(), "a refused value changes nothing");
    }

    #[test]
    fn a_refused_value_writes_nothing() {
        let (_dir, store) = store();
        assert!(store.set(SettingKey::Language, &json!("klingon")).is_err());
        assert!(store.set(SettingKey::StartAtLogin, &json!("yes")).is_err());
        assert!(!file_exists(&store));
    }

    #[test]
    fn a_setting_is_stored_in_the_given_file() {
        let (_dir, store) = store();
        assert_eq!(store.current(), Settings::default(), "no file yet");
        store.set(SettingKey::Appearance, &json!("light")).expect("valid appearance");
        assert!(file_exists(&store));
        let reopened = SettingsStore::at(store.file.clone().expect("a file"));
        assert_eq!(reopened.current(), Settings { appearance: Appearance::Light, ..Settings::default() });
    }

    #[test]
    fn forgotten_collections_are_no_longer_hidden() {
        let (_dir, store) = store();
        store.set_collection_shown("work", false).expect("hide work");
        store.set_collection_shown("other", false).expect("hide other");
        store.forget_collections(&["work".into(), "home".into()]).expect("forget");
        assert_eq!(store.current().hidden_collections, ["other"]);
    }

    #[test]
    fn hidden_collections_from_before_several_accounts_get_their_account() {
        let (_dir, store) = store();
        let owner = "nextcloud:anna@cloud.example.org";
        let new = format!("{owner}/https://cloud.example.org/dav/calendars/anna/home/");
        store.set_collection_shown("https://cloud.example.org/dav/calendars/anna/work/", false).expect("old id");
        store.set_collection_shown(&new, false).expect("new id");
        store.qualify_legacy_collections(owner).expect("rewritten");
        assert_eq!(
            store.current().hidden_collections,
            [format!("{owner}/https://cloud.example.org/dav/calendars/anna/work/"), new]
        );
        let written = fs::metadata(store.file.as_deref().expect("file")).expect("written").modified().ok();
        store.qualify_legacy_collections(owner).expect("nothing left to do");
        let again = fs::metadata(store.file.as_deref().expect("file")).expect("still there").modified().ok();
        assert_eq!(written, again, "a second start writes nothing");
    }

    #[test]
    fn reminder_styles_move_with_a_takeover_and_go_with_the_account() {
        use torro_core::settings::CalendarReminders;
        let (_dir, store) = store();
        let (from, to) = ("caldav:anna@cloud.example.org", "mail:anna@cloud.example.org");
        let url = "https://cloud.example.org/dav/calendars/anna/work/";
        let (old, new) = (format!("{from}/{url}"), format!("{to}/{url}"));
        store.set(SettingKey::Reminders, &json!({"calendars": {old.clone(): "full_screen"}})).expect("set");
        store.copy_collections(from, to).expect("copied");
        assert_eq!(store.current().reminders.calendar(&new), CalendarReminders::FullScreen, "under its new id");
        store.forget_account(from).expect("forgotten");
        let calendars = store.current().reminders.calendars;
        assert_eq!(calendars.keys().cloned().collect::<Vec<_>>(), [new], "the old id is gone");
    }

    #[test]
    fn forgetting_what_is_not_hidden_writes_nothing() {
        let (_dir, store) = store();
        store.forget_collections(&["work".into()]).expect("nothing to forget");
        assert!(!file_exists(&store));
    }

    #[test]
    fn a_test_store_switches_start_at_login_without_touching_the_login_item() {
        let (_dir, store) = store();
        let (off, on) = (Settings::default(), Settings { start_at_login: true, ..Settings::default() });
        assert_eq!(store.login_item_change(&off, &on), None, "a test store never reaches the system");
        assert_eq!(SettingsStore::without_file().login_item_change(&off, &on), None);

        // Through the real path of a change: stored in the file, nothing else.
        store.set(SettingKey::StartAtLogin, &json!(true)).expect("on");
        assert!(store.current().start_at_login);
        store.set(SettingKey::StartAtLogin, &json!(false)).expect("off");
        assert!(!store.current().start_at_login);
    }

    #[test]
    fn the_daemons_store_applies_only_a_changed_start_at_login() {
        // Built like `system()` but on a temp file, so the decision is checked without a call.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore { applies_login_item: true, ..SettingsStore::at(dir.path().join("settings.json")) };
        assert!(SettingsStore::system().applies_login_item);
        let (off, on) = (Settings::default(), Settings { start_at_login: true, ..Settings::default() });
        assert_eq!(store.login_item_change(&off, &on), Some(true));
        assert_eq!(store.login_item_change(&on, &off), Some(false));
        let german = Settings { language: Language::German, ..on.clone() };
        assert_eq!(store.login_item_change(&on, &german), None, "start at login did not change");
    }

    #[test]
    fn without_a_file_reads_give_defaults_and_changes_fail() {
        let store = SettingsStore::without_file();
        assert_eq!(store.current(), Settings::default());
        assert!(store.set(SettingKey::Appearance, &json!("dark")).is_err());
    }

    #[test]
    fn showing_and_hiding_keeps_each_id_once() {
        let mut settings = Settings { hidden_collections: vec!["work".into()], ..Settings::default() };
        show_in(&mut settings, "home", false);
        show_in(&mut settings, "home", false);
        assert_eq!(settings.hidden_collections, ["work", "home"]);
        show_in(&mut settings, "work", true);
        assert_eq!(settings.hidden_collections, ["home"]);
        show_in(&mut settings, "unknown", true);
        assert_eq!(settings.hidden_collections, ["home"]);
    }
}
