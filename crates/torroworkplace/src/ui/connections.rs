//! MCP servers (task MA.7 in `docs/tasks/mcp-anbindungen.md`, issue #302): what the suite runs
//! for its clients beyond its own commands – its built-in tools and the MCP servers it is a client of.
//! The assistant's settings only point here since this task (`torro_settings_ui::assistant::tools`).
//!
//! ```text
//! Allowed folders    a row per folder the tools that work with files may reach, Read or Read and write · Add folder…
//! Built-in tools     Organize files (switch) · whether the assistant may use it · No allowed folders (while none)
//!                    Make PDFs (switch) · whether the assistant may use it · Folder · Templates ›
//!                    Search the web (switch) · whether the assistant may use it · Search providers ›
//!   › PDF templates  Letter · Report · Note: paper, margins, font size, header, footer, page numbers, logo
//!   › Search providers  a group per provider of the tool `web` (`torro_settings_ui::assistant::web`)
//! Catalog            a row per entry of the core's catalog, with the status of an installed one
//! Your own servers   a row per server a person added, with its status
//! ```
//!
//! **Built-in tools** moved here unchanged from the assistant (task WZ.6): the switch
//! (`settings.set_setting tool_enabled`) says whether a tool exists on this computer, for every
//! client; what a client may use is its rights. The row "may use it" only reads the assistant's
//! client (`clients.list_clients`, loaded by the shell) and brings the assistant's window to the
//! front, where Access sets its rights ([`ClientTarget::request`]); nothing here writes a client.
//! Only a person switches a tool on and sets a path (point 9 of "Was nie nach außen geht" in
//! `docs/api.md`): this window has the app key, and folder and logo come from the system's choosers,
//! which only the shell opens ([`Action::Pick`] with a [`Chooser`], [`pick`]); it hands the path
//! back as [`Message::Picked`]. Every change of a template field sends `settings.set_setting
//! pdf_template` with the **whole** template as loaded, the one field changed ([`template_request`]):
//! the key replaces a template whole. Header and footer go out with Enter or "Save", when another
//! line is started, or when the templates' page is left – never on every key.
//!
//! **Allowed folders** (issue #304, task WD.4, here since #302): the folders the tool `files` (and later ArtCraft)
//! works in, each to read or to read and write (`settings.set_setting allowed_folder`, [`allowed_folder_request`]).
//! Only a person adds, changes and removes one (point 9): "Add folder…" opens the system's folder chooser, which only
//! the shell opens ([`Chooser::AllowedFolder`]), and a folder chosen comes back to read ([`added_folder_request`]).
//!
//! **Search providers** (issue #305, task WEB.6, here since #302): the view the setup's step "Web search"
//! shows too ([`web`]), on a page below the tool `web`: a provider's switch, its key
//! (`accounts.set_search_key`, only from this window, typed as a [`Secret`] and dropped when sent or when the
//! page is left) and SearXNG's address, whose check (`settings.set_setting searxng_address`) goes out as
//! [`Action::CheckAddress`] and comes back here ([`checked`]). The shell lists the providers
//! (`tools.list_search_providers`) only while this section or the setup is open: listing asks the keychain.
//!
//! **Catalog** and **Your own servers** list what `extensions.list_catalog` and
//! `extensions.list_extensions` answered ([`Data::catalog`], [`Data::extensions`], each a
//! [`Listing`]); a list that failed is a sentence with "Retry" in its group ([`Action::Reload`]).
//! The shell asks for the catalog only while this section is open (it looks for programs on disk
//! each time).
//!
//! **Installing** (task MA.8, `docs/tasks/mcp-anbindungen.md`, "Zustimmen"): a catalog entry with
//! nothing missing ([`installable`]) has "Install…", which opens the dialog ([`overlay`]): the
//! entry's tools with the access the catalog gives each, a hidden field per secret with the link
//! where to get one, and that the server runs as the person. "Install" sends
//! `extensions.install_extension`; the core starts the server once and stores it switched off with
//! its tools waiting. The dialog then shows the tools the server **reported** (the answer's
//! [`ExtensionInfo`]), and "Allow tools" sends `extensions.confirm_tools` with exactly that list's
//! fingerprint ([`Message::AllowTools`]). A list that changed meanwhile (`conflict`) is read again
//! (`extensions.get_extension`) and shown for another look.
//!
//! **A server's page** ([`Page::Server`]): its status in words with a dot and its switch
//! (`extensions.set_enabled`), its version with "Update" when the catalog has another one
//! (`extensions.update_extension`), its tools with access and state and "Allow tools" for those
//! that wait, its secrets as "Stored" or "Missing" with "Replace…" (`extensions.set_secret`), its
//! output (`extensions.read_logs`, read when the page opens, in a field to select and copy from),
//! and "Remove…" with a question first (`extensions.remove_extension`). Who may use it is set per
//! client (API, MCP clients, the assistant's Access), never here. The sidebar marks the section while a
//! server waits for its tools or stopped after errors ([`alert`]).
//!
//! Every request of this section goes out as [`Action::AskConnections`] with what it is for
//! ([`Asked`]), and its answer comes back here only ([`answered`]), with the core's code: the
//! dialog and the page show their outcome themselves. A secret is a [`Secret`] from the key press
//! on, so it never shows in `Debug`.
//!
//! **A server of one's own** (task MA.9): "Add server…" in the [`header`] of the list opens its form
//! ([`Custom`], [`overlay`]): a command or an address, a name, the id proposed from the name
//! ([`suggested_id`], [`valid_id`]), the program with one argument per line or the address, and
//! the variables or headers as rows with a "Secret" switch, on for names that look like one
//! ([`looks_secret`]). Every value is a [`Secret`] from the first key; one marked secret goes out
//! as [`Value::Secret`], the others as [`Value::Plain`]. "Paste configuration" fills the form from
//! an `mcpServers` block ([`parse_mcp_servers`]); a block with several servers lists them, and each
//! gets a form of its own, one after the other. What the form can check itself – name, id, command
//! or address, the names of the rows and the variables Torro never passes on ([`blocked_env`]) –
//! shows at its field after the first "Add server" ([`check`]); what only the core can tell (an
//! address that is neither `https://` nor on this computer, a program that does not start) comes
//! back as its sentence in the dialog. "Add server" sends one `extensions.install_extension` with
//! `custom`; then it goes on as the catalog does, through [`Install`]'s step of the reported tools
//! ([`Step::Tools`], [`tools_dialog`]) and [`Message::AllowTools`]. On the page of one's own server
//! "Update…" opens the same form filled in from it (its name and id stay); a stored secret left
//! empty goes out as [`Value::Stored`] and keeps its value – the core keeps it only for the same
//! program and arguments or the same address, and says so when it does not.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row, svg, text, text_editor, toggler};
use iced::{Alignment, Element, Fill, Length, Theme};

use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::{
    AllowedFolder, FolderAccess, MAX_ALLOWED_FOLDERS, Paper, PdfTemplate, PdfTemplateId, ToolId,
};
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status as Dot};
use torro_gui::{style, typo};
use torro_ipc::api::{ASSISTANT_CLIENT, Area};
use torro_ipc::extensions::{
    CatalogEntry, CrashReason, ExtensionInfo, ExtensionTool, InstallFrom, MAX_ID_CHARS, ParseError, ServerSpec, Source,
    Status, ToolState, Value, blocked_env, parse_mcp_servers, valid_env_name, valid_header_name, valid_id,
};
use torro_ipc::tools::{ToolAccess, ToolInfo};
use torro_ipc::{Request, Response, Secret, SettingKey};
use torro_settings_ui::assistant::web;
use torro_settings_ui::rights::{first_sentence, plain, tool_level};

use super::{
    Action, ClientTarget, Data, Header, chevron, confirm, dialog, field, notice, picker, pressable, row_frame,
    setting_row, switch,
};

/// The file endings the logo chooser offers, as `PdfTemplate::check` takes them.
pub const LOGO_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The even margins the picker offers, in millimetres; a margin set otherwise through the API is
/// listed among them ([`margin_options`]).
const EVEN_MARGINS: [u8; 8] = [5, 10, 15, 20, 25, 30, 40, 50];

/// The output field of a server's page shows this many lines before it scrolls.
const OUTPUT_LINES: f32 = 12.0;

/// The arguments field of a server of one's own shows this many lines before it scrolls.
const ARGUMENT_LINES: f32 = 3.0;

/// The ids of the fields of the form of a server of one's own: a try with fields marked wrong puts the
/// focus into the first of them ([`first_marked`]).
const NAME_FIELD: &str = "custom-server-name";
const ID_FIELD: &str = "custom-server-id";
const COMMAND_FIELD: &str = "custom-server-command";
const URL_FIELD: &str = "custom-server-url";

/// The id of the name field of the variable or header in the row `at` of the form.
fn value_field(at: usize) -> iced::widget::Id {
    format!("custom-server-value-{at}").into()
}

/// The id of the value field of the variable or header in the row `at` of the form.
fn value_text_field(at: usize) -> iced::widget::Id {
    format!("custom-server-value-{at}-text").into()
}

/// The field a configuration is pasted into shows this many lines before it scrolls.
const PASTE_LINES: f32 = 10.0;

/// A variable or header whose name holds one of these, in any case, is marked secret at first
/// ([`looks_secret`]): `GITHUB_TOKEN`, `API_KEY`, `Authorization`.
const SECRET_WORDS: [&str; 5] = ["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"];

/// What the core answered to one list request of this section (`extensions.list_catalog`,
/// `extensions.list_extensions`): the entries, or why there are none. The shell fills it with each
/// load ([`Listing::from_result`]); the default is an empty list, as before the first answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing<T> {
    /// The core's answer, in its order.
    Entries(Vec<T>),
    /// The request failed (an older core answers `unsupported`, the socket broke): the text for a
    /// person, shown in the list's group with "Retry".
    Failed(String),
}

impl<T> Default for Listing<T> {
    fn default() -> Self {
        Listing::Entries(Vec::new())
    }
}

impl<T> Listing<T> {
    /// `Ok` as [`Listing::Entries`], `Err` as [`Listing::Failed`].
    pub fn from_result(result: Result<Vec<T>, String>) -> Self {
        match result {
            Ok(entries) => Listing::Entries(entries),
            Err(why) => Listing::Failed(why),
        }
    }

    /// The entries; none when the list failed.
    pub fn entries(&self) -> &[T] {
        match self {
            Listing::Entries(entries) => entries,
            Listing::Failed(_) => &[],
        }
    }
}

/// The page of the section that is shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Page {
    /// The four parts: allowed folders, built-in tools, catalog, own servers.
    #[default]
    List,
    /// "PDF templates", below the PDF tool's row "Templates": Letter, Report and Note.
    PdfTemplates,
    /// "Search providers", below the row of that name of the tool `web`: a group per provider (task WEB.6).
    SearchProviders,
    /// The page of the installed server with this id (task MA.8), below its row in the list.
    Server(String),
}

/// The section's state: the page shown, the header or footer line being typed, why the last
/// chosen path could not be used, the install dialog and what a server's page holds besides the
/// server's data.
#[derive(Debug, Default)]
pub struct State {
    pub page: Page,
    draft: Option<Draft>,
    /// Shown above the page until the next chooser opens.
    problem: Option<String>,
    /// The install dialog of a catalog entry while it is open; also the step of the reported tools
    /// after a server of one's own was added.
    install: Option<Install>,
    /// The form of a server of one's own while it is open (task MA.9). Under the install dialog
    /// while that shows the tools of the server just added, so the next pasted server follows.
    custom: Option<Box<Custom>>,
    /// What the page of a server shows besides its data; new with every page opened.
    server: ServerState,
    /// What is typed into a search provider's key or SearXNG's address, and whether its check waits; new when the
    /// page of the search providers is left.
    web: web::State,
}

/// The install dialog (task MA.8): first the form, then the tools the server reported.
#[derive(Debug)]
pub struct Install {
    /// The catalog entry ([`CatalogEntry::id`]); installed, the server has this id. For a server
    /// of one's own (only at [`Step::Tools`]) the id it was added with.
    entry: String,
    /// What is typed into the fields of the entry's secrets, by name: [`Secret`]s from the first
    /// key, so `Debug` never shows them. Sent with "Install", dropped with the dialog.
    secrets: BTreeMap<String, Secret>,
    /// "Install" was pressed once: an empty secret says so from now on, not before.
    tried: bool,
    step: Step,
    /// A request of the dialog is on its way: its button shows that and takes no second press.
    busy: bool,
    /// Why the last request of the dialog did not go through.
    problem: Option<Problem>,
}

impl Install {
    fn new(entry: String) -> Install {
        Install { entry, secrets: BTreeMap::new(), tried: false, step: Step::Form, busy: false, problem: None }
    }

    /// The id of the installed server whose tools the dialog shows; `None` on the form.
    fn installed(&self) -> Option<&str> {
        match &self.step {
            Step::Form => None,
            Step::Tools(server) => Some(server.id.as_str()),
        }
    }
}

/// The two steps of the install dialog.
#[derive(Debug)]
pub enum Step {
    /// The catalog's tools, the secrets' fields and "Install".
    Form,
    /// The server as the core installed it, switched off: the tools it reported, which "Allow
    /// tools" allows with this list's fingerprint. MA.9 comes here after its own install.
    Tools(Box<ExtensionInfo>),
}

/// Why a request of the dialog did not go through: what that means for the person, when the code
/// says more than the core's sentence, and the core's sentence (in the suite's language).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Problem {
    what: Option<&'static str>,
    why: String,
}

/// The form of a server of one's own (task MA.9): "Add server…" on the list, or "Update…" on the
/// page of one's own server, filled in from it. "Add server" sends it as one
/// `extensions.install_extension` with `custom` ([`submit_custom`]), "Save" as
/// `extensions.update_extension`. `Debug` shows neither the arguments nor the address: a pasted block
/// often carries a token in them (`args`, `?api_key=`).
pub struct Custom {
    /// The id of the server being updated; its name and id stay. `None` while one is added.
    updating: Option<String>,
    kind: Kind,
    name: String,
    id: String,
    /// The person typed the id: it no longer follows the name.
    id_typed: bool,
    command: String,
    /// The arguments of the command, one per line.
    args: text_editor::Content,
    url: String,
    /// The variables of a command or the headers of an address, as listed; kept when the kind
    /// changes, so nothing typed is lost.
    values: Vec<Entry>,
    /// "Add server" or "Save" was pressed once: from now on the fields say what is wrong.
    tried: bool,
    /// The request is on its way: its button says so and takes no second press.
    busy: bool,
    /// Why the core refused the last request.
    problem: Option<Problem>,
    step: CustomStep,
    /// The servers of the block pasted last, each filled into the form on its own ([`Message::ChoosePasted`]).
    pasted: Vec<Pasted>,
    /// Which of [`Custom::pasted`] the form holds: added, it leaves the list.
    chosen: Option<usize>,
}

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Custom")
            .field("updating", &self.updating)
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("id", &self.id)
            .field("id_typed", &self.id_typed)
            .field("command", &self.command)
            .field("args", &format_args!("…"))
            .field("url", &format_args!("…"))
            .field("values", &self.values)
            .field("tried", &self.tried)
            .field("busy", &self.busy)
            .field("problem", &self.problem)
            .field("step", &self.step)
            .field("pasted", &self.pasted)
            .field("chosen", &self.chosen)
            .finish()
    }
}

impl Custom {
    /// An empty form for a new server, a command.
    fn new() -> Custom {
        Custom {
            updating: None,
            kind: Kind::Command,
            name: String::new(),
            id: String::new(),
            id_typed: false,
            command: String::new(),
            args: text_editor::Content::new(),
            url: String::new(),
            values: Vec::new(),
            tried: false,
            busy: false,
            problem: None,
            step: CustomStep::Form,
            pasted: Vec::new(),
            chosen: None,
        }
    }

    /// The form of "Update…" for `server`, filled in from it: its values as they are, a secret as
    /// [`Entry::stored`], which an empty field keeps.
    fn editing(server: &ExtensionInfo) -> Custom {
        let mut custom = Custom::new();
        custom.fill(&Pasted::of(server.name.clone(), server.server.clone(), false));
        (custom.updating, custom.id, custom.id_typed) = (Some(server.id.clone()), server.id.clone(), true);
        custom
    }

    /// The form filled in with `from`, as if typed: nothing tried yet, no refusal.
    fn fill(&mut self, from: &Pasted) {
        self.kind = from.kind;
        self.name.clone_from(&from.name);
        self.command.clone_from(&from.command);
        self.args = text_editor::Content::with_text(&from.args.join("\n"));
        self.url.clone_from(&from.url);
        self.values = from.values.clone();
        (self.tried, self.problem, self.step) = (false, None, CustomStep::Form);
    }

    /// The pasted server at `at` in the form, with an id proposed from its name.
    fn choose(&mut self, at: usize, data: &Data) {
        let Some(from) = self.pasted.get(at).cloned() else { return };
        self.fill(&from);
        self.id = suggested_id(&self.name, |id| server_of(data, id).is_some());
        (self.id_typed, self.chosen) = (false, Some(at));
    }

    /// The name and id as they go out: the name without white space around it.
    fn name(&self) -> &str {
        self.name.trim()
    }
}

/// What a server of one's own is: a program this computer starts, or an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// [`ServerSpec::Stdio`]: a command with arguments and variables.
    Command,
    /// [`ServerSpec::Http`]: an address with headers.
    Address,
}

/// One variable or header of the form.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    name: String,
    /// What is typed: a [`Secret`] whether marked secret or not, so `Debug` never shows it – the
    /// switch may be flipped after the value was typed.
    value: Secret,
    /// Goes out as [`Value::Secret`], the field hides it.
    secret: bool,
    /// The person flipped "Secret": it no longer follows the name ([`looks_secret`]).
    secret_set: bool,
    /// The core keeps a secret under this name for the server being updated: an empty field keeps
    /// it ([`Value::Stored`]).
    stored: bool,
}

impl Entry {
    /// The row of `name` with `value`. `guess`: whether "Secret" follows the name ([`looks_secret`])
    /// for a plain value – a pasted block marks nothing; a server being updated says what it holds.
    fn of(name: &str, value: &Value, guess: bool) -> Entry {
        let (value, secret, stored) = match value {
            Value::Plain(text) => (Secret::new(text.as_str()), guess && looks_secret(name), false),
            Value::Secret(secret) => (secret.clone(), true, false),
            Value::Stored { .. } => (Secret::default(), true, true),
        };
        Entry { name: name.to_string(), value, secret, secret_set: !guess, stored }
    }

    /// A row the person left empty: it goes nowhere and says nothing.
    fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.value.is_blank() && !self.stored
    }

    /// A new name: "Secret" follows it until the person flips it, and a stored secret belonged to
    /// the old name.
    fn rename(&mut self, name: String) {
        if name != self.name {
            self.stored = false;
        }
        if !self.secret_set {
            self.secret = looks_secret(&name);
        }
        self.name = name;
    }

    /// How the value goes out: a secret as typed, or kept when it is stored and nothing is typed;
    /// else plain.
    fn wire(&self) -> Value {
        match (self.secret, self.stored && self.value.is_blank()) {
            (true, true) => Value::Stored { secret: true },
            (true, false) => Value::Secret(self.value.clone()),
            (false, _) => Value::Plain(self.value.expose().to_string()),
        }
    }
}

/// The steps of the form of a server of one's own.
#[derive(Debug)]
enum CustomStep {
    /// The fields.
    Form,
    /// "Paste configuration": the field the block is pasted into, and why the last one could not
    /// be read.
    Paste { text: Hidden, problem: Option<String> },
    /// The pasted block names several servers ([`Custom::pasted`]): one is chosen for the form.
    Choose,
}

/// The text of a pasted block: it may hold tokens, so `Debug` never shows it.
pub struct Hidden(text_editor::Content);

impl fmt::Debug for Hidden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Hidden(…)")
    }
}

/// An edit of a field of several lines of the form (the arguments, the pasted block): `Debug`
/// never shows what it inserts, which may be a token.
#[derive(Clone, PartialEq)]
pub struct Typed(text_editor::Action);

impl Typed {
    /// `text` pasted at the cursor, as the clipboard does: what the demo pastes.
    pub fn paste(text: &str) -> Typed {
        Typed(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(text.to_string()))))
    }
}

impl fmt::Debug for Typed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Typed(…)")
    }
}

/// One server of a pasted block as the form takes it ([`Custom::fill`]): its values as
/// [`Secret`]s at once, and `Debug` leaves out its arguments and address, so the block's tokens
/// never show in `Debug`.
#[derive(Clone, PartialEq)]
struct Pasted {
    name: String,
    kind: Kind,
    command: String,
    args: Vec<String>,
    url: String,
    values: Vec<Entry>,
}

impl fmt::Debug for Pasted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pasted")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("command", &self.command)
            .field("args", &format_args!("…"))
            .field("url", &format_args!("…"))
            .field("values", &self.values)
            .finish()
    }
}

impl Pasted {
    /// The server `name` of a block or of the core, `guess` as for [`Entry::of`].
    fn of(name: String, spec: ServerSpec, guess: bool) -> Pasted {
        let values = spec.values().iter().map(|(name, value)| Entry::of(name, value, guess)).collect();
        match spec {
            ServerSpec::Stdio { command, args, .. } => {
                Pasted { name, kind: Kind::Command, command, args, url: String::new(), values }
            }
            ServerSpec::Http { url, .. } => {
                Pasted { name, kind: Kind::Address, command: String::new(), args: Vec::new(), url, values }
            }
        }
    }

    /// What runs, for its row in the choice: the program with its arguments, or the address.
    fn address(&self) -> String {
        match self.kind {
            Kind::Command => std::iter::once(self.command.as_str())
                .chain(self.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            Kind::Address => self.url.clone(),
        }
    }
}

/// What the form can tell itself is wrong ([`check`]), by field: the sentence for the person.
#[derive(Debug, Default, PartialEq, Eq)]
struct Checked {
    name: Option<&'static str>,
    id: Option<&'static str>,
    /// The command or the address.
    target: Option<&'static str>,
    /// Per row of [`Custom::values`].
    values: Vec<Option<RowProblem>>,
}

/// What is wrong with a row of variables or headers: the sentence for the person, and whether it is
/// about the value (a secret not given) rather than the name, so the focus goes to the field that is
/// wrong ([`first_marked`]) – whatever the sentence says in the suite's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RowProblem {
    text: &'static str,
    at_value: bool,
}

impl RowProblem {
    /// A problem with the row's name.
    fn name(text: &'static str) -> RowProblem {
        RowProblem { text, at_value: false }
    }
}

impl Checked {
    fn any(&self) -> bool {
        self.name.is_some() || self.id.is_some() || self.target.is_some() || self.values.iter().any(Option::is_some)
    }

    /// The sentence at the row `at`.
    fn value(&self, at: usize) -> Option<&'static str> {
        self.values.get(at).copied().flatten().map(|problem| problem.text)
    }
}

/// What a server's page holds besides the server's data, which comes with every reload.
#[derive(Debug, Default)]
struct ServerState {
    /// A request of the page on its way: its button shows that.
    waiting: Option<Waiting>,
    /// Why the last request of the page did not go through, above the page.
    problem: Option<String>,
    /// The server's output (`extensions.read_logs`).
    output: Output,
    /// The dialog that replaces or enters a secret.
    replacing: Option<Replacing>,
    /// "Remove…" asks first.
    removing: bool,
}

/// A request of a server's page on its way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Switch,
    Allow,
    Update,
    Remove,
}

/// The output of a server as the page has it.
#[derive(Debug, Default)]
enum Output {
    /// Asked for, no answer yet.
    #[default]
    Reading,
    /// What the core answered, in a field that can be selected but not changed.
    Lines(text_editor::Content),
    /// Why it could not be read.
    Failed(String),
}

/// The dialog that stores a new value for one secret of a server.
#[derive(Debug)]
struct Replacing {
    /// The variable or header it goes to.
    name: String,
    value: Secret,
    /// "Save" was pressed once: an empty field says so from now on.
    tried: bool,
    busy: bool,
    problem: Option<String>,
}

/// What a request of this section was for ([`Action::AskConnections`]), so its answer acts only for
/// it ([`answered`]): each carries the catalog entry or the server's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// `extensions.install_extension` of the dialog for this catalog entry, or of the form of a
    /// server of one's own with this id.
    Install(String),
    /// `extensions.confirm_tools` of this server, from the dialog or its page.
    Allow(String),
    /// `extensions.get_extension` after the tools changed under the dialog.
    Refresh(String),
    /// `extensions.set_enabled` on the server's page.
    Switch(String),
    /// `extensions.set_secret` from the server's page.
    Secret(String),
    /// `extensions.read_logs` for the server's page.
    Logs(String),
    /// `extensions.update_extension` on the server's page, or of the form "Update…" of one's own.
    Update(String),
    /// `extensions.remove_extension` on the server's page.
    Remove(String),
}

/// A header or footer line being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    template: PdfTemplateId,
    line: Line,
    text: String,
}

/// The two typed lines of a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Header,
    Footer,
}

impl Line {
    /// "Header", "Footer".
    fn label(self) -> &'static str {
        match self {
            Line::Header => tr("Header"),
            Line::Footer => tr("Footer"),
        }
    }

    /// This line of `template`.
    fn of(self, template: &PdfTemplate) -> &str {
        match self {
            Line::Header => &template.header,
            Line::Footer => &template.footer,
        }
    }
}

/// One of the system's choosers, which only the shell opens ([`Action::Pick`], [`pick`]): the path a
/// person chooses there comes back as [`Message::Picked`] with the same chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chooser {
    /// The folder new PDFs go to.
    Folder,
    /// The logo of a PDF template, a picture of [`LOGO_EXTENSIONS`].
    Logo(PdfTemplateId),
    /// A folder the tools that work with files may reach (task WD.4).
    AllowedFolder,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The switch of a tool: on or off for every client (`tool_enabled`).
    Switch(ToolId, bool),
    /// "Read" or "Read and write" of the allowed folder with this path (task WD.4).
    SetFolderAccess(String, FolderAccess),
    /// "Remove" at the allowed folder with this path.
    RemoveFolder(String),
    /// "The assistant may not use it": the assistant's window, whose Access sets its rights.
    Access,
    /// "Choose…" at the folder or at a template's logo: the shell opens the system's chooser.
    Choose(Chooser),
    /// The chooser closed: the path chosen, `None` when it was cancelled.
    Picked(Chooser, Option<PathBuf>),
    /// "Use desktop": no folder of its own any more (`pdf_folder` `null`).
    UseDesktop,
    /// The row "Templates": the page "PDF templates".
    Templates,
    /// The row "Search providers": their page.
    SearchProviders,
    /// A control of the search providers' page ([`web`]).
    Web(web::Message),
    /// The back button of a page below the list, or Escape there.
    Back,
    /// A paper of the template's picker.
    Paper(PdfTemplateId, Paper),
    /// The margins of the template's picker, top, right, bottom, left.
    Margins(PdfTemplateId, [u8; 4]),
    /// A font size of the template's picker, in points.
    FontSize(PdfTemplateId, u8),
    /// What is typed into a header or footer.
    Type(PdfTemplateId, Line, String),
    /// Enter in the line being typed, or its "Save".
    SaveLine,
    /// Escape while a line is typed: back to the stored one.
    DiscardLine,
    /// "Page numbers" on or off.
    PageNumbers(PdfTemplateId, bool),
    /// "Remove" at a template's logo.
    RemoveLogo(PdfTemplateId),
    /// "Retry" in a group whose list failed: load everything again now.
    Retry,
    /// The row of an installed server: its page, which reads its output.
    Open(String),
    /// "Install…" at a catalog entry: the install dialog, only for an entry [`installable`] names.
    Install(String),
    /// What is typed into the dialog's field of the secret with this name.
    TypeSecret(String, Secret),
    /// "Install" in the dialog: `extensions.install_extension` with the secrets typed.
    SubmitInstall,
    /// "Allow tools" in the dialog or on a server's page: `extensions.confirm_tools` for the server
    /// with the fingerprint of the list that is shown ([`ExtensionInfo::fingerprint`]).
    AllowTools(String, String),
    /// "Cancel", "Not now", a click next to the dialog or Escape: closes the dialog on top – the
    /// install dialog, else the form of a server of one's own.
    CloseInstall,
    /// "Add server…" on the list: the empty form of a server of one's own (task MA.9).
    AddServer,
    /// "Update…" on the page of one's own server: the form filled in from it.
    EditServer(String),
    /// Command or address in the form.
    CustomKind(Kind),
    /// What is typed into the name; the id follows it until it is typed itself.
    CustomName(String),
    /// What is typed into the id.
    CustomId(String),
    /// What is typed into the command.
    CustomCommand(String),
    /// An edit of the arguments, one per line.
    CustomArgs(Typed),
    /// What is typed into the address: a [`Secret`] only so `Debug` never shows it, since an address
    /// may carry a token (`?api_key=`); it goes out as plain text.
    CustomUrl(Secret),
    /// "Add variable" or "Add header": an empty row.
    AddValue,
    /// What is typed into the name of the row at this place.
    ValueName(usize, String),
    /// What is typed into the value of the row at this place.
    ValueText(usize, Secret),
    /// "Secret" at the row at this place: on or off.
    ValueSecret(usize, bool),
    /// "Remove" at the row at this place.
    RemoveValue(usize),
    /// "Add server" or "Save" in the form: `extensions.install_extension` or
    /// `extensions.update_extension`, once nothing the form checks is wrong.
    SubmitCustom,
    /// "Paste configuration": the field for an `mcpServers` block.
    OpenPaste,
    /// An edit of the pasted block.
    PasteText(Typed),
    /// "Fill in": the form from the pasted block, or the choice of its servers.
    FillFromPaste,
    /// One server of the pasted block, by its place: into the form.
    ChoosePasted(usize),
    /// "Choose another" in the form: the servers of the pasted block again.
    ShowPasted,
    /// "Back" from the pasted block or the choice, or Escape there: the form.
    BackToForm,
    /// The switch on a server's page.
    Enable(String, bool),
    /// "Replace…" or "Enter…" at the secret with this name on a server's page.
    Replace(String),
    /// What is typed into the field of the secret being replaced.
    TypeReplacement(Secret),
    /// "Save" in the dialog of the secret.
    SubmitReplacement,
    /// "Cancel" there, a click next to it or Escape.
    CloseReplacement,
    /// "Refresh" at the output: read it again.
    ReadOutput,
    /// Selecting in the output field; edits are dropped, the field only shows.
    Output(text_editor::Action),
    /// "Copy" at the output: all of it to the clipboard.
    CopyOutput,
    /// "Update" on a server's page: the catalog's version (`extensions.update_extension`).
    Update(String),
    /// "Remove…" on a server's page: asks first.
    AskRemove,
    /// "Remove server" in the question.
    Remove,
    /// "Cancel" in the question, a click next to it or Escape.
    CancelRemove,
    /// A link of a catalog entry (its website, where to get a secret): the browser.
    Link(String),
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        Message::Switch(tool, on) => Action::Send(switch_request(tool, on)),
        Message::Access => Action::Send(ClientTarget::Assistant.request()),
        Message::Choose(chooser) => {
            state.problem = None;
            Action::Pick(chooser)
        }
        Message::Picked(_, None) => Action::None,
        Message::Picked(chooser, Some(path)) => {
            let request = match chooser {
                Chooser::Folder => folder_request(&path),
                Chooser::Logo(id) => logo_request(data, id, &path),
                Chooser::AllowedFolder => added_folder_request(data, &path),
            };
            match request {
                Some(request) => Action::Send(request),
                None => {
                    state.problem = Some(tr("This path cannot be used.").to_string());
                    Action::None
                }
            }
        }
        Message::UseDesktop => {
            Action::Send(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null })
        }
        Message::SetFolderAccess(path, access) => {
            let stored = data.settings.tools.folders.iter().find(|folder| folder.path == path);
            match stored.is_some_and(|folder| folder.access == access) {
                true => Action::None,
                false => Action::Send(allowed_folder_request(&path, Some(access))),
            }
        }
        Message::RemoveFolder(path) => Action::Send(allowed_folder_request(&path, None)),
        Message::Templates => {
            state.page = Page::PdfTemplates;
            Action::None
        }
        Message::SearchProviders => {
            state.page = Page::SearchProviders;
            Action::None
        }
        // The check's answer comes back here alone ([`checked`]).
        Message::Web(web::Message::CheckAddress) => {
            match web::update(&mut state.web, web::Message::CheckAddress, &data.search_providers) {
                torro_settings_ui::Action::Send(mut requests) if requests.len() == 1 => {
                    requests.pop().map_or(Action::None, Action::CheckAddress)
                }
                other => from_view(other),
            }
        }
        Message::Web(message) => from_view(web::update(&mut state.web, message, &data.search_providers)),
        // Leaving the templates saves a line being typed, so nothing typed is dropped silently; a key typed for a
        // search provider is dropped.
        Message::Back => {
            state.page = Page::List;
            state.server = ServerState::default();
            state.web = web::State::default();
            saved_draft(state, data)
        }
        Message::Paper(id, paper) => changed(data, id, |template| template.paper = paper),
        Message::Margins(id, margins) => changed(data, id, |template| template.margins_mm = margins),
        Message::FontSize(id, points) => changed(data, id, |template| template.font_size_pt = points),
        Message::PageNumbers(id, on) => changed(data, id, |template| template.page_numbers = on),
        Message::RemoveLogo(id) => changed(data, id, |template| template.logo = None),
        Message::Type(id, line, text) => {
            // Starting another line saves the one before, so nothing typed is dropped silently.
            let before = state.draft.take().filter(|draft| draft.template != id || draft.line != line);
            let text = text.chars().filter(|c| !c.is_control()).take(PdfTemplate::MAX_LINE_CHARS).collect();
            state.draft = Some(Draft { template: id, line, text });
            before.and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::Send)
        }
        Message::SaveLine => saved_draft(state, data),
        Message::DiscardLine => {
            state.draft = None;
            Action::None
        }
        Message::Retry => Action::Reload,
        Message::Open(id) => {
            state.page = Page::Server(id.clone());
            state.server = ServerState::default();
            logs_request(id)
        }
        Message::Install(entry) => {
            if installable(data, &entry).is_some() {
                state.install = Some(Install::new(entry));
            }
            Action::None
        }
        Message::TypeSecret(name, value) => {
            if let Some(install) = state.install.as_mut() {
                install.secrets.insert(name, value);
            }
            Action::None
        }
        Message::SubmitInstall => submit_install(state, data),
        Message::AllowTools(id, fingerprint) => {
            if let Some(install) = state.install.as_mut().filter(|install| install.installed() == Some(id.as_str())) {
                (install.busy, install.problem) = (true, None);
            }
            if state.page == Page::Server(id.clone()) {
                (state.server.waiting, state.server.problem) = (Some(Waiting::Allow), None);
            }
            ask(Asked::Allow(id.clone()), Request::ConfirmExtensionTools { id, fingerprint })
        }
        Message::CloseInstall => {
            if state.install.take().is_none() {
                state.custom = None;
            }
            Action::None
        }
        Message::AddServer => {
            state.custom = Some(Box::new(Custom::new()));
            Action::None
        }
        Message::EditServer(id) => {
            if let Some(server) = server_of(data, &id).filter(|server| server.source == Source::Custom) {
                state.custom = Some(Box::new(Custom::editing(server)));
            }
            Action::None
        }
        Message::CustomKind(kind) => edit(state, |custom| custom.kind = kind),
        Message::CustomName(name) => edit(state, |custom| {
            custom.name = name;
            if !custom.id_typed && custom.updating.is_none() {
                custom.id = suggested_id(&custom.name, |id| server_of(data, id).is_some());
            }
        }),
        // An id typed away follows the name again.
        Message::CustomId(id) => edit(state, |custom| (custom.id_typed, custom.id) = (!id.is_empty(), id)),
        Message::CustomCommand(command) => edit(state, |custom| custom.command = command),
        Message::CustomArgs(Typed(action)) => edit(state, |custom| custom.args.perform(action)),
        Message::CustomUrl(url) => edit(state, |custom| custom.url = url.expose().to_string()),
        Message::AddValue => edit(state, |custom| custom.values.push(Entry::default())),
        Message::ValueName(at, name) => edit(state, |custom| {
            if let Some(entry) = custom.values.get_mut(at) {
                entry.rename(name);
            }
        }),
        Message::ValueText(at, value) => edit(state, |custom| {
            if let Some(entry) = custom.values.get_mut(at) {
                entry.value = value;
            }
        }),
        Message::ValueSecret(at, on) => edit(state, |custom| {
            if let Some(entry) = custom.values.get_mut(at) {
                (entry.secret, entry.secret_set) = (on, true);
            }
        }),
        Message::RemoveValue(at) => edit(state, |custom| {
            if at < custom.values.len() {
                custom.values.remove(at);
            }
        }),
        Message::SubmitCustom => submit_custom(state, data),
        Message::OpenPaste => edit(state, |custom| {
            custom.step = CustomStep::Paste { text: Hidden(text_editor::Content::new()), problem: None };
        }),
        Message::PasteText(Typed(action)) => edit(state, |custom| {
            if let CustomStep::Paste { text, .. } = &mut custom.step {
                text.0.perform(action);
            }
        }),
        Message::FillFromPaste => edit(state, |custom| fill_from_paste(custom, data)),
        Message::ChoosePasted(at) => edit(state, |custom| custom.choose(at, data)),
        Message::ShowPasted => edit(state, |custom| {
            if !custom.pasted.is_empty() {
                custom.step = CustomStep::Choose;
            }
        }),
        Message::BackToForm => edit(state, |custom| custom.step = CustomStep::Form),
        Message::Enable(id, enabled) => {
            (state.server.waiting, state.server.problem) = (Some(Waiting::Switch), None);
            ask(Asked::Switch(id.clone()), Request::SetExtensionEnabled { id, enabled })
        }
        Message::Replace(name) => {
            state.server.replacing =
                Some(Replacing { name, value: Secret::default(), tried: false, busy: false, problem: None });
            Action::None
        }
        Message::TypeReplacement(value) => {
            if let Some(replacing) = state.server.replacing.as_mut() {
                replacing.value = value;
            }
            Action::None
        }
        Message::SubmitReplacement => submit_replacement(state),
        Message::CloseReplacement => {
            state.server.replacing = None;
            Action::None
        }
        Message::ReadOutput => match &state.page {
            Page::Server(id) => {
                let id = id.clone();
                state.server.output = Output::Reading;
                logs_request(id)
            }
            _ => Action::None,
        },
        Message::Output(action) => {
            // The field only shows: selecting and copying pass, typing does not.
            if let Output::Lines(content) = &mut state.server.output
                && !action.is_edit()
            {
                content.perform(action);
            }
            Action::None
        }
        Message::CopyOutput => match &state.server.output {
            Output::Lines(content) => Action::Copy(content.text()),
            _ => Action::None,
        },
        Message::Update(id) => {
            (state.server.waiting, state.server.problem) = (Some(Waiting::Update), None);
            ask(Asked::Update(id.clone()), Request::UpdateExtension { id, server: None })
        }
        Message::AskRemove => {
            state.server.removing = true;
            Action::None
        }
        Message::CancelRemove => {
            state.server.removing = false;
            Action::None
        }
        Message::Remove => {
            state.server.removing = false;
            let Page::Server(id) = &state.page else { return Action::None };
            let id = id.clone();
            (state.server.waiting, state.server.problem) = (Some(Waiting::Remove), None);
            ask(Asked::Remove(id.clone()), Request::RemoveExtension { id })
        }
        Message::Link(url) => Action::OpenUrl(url),
    }
}

/// A request of this section, with what it is for.
fn ask(asked: Asked, request: Request) -> Action {
    Action::AskConnections(asked, request)
}

/// `extensions.read_logs` for the page of the server `id`.
fn logs_request(id: String) -> Action {
    ask(Asked::Logs(id.clone()), Request::ReadExtensionLogs { id })
}

/// "Install": nothing while a request is on its way; with a secret of the entry left empty, only
/// the dialog says so from now on ("Install" stays pressable, design skill, section 5); else
/// `extensions.install_extension` with every secret of the entry as typed.
fn submit_install(state: &mut State, data: &Data) -> Action {
    let Some(install) = state.install.as_mut().filter(|install| !install.busy && install.installed().is_none()) else {
        return Action::None;
    };
    let Some(entry) = data.catalog.entries().iter().find(|entry| entry.id == install.entry) else {
        return Action::None;
    };
    install.tried = true;
    let typed = |name: &str| install.secrets.get(name).filter(|secret| !secret.is_blank());
    if entry.secrets.iter().any(|secret| typed(&secret.name).is_none()) {
        return Action::None;
    }
    let secrets: BTreeMap<String, Secret> =
        entry.secrets.iter().filter_map(|secret| Some((secret.name.clone(), typed(&secret.name)?.clone()))).collect();
    (install.busy, install.problem) = (true, None);
    let from = InstallFrom::Catalog { entry: entry.id.clone(), secrets };
    ask(Asked::Install(entry.id.clone()), Request::InstallExtension { from })
}

/// `change` applied to the open form of a server of one's own; nothing without one.
fn edit(state: &mut State, change: impl FnOnce(&mut Custom)) -> Action {
    if let Some(custom) = state.custom.as_deref_mut() {
        change(custom);
    }
    Action::None
}

/// "Fill in": the pasted block read with [`parse_mcp_servers`]. One server goes into the form, several
/// are listed to choose from; a block that cannot be read stays with the sentence why.
fn fill_from_paste(custom: &mut Custom, data: &Data) {
    let CustomStep::Paste { text, problem } = &mut custom.step else { return };
    match parse_mcp_servers(&text.0.text()) {
        Err(why) => *problem = Some(paste_problem(&why)),
        Ok(servers) => {
            custom.pasted = servers.into_iter().map(|(name, spec)| Pasted::of(name, spec, true)).collect();
            match custom.pasted.len() {
                1 => custom.choose(0, data),
                _ => custom.step = CustomStep::Choose,
            }
        }
    }
}

/// "Add server" or "Save" in the form: nothing while it is on its way or off the form; with
/// anything the form checks wrong, only the fields say so from now on ("Add server" stays pressable,
/// design skill, section 5); else exactly one `extensions.install_extension` with `custom` – or
/// `extensions.update_extension` for a server being updated – with the values as [`Entry::wire`]
/// sends them.
fn submit_custom(state: &mut State, data: &Data) -> Action {
    let Some(custom) =
        state.custom.as_deref_mut().filter(|custom| !custom.busy && matches!(custom.step, CustomStep::Form))
    else {
        return Action::None;
    };
    custom.tried = true;
    let checked = check(custom, data);
    if checked.any() {
        return first_marked(custom.kind, &checked).map_or(Action::None, Action::Focus);
    }
    (custom.busy, custom.problem) = (true, None);
    let server = spec(custom);
    match custom.updating.clone() {
        Some(id) => ask(Asked::Update(id.clone()), Request::UpdateExtension { id, server: Some(server) }),
        None => {
            let (id, name) = (custom.id.clone(), custom.name().to_string());
            ask(
                Asked::Install(id.clone()),
                Request::InstallExtension { from: InstallFrom::Custom { id, name, server } },
            )
        }
    }
}

/// The program or address the form describes, as `extensions.install_extension` takes it: the
/// arguments one per line without white space around them and empty lines dropped, the rows left
/// empty dropped, names without white space around them.
fn spec(custom: &Custom) -> ServerSpec {
    let values: BTreeMap<String, Value> = custom
        .values
        .iter()
        .filter(|entry| !entry.is_empty())
        .map(|entry| (entry.name.trim().to_string(), entry.wire()))
        .collect();
    match custom.kind {
        Kind::Command => ServerSpec::Stdio {
            command: custom.command.trim().to_string(),
            args: arguments(&custom.args.text()),
            env: values,
        },
        Kind::Address => ServerSpec::Http { url: custom.url.trim().to_string(), headers: values },
    }
}

/// One argument per line of `text`, without white space around it; empty lines dropped.
fn arguments(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect()
}

/// "Save" in the dialog of a secret: nothing while it is on its way or without a server's page;
/// with an empty field, only the dialog says so; else `extensions.set_secret`.
fn submit_replacement(state: &mut State) -> Action {
    let Page::Server(id) = &state.page else { return Action::None };
    let Some(replacing) = state.server.replacing.as_mut().filter(|replacing| !replacing.busy) else {
        return Action::None;
    };
    replacing.tried = true;
    if replacing.value.is_blank() {
        return Action::None;
    }
    (replacing.busy, replacing.problem) = (true, None);
    let request =
        Request::SetExtensionSecret { id: id.clone(), name: replacing.name.clone(), value: replacing.value.clone() };
    ask(Asked::Secret(id.clone()), request)
}

/// What an answer says for this section.
enum Outcome<'r> {
    /// A server as the core has it now.
    Server(&'r ExtensionInfo),
    /// A server's output.
    Logs(&'r [String]),
    /// Done, nothing to show (`ok`).
    Done,
    /// Refused with the core's code, or no answer (`None`), and the sentence for the person.
    Refused(Option<&'r str>, String),
}

fn outcome(result: &Result<Response, String>) -> Outcome<'_> {
    match result {
        Ok(Response::Extension { extension }) => Outcome::Server(extension),
        Ok(Response::Logs { lines }) => Outcome::Logs(lines),
        Ok(Response::Error { code, message }) => Outcome::Refused(Some(code.as_str()), message.clone()),
        Ok(_) => Outcome::Done,
        Err(message) => Outcome::Refused(None, message.clone()),
    }
}

/// The core answered a request of this section ([`Action::AskConnections`]). It acts only where the
/// dialog or the page that asked is still open for the same entry or server; what it says shows
/// there. Returns what follows: after a list of tools that changed under the dialog
/// (`conflict`), that server read again; else nothing (the shell reloads either way).
pub fn answered(state: &mut State, asked: Asked, result: &Result<Response, String>) -> Action {
    let outcome = outcome(result);
    let removing = matches!(asked, Asked::Remove(_));
    match asked {
        // The form of one's own server that waits for it, for a new server or one being updated.
        Asked::Install(id) if custom_waits(state, None, &id) => {
            custom_installed(state, id, outcome);
            Action::None
        }
        Asked::Update(id) if custom_waits(state, Some(&id), &id) => {
            if let Some(custom) = state.custom.as_deref_mut() {
                custom.busy = false;
                match outcome {
                    Outcome::Refused(code, why) => {
                        custom.problem = Some(Problem { what: custom_problem(code, true), why })
                    }
                    // Saved: the page shows the server as reloaded, with what waits for the person.
                    _ => state.custom = None,
                }
            }
            Action::None
        }
        // Only the form that waits for it takes the answer.
        Asked::Install(entry) => {
            let Some(install) = state
                .install
                .as_mut()
                .filter(|install| install.entry == entry && install.busy && install.installed().is_none())
            else {
                return Action::None;
            };
            install.busy = false;
            match outcome {
                Outcome::Server(server) => install.step = Step::Tools(Box::new(server.clone())),
                Outcome::Refused(code, why) => install.problem = Some(Problem { what: install_problem(code), why }),
                Outcome::Logs(_) | Outcome::Done => {}
            }
            Action::None
        }
        Asked::Allow(id) => {
            if state.page == Page::Server(id.clone()) {
                state.server.waiting = None;
            }
            let in_dialog = state.install.as_ref().is_some_and(|install| install.installed() == Some(id.as_str()));
            match outcome {
                Outcome::Refused(code, why) if in_dialog => {
                    if let Some(install) = state.install.as_mut() {
                        (install.busy, install.problem) = (false, Some(Problem { what: None, why }));
                    }
                    match code == Some("conflict") {
                        true => ask(Asked::Refresh(id.clone()), Request::GetExtension { id }),
                        false => Action::None,
                    }
                }
                Outcome::Refused(_, why) => {
                    if state.page == Page::Server(id) {
                        state.server.problem = Some(why);
                    }
                    Action::None
                }
                // Allowed: the dialog has done its job; the page shows the server as reloaded.
                _ => {
                    if in_dialog {
                        state.install = None;
                    }
                    Action::None
                }
            }
        }
        Asked::Refresh(id) => {
            if let (Some(install), Outcome::Server(server)) = (state.install.as_mut(), outcome)
                && install.installed() == Some(id.as_str())
            {
                install.step = Step::Tools(Box::new(server.clone()));
            }
            Action::None
        }
        Asked::Logs(id) => {
            if state.page == Page::Server(id) {
                state.server.output = match outcome {
                    Outcome::Logs(lines) => Output::Lines(text_editor::Content::with_text(&lines.join("\n"))),
                    Outcome::Refused(_, why) => Output::Failed(why),
                    Outcome::Server(_) | Outcome::Done => Output::Lines(text_editor::Content::new()),
                };
            }
            Action::None
        }
        Asked::Secret(id) => {
            if state.page == Page::Server(id)
                && let Some(replacing) = state.server.replacing.as_mut()
            {
                match outcome {
                    Outcome::Refused(_, why) => (replacing.busy, replacing.problem) = (false, Some(why)),
                    _ => state.server.replacing = None,
                }
            }
            Action::None
        }
        Asked::Switch(id) | Asked::Update(id) | Asked::Remove(id) => {
            if state.page != Page::Server(id) {
                return Action::None;
            }
            state.server.waiting = None;
            match outcome {
                Outcome::Refused(_, why) => state.server.problem = Some(why),
                // Removed: its page has nothing left to show.
                _ if removing => {
                    state.page = Page::List;
                    state.server = ServerState::default();
                }
                _ => {}
            }
            Action::None
        }
    }
}

/// Whether the form of a server of one's own waits for the answer about `id`: the one being
/// updated (`updating`), or a new one with that id (`None`).
fn custom_waits(state: &State, updating: Option<&str>, id: &str) -> bool {
    state.custom.as_deref().is_some_and(|custom| {
        custom.busy && custom.updating.as_deref() == updating && (updating.is_some() || custom.id == id)
    })
}

/// The answer to "Add server": added, the install dialog shows the tools it reported ([`Step::Tools`])
/// over the form, which goes on with the next server of a pasted block or closes; refused, the
/// form says why.
fn custom_installed(state: &mut State, id: String, outcome: Outcome<'_>) {
    let Some(custom) = state.custom.as_deref_mut() else { return };
    custom.busy = false;
    match outcome {
        Outcome::Server(server) => {
            let mut pasted = std::mem::take(&mut custom.pasted);
            if let Some(at) = custom.chosen.filter(|at| *at < pasted.len()) {
                pasted.remove(at);
            }
            state.custom =
                (!pasted.is_empty()).then(|| Box::new(Custom { pasted, step: CustomStep::Choose, ..Custom::new() }));
            state.install = Some(Install { step: Step::Tools(Box::new(server.clone())), ..Install::new(id) });
        }
        Outcome::Refused(code, why) => custom.problem = Some(Problem { what: custom_problem(code, false), why }),
        Outcome::Logs(_) | Outcome::Done => {}
    }
}

/// What a refused "Add server" or "Save" means for the person, by the core's code: the id taken
/// (`conflict`), else that it could not be added or updated – the core's sentence says why (an
/// address it does not reach, a program that does not start, a secret to type again).
fn custom_problem(code: Option<&str>, updating: bool) -> Option<&'static str> {
    match (code, updating) {
        (Some("conflict"), false) => Some(tr("Another server has this id already.")),
        (_, false) => Some(tr("It could not be added.")),
        (_, true) => Some(tr("It could not be updated.")),
    }
}

/// What a refused install means for the person, by the core's code: something to install first
/// (`unsupported`, the core names it), the id taken (`conflict`), else that it did not install.
fn install_problem(code: Option<&str>) -> Option<&'static str> {
    match code {
        Some("unsupported") => Some(tr("Something it needs is missing on this computer.")),
        Some("conflict") => Some(tr("It is installed already.")),
        _ => Some(tr("It could not be installed.")),
    }
}

/// What Escape means here: close the open dialog, drop the line being typed, else leave the page
/// below the list. `None` on the list while nothing is open.
pub fn escape(state: &State) -> Option<Message> {
    if state.install.is_some() {
        return Some(Message::CloseInstall);
    }
    if let Some(custom) = &state.custom {
        return Some(match custom.step {
            CustomStep::Form => Message::CloseInstall,
            CustomStep::Paste { .. } | CustomStep::Choose => Message::BackToForm,
        });
    }
    if state.server.replacing.is_some() {
        return Some(Message::CloseReplacement);
    }
    if state.server.removing {
        return Some(Message::CancelRemove);
    }
    if let Some(typed) = web::escape(&state.web).filter(|_| state.page == Page::SearchProviders) {
        return Some(Message::Web(typed));
    }
    match (&state.draft, &state.page) {
        (Some(_), _) => Some(Message::DiscardLine),
        (None, Page::PdfTemplates | Page::SearchProviders | Page::Server(_)) => Some(Message::Back),
        (None, Page::List) => None,
    }
}

/// The header bar: on a page below the list its name (the templates', a server's) with a back
/// button; on the list "Add server…" (task MA.9), as an icon in a narrow window.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    match &state.page {
        Page::PdfTemplates => {
            Header { sub_page: Some((tr("PDF templates").to_string(), Message::Back)), actions: vec![] }
        }
        Page::SearchProviders => {
            Header { sub_page: Some((tr("Search providers").to_string(), Message::Back)), actions: vec![] }
        }
        Page::Server(id) => {
            let name = server_of(data, id).map_or_else(|| id.clone(), |server| server.name.clone());
            Header { sub_page: Some((name, Message::Back)), actions: vec![] }
        }
        Page::List => {
            let add = Some(Message::AddServer);
            let button = match class {
                SizeClass::Wide | SizeClass::Medium => {
                    widgets::text_button(Some(Icon::Plus), tr("Add server…"), style::button::primary, add)
                }
                _ => widgets::compact_button(Icon::Plus, tr("Add server…"), style::button::primary, add),
            };
            Header { sub_page: None, actions: vec![button] }
        }
    }
}

/// The dialog the section has open, laid over the whole window by the shell: installing (also the
/// tools of a server of one's own just added), the form of a server of one's own, a secret to
/// replace, or the question before a server is removed. `class` is the window's: narrower than
/// [`SizeClass::Medium`] the rows of the form's variables stack ([`entry_row`]).
pub fn overlay<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Option<Element<'a, Message>> {
    if let Some(install) = &state.install {
        return install_dialog(install, data);
    }
    if let Some(custom) = &state.custom {
        return Some(custom_dialog(custom, data, class));
    }
    let Page::Server(id) = &state.page else { return None };
    let server = server_of(data, id)?;
    if let Some(replacing) = &state.server.replacing {
        return Some(replace_dialog(replacing, server));
    }
    state.server.removing.then(|| {
        confirm(
            tr("Remove this MCP server?"),
            trf(
                "{name} stops, and its settings, its secrets and what clients may do with it are deleted. You can install it again later.",
                &[("name", &server.name)],
            ),
            tr("Remove server"),
            Message::Remove,
            Message::CancelRemove,
        )
    })
}

/// The count after the section in the sidebar and the menu: the servers installed.
pub fn count(data: &Data) -> usize {
    data.extensions.entries().len()
}

/// Opens the system's chooser `chooser` and waits for it: the folder or picture chosen, `None` when
/// it was cancelled. Run by the shell for [`Action::Pick`], never in the demo.
pub async fn pick(chooser: Chooser) -> Option<PathBuf> {
    let dialog = rfd::AsyncFileDialog::new();
    let chosen = match chooser {
        Chooser::Folder => dialog.set_title(tr("Choose a folder for PDFs")).pick_folder().await,
        Chooser::Logo(_) => {
            dialog.set_title(tr("Choose a logo")).add_filter(tr("Pictures"), &LOGO_EXTENSIONS).pick_file().await
        }
        Chooser::AllowedFolder => dialog.set_title(tr("Choose a folder for the tools")).pick_folder().await,
    };
    chosen.map(|file| file.path().to_path_buf())
}

// ----- Requests ----------------------------------------------------------------------------------

/// `settings.set_setting tool_enabled` for `tool`: exactly one request per flip of its switch.
pub fn switch_request(tool: ToolId, on: bool) -> Request {
    Request::SetSetting {
        key: SettingKey::ToolEnabled,
        value: serde_json::json!({ "tool": tool.name(), "enabled": on }),
    }
}

/// `settings.set_setting allowed_folder` for the folder at `path` (task WD.4): with `access` it is allowed (or its
/// access changed), without it removed. Only a person sends it (point 9).
pub fn allowed_folder_request(path: &str, access: Option<FolderAccess>) -> Request {
    Request::SetSetting { key: SettingKey::AllowedFolder, value: serde_json::json!({ "path": path, "access": access }) }
}

/// The request for a folder the person chose with "Add folder…": a new one to read, one allowed already as it is.
/// `None` for a path that is not plain text.
pub fn added_folder_request(data: &Data, folder: &Path) -> Option<Request> {
    let path = folder.to_str()?;
    let stored = data.settings.tools.folders.iter().find(|allowed| Path::new(&allowed.path) == folder);
    Some(allowed_folder_request(path, Some(stored.map_or(FolderAccess::Read, |allowed| allowed.access))))
}

/// `settings.set_setting pdf_template` with the whole template `id`: what every change of a field
/// sends, so the template is replaced by itself with that one field changed.
pub fn template_request(id: PdfTemplateId, template: &PdfTemplate) -> Request {
    Request::SetSetting { key: SettingKey::PdfTemplate, value: template.to_value(id) }
}

/// `settings.set_setting pdf_folder` with the folder the person chose. `None` for a path that is not
/// plain text (it could not be stored as the person sees it).
pub fn folder_request(folder: &Path) -> Option<Request> {
    let path = folder.to_str()?;
    Some(Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::String(path.to_string()) })
}

/// `settings.set_setting pdf_template` with the template `id` as `data` holds it now and `logo` as
/// its logo. `None` for a path that is not plain text.
pub fn logo_request(data: &Data, id: PdfTemplateId, logo: &Path) -> Option<Request> {
    let logo = logo.to_str()?.to_string();
    let mut template = data.settings.tools.pdf.template(id).clone();
    template.logo = Some(logo);
    Some(template_request(id, &template))
}

/// The whole template `id` from `data` with `change` applied, as one request.
fn changed(data: &Data, id: PdfTemplateId, change: impl FnOnce(&mut PdfTemplate)) -> Action {
    let mut template = data.settings.tools.pdf.template(id).clone();
    change(&mut template);
    Action::Send(template_request(id, &template))
}

/// The line being typed, sent and let go; nothing when none is typed or it is what is stored.
fn saved_draft(state: &mut State, data: &Data) -> Action {
    state.draft.take().and_then(|draft| line_request(data, &draft)).map_or(Action::None, Action::Send)
}

/// The request for a typed line; `None` when it is what is stored already.
fn line_request(data: &Data, draft: &Draft) -> Option<Request> {
    let mut template = data.settings.tools.pdf.template(draft.template).clone();
    let stored = match draft.line {
        Line::Header => &mut template.header,
        Line::Footer => &mut template.footer,
    };
    if *stored == draft.text {
        return None;
    }
    stored.clone_from(&draft.text);
    Some(template_request(draft.template, &template))
}

// ----- Views -------------------------------------------------------------------------------------

/// The page: why a chosen path was not taken, then the list's four parts or the templates. In a
/// narrow window the buttons are icons with a tooltip, so the labels keep their room (design skill,
/// section 6).
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let words = matches!(class, SizeClass::Wide | SizeClass::Medium);
    let body: Element<'a, Message> = match &state.page {
        Page::List => {
            column![folders_group(data, words), tools_group(data, words), catalog_group(data, words), own_group(data)]
                .spacing(layout::section_gap())
                .into()
        }
        Page::PdfTemplates => templates_page(state, data, words),
        Page::SearchProviders => providers_page(state, data, class),
        Page::Server(id) => server_page(state, data, id, words),
    };
    column![].push(state.problem.clone().map(notice)).push(body).spacing(layout::section_gap()).into()
}

/// "Built-in tools": per tool its switch with what it does and whether the assistant may use it, for
/// the PDF tool the folder its files go to and the row to its templates, for the tool `web` the row to
/// its search providers.
fn tools_group<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let mut rows = Vec::new();
    for info in page_order(&data.tools) {
        rows.push(switch_row(info));
        rows.push(access_row(data, info.id));
        if works_in_allowed_folders(info.id) && data.settings.tools.folders.is_empty() {
            rows.push(no_folder_row(no_folder_flagged(data), words));
        }
        if info.id == ToolId::Pdf {
            rows.push(folder_row(data, words));
            rows.push(templates_row());
        }
        if info.id == ToolId::Web {
            rows.push(providers_row(data));
        }
    }
    if rows.is_empty() {
        rows.push(setting_row(None, tr("No tools"), None, Space::new()));
    }
    let footer = tr(
        "A tool works only while it is on here and a client's rights allow it. Only you can switch it on and choose the folder and the logos.",
    );
    widgets::group(tr("Built-in tools"), rows, Some(footer.to_string()))
}

/// A tool's switch, with what it does below its name.
fn switch_row(info: &ToolInfo) -> Element<'_, Message> {
    let id = info.id;
    wrapping_row(switch_label(id), info.description.as_str(), switch(info.enabled, move |on| Message::Switch(id, on)))
}

/// A row whose line below the label wraps instead of being cut: what a tool does is a sentence a
/// narrow window must not cut off. With room for it on one line it is as high as a [`setting_row`]
/// with a detail; it grows with each further line.
fn wrapping_row<'a>(label: &'a str, detail: &'a str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let lines = column![
        widgets::ellipsized(typo::body(), label, theme::text),
        typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph)
    ]
    .width(Fill);
    let text = row![tall_strut(), lines].width(Fill).align_y(Alignment::Center);
    let content = row![text, control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding(tall_padding()).into()
}

/// The padding of every row of this section whose text may take several lines (a tool, a status, a
/// catalog entry, a variable, a field of the form): [`space::m()`] above and below, the sides of a
/// [`setting_row`], so they all keep the same room around their text.
fn tall_padding() -> [f32; 2] {
    [space::m(), space::l()]
}

/// A strut as high as a [`setting_row`] with a detail inside [`tall_padding`]: that row is
/// `header + m` high, the padding takes `m` above and below, which leaves `header − m` (64 + 12 −
/// 2 × 12 = 52 in the standard), so a row of several lines with one line of detail is exactly as
/// high as the rows around it.
fn tall_strut() -> Space {
    Space::new().height(size::header() - space::m())
}

/// What the switch of a tool is called.
fn switch_label(id: ToolId) -> &'static str {
    match id {
        ToolId::Pdf => tr("Make PDFs"),
        ToolId::Web => tr("Search the web"),
        ToolId::Files => tr("Organize files"),
    }
}

/// Whether the assistant may use the tool: a green dot, or an orange one (the tool may be on, but
/// the assistant may not use it: something to do) and a row that brings the assistant's window to
/// the front, where Access chooses its rights – the arrow of another window.
fn access_row<'a>(data: &Data, id: ToolId) -> Element<'a, Message> {
    if assistant_may_use(data, id) {
        return setting_row(Some(widgets::status_dot(Dot::Ok)), tr("The assistant may use it"), None, Space::new());
    }
    let detail = tr("Allow it in the assistant under Access");
    let row = setting_row(
        Some(widgets::status_dot(Dot::Attention)),
        tr("The assistant may not use it"),
        Some(detail.to_string()),
        arrow(),
    );
    pressable(row, Some(Message::Access))
}

/// "Folder": where new PDFs go, the desktop without one; "Choose…" and, with one, "Use desktop".
fn folder_row<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let folder = data.settings.tools.pdf.folder.as_deref();
    let detail = folder.map_or_else(|| tr("Desktop").to_string(), str::to_string);
    let mut buttons = row![choose_button(words, Chooser::Folder)].spacing(space::s());
    if folder.is_some() {
        buttons = buttons.push(match words {
            true => widgets::text_button(None, tr("Use desktop"), style::button::ghost, Some(Message::UseDesktop)),
            // Not the ✕: nothing is deleted, the folder goes back to the desktop.
            false => widgets::icon_button(Icon::History, tr("Use desktop"), Some(Message::UseDesktop)),
        });
    }
    setting_row(None, tr("Folder"), Some(detail), buttons.align_y(Alignment::Center))
}

/// "Templates": the names of the PDF templates, and the page they are set on.
fn templates_row<'a>() -> Element<'a, Message> {
    let names: Vec<&str> = PdfTemplateId::ALL.into_iter().map(template_name).collect();
    let row = setting_row(None, tr("Templates"), Some(names.join(", ")), chevron());
    pressable(row, Some(Message::Templates))
}

/// "Allowed folders" (task WD.4): a row per folder with its access and "Remove", and "Add folder…" while there is room
/// for another ([`MAX_ALLOWED_FOLDERS`]).
fn folders_group<'a>(data: &'a Data, words: bool) -> Element<'a, Message> {
    let folders = &data.settings.tools.folders;
    let mut rows: Vec<Element<'a, Message>> = folders.iter().map(|folder| allowed_folder_row(folder, words)).collect();
    let room = folders.len() < MAX_ALLOWED_FOLDERS;
    let plus = icon::icon(Icon::Plus, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_2) });
    let add = setting_row(Some(plus.into()), tr("Add folder…"), None, Space::new());
    rows.push(pressable(add, room.then_some(Message::Choose(Chooser::AllowedFolder))));
    widgets::group(tr("Allowed folders"), rows, Some(folders_footer(folders.len())))
}

/// One allowed folder: its name, its path below, the picker "Read" / "Read and write" and "Remove".
fn allowed_folder_row<'a>(folder: &'a AllowedFolder, words: bool) -> Element<'a, Message> {
    let path = folder.path.as_str();
    let access = picker(AccessChoice::ALL.to_vec(), AccessChoice(folder.access), move |choice: AccessChoice| {
        Message::SetFolderAccess(path.to_string(), choice.0)
    });
    let remove = Message::RemoveFolder(folder.path.clone());
    let remove = match words {
        true => widgets::text_button(None, tr("Remove"), style::button::ghost, Some(remove)),
        false => widgets::icon_button(Icon::X, tr("Remove"), Some(remove)),
    };
    let control = row![access, remove].spacing(space::s()).align_y(Alignment::Center);
    setting_row(Some(folder_mark()), file_name(path), Some(path.to_string()), control)
}

/// The mark of a folder row: the folder icon in `text_2`.
fn folder_mark<'a>() -> Element<'a, Message> {
    icon::icon(Icon::Folder, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_2) }).into()
}

/// Under the tool `files` while no folder is allowed: it works in none yet, with "Add folder…" right there; an orange
/// dot when the assistant may use the tool (something to do), else the folder mark.
fn no_folder_row<'a>(flagged: bool, words: bool) -> Element<'a, Message> {
    let add = Message::Choose(Chooser::AllowedFolder);
    let button = match words {
        true => widgets::text_button(Some(Icon::Plus), tr("Add folder…"), style::button::ghost, Some(add)),
        false => widgets::icon_button(Icon::Plus, tr("Add folder…"), Some(add)),
    };
    let detail = tr("The tool works only in allowed folders.");
    let mark = match flagged {
        true => widgets::status_dot(Dot::Attention),
        false => folder_mark(),
    };
    setting_row(Some(mark), tr("No allowed folders"), Some(detail.to_string()), button)
}

/// Whether "No allowed folders" carries the orange dot: once the assistant may use the tool `files`, the missing
/// folder is what is left to do; before, the row to its Access has the one dot of the tool.
fn no_folder_flagged(data: &Data) -> bool {
    assistant_may_use(data, ToolId::Files)
}

/// What the allowed folders' group says below it.
const FOLDERS_FOOTER: &str =
    "Tools like Files work only in these folders; ArtCraft will use them too. Only you can change this list.";

/// What the allowed folders' group says below it, with the limit once it is reached.
fn folders_footer(count: usize) -> String {
    let footer = tr(FOLDERS_FOOTER);
    match count < MAX_ALLOWED_FOLDERS {
        true => footer.to_string(),
        false => {
            let limit = trf("At most {n} folders. Remove one to add another.", &[("n", &MAX_ALLOWED_FOLDERS)]);
            format!("{footer} {limit}")
        }
    }
}

/// The tools in the order of the group: those that work in the allowed folders first, next to them, then the others
/// in the core's order.
pub fn page_order(tools: &[ToolInfo]) -> Vec<&ToolInfo> {
    let mut ordered: Vec<&ToolInfo> = tools.iter().collect();
    ordered.sort_by_key(|info| !works_in_allowed_folders(info.id));
    ordered
}

/// Whether `tool` works in the allowed folders: every tool is placed here on purpose, so a new one that works with
/// files stands next to them.
fn works_in_allowed_folders(tool: ToolId) -> bool {
    match tool {
        ToolId::Files => true,
        ToolId::Pdf | ToolId::Web => false,
    }
}

/// "Search providers": the providers switched on, in the order they are asked, and the page they are set on.
fn providers_row(data: &Data) -> Element<'_, Message> {
    let on: Vec<&str> =
        data.search_providers.iter().filter(|info| info.enabled).map(|info| info.name.as_str()).collect();
    let detail = match on.is_empty() {
        true => tr("No provider is on, so the web search is off.").to_string(),
        false => on.join(", "),
    };
    let row = setting_row(None, tr("Search providers"), Some(detail), chevron());
    pressable(row, Some(Message::SearchProviders))
}

/// "Search providers": the order they are asked in and that the assistant asks first ([`web::summary`]), then a
/// group per provider ([`web::view`]).
fn providers_page<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let summary = typo::text(typo::meta(), web::summary(&data.search_providers))
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph);
    let groups = web::view(&state.web, &data.search_providers, class).map(Message::Web);
    column![summary, groups].spacing(layout::section_gap()).into()
}

/// A reload of `data`: SearXNG's check ends once its address is stored ([`web::sent`]).
pub fn loaded(state: &mut State, data: &Data) {
    web::sent(&mut state.web, Ok(web::stored_address(&data.search_providers)));
}

/// The answer to the check of a SearXNG address ([`Action::CheckAddress`]) while it waits here: a refusal shows its
/// reason under the field; a success ends the check with the next reload, once the address is stored. `true` when
/// this page shows it (the shell then shows no notice).
pub fn checked(state: &mut State, result: &Result<Response, String>) -> bool {
    if !web::checking(&state.web) {
        return false;
    }
    if let Err(problem) = result {
        web::sent(&mut state.web, Err(problem));
    }
    true
}

/// What the search providers' view asks for, as the shell's [`Action`]: one request, several, or the terms to open.
fn from_view(action: torro_settings_ui::Action<()>) -> Action {
    match action {
        torro_settings_ui::Action::Send(mut requests) if requests.len() == 1 => {
            requests.pop().map_or(Action::None, Action::Send)
        }
        torro_settings_ui::Action::Send(requests) if !requests.is_empty() => Action::SendAll(requests),
        torro_settings_ui::Action::OpenUrl(url) => Action::OpenUrl(url),
        _ => Action::None,
    }
}

/// "PDF templates": a group per template; the last one says below it what header and footer fill in.
fn templates_page<'a>(state: &'a State, data: &'a Data, words: bool) -> Element<'a, Message> {
    let last = PdfTemplateId::ALL.len() - 1;
    let groups =
        PdfTemplateId::ALL.into_iter().enumerate().map(|(at, id)| template_group(state, data, id, words, at == last));
    column(groups).spacing(layout::section_gap()).into()
}

/// One template: paper, margins, font size, header, footer, page numbers and logo.
fn template_group<'a>(
    state: &'a State,
    data: &'a Data,
    id: PdfTemplateId,
    words: bool,
    last: bool,
) -> Element<'a, Message> {
    let template = data.settings.tools.pdf.template(id);
    let paper = picker(PaperChoice::ALL.to_vec(), PaperChoice(template.paper), move |choice: PaperChoice| {
        Message::Paper(id, choice.0)
    });
    let margins = picker(margin_options(template.margins_mm), Margins(template.margins_mm), move |choice: Margins| {
        Message::Margins(id, choice.0)
    });
    let sizes: Vec<Points> = (PdfTemplate::MIN_FONT_PT..=PdfTemplate::MAX_FONT_PT).map(Points).collect();
    let font = picker(sizes, Points(template.font_size_pt), move |choice: Points| Message::FontSize(id, choice.0));
    let numbers = switch(template.page_numbers, move |on| Message::PageNumbers(id, on));
    let example = trf("Page {n} of {m}", &[("n", &1), ("m", &3)]);
    let rows = vec![
        setting_row(None, tr("Paper"), None, paper),
        setting_row(None, tr("Margins"), None, margins),
        setting_row(None, tr("Font size"), None, font),
        line_row(state, id, Line::Header, template, words),
        line_row(state, id, Line::Footer, template, words),
        setting_row(None, tr("Page numbers"), Some(example), numbers),
        logo_row(id, template, words),
    ];
    let footer = last.then(|| {
        tr("In header and footer, {title} becomes the title of the PDF and {date} the day it is made.").to_string()
    });
    widgets::group(template_name(id), rows, footer)
}

/// A header or footer: the field with what is stored or being typed, and "Save" – always there, so
/// the field keeps its width while typing; it can be pressed only while the line differs.
fn line_row<'a>(
    state: &'a State,
    id: PdfTemplateId,
    line: Line,
    template: &'a PdfTemplate,
    words: bool,
) -> Element<'a, Message> {
    let stored = line.of(template);
    let typing = state.draft.as_ref().filter(|draft| draft.template == id && draft.line == line);
    let value = typing.map_or(stored, |draft| draft.text.as_str());
    // Narrow, the field shares its row with the label and the check: only the example fits.
    let placeholder = if words { tr("For example {title} · {date}") } else { tr("{title} · {date}") };
    let input =
        field(placeholder, value).on_input(move |text| Message::Type(id, line, text)).on_submit(Message::SaveLine);
    let save = typing.is_some_and(|draft| draft.text != stored).then_some(Message::SaveLine);
    let button = match words {
        true => widgets::text_button(None, tr("Save"), style::button::secondary, save),
        false => widgets::icon_button(Icon::Check, tr("Save"), save),
    };
    let control = row![input, button].spacing(space::s()).align_y(Alignment::Center);
    setting_row(None, line.label(), None, control)
}

/// "Logo": the file's name or "No logo"; "Choose…" and, with one, "Remove".
fn logo_row<'a>(id: PdfTemplateId, template: &PdfTemplate, words: bool) -> Element<'a, Message> {
    let detail = template.logo.as_deref().map_or_else(|| tr("No logo").to_string(), file_name);
    let mut buttons = row![choose_button(words, Chooser::Logo(id))].spacing(space::s());
    if template.logo.is_some() {
        let remove = Message::RemoveLogo(id);
        buttons = buttons.push(match words {
            true => widgets::text_button(None, tr("Remove"), style::button::ghost, Some(remove)),
            false => widgets::icon_button(Icon::X, tr("Remove"), Some(remove)),
        });
    }
    setting_row(None, tr("Logo"), Some(detail), buttons.align_y(Alignment::Center))
}

/// "Choose…", with its word or as a folder icon.
fn choose_button<'a>(words: bool, chooser: Chooser) -> Element<'a, Message> {
    let message = Message::Choose(chooser);
    match words {
        true => widgets::text_button(None, tr("Choose…"), style::button::secondary, Some(message)),
        false => widgets::icon_button(Icon::Folder, tr("Choose…"), Some(message)),
    }
}

/// "Catalog": a row per entry of the core's catalog ([`catalog_row`]), then the installed catalog
/// servers whose entry the catalog no longer has. A failed list is a sentence with "Retry".
fn catalog_group(data: &Data, words: bool) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = match &data.catalog {
        Listing::Failed(why) => vec![failed_row(tr("The catalog could not be listed"), why)],
        Listing::Entries(entries) => entries.iter().map(|entry| catalog_row(data, entry, words)).collect(),
    };
    let known = |entry: &str| data.catalog.entries().iter().any(|listed| listed.id == entry);
    rows.extend(
        data.extensions
            .entries()
            .iter()
            .filter(|server| matches!(&server.source, Source::Catalog { entry, .. } if !known(entry)))
            .map(|server| server_row(server, tr("No longer in the catalog").to_string())),
    );
    if rows.is_empty() {
        rows.push(setting_row(None, tr("The catalog is empty"), None, Space::new()));
    }
    let footer = tr("MCP servers Torro Workplace knows, each in a fixed version. They run as you on this computer.");
    widgets::group(tr("Catalog"), rows, Some(footer.to_string()))
}

/// "Your own servers": a row per server a person added, with what runs and its status; without any,
/// one sentence. A failed list is a sentence with "Retry".
fn own_group(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = match &data.extensions {
        Listing::Failed(why) => vec![failed_row(tr("The MCP servers could not be listed"), why)],
        Listing::Entries(servers) => servers
            .iter()
            .filter(|server| server.source == Source::Custom)
            .map(|server| server_row(server, address(&server.server)))
            .collect(),
    };
    if rows.is_empty() {
        rows.push(setting_row(None, tr("No servers of your own yet"), None, Space::new()));
    }
    let footer = tr("Programs or addresses that speak MCP, added by you. They run as you on this computer.");
    widgets::group(tr("Your own servers"), rows, Some(footer.to_string()))
}

/// An installed server: its name, `detail` below it, its status at the end and the chevron to its
/// page; the whole row opens the page.
fn server_row<'a>(server: &'a ExtensionInfo, detail: String) -> Element<'a, Message> {
    let control = row![status_words(server), chevron()].spacing(space::s()).align_y(Alignment::Center);
    pressable(setting_row(None, server.name.as_str(), Some(detail), control), Some(Message::Open(server.id.clone())))
}

/// How a server stands, at the end of its row: a dot and a word, so it never rests on colour alone.
fn status_words<'a>(server: &ExtensionInfo) -> Element<'a, Message> {
    let (dot, word) = status_of(server);
    row![widgets::status_dot(dot), typo::text(typo::meta(), word).style(theme::text_2).wrapping(Wrapping::None)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// A catalog entry: its name (with "Experimental" where the publisher says so), what it does, who
/// makes it under which licence in which version, and what of it is missing on this computer, in the
/// attention colour with what to install. At the end the installed server's status and the chevron
/// to its page (the whole row opens it; the website is there), else its website and "Install…" when
/// nothing is missing – never the arrow of the website next to the chevron.
fn catalog_row<'a>(data: &'a Data, entry: &'a CatalogEntry, words: bool) -> Element<'a, Message> {
    let server = installed(data, &entry.id);
    let mut name = row![widgets::ellipsized(typo::body(), entry.name.as_str(), theme::text)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if entry.experimental {
        name = name.push(widgets::pill(None, tr("Experimental")));
    }
    let mut lines = column![
        name,
        typo::text(typo::meta(), entry.description.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        widgets::ellipsized(typo::meta(), facts(entry), theme::text_3),
    ]
    .width(Fill);
    if server.is_none() && !entry.missing.is_empty() {
        let needs =
            typo::text(typo::meta(), missing_sentence(entry)).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
        let line = row![widgets::status_dot(Dot::Attention), needs].spacing(space::s()).align_y(Alignment::Center);
        lines = lines.push(container(line).padding([space::xs(), 0.0]));
    }
    let mut controls = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some(server) = server {
        controls = controls.push(status_words(server)).push(chevron());
    } else {
        controls = controls.push(website_button(&entry.url));
    }
    if server.is_none() && installable(data, &entry.id).is_some() {
        let install = Message::Install(entry.id.clone());
        controls = controls.push(match words {
            true => widgets::text_button(None, tr("Install…"), style::button::secondary, Some(install)),
            false => widgets::icon_button(Icon::Plus, tr("Install…"), Some(install)),
        });
    }
    let content = row![lines, controls].spacing(space::l()).align_y(Alignment::Center);
    let row = container(content).width(Fill).padding(tall_padding());
    match server {
        Some(server) => pressable(row.into(), Some(Message::Open(server.id.clone()))),
        None => row.into(),
    }
}

/// "Model Context Protocol · MIT · version 2026.10.19": who makes an entry, its licence and the
/// version the catalog installs.
fn facts(entry: &CatalogEntry) -> String {
    trf(
        "{publisher} · {license} · version {version}",
        &[("publisher", &entry.publisher), ("license", &entry.license), ("version", &entry.version)],
    )
}

/// What a catalog entry misses, as the sentence to act on: "Needs Docker. Install it to use this
/// server."
fn missing_sentence(entry: &CatalogEntry) -> String {
    let needs: Vec<String> = entry.missing.iter().map(|what| what.needs()).collect();
    trf("{needs}. Install it to use this server.", &[("needs", &needs.join(" · "))])
}

// ----- A server's page ---------------------------------------------------------------------------

/// The page of the server `id`: a refusal of the page's last request, then its status, tools,
/// secrets (when it has any), output and "Remove…". A server no longer installed (removed elsewhere)
/// is one sentence.
fn server_page<'a>(state: &'a State, data: &'a Data, id: &str, words: bool) -> Element<'a, Message> {
    let Some(server) = server_of(data, id) else {
        let gone = setting_row(None, tr("This MCP server isn't installed any more"), None, Space::new());
        return widgets::group(tr("Status"), vec![gone], None);
    };
    let page = &state.server;
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    if let Some(problem) = &page.problem {
        parts.push(notice(problem.clone()));
    }
    parts.push(status_group(page, data, server, words));
    parts.push(tools_of_server(page, server, words));
    if !server.secrets.is_empty() {
        parts.push(secrets_group(server, words));
    }
    parts.push(output_group(page));
    parts.push(remove_group(page, words));
    column(parts).spacing(layout::section_gap()).into()
}

/// "Status": how the server stands with its switch, and where it comes from with "Update" when the
/// catalog has another version. Below it, who may use it: the clients' rights, set elsewhere.
fn status_group<'a>(
    page: &ServerState,
    data: &'a Data,
    server: &'a ExtensionInfo,
    words: bool,
) -> Element<'a, Message> {
    let (dot, word) = status_of(server);
    // A server whose tools were never allowed cannot be switched on: "Allow tools" does that.
    let never_allowed = !server.enabled && !server.tools.iter().any(|tool| tool.state == ToolState::Allowed);
    let id = server.id.clone();
    // Without a message the switch is drawn disabled; `switch` always takes one.
    let mut switch = toggler(server.enabled).size(style::CHECK_SIZE).style(style::toggler);
    if !never_allowed && page.waiting.is_none() {
        switch = switch.on_toggle(move |on| Message::Enable(id.clone(), on));
    }
    let rows = vec![
        status_row(widgets::status_dot(dot), word, status_detail(server, never_allowed), switch),
        source_row(page, data, server, words),
    ];
    let footer = tr("Who may use it: API, MCP clients and the assistant's Access.");
    widgets::group(tr("Status"), rows, Some(footer.to_string()))
}

/// A row with a mark, a label and a sentence below it that wraps instead of being cut ([`wrapping_row`]
/// with a mark): what a server's status means is a sentence a narrow window must not cut off.
fn status_row<'a>(
    mark: Element<'a, Message>,
    label: String,
    detail: Option<String>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut lines = column![widgets::ellipsized(typo::body(), label, theme::text)].width(Fill);
    if let Some(detail) = detail {
        lines = lines.push(typo::text(typo::meta(), detail).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let text = row![tall_strut(), mark, lines].spacing(space::m()).width(Fill).align_y(Alignment::Center);
    let content = row![text, control.into()].spacing(space::l()).align_y(Alignment::Center);
    container(content).width(Fill).padding(tall_padding()).into()
}

/// What the status of a server means and what a person can do, below its word; `None` where the
/// word says it all.
fn status_detail(server: &ExtensionInfo, never_allowed: bool) -> Option<String> {
    match &server.status {
        Status::Review if never_allowed => Some(tr("Allow its tools below to switch it on.").into()),
        Status::Review => Some(tr("Changed tools wait for you below; the others are offered as before.").into()),
        Status::Crashed { reason } => {
            Some(format!("{} {}", crash_reason(*reason), tr("Switch it off and on again to try again.")))
        }
        Status::Missing { what } => Some(trf("{needs}. Install it to use this server.", &[("needs", &what.needs())])),
        _ if !server.enabled => Some(tr("None of its tools is offered.").into()),
        Status::Stopped => Some(tr("It starts when one of its tools is called.").into()),
        Status::Running | Status::Starting => None,
    }
}

/// Why a server stopped, in the words the core uses for it.
fn crash_reason(reason: CrashReason) -> &'static str {
    match reason {
        CrashReason::Exited => tr("The MCP server stopped unexpectedly."),
        CrashReason::StartTimeout => tr("The MCP server didn't start in time."),
        CrashReason::SpawnFailed => tr("The MCP server's program couldn't be started."),
        CrashReason::Protocol => tr("The MCP server sent something Torro Workplace doesn't understand."),
    }
}

/// Where the server comes from: a catalog entry in its version with its website, and "Update" when
/// the catalog has another one; one's own with its program or address and "Update…", the form filled
/// in from it (task MA.9).
fn source_row<'a>(page: &ServerState, data: &'a Data, server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let Source::Catalog { entry, version } = &server.source else {
        let label = match server.server {
            ServerSpec::Stdio { .. } => tr("Program"),
            ServerSpec::Http { .. } => tr("Address"),
        };
        let press = page.waiting.is_none().then(|| Message::EditServer(server.id.clone()));
        let button = match words {
            true => widgets::text_button(None, tr("Update…"), style::button::secondary, press),
            false => widgets::icon_button(Icon::SquarePen, tr("Update…"), press),
        };
        return setting_row(None, label, Some(address(&server.server)), button);
    };
    let label = trf("Version {version}", &[("version", version)]);
    let from = data.catalog.entries().iter().find(|listed| &listed.id == entry);
    let website = from.map(|entry| website_button(&entry.url));
    let Some(newer) = update_available(data, server) else {
        let detail = from.map_or_else(
            || tr("No longer in the catalog").to_string(),
            |entry| format!("{} · {}", entry.publisher, entry.license),
        );
        let control = row![].push(website).align_y(Alignment::Center);
        return setting_row(None, label, Some(detail), control);
    };
    let detail = trf("Version {version} is in the catalog", &[("version", &newer)]);
    let button: Element<'a, Message> = match (page.waiting, words) {
        (Some(Waiting::Update), _) => widgets::busy_button(tr("Updating…"), style::button::secondary),
        (Some(_), true) => widgets::text_button(None, tr("Update"), style::button::secondary, None),
        (Some(_), false) => widgets::icon_button(Icon::RefreshCw, tr("Update"), None),
        (None, true) => {
            widgets::text_button(None, tr("Update"), style::button::secondary, Some(Message::Update(server.id.clone())))
        }
        (None, false) => widgets::icon_button(Icon::RefreshCw, tr("Update"), Some(Message::Update(server.id.clone()))),
    };
    let control = row![].push(website).push(button).spacing(space::s()).align_y(Alignment::Center);
    setting_row(None, label, Some(detail), control)
}

/// The website of a catalog entry, as the arrow of another window: the browser.
fn website_button<'a>(url: &str) -> Element<'a, Message> {
    widgets::icon_button(Icon::ArrowUpRight, tr("Website"), Some(Message::Link(url.to_string())))
}

/// "Tools": each with its state, the access a call needs and what the server says it does; then,
/// when any waits for a person, "Allow tools" for exactly this list. What the core noticed (tools it
/// does not offer) below.
fn tools_of_server<'a>(page: &ServerState, server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = server.tools.iter().map(|tool| tool_row(server, tool)).collect();
    if rows.is_empty() {
        rows.push(setting_row(None, tr("It reported no tools"), None, Space::new()));
    }
    if server.tools.iter().any(|tool| tool.state == ToolState::Review) {
        let busy = page.waiting == Some(Waiting::Allow);
        rows.push(setting_row(
            None,
            tr("Not offered until you allow them"),
            Some(tr("Allow the tools as they are listed here.").to_string()),
            allow_button(server, busy, page.waiting.is_some(), words),
        ));
    }
    let footer = (!server.notes.is_empty()).then(|| server.notes.join(" "));
    widgets::group(tr("Tools"), rows, footer)
}

/// "Allow tools" for `server` with the fingerprint of the list shown: the primary button of the page
/// or the dialog, "Allowing…" while it is on its way, not pressable while another request is or
/// without a list the core read.
fn allow_button<'a>(server: &ExtensionInfo, busy: bool, blocked: bool, words: bool) -> Element<'a, Message> {
    if busy {
        return widgets::busy_button(tr("Allowing…"), style::button::primary);
    }
    let press = server
        .fingerprint
        .clone()
        .filter(|_| !blocked)
        .map(|fingerprint| Message::AllowTools(server.id.clone(), fingerprint));
    match words {
        true => widgets::text_button(None, tr("Allow tools"), style::button::primary, press),
        false => widgets::compact_button(Icon::Check, tr("Allow tools"), style::button::primary, press),
    }
}

/// One tool of a server: a dot and its name, below it its state, the access it needs and the first
/// sentence of its description – the server's text, shown as data.
fn tool_row<'a>(server: &ExtensionInfo, tool: &'a ExtensionTool) -> Element<'a, Message> {
    let (dot, state) = tool_state(server, tool);
    let level = widgets::level_label(tool_level(tool.access));
    let said = plain(first_sentence(tool.description.trim()));
    let mut detail = vec![state, level.to_string()];
    if !said.is_empty() {
        detail.push(said);
    }
    setting_row(Some(widgets::status_dot(dot)), tool.name.as_str(), Some(detail.join(" · ")), Space::new())
}

/// The state of a tool in a dot and a word. A tool waiting for a person is "Not allowed yet" while
/// none of the server's tools was ever allowed (just installed), else it is new or changed since the
/// person allowed the list.
fn tool_state(server: &ExtensionInfo, tool: &ExtensionTool) -> (Dot, String) {
    match tool.state {
        ToolState::Allowed => (Dot::Ok, tr("Allowed").to_string()),
        ToolState::Review if server.tools.iter().any(|tool| tool.state == ToolState::Allowed) => {
            (Dot::Attention, tr("Changed since you allowed it").to_string())
        }
        ToolState::Review => (Dot::Attention, tr("Not allowed yet").to_string()),
        ToolState::Unsupported => (Dot::Inactive, tr("Not offered").to_string()),
    }
}

/// "Secrets": each as "Stored" or "Missing", never its value, with "Replace…" or "Enter…".
fn secrets_group<'a>(server: &'a ExtensionInfo, words: bool) -> Element<'a, Message> {
    let rows = server
        .secrets
        .iter()
        .map(|secret| {
            let (dot, word, action) = match secret.stored {
                true => (Dot::Ok, tr("Stored"), tr("Replace…")),
                false => (Dot::Attention, tr("Missing"), tr("Enter…")),
            };
            let press = Some(Message::Replace(secret.name.clone()));
            let button = match words {
                true => widgets::text_button(None, action, style::button::secondary, press),
                false => widgets::icon_button(Icon::SquarePen, action, press),
            };
            setting_row(Some(widgets::status_dot(dot)), secret.name.as_str(), Some(word.to_string()), button)
        })
        .collect();
    let footer = tr("Secrets stay in the system keychain. The server gets them when it starts.");
    widgets::group(tr("Secrets"), rows, Some(footer.to_string()))
}

/// "Output": what the server last wrote, in a field to select and copy from; below it the sentence
/// about it with "Refresh" and "Copy", so the heading is as high as every other group's title.
fn output_group(page: &ServerState) -> Element<'_, Message> {
    let copy = matches!(&page.output, Output::Lines(content) if !content.text().trim().is_empty());
    let actions = vec![
        widgets::icon_button(Icon::RefreshCw, tr("Refresh"), Some(Message::ReadOutput)),
        widgets::icon_button(Icon::Copy, tr("Copy"), copy.then_some(Message::CopyOutput)),
    ];
    let body: Element<'_, Message> = match &page.output {
        Output::Reading => one_sentence(tr("Reading…")),
        Output::Failed(why) => one_sentence(why.as_str()),
        Output::Lines(content) if content.text().trim().is_empty() => one_sentence(tr("It wrote nothing yet.")),
        Output::Lines(content) => text_editor(content)
            .on_action(Message::Output)
            .font(typo::meta().font)
            .size(typo::meta().size)
            .line_height(typo::meta().line_height)
            .padding(style::INPUT_PADDING)
            .wrapping(Wrapping::WordOrGlyph)
            .height(output_height())
            .style(style::text_editor)
            .into(),
    };
    let footer = typo::text(typo::meta(), tr("The last lines it wrote, kept in memory only. Agents never see them."))
        .style(theme::text_2)
        .wrapping(Wrapping::WordOrGlyph)
        .width(Fill);
    let foot = row![footer, row(actions).spacing(space::xs())].spacing(space::s()).align_y(Alignment::Center);
    column![widgets::section_title(tr("Output")), body, foot].spacing(layout::item_gap()).into()
}

/// A sentence in a group's frame, where a field would be.
fn one_sentence<'a>(sentence: &'a str) -> Element<'a, Message> {
    let line = typo::text(typo::body(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph);
    container(row_frame(line, false)).width(Fill).style(style::container::group).into()
}

/// The room of the output field: [`OUTPUT_LINES`] of `META` and the padding of a field.
fn output_height() -> f32 {
    OUTPUT_LINES * typo::line_height(typo::meta()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// "Remove": what removing does, and "Remove…", which asks first.
fn remove_group<'a>(page: &ServerState, words: bool) -> Element<'a, Message> {
    let button = match (page.waiting, words) {
        (Some(Waiting::Remove), _) => widgets::busy_button(tr("Removing…"), style::button::danger),
        (waiting, true) => widgets::text_button(
            None,
            tr("Remove…"),
            style::button::danger,
            waiting.is_none().then_some(Message::AskRemove),
        ),
        // Narrow, the icon keeps the colour of the word: destructive at a glance.
        (waiting, false) => widgets::compact_button(
            Icon::Trash2,
            tr("Remove…"),
            style::button::danger,
            waiting.is_none().then_some(Message::AskRemove),
        ),
    };
    let row = setting_row(
        None,
        tr("Remove this MCP server"),
        Some(tr("Stops it and deletes its settings, secrets and rights.").to_string()),
        button,
    );
    widgets::group(tr("Remove"), vec![row], None)
}

// ----- Dialogs -----------------------------------------------------------------------------------

/// The install dialog: the form of a catalog entry, or the tools the installed server reported –
/// of a catalog entry or of one's own. `None` when the entry of the form left the catalog meanwhile.
fn install_dialog<'a>(install: &'a Install, data: &'a Data) -> Option<Element<'a, Message>> {
    let entry = data.catalog.entries().iter().find(|entry| entry.id == install.entry);
    let content = match &install.step {
        Step::Form => form_dialog(install, entry?),
        Step::Tools(server) => {
            // A server of one's own may have the id of an entry that is not installed.
            let entry = entry.filter(|_| matches!(server.source, Source::Catalog { .. }));
            tools_dialog(install, entry, server)
        }
    };
    Some(dialog(content, widgets::DIALOG_WIDTH, Message::CloseInstall))
}

/// The head of a dialog of this section: `title`, then the `facts` line when there is one, the
/// website of a catalog entry when there is one, and the close button. The close button sends
/// `close`, which is what Escape does in that dialog ([`escape`]): back to the form from the pasted
/// block and the choice, closed elsewhere.
fn dialog_head<'a>(
    title: String,
    facts: Option<Element<'a, Message>>,
    website: Option<&str>,
    close: Message,
) -> Element<'a, Message> {
    let words = column![typo::text(typo::title(), title)].push(facts).spacing(space::xs()).width(Fill);
    let close = widgets::icon_button(Icon::X, tr("Close"), Some(close));
    let buttons = row![].push(website.map(website_button)).push(close).spacing(space::xs());
    row![words, buttons].spacing(space::m()).align_y(Alignment::Start).into()
}

/// Who makes a catalog entry in which version, and "Experimental": the facts line of its dialogs.
fn entry_facts(entry: &CatalogEntry) -> Element<'_, Message> {
    let mut facts = row![widgets::ellipsized(typo::meta(), facts(entry), theme::text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    if entry.experimental {
        facts = facts.push(widgets::pill(None, tr("Experimental")));
    }
    facts.into()
}

/// The form: what the entry does, that it runs as the person, a hidden field per secret with where
/// to get one, and its tools with the access each needs; "Install" sends it.
fn form_dialog<'a>(install: &'a Install, entry: &'a CatalogEntry) -> Element<'a, Message> {
    let head = dialog_head(
        trf("Install {name}", &[("name", &entry.name)]),
        Some(entry_facts(entry)),
        Some(&entry.url),
        Message::CloseInstall,
    );
    let mut body = column![
        typo::text(typo::body(), entry.description.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        runs_as_you(),
    ]
    .spacing(space::l());
    if !entry.secrets.is_empty() {
        let rows = entry.secrets.iter().map(|secret| secret_field(install, secret)).collect();
        let footer = tr("Kept in the system keychain; only this server gets it, when it starts.");
        body = body.push(widgets::group(tr("Secrets"), rows, Some(footer.to_string())));
    }
    let footer = tr("What a call of each tool needs. None is offered until you allow the tools it reports.");
    body = body.push(widgets::group(tr("Tools"), vec![tool_lines(&entry.tools)], Some(footer.to_string())));
    let install_button = match install.busy {
        true => widgets::busy_button(tr("Installing…"), style::button::primary),
        false => widgets::text_button(None, tr("Install"), style::button::primary, Some(Message::SubmitInstall)),
    };
    let wait = install.busy.then(|| tr("The first start can take a few minutes while the program is downloaded."));
    let cancel = (tr("Cancel"), Message::CloseInstall);
    let foot = dialog_foot(install.problem.clone().map(said), wait, cancel, Some(install_button));
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// The tools of a catalog entry in one row of a group, a line each: the name and, at the right, the
/// access a call needs in `META` and `text_3`. Lines of [`size::row_sidebar()`] without rules
/// between them, so a server with twenty tools stays a list to read through, not a page to scroll.
fn tool_lines<'a>(tools: &'a [torro_ipc::extensions::CatalogTool]) -> Element<'a, Message> {
    if tools.is_empty() {
        return setting_row(None, tr("No tools"), None, Space::new());
    }
    let lines = tools.iter().map(|tool| {
        let level = typo::text(typo::meta(), widgets::level_label(tool_level(tool.access)))
            .style(theme::text_3)
            .wrapping(Wrapping::None);
        let line = row![widgets::ellipsized(typo::body(), tool.name.as_str(), theme::text), level]
            .spacing(space::l())
            .align_y(Alignment::Center);
        container(line).width(Fill).center_y(size::row_sidebar()).into()
    });
    container(column(lines)).width(Fill).padding([space::s(), space::l()]).into()
}

/// "It runs as you on this computer": the sentence every install shows, with the info icon.
fn runs_as_you<'a>() -> Element<'a, Message> {
    let info = icon::icon(Icon::Info, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let sentence = tr("It runs as you on this computer and can reach what you can.");
    row![info, typo::text(typo::body(), sentence).wrapping(Wrapping::WordOrGlyph)]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
}

/// The field of one secret of the entry: its name, what it is, the hidden field and the link where
/// to get one; after the first "Install", a sentence when it is empty.
fn secret_field<'a>(install: &'a Install, secret: &'a torro_ipc::extensions::CatalogSecret) -> Element<'a, Message> {
    let typed = install.secrets.get(&secret.name);
    let name = secret.name.clone();
    let input = field("", typed.map_or("", Secret::expose))
        .secure(true)
        .on_input(move |value| Message::TypeSecret(name.clone(), Secret::new(value)))
        .on_submit(Message::SubmitInstall);
    let mut line = row![input].spacing(space::s()).align_y(Alignment::Center);
    if let Some(url) = &secret.url {
        line = line.push(widgets::text_button(
            Some(Icon::ArrowUpRight),
            tr("Get one"),
            style::button::ghost,
            Some(Message::Link(url.clone())),
        ));
    }
    let mut lines = column![
        widgets::ellipsized(typo::body(), secret.name.as_str(), theme::text),
        typo::text(typo::meta(), secret.hint.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        line,
    ]
    .spacing(space::xs());
    if install.tried && typed.is_none_or(Secret::is_blank) {
        lines = lines.push(error_text(tr("Enter it to install the server.")));
    }
    container(lines).width(Fill).padding(tall_padding()).into()
}

/// The tools the installed server reported, each with its state, access and what it says it does,
/// and "Allow tools" for exactly this list; "Not now" leaves it installed and switched off, its
/// page has the same button. Who may use it is the clients' rights. The head names the catalog
/// `entry`'s facts, or for a server of one's own (`None`) what runs.
pub fn tools_dialog<'a>(
    install: &'a Install,
    entry: Option<&'a CatalogEntry>,
    server: &'a ExtensionInfo,
) -> Element<'a, Message> {
    let facts = match entry {
        Some(entry) => entry_facts(entry),
        None => widgets::ellipsized(typo::meta(), address(&server.server), theme::text_3),
    };
    let website = entry.map(|entry| entry.url.as_str());
    let head = dialog_head(
        trf("Allow the tools of {name}?", &[("name", &server.name)]),
        Some(facts),
        website,
        Message::CloseInstall,
    );
    let mut rows: Vec<Element<'a, Message>> = server.tools.iter().map(|tool| tool_row(server, tool)).collect();
    if rows.is_empty() {
        rows.push(setting_row(None, tr("It reported no tools"), None, Space::new()));
    }
    let footer = (!server.notes.is_empty()).then(|| server.notes.join(" "));
    let body = column![
        typo::text(typo::body(), tr("It started and reports these tools. None is offered until you allow them."))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        widgets::group(tr("Tools"), rows, footer),
        typo::text(typo::meta(), tr("Who may use it: API, MCP clients and the assistant's Access."))
            .style(theme::text_2),
    ]
    .spacing(space::l());
    let allow = allow_button(server, install.busy, false, true);
    let foot =
        dialog_foot(install.problem.clone().map(said), None, (tr("Not now"), Message::CloseInstall), Some(allow));
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// The foot of a dialog of this section: what went wrong when anything did ([`said`], or the fields
/// to check), a quiet note while it waits, then the `cancel` button with its message and the primary
/// button at the right.
fn dialog_foot<'a>(
    wrong: Option<Element<'a, Message>>,
    wait: Option<&'a str>,
    (cancel, on_cancel): (&'a str, Message),
    primary: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut foot = column![].push(wrong).spacing(space::s());
    if let Some(wait) = wait {
        foot = foot.push(typo::text(typo::meta(), wait).style(theme::text_3).wrapping(Wrapping::WordOrGlyph));
    }
    let buttons =
        row![Space::new().width(Fill), widgets::text_button(None, cancel, style::button::secondary, Some(on_cancel)),]
            .push(primary)
            .spacing(space::s())
            .align_y(Alignment::Center);
    foot.push(buttons).into()
}

/// Why the last request of a dialog failed: what it means in the error colour, then the core's
/// sentence when there is one.
fn said<'a>(problem: Problem) -> Element<'a, Message> {
    let mut said = column![].spacing(space::xs());
    if let Some(what) = problem.what {
        said = said.push(error_text(what));
    }
    if !problem.why.is_empty() {
        said = said.push(typo::text(typo::meta(), problem.why).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    said.into()
}

// ----- The form of a server of one's own (task MA.9) ----------------------------------------------

/// The dialog of a server of one's own at its step: the form, the field for a pasted block, or the
/// choice among the servers of one.
fn custom_dialog<'a>(custom: &'a Custom, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let content = match &custom.step {
        CustomStep::Form => custom_form(custom, data, class),
        CustomStep::Paste { text, problem } => paste_dialog(text, problem.as_deref()),
        CustomStep::Choose => choose_dialog(custom),
    };
    dialog(content, widgets::DIALOG_WIDTH, Message::CloseInstall)
}

/// The form: "Paste configuration" (only when adding), that it runs as the person, what it is and
/// how it is reached, and its variables or headers, stacked narrower than [`SizeClass::Medium`].
/// After the first try each field says what is wrong with it, and the foot names those fields
/// ([`check_sentence`]) – the dialog cannot scroll to them itself, but the first takes the focus
/// ([`first_marked`]); what the core refused shows in the foot instead.
fn custom_form<'a>(custom: &'a Custom, data: &'a Data, class: SizeClass) -> Element<'a, Message> {
    let checked = if custom.tried { check(custom, data) } else { Checked::default() };
    let title = match &custom.updating {
        Some(_) => trf("Update {name}", &[("name", &custom.name)]),
        None => tr("Add an MCP server").to_string(),
    };
    let mut body = column![].spacing(space::l());
    if custom.updating.is_none() {
        body = body.push(paste_offer(custom));
    }
    let stacked = !matches!(class, SizeClass::Wide | SizeClass::Medium);
    body = body.push(runs_as_you()).push(server_group(custom, &checked)).push(values_group(custom, &checked, stacked));
    body = body.push(
        typo::text(typo::meta(), tr("Its tools are listed once it has started. None is offered until you allow them."))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
    );
    let (label, busy) = match custom.updating {
        Some(_) => (tr("Save"), tr("Saving…")),
        None => (tr("Add server"), tr("Adding…")),
    };
    let primary = match custom.busy {
        true => widgets::busy_button(busy, style::button::primary),
        false => widgets::text_button(None, label, style::button::primary, Some(Message::SubmitCustom)),
    };
    let wrong = match custom.problem.clone() {
        Some(problem) => Some(said(problem)),
        None => check_sentence(&marked_fields(custom.kind, &checked)).map(error_text),
    };
    let wait = custom.busy.then(|| tr("The first start can take a few minutes while the program is downloaded."));
    let foot = dialog_foot(wrong, wait, (tr("Cancel"), Message::CloseInstall), Some(primary));
    torro_gui::dialog::frame(dialog_head(title, None, None, Message::CloseInstall), body, foot, space::xl())
}

/// The labels of the fields the form marks wrong ([`check`]), in the form's order: "Name", "Id",
/// "Command" or "Address", "Variables" or "Headers".
fn marked_fields(kind: Kind, checked: &Checked) -> Vec<&'static str> {
    let (target, values) = match kind {
        Kind::Command => (tr("Command"), tr("Variables")),
        Kind::Address => (tr("Address"), tr("Headers")),
    };
    [
        (checked.name.is_some(), tr("Name")),
        (checked.id.is_some(), tr("Id")),
        (checked.target.is_some(), target),
        (checked.values.iter().any(Option::is_some), values),
    ]
    .into_iter()
    .filter_map(|(wrong, label)| wrong.then_some(label))
    .collect()
}

/// The first field marked wrong, in the form's order (name, id, command or address, the rows of
/// variables or headers), for the focus after a try: typing goes there at once. The dialog cannot
/// scroll to it, so the foot names the fields too ([`check_sentence`]).
fn first_marked(kind: Kind, checked: &Checked) -> Option<iced::widget::Id> {
    let target = match kind {
        Kind::Command => COMMAND_FIELD,
        Kind::Address => URL_FIELD,
    };
    let fixed = [(checked.name, NAME_FIELD), (checked.id, ID_FIELD), (checked.target, target)];
    match fixed.into_iter().find(|(wrong, _)| wrong.is_some()) {
        Some((_, field)) => Some(field.into()),
        // A row whose name is fine but whose secret is missing: the value field, so typing fills it in.
        None => checked.values.iter().enumerate().find_map(|(at, problem)| match problem {
            Some(RowProblem { at_value: true, .. }) => Some(value_text_field(at)),
            Some(_) => Some(value_field(at)),
            None => None,
        }),
    }
}

/// The foot's sentence after a try with fields marked wrong, naming them – "Check Name and
/// Command." –, so the person knows where to look in a form taller than the window. `None` while
/// nothing is marked.
fn check_sentence(fields: &[&str]) -> Option<String> {
    match fields {
        [] => None,
        [field] => Some(trf("Check {field}.", &[("field", field)])),
        [first @ .., last] => Some(trf("Check {fields} and {last}.", &[("fields", &first.join(", ")), ("last", last)])),
    }
}

/// "Paste configuration" at the top of a new form, and, while a pasted block named several
/// servers, which of them the form holds with "Choose another".
fn paste_offer(custom: &Custom) -> Element<'_, Message> {
    let (sentence, label, press) = match custom.pasted.len() {
        0 | 1 => (
            tr("Copied a configuration from another app? Paste it to fill in the form.").to_string(),
            tr("Paste configuration"),
            Message::OpenPaste,
        ),
        n => (trf("One of {n} pasted servers.", &[("n", &n)]), tr("Choose another"), Message::ShowPasted),
    };
    let text = typo::text(typo::body(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).width(Fill);
    let button = widgets::text_button(None, label, style::button::secondary, Some(press));
    row![text, button].spacing(space::l()).align_y(Alignment::Center).into()
}

/// "Server": command or address; the name and the id (only a name and id to read when updating);
/// the command with its arguments, or the address.
fn server_group<'a>(custom: &'a Custom, checked: &Checked) -> Element<'a, Message> {
    let active = match custom.kind {
        Kind::Command => 0,
        Kind::Address => 1,
    };
    let kinds = widgets::segmented(
        [(tr("Command"), Message::CustomKind(Kind::Command)), (tr("Address"), Message::CustomKind(Kind::Address))],
        active,
    );
    let mut rows = vec![setting_row(None, tr("Type"), None, kinds)];
    match &custom.updating {
        Some(id) => {
            let detail = trf("Its tools are offered as ext_{id}_…", &[("id", id)]);
            rows.push(setting_row(None, custom.name.as_str(), Some(detail), Space::new()));
        }
        None => {
            // A product's name, the same in every language, so the id below matches it.
            let name = field("Home Assistant", &custom.name)
                .id(NAME_FIELD)
                .on_input(Message::CustomName)
                .on_submit(Message::SubmitCustom);
            rows.push(form_field(tr("Name"), None, name.into(), checked.name));
            let hint = match custom.id.is_empty() {
                true => tr("Lowercase letters, digits and hyphens. Its tools are offered under it.").to_string(),
                false => trf("Its tools are offered as ext_{id}_…", &[("id", &custom.id)]),
            };
            let id = field("home-assistant", &custom.id)
                .id(ID_FIELD)
                .on_input(Message::CustomId)
                .on_submit(Message::SubmitCustom);
            rows.push(form_field(tr("Id"), Some(hint), id.into(), checked.id));
        }
    }
    match custom.kind {
        Kind::Command => {
            let command = field("uvx", &custom.command)
                .id(COMMAND_FIELD)
                .on_input(Message::CustomCommand)
                .on_submit(Message::SubmitCustom);
            let hint = tr("A program such as npx, uvx or docker, or its full path. No shell runs it.");
            rows.push(form_field(tr("Command"), Some(hint.to_string()), command.into(), checked.target));
            let args = text_editor(&custom.args)
                .on_action(|action| Message::CustomArgs(Typed(action)))
                .placeholder(tr("One argument per line"))
                .font(typo::body().font)
                .size(typo::body().size)
                .line_height(typo::body().line_height)
                .padding(style::INPUT_PADDING)
                .wrapping(Wrapping::WordOrGlyph)
                .height(lines_height(ARGUMENT_LINES, typo::body()))
                .style(style::text_editor);
            rows.push(form_field(tr("Arguments"), Some(tr("One per line.").to_string()), args.into(), None));
        }
        Kind::Address => {
            let url = field("https://", &custom.url)
                .id(URL_FIELD)
                .on_input(|url| Message::CustomUrl(Secret::new(url)))
                .on_submit(Message::SubmitCustom);
            let hint = tr("An address that speaks Streamable HTTP: https://, or http:// on this computer only.");
            rows.push(form_field(tr("Address"), Some(hint.to_string()), url.into(), checked.target));
        }
    }
    widgets::group(tr("Server"), rows, None)
}

/// "Variables" of a command or "Headers" of an address: a row per entry with name, value, "Secret"
/// and "Remove" (`stacked`: the name above the rest), then "Add variable" or "Add header".
fn values_group<'a>(custom: &'a Custom, checked: &Checked, stacked: bool) -> Element<'a, Message> {
    let (title, add) = match custom.kind {
        Kind::Command => (tr("Variables"), tr("Add variable")),
        Kind::Address => (tr("Headers"), tr("Add header")),
    };
    let mut rows: Vec<Element<'a, Message>> = custom
        .values
        .iter()
        .enumerate()
        .map(|(at, entry)| entry_row(custom.kind, at, entry, checked.value(at), stacked))
        .collect();
    let add = widgets::text_button(Some(Icon::Plus), add, style::button::ghost, Some(Message::AddValue));
    rows.push(row_frame(add, false));
    let footer = tr("A value marked Secret is kept in the system keychain; only this server gets it, when it starts.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// One variable or header: its name, its value (hidden while it is a secret; a stored one says so
/// and stays while nothing is typed), "Secret" and "Remove"; below, after the first try, what is
/// wrong with it. `stacked` (a window narrower than [`SizeClass::Medium`]): the name on a line of its
/// own above value, "Secret" and "Remove", so neither field is squeezed to a few letters.
fn entry_row<'a>(
    kind: Kind,
    at: usize,
    entry: &'a Entry,
    problem: Option<&'static str>,
    stacked: bool,
) -> Element<'a, Message> {
    let placeholder = match kind {
        Kind::Command => tr("NAME"),
        Kind::Address => tr("Header name"),
    };
    let name = field(placeholder, &entry.name)
        .id(value_field(at))
        .on_input(move |name| Message::ValueName(at, name))
        .on_submit(Message::SubmitCustom);
    let hint = if entry.secret && entry.stored { tr("Stored – type to replace") } else { tr("Value") };
    let value = field(hint, entry.value.expose())
        .id(value_text_field(at))
        .secure(entry.secret)
        .on_input(move |value| Message::ValueText(at, Secret::new(value)))
        .on_submit(Message::SubmitCustom);
    let secret = widgets::choice_chip(tr("Secret"), None, entry.secret, Some(Message::ValueSecret(at, !entry.secret)));
    let remove = widgets::icon_button(Icon::X, tr("Remove"), Some(Message::RemoveValue(at)));
    let lines: Element<'a, Message> = match stacked {
        true => {
            let rest = row![value, secret, remove].spacing(space::s()).align_y(Alignment::Center);
            column![name, rest].spacing(space::s()).into()
        }
        false => row![name.width(Length::FillPortion(2)), value.width(Length::FillPortion(3)), secret, remove]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
    };
    let lines = column![lines].push(problem.map(error_text)).spacing(space::xs());
    container(lines).width(Fill).padding(tall_padding()).into()
}

/// A field of the form with its label above it, a quiet line about it, and after the first try
/// what is wrong with it – the layout of a secret's field in the install dialog.
fn form_field<'a>(
    label: &'a str,
    hint: Option<String>,
    control: Element<'a, Message>,
    problem: Option<&'static str>,
) -> Element<'a, Message> {
    let mut lines = column![widgets::ellipsized(typo::body(), label, theme::text)].spacing(space::xs());
    if let Some(hint) = hint {
        lines = lines.push(typo::text(typo::meta(), hint).style(theme::text_2).wrapping(Wrapping::WordOrGlyph));
    }
    let lines = lines.push(control).push(problem.map(error_text));
    container(lines).width(Fill).padding(tall_padding()).into()
}

/// "Paste configuration": what to paste, the field, why the last block could not be read; "Back"
/// to the form and "Fill in".
fn paste_dialog<'a>(text: &'a Hidden, problem: Option<&'a str>) -> Element<'a, Message> {
    let field = text_editor(&text.0)
        .on_action(|action| Message::PasteText(Typed(action)))
        .placeholder("{ \"mcpServers\": { … } }")
        .font(typo::meta().font)
        .size(typo::meta().size)
        .line_height(typo::meta().line_height)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(lines_height(PASTE_LINES, typo::meta()))
        .style(style::text_editor);
    let sentence = tr(
        "Paste the mcpServers block from the settings of Claude Desktop, Cursor or another app. It fills in the form; nothing is added yet.",
    );
    let body = column![typo::text(typo::body(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph), field]
        .push(problem.map(error_text))
        .spacing(space::l());
    let fill = widgets::text_button(None, tr("Fill in"), style::button::primary, Some(Message::FillFromPaste));
    let foot = dialog_foot(None, None, (tr("Back"), Message::BackToForm), Some(fill));
    let head = dialog_head(tr("Paste configuration").to_string(), None, None, Message::BackToForm);
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// The servers of a pasted block: each a row with what runs, which fills it into the form.
fn choose_dialog(custom: &Custom) -> Element<'_, Message> {
    let rows = custom
        .pasted
        .iter()
        .enumerate()
        .map(|(at, pasted)| {
            let row = setting_row(None, pasted.name.as_str(), Some(pasted.address()), chevron());
            pressable(row, Some(Message::ChoosePasted(at)))
        })
        .collect();
    let sentence = tr("Choose one to fill in the form. Once it is added, the others are listed here again.");
    let body = column![
        typo::text(typo::body(), sentence).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        widgets::group(tr("Pasted servers"), rows, None),
    ]
    .spacing(space::l());
    let foot = dialog_foot(None, None, (tr("Cancel"), Message::CloseInstall), None);
    let head = dialog_head(tr("Choose a server").to_string(), None, None, Message::BackToForm);
    torro_gui::dialog::frame(head, body, foot, space::xl())
}

/// The room of a field of several lines: `lines` of `level` and the padding of a field.
fn lines_height(lines: f32, level: typo::Level) -> f32 {
    lines * typo::line_height(level) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// The dialog of one secret of the server, built like the section's other dialogs (head with ✕,
/// body, foot) at the width of a popover: the hidden field with a sentence when it is empty after
/// the first "Save", in the foot what the core refused, "Cancel" and "Save".
fn replace_dialog<'a>(replacing: &'a Replacing, server: &'a ExtensionInfo) -> Element<'a, Message> {
    let stored = server.secrets.iter().any(|secret| secret.name == replacing.name && secret.stored);
    let title = match stored {
        true => trf("Replace {name}", &[("name", &replacing.name)]),
        false => trf("Enter {name}", &[("name", &replacing.name)]),
    };
    let input = field("", replacing.value.expose())
        .secure(true)
        .on_input(|value| Message::TypeReplacement(Secret::new(value)))
        .on_submit(Message::SubmitReplacement);
    let mut body = column![
        typo::text(typo::body(), trf("{name} gets it when it starts next.", &[("name", &server.name)]))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph),
        widgets::group(replacing.name.as_str(), vec![row_frame(input, false)], None),
    ]
    .spacing(space::l());
    if replacing.tried && replacing.value.is_blank() {
        body = body.push(error_text(tr("Enter the new value.")));
    }
    let save = match replacing.busy {
        true => widgets::busy_button(tr("Saving…"), style::button::primary),
        false => widgets::text_button(None, tr("Save"), style::button::primary, Some(Message::SubmitReplacement)),
    };
    let refused = replacing.problem.clone().map(|why| said(Problem { what: Some(tr("It could not be saved.")), why }));
    let foot = dialog_foot(refused, None, (tr("Cancel"), Message::CloseReplacement), Some(save));
    let head = dialog_head(title, None, None, Message::CloseReplacement);
    dialog(torro_gui::dialog::frame(head, body, foot, space::xl()), widgets::POPOVER_WIDTH, Message::CloseReplacement)
}

/// A sentence in the error colour (a refusal, an empty field after the first try).
fn error_text<'a>(sentence: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    typo::text(typo::body(), sentence)
        .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
        .wrapping(Wrapping::WordOrGlyph)
        .into()
}

/// A list that failed: an alert in the error colour, what failed with the core's words below it,
/// and "Retry".
fn failed_row<'a>(what: &'a str, why: &str) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    let retry = widgets::text_button(None, tr("Retry"), style::button::secondary, Some(Message::Retry));
    setting_row(Some(alert.into()), what, Some(why.to_string()), retry)
}

/// The arrow at the end of a row that leads to another window: 16, `text_3`.
fn arrow<'a>() -> Element<'a, Message> {
    icon::icon(Icon::ArrowUpRight, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) })
        .into()
}

// ----- Pure parts (unit-tested) ------------------------------------------------------------------

/// How a server stands, as a dot and a word: what a person decides first (tools to review), what
/// keeps it from starting (missing, crashed), then whether it is switched on and running.
pub fn status_of(server: &ExtensionInfo) -> (Dot, String) {
    match &server.status {
        Status::Review => (Dot::Attention, tr("Tools to review").to_string()),
        Status::Missing { what } => (Dot::Attention, what.needs()),
        Status::Crashed { .. } => (Dot::Error, tr("Stopped after errors").to_string()),
        _ if !server.enabled => (Dot::Inactive, tr("Off").to_string()),
        Status::Running => (Dot::Ok, tr("Running").to_string()),
        Status::Starting => (Dot::Inactive, tr("Starting…").to_string()),
        // Not running, but on: it starts with the next call.
        Status::Stopped => (Dot::Ok, tr("On").to_string()),
    }
}

/// What the section's row in the sidebar and the section menu marks (task MA.8): [`Dot::Error`]
/// while a server stopped after errors, else [`Dot::Attention`] while one has tools waiting for a
/// person; `None` when no server needs the person.
pub fn alert(data: &Data) -> Option<Dot> {
    let servers = data.extensions.entries();
    if servers.iter().any(|server| matches!(server.status, Status::Crashed { .. })) {
        return Some(Dot::Error);
    }
    servers.iter().any(|server| server.status == Status::Review).then_some(Dot::Attention)
}

/// Whether a variable or header called `name` looks like a secret: it holds `KEY`, `TOKEN`,
/// `SECRET`, `PASSWORD` or `AUTH`, in any case. Its row starts with "Secret" on.
pub fn looks_secret(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_WORDS.iter().any(|word| upper.contains(word))
}

/// The id proposed for a server called `name` ([`valid_id`]): its letters and digits in lower
/// case, every other run of characters one hyphen, from its first letter on, at most
/// [`MAX_ID_CHARS`]; with `-2`, `-3` … when `taken` says an installed server has it. Empty when the
/// name has no letter a-z.
pub fn suggested_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut id = String::new();
    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || (c.is_ascii_digit() && !id.is_empty()) {
            id.push(c);
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let cut = |id: &str, room: usize| id.chars().take(room).collect::<String>().trim_end_matches('-').to_string();
    let base = cut(&id, MAX_ID_CHARS);
    if base.is_empty() || !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let suffix = format!("-{n}");
            format!("{}{suffix}", cut(&base, MAX_ID_CHARS - suffix.len()))
        })
        .find(|id| !taken(id))
        .unwrap_or(base)
}

/// What is wrong with the form that it can tell itself, by field ([`Checked`]): a name, a valid id
/// no installed server has (only when adding), a command or an address, and per row a name that
/// can name a variable or header, is not blocked ([`blocked_env`]) and not listed twice, and a
/// secret's value (a stored one may stay empty). Rows left empty are not checked; they go nowhere.
/// The address itself (`https://`, or `http://` on this computer) is the core's to check.
fn check(custom: &Custom, data: &Data) -> Checked {
    let adding = custom.updating.is_none();
    let name = (adding && custom.name().is_empty()).then(|| tr("Enter a name."));
    let id = match adding {
        false => None,
        true if !valid_id(&custom.id) => {
            Some(tr("Use up to 24 lowercase letters, digits and hyphens, starting with a letter."))
        }
        true if server_of(data, &custom.id).is_some() => Some(tr("Another server has this id already.")),
        true => None,
    };
    let target = match custom.kind {
        Kind::Command => custom.command.trim().is_empty().then(|| tr("Enter the program to start.")),
        Kind::Address => custom.url.trim().is_empty().then(|| tr("Enter the address.")),
    };
    let values = custom
        .values
        .iter()
        .enumerate()
        .map(|(at, entry)| entry_problem(custom.kind, &custom.values[..at], entry))
        .collect();
    Checked { name, id, target, values }
}

/// What is wrong with the row `entry` after the rows `before` it, for a command's variables or an
/// address's headers.
fn entry_problem(kind: Kind, before: &[Entry], entry: &Entry) -> Option<RowProblem> {
    if entry.is_empty() {
        return None;
    }
    let name = entry.name.trim();
    let twice = |other: &Entry| match kind {
        Kind::Command => other.name.trim() == name,
        // Header names are the same in any case.
        Kind::Address => other.name.trim().eq_ignore_ascii_case(name),
    };
    let problem = match kind {
        _ if name.is_empty() => tr("Give it a name."),
        Kind::Command if !valid_env_name(name) => tr("A variable's name has no spaces and no =."),
        Kind::Command if blocked_env(name) => tr("Torro Workplace never passes this variable on to a server."),
        Kind::Address if !valid_header_name(name) => tr("A header's name has no spaces and no colon."),
        _ if before.iter().any(twice) => tr("This name is listed twice."),
        _ if entry.secret && entry.value.is_blank() && !entry.stored => {
            return Some(RowProblem { text: tr("Enter its value."), at_value: true });
        }
        _ => return None,
    };
    Some(RowProblem::name(problem))
}

/// Why a pasted block could not be read, as a sentence for the person in the suite's language. The
/// names are the block's; a reason or the parser's position stays English, as the core gives it.
fn paste_problem(error: &ParseError) -> String {
    match error {
        ParseError::Json(at) => trf("This isn't a configuration Torro Workplace can read ({at}).", &[("at", at)]),
        ParseError::NoServers => tr("There is no server in it.").to_string(),
        // The reason is one of the fixed English phrases of `torro_ipc::extensions`; de.rs has them.
        ParseError::Invalid { server, reason } => {
            trf("{server} can't be added: {reason}.", &[("server", server), ("reason", &tr(reason))])
        }
        ParseError::Blocked { server, variable } => trf(
            "{server} would get the variable {variable}, which Torro Workplace never passes on to a server. Remove it and paste again.",
            &[("server", server), ("variable", variable)],
        ),
        ParseError::Transport { server, transport } => trf(
            "{server} speaks {transport}, which Torro Workplace doesn't support. Only addresses with Streamable HTTP work.",
            &[("server", server), ("transport", transport)],
        ),
    }
}

/// The catalog entry `entry` when "Install…" is offered for it: it is in the catalog, nothing it
/// needs is missing on this computer, and no installed server has its id.
pub fn installable<'a>(data: &'a Data, entry: &str) -> Option<&'a CatalogEntry> {
    let taken = data.extensions.entries().iter().any(|server| server.id == entry);
    data.catalog.entries().iter().find(|listed| listed.id == entry && listed.missing.is_empty() && !taken)
}

/// The installed server with the id `id`.
pub fn server_of<'a>(data: &'a Data, id: &str) -> Option<&'a ExtensionInfo> {
    data.extensions.entries().iter().find(|server| server.id == id)
}

/// The version "Update" brings a catalog server to: the catalog's, when it differs from the one the
/// server was installed or last updated in. `None` for one's own server, one whose entry left the
/// catalog, and one in the catalog's version.
pub fn update_available(data: &Data, server: &ExtensionInfo) -> Option<String> {
    let Source::Catalog { entry, version } = &server.source else { return None };
    let listed = data.catalog.entries().iter().find(|listed| &listed.id == entry)?;
    (listed.version != *version).then(|| listed.version.clone())
}

/// The installed server of the catalog entry `entry`, if any.
pub fn installed<'a>(data: &'a Data, entry: &str) -> Option<&'a ExtensionInfo> {
    data.extensions
        .entries()
        .iter()
        .find(|server| matches!(&server.source, Source::Catalog { entry: from, .. } if from == entry))
}

/// What runs for a server: the program with its arguments, or the address.
pub fn address(spec: &ServerSpec) -> String {
    match spec {
        ServerSpec::Stdio { command, args, .. } => {
            std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
        }
        ServerSpec::Http { url, .. } => url.clone(),
    }
}

/// Whether the assistant may use `tool`, after its client's rights as `clients.list_clients` lists
/// them: the level the tool needs in `tools` (`ToolAccess::access`: writing for `pdf`, reading for `web`) and the
/// tool in its selection – `web` only when it is ticked by name, never through "All" (`Rights::reaches_tool`).
/// The switch is the other half. `false` while the assistant is off (it has no client).
pub fn assistant_may_use(data: &Data, tool: ToolId) -> bool {
    data.clients
        .iter()
        .find(|client| client.kind == ASSISTANT_CLIENT)
        .is_some_and(|client| client.rights.area(Area::Tools).allows(tool.access()) && client.rights.reaches_tool(tool))
}

/// "Letter", "Report", "Note": the template's name, told apart from the paper "US Letter" and a
/// note of the memory.
pub fn template_name(id: PdfTemplateId) -> &'static str {
    match id {
        PdfTemplateId::Letter => trc("template", "Letter"),
        PdfTemplateId::Report => trc("template", "Report"),
        PdfTemplateId::Note => trc("template", "Note"),
    }
}

/// The margins the picker offers: the even ones, and `current` first when it is none of them.
pub fn margin_options(current: [u8; 4]) -> Vec<Margins> {
    let mut options: Vec<Margins> = EVEN_MARGINS.into_iter().map(|margin| Margins([margin; 4])).collect();
    if !options.contains(&Margins(current)) {
        options.insert(0, Margins(current));
    }
    options
}

/// The last part of a path, for the logo row; the whole path when it has none.
fn file_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path).to_string()
}

/// A paper in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperChoice(pub Paper);

impl PaperChoice {
    const ALL: [PaperChoice; 3] = [PaperChoice(Paper::A4), PaperChoice(Paper::A5), PaperChoice(Paper::Letter)];
}

impl fmt::Display for PaperChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = match self.0 {
            Paper::A4 => "A4",
            Paper::A5 => "A5",
            Paper::Letter => tr("US Letter"),
        };
        f.write_str(word)
    }
}

/// An allowed folder's access in the picker: "Read", "Read and write".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessChoice(pub FolderAccess);

impl AccessChoice {
    const ALL: [AccessChoice; 2] = [AccessChoice(FolderAccess::Read), AccessChoice(FolderAccess::ReadWrite)];
}

impl fmt::Display for AccessChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            FolderAccess::Read => tr("Read"),
            FolderAccess::ReadWrite => tr("Read and write"),
        })
    }
}

/// Margins in the picker, top, right, bottom, left: "20 mm", or "20 · 25 · 20 · 25 mm" when they
/// differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Margins(pub [u8; 4]);

impl fmt::Display for Margins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [top, right, bottom, left] = self.0;
        let n = match self.0.iter().all(|margin| *margin == top) {
            true => top.to_string(),
            false => format!("{top} · {right} · {bottom} · {left}"),
        };
        f.write_str(&trf("{n} mm", &[("n", &n)]))
    }
}

/// A font size in the picker: "11 pt".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Points(pub u8);

impl fmt::Display for Points {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&trf("{n} pt", &[("n", &self.0)]))
    }
}

#[cfg(test)]
mod tests {
    use torro_core::settings::ToolSwitch;
    use torro_ipc::api::{Level, Selection};
    use torro_ipc::extensions::{CatalogEntry, CrashReason, Prerequisite};
    use torro_ipc::web::SearchProviderId;

    use super::super::launch;
    use super::*;

    /// The demo's data with the PDF tool on, a folder and a logo (`--open connections`).
    fn data() -> Data {
        let mut data = launch::demo_data();
        launch::demo_connections(&mut data);
        data
    }

    /// The one request of an action.
    fn one(action: Action) -> Request {
        match action {
            Action::Send(request) => request,
            other => panic!("expected exactly one request, got {other:?}"),
        }
    }

    /// The template a `pdf_template` request carries.
    fn template_of(request: &Request) -> (PdfTemplateId, PdfTemplate) {
        match request {
            Request::SetSetting { key: SettingKey::PdfTemplate, value } => {
                PdfTemplate::from_value(value).expect("a whole template the core takes")
            }
            other => panic!("expected pdf_template, got {other:?}"),
        }
    }

    /// Task MA.7: the switch sends exactly one `set_setting tool_enabled`, both ways, which the core
    /// reads back.
    #[test]
    fn the_switch_sends_exactly_one_set_setting() {
        let data = data();
        let mut state = State::default();
        for on in [true, false] {
            let request = one(update(&mut state, Message::Switch(ToolId::Pdf, on), &data));
            let Request::SetSetting { key: SettingKey::ToolEnabled, value } = &request else {
                panic!("expected tool_enabled, got {request:?}")
            };
            let switch: ToolSwitch = serde_json::from_value(value.clone()).expect("the core reads it");
            assert_eq!(switch, ToolSwitch { tool: ToolId::Pdf, enabled: on });
        }
    }

    /// Every field of a template sends `pdf_template` with the whole template as loaded and that one
    /// field changed – the logo the demo set included.
    #[test]
    fn every_field_sends_the_whole_template() {
        let data = data();
        let id = PdfTemplateId::Report;
        let stored = data.settings.tools.pdf.template(id).clone();
        assert!(stored.logo.is_some(), "the demo's report has a logo, so a lost one would show");
        let with = |change: &dyn Fn(&mut PdfTemplate)| {
            let mut expected = stored.clone();
            change(&mut expected);
            expected
        };
        let cases = [
            (Message::Paper(id, Paper::Letter), with(&|t| t.paper = Paper::Letter)),
            (Message::Margins(id, [10; 4]), with(&|t| t.margins_mm = [10; 4])),
            (Message::FontSize(id, 13), with(&|t| t.font_size_pt = 13)),
            (Message::PageNumbers(id, false), with(&|t| t.page_numbers = false)),
            (Message::RemoveLogo(id), with(&|t| t.logo = None)),
        ];
        let mut state = State::default();
        for (message, expected) in cases {
            let request = one(update(&mut state, message.clone(), &data));
            assert_eq!(template_of(&request), (id, expected), "{message:?}");
        }
    }

    /// Typing sends nothing; Enter sends the whole template with the line, Escape drops it, starting
    /// another line or leaving the templates' page saves the one before.
    #[test]
    fn a_line_goes_out_whole_when_it_is_saved() {
        let data = data();
        let id = PdfTemplateId::Letter;
        let stored = data.settings.tools.pdf.template(id).clone();
        let mut state = State::default();
        assert!(matches!(update(&mut state, Message::Templates, &data), Action::None));
        assert_eq!(state.page, Page::PdfTemplates);
        let typed = update(&mut state, Message::Type(id, Line::Header, "ACME · {date}".into()), &data);
        assert!(matches!(typed, Action::None), "typing sends nothing");
        let request = one(update(&mut state, Message::SaveLine, &data));
        let expected = PdfTemplate { header: "ACME · {date}".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert_eq!(escape(&state), Some(Message::Back), "saved, so Escape leaves the page");

        // Escape drops a line being typed.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Draft".into()), &data);
        assert_eq!(escape(&state), Some(Message::DiscardLine));
        assert!(matches!(update(&mut state, Message::DiscardLine, &data), Action::None));
        assert!(matches!(update(&mut state, Message::SaveLine, &data), Action::None), "nothing left to save");

        // Another line saves the one before; a line as stored sends nothing.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Confidential".into()), &data);
        let request = one(update(&mut state, Message::Type(id, Line::Header, stored.header.clone()), &data));
        let expected = PdfTemplate { footer: "Confidential".into(), ..stored.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(matches!(update(&mut state, Message::SaveLine, &data), Action::None), "the header is as stored");

        // Leaving the page saves what is typed.
        let _ = update(&mut state, Message::Type(id, Line::Footer, "Internal".into()), &data);
        let request = one(update(&mut state, Message::Back, &data));
        assert_eq!(template_of(&request).1.footer, "Internal");
        assert_eq!((state.page.clone(), escape(&state)), (Page::List, None));

        // A line never grows beyond what the core takes, and never breaks.
        let long: String = "x".repeat(PdfTemplate::MAX_LINE_CHARS + 20) + "\n";
        let _ = update(&mut state, Message::Type(id, Line::Header, long), &data);
        let (_, template) = template_of(&one(update(&mut state, Message::SaveLine, &data)));
        assert_eq!(template.header.chars().count(), PdfTemplate::MAX_LINE_CHARS);
    }

    /// Folder and logo: the choosers are the shell's; what comes back carries the path, the logo
    /// inside the whole template; cancelled, nothing; a path that is no text, a sentence above the
    /// page. "Use desktop" removes the folder.
    #[test]
    fn the_choosers_are_the_shell_s_and_send_the_path() {
        let data = data();
        let mut state = State::default();
        let id = PdfTemplateId::Note;
        for chooser in [Chooser::Folder, Chooser::Logo(id)] {
            assert!(matches!(update(&mut state, Message::Choose(chooser), &data), Action::Pick(c) if c == chooser));
            assert!(matches!(update(&mut state, Message::Picked(chooser, None), &data), Action::None), "cancelled");
        }
        assert_eq!(
            one(update(&mut state, Message::UseDesktop, &data)),
            Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::Value::Null }
        );
        let folder = PathBuf::from("/home/anna/PDFs");
        assert_eq!(
            one(update(&mut state, Message::Picked(Chooser::Folder, Some(folder)), &data)),
            Request::SetSetting { key: SettingKey::PdfFolder, value: serde_json::json!("/home/anna/PDFs") }
        );
        let logo = PathBuf::from("/home/anna/logo.png");
        let request = one(update(&mut state, Message::Picked(Chooser::Logo(id), Some(logo.clone())), &data));
        let expected = PdfTemplate { logo: Some("/home/anna/logo.png".into()), ..data.settings.tools.pdf.note.clone() };
        assert_eq!(template_of(&request), (id, expected));
        assert!(LOGO_EXTENSIONS.iter().all(|ext| logo_request(&data, id, &logo.with_extension(ext)).is_some()));
        assert!(state.problem.is_none());
    }

    /// A path the system hands over that is not plain text cannot be stored: nothing is sent, the
    /// page says so until the next chooser opens.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_no_text_is_refused_with_a_sentence() {
        use std::os::unix::ffi::OsStrExt;
        let data = data();
        let mut state = State::default();
        let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/home/anna/\xff"));
        assert!(matches!(update(&mut state, Message::Picked(Chooser::Folder, Some(odd)), &data), Action::None));
        assert_eq!(state.problem.as_deref(), Some(tr("This path cannot be used.")));
        let _ = update(&mut state, Message::Choose(Chooser::Folder), &data);
        assert!(state.problem.is_none());
    }

    /// Nothing here reaches the clients: every request is a setting or brings the assistant's window
    /// to the front ("may not use it"); the rest is the shell's chooser, a page or a reload.
    #[test]
    fn no_message_sends_a_clients_request() {
        let data = data();
        let mut state = State::default();
        let messages = [
            Message::Switch(ToolId::Pdf, true),
            Message::Access,
            Message::Choose(Chooser::Folder),
            Message::Picked(Chooser::Folder, Some(PathBuf::from("/tmp/pdfs"))),
            Message::UseDesktop,
            Message::Templates,
            Message::Paper(PdfTemplateId::Letter, Paper::A5),
            Message::Margins(PdfTemplateId::Letter, [30; 4]),
            Message::FontSize(PdfTemplateId::Letter, 9),
            Message::Type(PdfTemplateId::Letter, Line::Header, "x".into()),
            Message::SaveLine,
            Message::DiscardLine,
            Message::PageNumbers(PdfTemplateId::Letter, true),
            Message::Choose(Chooser::Logo(PdfTemplateId::Letter)),
            Message::RemoveLogo(PdfTemplateId::Letter),
            Message::Back,
            Message::Choose(Chooser::AllowedFolder),
            Message::Picked(Chooser::AllowedFolder, Some(PathBuf::from("/home/anna/Invoices"))),
            Message::SetFolderAccess("/home/anna/Documents".into(), FolderAccess::ReadWrite),
            Message::RemoveFolder("/home/anna/Documents".into()),
            Message::Switch(ToolId::Files, true),
            Message::Switch(ToolId::Web, true),
            Message::SearchProviders,
            Message::Web(web::Message::Switch(SearchProviderId::Brave, true)),
            Message::Web(web::Message::TypeKey(SearchProviderId::Brave, Secret::new("key"))),
            Message::Web(web::Message::SaveKey),
            Message::Web(web::Message::RemoveKey(SearchProviderId::Brave)),
            Message::Web(web::Message::TypeAddress("https://search.example.org".into())),
            Message::Web(web::Message::CheckAddress),
            Message::Web(web::Message::RemoveAddress),
            Message::Web(web::Message::Terms(SearchProviderId::Duckduckgo)),
            Message::Back,
            Message::Retry,
        ];
        for message in messages {
            match update(&mut state, message.clone(), &data) {
                Action::Send(Request::OpenApp { app, target: None }) => {
                    assert_eq!(app, torro_core::AppId::ASSISTANT.binary, "{message:?}");
                }
                // A search provider's key goes to the keychain (`accounts`), never into the settings.
                Action::Send(request @ Request::SetSearchKey { .. }) => {
                    assert_eq!(request.operation().area(), Area::Accounts, "{message:?}");
                }
                Action::Send(request) | Action::CheckAddress(request) => {
                    assert_eq!(request.operation().area(), Area::Settings, "{message:?}");
                }
                Action::OpenUrl(url) => assert!(url.starts_with("https://"), "{message:?}"),
                Action::Pick(_) | Action::Reload | Action::None => {}
                other => panic!("{message:?} did {other:?}"),
            }
        }
    }

    /// Issue #305 here since #302: the tool `web` has its switch, whether the assistant may use it and the row to
    /// its search providers; their page shows the core's providers, Escape drops a key typed there first, leaving
    /// the page drops it too, and SearXNG's check comes back here, a refusal at its field.
    #[test]
    fn the_web_has_its_providers_on_a_page_of_their_own() {
        let mut data = data();
        launch::demo_connections(&mut data);
        assert!(data.tools.iter().any(|tool| tool.id == ToolId::Web && tool.enabled));
        let mut state = State::default();
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            let _ = view(&state, &data, class);
        }
        let _ = update(&mut state, Message::SearchProviders, &data);
        assert_eq!(state.page, Page::SearchProviders);
        assert_eq!(
            header(&state, &data, SizeClass::Wide).sub_page.map(|(name, _)| name),
            Some(tr("Search providers").into())
        );
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            let _ = view(&state, &data, class);
        }
        let typed = Message::Web(web::Message::TypeKey(SearchProviderId::Exa, Secret::new("key")));
        let _ = update(&mut state, typed.clone(), &data);
        assert_eq!(escape(&state), Some(Message::Web(web::Message::Discard)));
        let _ = update(&mut state, Message::Back, &data);
        assert_eq!((state.page.clone(), escape(&state)), (Page::List, None), "the key went with the page");

        let _ = update(&mut state, Message::SearchProviders, &data);
        let _ = update(&mut state, Message::Web(web::Message::TypeAddress("https://s.example.org".into())), &data);
        let check = update(&mut state, Message::Web(web::Message::CheckAddress), &data);
        let Action::CheckAddress(request) = check else { panic!("the check apart, got {check:?}") };
        assert_eq!(request, web::address_request(Some("https://s.example.org")));
        assert!(checked(&mut state, &Err("This SearXNG instance does not answer in JSON.".into())), "shown here");
        assert!(!checked(&mut state, &Ok(Response::Ok)), "no check waits any more");
    }

    /// Issue #304 here since #302: the allowed folders come first with their access and "Remove"; a folder chosen
    /// with "Add folder…" goes out to read, one allowed already keeps its access, and an access as stored sends
    /// nothing. The tool `files` comes first among the tools and says so while no folder is allowed.
    #[test]
    fn allowed_folders_are_set_here_and_the_files_tool_comes_first() {
        let mut data = data();
        launch::demo_connections(&mut data);
        let folders = data.settings.tools.folders.clone();
        assert!(!folders.is_empty(), "the demo allows folders");
        let mut state = State::default();
        let chosen = Message::Picked(Chooser::AllowedFolder, Some(PathBuf::from("/home/anna/Invoices")));
        assert_eq!(
            one(update(&mut state, chosen, &data)),
            allowed_folder_request("/home/anna/Invoices", Some(FolderAccess::Read))
        );
        let again = Message::Picked(Chooser::AllowedFolder, Some(PathBuf::from(&folders[0].path)));
        assert_eq!(
            one(update(&mut state, again, &data)),
            allowed_folder_request(&folders[0].path, Some(folders[0].access))
        );
        let same = Message::SetFolderAccess(folders[0].path.clone(), folders[0].access);
        assert!(matches!(update(&mut state, same, &data), Action::None), "as stored");
        let removed = one(update(&mut state, Message::RemoveFolder(folders[0].path.clone()), &data));
        assert_eq!(removed, allowed_folder_request(&folders[0].path, None));
        let listed: Vec<ToolId> = data.tools.iter().map(|info| info.id).collect();
        assert_eq!(listed, ToolId::ALL, "the core's order");
        let shown: Vec<ToolId> = page_order(&data.tools).iter().map(|info| info.id).collect();
        assert_eq!(shown, [ToolId::Files, ToolId::Pdf, ToolId::Web], "web keeps its place after pdf");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let odd = Path::new(std::ffi::OsStr::from_bytes(b"/home/anna/\xff"));
            assert_eq!(added_folder_request(&data, odd), None, "not plain text");
        }
        // As many folders as may be allowed: "Add folder…" stands disabled and the sentence below says why.
        let mut full = data.clone();
        full.settings.tools.folders = (0..MAX_ALLOWED_FOLDERS)
            .map(|at| AllowedFolder { path: format!("/home/anna/{at}"), access: FolderAccess::Read })
            .collect();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            let _ = view(&state, &full, class);
        }
        for class in [SizeClass::Wide, SizeClass::Narrow] {
            let _ = view(&state, &data, class);
            data.settings.tools.folders.clear();
            let _ = view(&state, &data, class);
            data.settings.tools.folders = folders.clone();
        }
    }

    /// Sight check of WD.4: one orange dot in the files tool's rows. While the assistant may not use the tool, the
    /// row that leads to Access has it and "No allowed folders" shows the folder icon; once it may, the missing
    /// folder is what is left to do.
    #[test]
    fn the_files_tool_has_one_orange_dot_at_most() {
        let mut empty = data();
        empty.settings.tools.folders.clear();
        fn assistant(data: &mut Data) -> &mut torro_ipc::api::Rights {
            &mut data.clients.iter_mut().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant").rights
        }
        assistant(&mut empty).set_level(Area::Tools, Level::NoAccess, Vec::new);
        assert!(!assistant_may_use(&empty, ToolId::Files));
        assert!(!no_folder_flagged(&empty), "the row to Access has the dot");
        assistant(&mut empty).set_level(Area::Tools, Level::Read, Vec::new);
        assistant(&mut empty).tools = Selection::All;
        assert!(no_folder_flagged(&empty), "allowed, the missing folder has it");
    }

    /// Sight check of WD.4: with as many folders as may be allowed, the sentence below the list says why "Add
    /// folder…" stands disabled; below that it does not.
    #[test]
    fn a_full_list_names_the_limit() {
        let limit = trf("At most {n} folders. Remove one to add another.", &[("n", &MAX_ALLOWED_FOLDERS)]);
        assert!(folders_footer(MAX_ALLOWED_FOLDERS).ends_with(&limit));
        assert!(!folders_footer(MAX_ALLOWED_FOLDERS - 1).contains(&limit));
        assert_eq!(folders_footer(0), tr(FOLDERS_FOOTER));
    }

    /// The access picker says what each step allows, and a folder shows its name.
    #[test]
    fn a_folder_row_says_its_name_and_access() {
        assert_eq!(AccessChoice(FolderAccess::Read).to_string(), tr("Read"));
        assert_eq!(AccessChoice(FolderAccess::ReadWrite).to_string(), tr("Read and write"));
        assert_eq!(file_name("/home/anna/Documents"), "Documents");
        assert_eq!(file_name("/"), "/", "a folder without a name shows its path");
    }

    /// Task WEB.6: the tool `web` needs reading in `tools` and `web` ticked by name; "All" tools is not enough, as
    /// it is for the PDF tool.
    #[test]
    fn the_assistant_may_use_the_web_only_when_it_is_ticked() {
        let mut data = data();
        let rights =
            &mut data.clients.iter_mut().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant").rights;
        rights.set_level(Area::Tools, Level::Read, Vec::new);
        rights.tools = Selection::All;
        assert!(!assistant_may_use(&data, ToolId::Web), "All tools does not include web");
        let rights =
            &mut data.clients.iter_mut().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant").rights;
        rights.tools = Selection::Only(vec!["web".into()]);
        assert!(assistant_may_use(&data, ToolId::Web), "reading is enough for web");
        assert!(!assistant_may_use(&data, ToolId::Pdf), "PDF needs writing and its own tick");
    }

    /// The assistant may use a tool with write access to `tools` and the tool chosen; reading or a
    /// selection without it is not enough, and an assistant that is off may use nothing.
    #[test]
    fn whether_the_assistant_may_use_it_follows_its_rights() {
        let mut data = data();
        assert!(assistant_may_use(&data, ToolId::Pdf), "the demo's assistant may make PDFs");
        fn rights(data: &mut Data) -> &mut torro_ipc::api::Rights {
            &mut data.clients.iter_mut().find(|client| client.kind == ASSISTANT_CLIENT).expect("the assistant").rights
        }
        rights(&mut data).tools = Selection::Only(Vec::new());
        assert!(!assistant_may_use(&data, ToolId::Pdf), "not in the selection");
        rights(&mut data).tools = Selection::All;
        rights(&mut data).set_level(Area::Tools, Level::Read, Vec::new);
        assert!(!assistant_may_use(&data, ToolId::Pdf), "reading only");
        rights(&mut data).set_level(Area::Tools, Level::Write, || vec!["pdf".into()]);
        assert!(assistant_may_use(&data, ToolId::Pdf));
        data.clients.retain(|client| client.kind != ASSISTANT_CLIENT);
        assert!(!assistant_may_use(&data, ToolId::Pdf), "the assistant is off");
    }

    /// A server's status in words, with the dot's colour as the second sign: what a person must do
    /// first, then whether it is on.
    #[test]
    fn a_server_says_how_it_stands() {
        let data = launch::demo_data();
        let mut server = data.extensions.entries().first().cloned().expect("the demo has servers");
        let cases = [
            (Status::Running, true, Dot::Ok, tr("Running").to_string()),
            (Status::Stopped, true, Dot::Ok, tr("On").to_string()),
            (Status::Starting, true, Dot::Inactive, tr("Starting…").to_string()),
            (Status::Running, false, Dot::Inactive, tr("Off").to_string()),
            (Status::Review, false, Dot::Attention, tr("Tools to review").to_string()),
            (Status::Missing { what: Prerequisite::Npx }, true, Dot::Attention, Prerequisite::Npx.needs()),
            (
                Status::Crashed { reason: CrashReason::Exited },
                false,
                Dot::Error,
                tr("Stopped after errors").to_string(),
            ),
        ];
        for (status, enabled, dot, word) in cases {
            (server.status, server.enabled) = (status.clone(), enabled);
            assert_eq!(status_of(&server), (dot, word), "{status:?}, on: {enabled}");
        }
        let program = ServerSpec::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "notes".into()],
            env: Default::default(),
        };
        assert_eq!(address(&program), "npx -y notes");
        let web = ServerSpec::Http { url: "https://mcp.example.org/".into(), headers: Default::default() };
        assert_eq!(address(&web), "https://mcp.example.org/");
    }

    /// The demo's catalog knows which entries are installed; its servers of one's own are the others.
    #[test]
    fn the_catalog_knows_what_is_installed() {
        let data = launch::demo_data();
        assert!(installed(&data, "time").is_some_and(|server| server.status == Status::Running));
        assert!(installed(&data, "sequential-thinking").is_none());
        let own = data.extensions.entries().iter().filter(|server| server.source == Source::Custom).count();
        assert!(own > 0 && count(&data) > own, "both kinds, so both groups show rows");
        let missing = data.catalog.entries().iter().find(|entry| entry.id == "sequential-thinking");
        assert!(missing.is_some_and(|entry| entry.missing == [Prerequisite::Npx]), "the demo has no Node");
    }

    /// The pickers' words and the margins set elsewhere.
    #[test]
    fn the_pickers_say_what_they_hold() {
        assert_eq!(margin_options([20; 4]).len(), EVEN_MARGINS.len());
        let odd = margin_options([20, 25, 20, 25]);
        assert_eq!(odd[0], Margins([20, 25, 20, 25]), "set through the API, listed first");
        assert_eq!(Margins([20; 4]).to_string(), trf("{n} mm", &[("n", &20)]));
        assert_eq!(Margins([20, 25, 20, 25]).to_string(), trf("{n} mm", &[("n", &"20 · 25 · 20 · 25")]));
        assert_eq!(Points(11).to_string(), trf("{n} pt", &[("n", &11)]));
        assert_eq!(PaperChoice(Paper::Letter).to_string(), tr("US Letter"));
        assert_eq!(template_name(PdfTemplateId::Letter), trc("template", "Letter"));
        assert_eq!(file_name("/home/anna/logo.png"), "logo.png");
    }

    /// Every state builds in every size: both pages, a line typed, servers listed, none, and both
    /// lists failed; the header names the templates' page.
    #[test]
    fn the_section_builds_in_every_state() {
        let data = data();
        let mut empty = data.clone();
        empty.extensions = Listing::Entries(Vec::new());
        empty.tools.clear();
        let mut failed = data.clone();
        failed.extensions = Listing::Failed("unsupported".into());
        failed.catalog = Listing::Failed("unsupported".into());
        assert!(failed.extensions.entries().is_empty() && count(&failed) == 0);
        let mut typing = State::default();
        let _ = update(&mut typing, Message::Type(PdfTemplateId::Report, Line::Footer, "Draft".into()), &data);
        let templates = State { page: Page::PdfTemplates, ..State::default() };
        let list = State::default();
        assert!(header(&list, &data, SizeClass::Wide).sub_page.is_none());
        assert!(
            header(&templates, &data, SizeClass::Wide)
                .sub_page
                .is_some_and(|(name, back)| { name == tr("PDF templates") && back == Message::Back })
        );
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&list, &typing, &templates] {
                for data in [&data, &empty, &failed] {
                    let _ = view(state, data, class);
                }
            }
        }
        assert_eq!(Listing::from_result(Err::<Vec<CatalogEntry>, _>("no".into())), Listing::Failed("no".into()));
    }

    // ----- Task MA.8: catalog, installing, a server's page ------------------------------------------

    /// The request and what it is for of an action of this section.
    fn asked(action: Action) -> (Asked, Request) {
        match action {
            Action::AskConnections(asked, request) => (asked, request),
            other => panic!("expected exactly one request of the section, got {other:?}"),
        }
    }

    /// The text an action puts on the clipboard.
    fn copied(action: Action) -> Option<String> {
        match action {
            Action::Copy(text) => Some(text),
            _ => None,
        }
    }

    /// The demo's data without its GitHub server, so its catalog entry can be installed.
    fn uninstalled() -> Data {
        let mut data = launch::demo_data();
        launch::demo_uninstalled(&mut data);
        data
    }

    const TOKEN_NAME: &str = "GITHUB_PERSONAL_ACCESS_TOKEN";

    /// A server as the core installed it: off, its tools waiting.
    fn reported(data: &Data, entry: &str) -> ExtensionInfo {
        match launch::demo_answer(&Request::GetExtension { id: entry.into() }, data) {
            Some(Response::Extension { extension }) => extension,
            other => panic!("expected the server, got {other:?}"),
        }
    }

    /// The install dialog of GitHub at the tools it reported, as the demo installs it.
    fn at_its_tools(data: &Data) -> State {
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), data);
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), data);
        let (what, _) = asked(update(&mut state, Message::SubmitInstall, data));
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: reported(data, "github") }));
        assert!(state.install.as_ref().is_some_and(|install| install.installed() == Some("github")));
        state
    }

    /// "Install…" only for an entry with nothing missing that is not installed: the demo has no Node,
    /// so Sequential Thinking has none, and Time is installed.
    #[test]
    fn install_is_offered_only_without_anything_missing() {
        let data = uninstalled();
        assert!(installable(&data, "github").is_some());
        assert!(installable(&data, "sequential-thinking").is_none(), "needs Node.js, which the demo lacks");
        assert!(installable(&data, "time").is_none(), "installed already");
        assert!(installable(&data, "nothing").is_none());
        let mut state = State::default();
        for entry in ["sequential-thinking", "time"] {
            assert!(matches!(update(&mut state, Message::Install(entry.into()), &data), Action::None));
            assert!(state.install.is_none() && overlay(&state, &data, SizeClass::Wide).is_none(), "{entry}: no dialog");
        }
        let entry = data.catalog.entries().iter().find(|entry| entry.id == "sequential-thinking").expect("listed");
        assert_eq!(
            missing_sentence(entry),
            trf("{needs}. Install it to use this server.", &[("needs", &tr("Needs Node.js (npx)"))])
        );
        let _ = update(&mut state, Message::Install("github".into()), &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());
    }

    /// "Install" sends exactly one `install_extension` with the typed secrets as `Secret`s – only
    /// after every secret of the entry is typed, else the dialog says so after the first press. The
    /// answer shows the reported tools, and "Allow tools" sends `confirm_tools` with exactly the
    /// fingerprint of that list; allowed, the dialog closes.
    #[test]
    fn installing_sends_install_then_allow_with_the_fingerprint_shown() {
        let data = uninstalled();
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        assert!(matches!(update(&mut state, Message::SubmitInstall, &data), Action::None), "no token yet");
        assert!(state.install.as_ref().is_some_and(|install| install.tried && !install.busy));
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("github_pat_123")), &data);
        let (what, request) = asked(update(&mut state, Message::SubmitInstall, &data));
        let secrets = BTreeMap::from([(TOKEN_NAME.to_string(), Secret::new("github_pat_123"))]);
        let from = InstallFrom::Catalog { entry: "github".into(), secrets };
        assert_eq!((what.clone(), request), (Asked::Install("github".into()), Request::InstallExtension { from }));
        assert!(matches!(update(&mut state, Message::SubmitInstall, &data), Action::None), "on its way: once");

        let mut server = reported(&data, "github");
        server.fingerprint = Some("list-the-person-saw".into());
        let next = answered(&mut state, what, &Ok(Response::Extension { extension: server.clone() }));
        assert!(matches!(next, Action::None));
        assert_eq!(state.install.as_ref().and_then(Install::installed), Some("github"));
        let allow = Message::AllowTools("github".into(), "list-the-person-saw".into());
        let (what, request) = asked(update(&mut state, allow, &data));
        let confirm = Request::ConfirmExtensionTools { id: "github".into(), fingerprint: "list-the-person-saw".into() };
        assert_eq!((what.clone(), request), (Asked::Allow("github".into()), confirm));
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: server }));
        assert!(state.install.is_none(), "allowed: the dialog has done its job");
    }

    /// The button of the dialog carries the fingerprint of the list it shows.
    #[test]
    fn the_allow_button_carries_the_fingerprint_of_the_list_shown() {
        let data = uninstalled();
        let server = reported(&data, "github");
        let press = |server: &ExtensionInfo| {
            server.fingerprint.clone().map(|fingerprint| Message::AllowTools(server.id.clone(), fingerprint))
        };
        assert_eq!(press(&server), Some(Message::AllowTools("github".into(), "demo-github".into())));
        // The dialog and the page build with a server whose list was never read: no button to press.
        let mut unread = server.clone();
        unread.fingerprint = None;
        let _ = allow_button(&unread, false, false, true);
        assert_eq!(press(&unread), None);
    }

    /// A refused install says what that means in the dialog by the core's code; tools that changed
    /// under the dialog are read again and shown for another look.
    #[test]
    fn refusals_show_in_the_dialog_with_a_clear_sentence() {
        let data = uninstalled();
        let refused = |code: &str| Ok(Response::Error { code: code.into(), message: format!("core says {code}") });
        for (code, what) in [
            ("unsupported", tr("Something it needs is missing on this computer.")),
            ("conflict", tr("It is installed already.")),
            ("error", tr("It could not be installed.")),
        ] {
            let mut state = State::default();
            let _ = update(&mut state, Message::Install("github".into()), &data);
            let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), &data);
            let (asked_for, _) = asked(update(&mut state, Message::SubmitInstall, &data));
            let _ = answered(&mut state, asked_for, &refused(code));
            let install = state.install.as_ref().expect("still open");
            assert_eq!(install.problem, Some(Problem { what: Some(what), why: format!("core says {code}") }));
            assert!(!install.busy && install.installed().is_none(), "{code}: the form again");
            let _ = view(&state, &data, SizeClass::Wide);
            assert!(overlay(&state, &data, SizeClass::Wide).is_some());
        }
        let no_answer = Err("The core did not answer".to_string());
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        let _ = answered(&mut state, Asked::Install("github".into()), &no_answer);
        assert!(state.install.as_ref().is_some_and(|install| install.problem.is_none()), "not asked: not taken");

        // The list changed: the server is read again, its new list shown.
        let server = reported(&data, "github");
        let mut state = at_its_tools(&data);
        let fingerprint = server.fingerprint.clone().expect("read");
        let _ = update(&mut state, Message::AllowTools("github".into(), fingerprint), &data);
        let (what, request) = asked(answered(&mut state, Asked::Allow("github".into()), &refused("conflict")));
        assert_eq!(
            (what.clone(), request),
            (Asked::Refresh("github".into()), Request::GetExtension { id: "github".into() })
        );
        let mut changed = server;
        changed.fingerprint = Some("the new list".into());
        let _ = answered(&mut state, what, &Ok(Response::Extension { extension: changed }));
        let Some(Install { step: Step::Tools(shown), busy: false, .. }) = &state.install else {
            panic!("the tools again")
        };
        assert_eq!(shown.fingerprint.as_deref(), Some("the new list"));
    }

    /// Every button of a server's page sends exactly its request: opening reads the output, the
    /// switch, "Allow tools", "Replace…" with "Save", "Refresh", "Update" and "Remove…" with its
    /// question; "Copy" copies what was read; leaving the page forgets it.
    #[test]
    fn every_button_of_a_server_s_page_sends_exactly_its_request() {
        let data = launch::demo_data();
        let mut state = State::default();
        let id = || "github".to_string();
        let (what, request) = asked(update(&mut state, Message::Open(id()), &data));
        assert_eq!((what.clone(), request), (Asked::Logs(id()), Request::ReadExtensionLogs { id: id() }));
        assert_eq!(state.page, Page::Server(id()));
        let lines = vec!["one".to_string(), "two".to_string()];
        let _ = answered(&mut state, what, &Ok(Response::Logs { lines }));
        assert_eq!(copied(update(&mut state, Message::CopyOutput, &data)), Some("one\ntwo".to_string()));
        let typed = text_editor::Action::Edit(text_editor::Edit::Insert('x'));
        let _ = update(&mut state, Message::Output(typed), &data);
        assert_eq!(copied(update(&mut state, Message::CopyOutput, &data)), Some("one\ntwo".to_string()), "shows only");

        let cases = [
            (
                Message::Enable(id(), false),
                Asked::Switch(id()),
                Request::SetExtensionEnabled { id: id(), enabled: false },
            ),
            (
                Message::AllowTools(id(), "demo-github".into()),
                Asked::Allow(id()),
                Request::ConfirmExtensionTools { id: id(), fingerprint: "demo-github".into() },
            ),
            (Message::ReadOutput, Asked::Logs(id()), Request::ReadExtensionLogs { id: id() }),
            (Message::Update(id()), Asked::Update(id()), Request::UpdateExtension { id: id(), server: None }),
        ];
        for (message, what, request) in cases {
            assert_eq!(asked(update(&mut state, message.clone(), &data)), (what.clone(), request), "{message:?}");
            let _ = answered(&mut state, what, &Ok(Response::Ok));
        }

        let _ = update(&mut state, Message::Replace(TOKEN_NAME.into()), &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());
        assert!(matches!(update(&mut state, Message::SubmitReplacement, &data), Action::None), "empty: only says so");
        let _ = update(&mut state, Message::TypeReplacement(Secret::new("github_pat_new")), &data);
        let (what, request) = asked(update(&mut state, Message::SubmitReplacement, &data));
        let secret =
            Request::SetExtensionSecret { id: id(), name: TOKEN_NAME.into(), value: Secret::new("github_pat_new") };
        assert_eq!((what.clone(), request), (Asked::Secret(id()), secret));
        let _ = answered(&mut state, what, &Ok(Response::Ok));
        assert!(state.server.replacing.is_none(), "stored: the dialog closes");

        assert!(matches!(update(&mut state, Message::AskRemove, &data), Action::None), "asks first");
        assert_eq!(escape(&state), Some(Message::CancelRemove));
        let (what, request) = asked(update(&mut state, Message::Remove, &data));
        assert_eq!((what.clone(), request), (Asked::Remove(id()), Request::RemoveExtension { id: id() }));
        let _ = answered(&mut state, what, &Ok(Response::Ok));
        assert_eq!(state.page, Page::List, "removed: back to the list");

        let _ = update(&mut state, Message::Open(id()), &data);
        assert!(header(&state, &data, SizeClass::Wide).sub_page.is_some_and(|(name, _)| name == "GitHub"));
        assert_eq!(escape(&state), Some(Message::Back));
        let _ = update(&mut state, Message::Back, &data);
        assert!(matches!(state.server.output, Output::Reading) && state.page == Page::List);
        let link = update(&mut state, Message::Link("https://example.org".into()), &data);
        assert!(matches!(link, Action::OpenUrl(url) if url == "https://example.org"));
    }

    /// A refusal on a server's page shows above it; an answer for a page left meanwhile changes nothing.
    #[test]
    fn a_refusal_on_a_server_s_page_shows_above_it() {
        let data = launch::demo_data();
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("notes".into()), &data);
        let _ = update(&mut state, Message::Enable("notes".into(), true), &data);
        let refused = Ok(Response::Error { code: "conflict".into(), message: "Allow its tools first".into() });
        let _ = answered(&mut state, Asked::Switch("notes".into()), &refused);
        assert_eq!(state.server.problem.as_deref(), Some("Allow its tools first"));
        assert!(state.server.waiting.is_none());
        let _ = answered(&mut state, Asked::Remove("time".into()), &Ok(Response::Ok));
        assert_eq!(state.page, Page::Server("notes".into()), "another server's answer");
        let _ = view(&state, &data, SizeClass::Narrow);
    }

    /// What is typed into a secret never shows in the state's `Debug`: in the install dialog and in
    /// the dialog that replaces one.
    #[test]
    fn secrets_never_show_in_the_debug_of_the_state() {
        let data = uninstalled();
        let mut state = State::default();
        let _ = update(&mut state, Message::Install("github".into()), &data);
        let _ = update(&mut state, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("ghp_install_secret")), &data);
        let message = Message::TypeReplacement(Secret::new("ghp_replace_secret"));
        assert!(!format!("{message:?}").contains("ghp_replace_secret"));
        let _ = update(&mut state, Message::Open("time".into()), &data);
        let _ = update(&mut state, Message::Replace("TOKEN".into()), &data);
        let _ = update(&mut state, message, &data);
        let (_, request) = asked(update(&mut state, Message::SubmitReplacement, &data));
        let debug = format!("{state:?} {request:?}");
        assert!(debug.contains(TOKEN_NAME) && debug.contains("TOKEN"), "the names show: {debug}");
        assert!(!debug.contains("ghp_install_secret") && !debug.contains("ghp_replace_secret"), "{debug}");
    }

    /// The sidebar marks the section while a server stopped after errors (first) or waits for its
    /// tools; the version "Update" brings is the catalog's when it differs.
    #[test]
    fn the_sidebar_marks_servers_that_need_the_person() {
        let mut data = launch::demo_data();
        assert_eq!(alert(&data), Some(Dot::Error), "GitHub stopped after errors");
        launch::demo_uninstalled(&mut data);
        assert_eq!(alert(&data), Some(Dot::Attention), "Notes waits for its tools");
        data.extensions = Listing::Entries(Vec::new());
        assert_eq!(alert(&data), None);

        let mut data = launch::demo_data();
        let time = server_of(&data, "time").cloned().expect("Time");
        assert_eq!(update_available(&data, &time), None, "in the catalog's version");
        let mut old = time;
        old.source = Source::Catalog { entry: "time".into(), version: "2025.1.1".into() };
        let catalog = data.catalog.entries().iter().find(|entry| entry.id == "time").map(|entry| entry.version.clone());
        assert_eq!(update_available(&data, &old), catalog);
        assert_eq!(update_available(&data, server_of(&data, "notes").expect("Notes")), None, "one's own");
        launch::demo_review(&mut data);
        let time = server_of(&data, "time").expect("Time");
        let changed = time.tools.iter().find(|tool| tool.name == "convert_time").expect("changed");
        assert_eq!(tool_state(time, changed), (Dot::Attention, tr("Changed since you allowed it").to_string()));
        let fresh = reported(&uninstalled(), "github");
        assert_eq!(tool_state(&fresh, &fresh.tools[0]), (Dot::Attention, tr("Not allowed yet").to_string()));
    }

    /// Every new state builds in every size: the catalog with "Install…", both steps of the dialog
    /// busy and not, each server's page with its output read, failed or empty, and its dialogs.
    #[test]
    fn the_catalog_the_dialog_and_the_pages_build_in_every_size() {
        let data = uninstalled();
        let full = launch::demo_data();
        let mut form = State::default();
        let _ = update(&mut form, Message::Install("github".into()), &data);
        let _ = update(&mut form, Message::SubmitInstall, &data);
        let mut busy = State::default();
        let _ = update(&mut busy, Message::Install("github".into()), &data);
        let _ = update(&mut busy, Message::TypeSecret(TOKEN_NAME.into(), Secret::new("t")), &data);
        let _ = update(&mut busy, Message::SubmitInstall, &data);
        let tools = at_its_tools(&data);
        let mut pages = Vec::new();
        for id in ["time", "github", "notes", "gone"] {
            let mut page = State::default();
            let _ = update(&mut page, Message::Open(id.into()), &full);
            pages.push(page);
            let mut read = State::default();
            let (what, _) = asked(update(&mut read, Message::Open(id.into()), &full));
            let _ = answered(&mut read, what, &Ok(Response::Logs { lines: vec!["started".into()] }));
            let _ = update(&mut read, Message::Replace(TOKEN_NAME.into()), &full);
            pages.push(read);
            let mut failed = State::default();
            let (what, _) = asked(update(&mut failed, Message::Open(id.into()), &full));
            let _ = answered(&mut failed, what, &Err("no answer".into()));
            let _ = update(&mut failed, Message::AskRemove, &full);
            pages.push(failed);
        }
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&form, &busy, &tools] {
                let _ = view(state, &data, class);
                assert!(overlay(state, &data, class).is_some());
            }
            for state in &pages {
                let _ = view(state, &full, class);
                let _ = overlay(state, &full, class);
            }
            let _ = view(&State::default(), &full, class);
        }
    }

    // ----- Task MA.9: a server of one's own -----------------------------------------------------

    /// A block as another app has it: a command with arguments, a token and a plain variable.
    const BLOCK: &str = r#"{"mcpServers": {"Weather API": {"command": "uvx", "args": ["mcp-weather", "--units", "metric"],
        "env": {"WEATHER_API_TOKEN": "tok_pasted_123", "WEATHER_REGION": "eu"}}}}"#;

    /// The form after "Add server…" with `block` pasted and "Fill in" pressed.
    fn pasted(block: &str, data: &Data) -> State {
        let mut state = State::default();
        let steps =
            [Message::AddServer, Message::OpenPaste, Message::PasteText(Typed::paste(block)), Message::FillFromPaste];
        for message in steps {
            assert!(matches!(update(&mut state, message.clone(), data), Action::None), "{message:?} sends nothing");
        }
        state
    }

    /// The open form of a server of one's own.
    fn form(state: &State) -> &Custom {
        state.custom.as_deref().expect("the form is open")
    }

    /// Acceptance: a pasted block with `env` fills the form – name, the id proposed from it, command,
    /// one argument per line – and the token's row is marked secret, the other one not.
    #[test]
    fn a_pasted_block_fills_the_form_and_marks_the_token_secret() {
        let data = launch::demo_data();
        let state = pasted(BLOCK, &data);
        let custom = form(&state);
        assert!(matches!(custom.step, CustomStep::Form), "one server: straight into the form");
        assert_eq!(
            (custom.kind, custom.name.as_str(), custom.id.as_str(), custom.command.as_str()),
            (Kind::Command, "Weather API", "weather-api", "uvx")
        );
        assert_eq!(arguments(&custom.args.text()), ["mcp-weather", "--units", "metric"]);
        let row = |name: &str| custom.values.iter().find(|entry| entry.name == name).expect(name);
        let token = row("WEATHER_API_TOKEN");
        assert!(token.secret && !token.stored, "a token is a secret");
        assert_eq!(token.value, Secret::new("tok_pasted_123"));
        assert!(!row("WEATHER_REGION").secret);
        assert!(!custom.tried && check(custom, &data) == Checked { values: vec![None, None], ..Checked::default() });
    }

    /// Review of #302: the arguments and the address of a pasted server never show in `Debug`, in the
    /// list of the block nor in the form; a block of Claude Desktop often carries its token there.
    #[test]
    fn the_arguments_and_the_address_never_show_in_debug() {
        let data = launch::demo_data();
        let block = r#"{"mcpServers": {"a": {"command": "uvx", "args": ["srv", "--token", "ghp_in_args"]},
            "b": {"url": "https://mcp.example.com/mcp?api_key=key_in_url"}}}"#;
        let mut state = pasted(block, &data);
        assert!(matches!(form(&state).step, CustomStep::Choose), "two servers to choose from");
        for at in [0, 1] {
            let _ = update(&mut state, Message::ChoosePasted(at), &data);
            let shown = format!("{state:?}");
            assert!(!shown.contains("ghp_in_args") && !shown.contains("key_in_url"), "{shown}");
        }
        assert_eq!(form(&state).url, "https://mcp.example.com/mcp?api_key=key_in_url", "kept for the form");
        // Second review of #302: typed into the field, the address stays out of `Debug` too.
        let typed = Message::CustomUrl(Secret::new("https://mcp.example.com/mcp?api_key=typed_key"));
        assert!(!format!("{typed:?}").contains("typed_key"), "{typed:?}");
        let _ = update(&mut state, typed, &data);
        assert!(!format!("{state:?}").contains("typed_key"));
        assert_eq!(form(&state).url, "https://mcp.example.com/mcp?api_key=typed_key");
    }

    /// Review of #302: why a pasted server can't be added is in the suite's language, for every fixed
    /// reason `torro_ipc::extensions` gives.
    #[test]
    fn why_a_pasted_server_cant_be_added_is_translated() {
        use torro_core::i18n::{Lang, tr_in};
        let cases = [
            r#"{" ": {"command": "x"}}"#,
            r#"{"a": 1}"#,
            r#"{"a": {"type": 1, "command": "x"}}"#,
            r#"{"a": {"command": ""}}"#,
            r#"{"a": {"command": "x", "url": "https://x.example"}}"#,
            r#"{"a": {"command": "x", "args": [1]}}"#,
            r#"{"a": {"command": "x", "args": "x"}}"#,
            r#"{"a": {"type": "stdio", "url": "https://x.example"}}"#,
            r#"{"a": {}}"#,
            r#"{"a": {"command": "x", "env": 1}}"#,
            r#"{"a": {"command": "x", "env": {"A=B": "x"}}}"#,
            r#"{"a": {"url": "https://x.example", "headers": {"bad header": "x"}}}"#,
            r#"{"a": {"command": "x", "env": {"A": 1}}}"#,
        ];
        let mut reasons = Vec::new();
        for case in cases {
            let Err(error @ ParseError::Invalid { reason, .. }) = torro_ipc::extensions::parse_mcp_servers(case) else {
                panic!("{case} is invalid");
            };
            assert_ne!(tr_in(Lang::De, reason), reason, "“{reason}” has a German translation");
            assert!(paste_problem(&error).contains(tr(reason)), "{reason}");
            reasons.push(reason);
        }
        reasons.sort_unstable();
        reasons.dedup();
        assert_eq!(reasons.len(), cases.len(), "each case its own reason");
    }

    /// Acceptance: a blocked variable shows its sentence at its row, only after the first try, and
    /// nothing goes out; pasted, the block says so in one sentence.
    #[test]
    fn a_blocked_variable_shows_its_error_at_the_row() {
        let data = launch::demo_data();
        let mut state = pasted(BLOCK, &data);
        let _ = update(&mut state, Message::AddValue, &data);
        let at = form(&state).values.len() - 1;
        let _ = update(&mut state, Message::ValueName(at, "ld_preload".into()), &data);
        let _ = update(&mut state, Message::ValueText(at, Secret::new("/tmp/hook.so")), &data);
        assert!(!form(&state).tried, "nothing is said before the first try");
        let tried = update(&mut state, Message::SubmitCustom, &data);
        assert!(matches!(&tried, Action::Focus(id) if *id == value_field(at)), "nothing goes out: {tried:?}");
        let custom = form(&state);
        assert!(custom.tried && !custom.busy);
        let checked = check(custom, &data);
        assert_eq!(checked.value(at), Some(tr("Torro Workplace never passes this variable on to a server.")));
        assert_eq!((checked.name, checked.id, checked.target, checked.value(0)), (None, None, None, None), "only it");
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());
        // Removed, the form goes out.
        let _ = update(&mut state, Message::RemoveValue(at), &data);
        assert!(matches!(update(&mut state, Message::SubmitCustom, &data), Action::AskConnections(..)));

        let blocked = r#""mcpServers": {"notes": {"command": "notes", "env": {"PATH": "/tmp"}}}"#;
        let mut state = State::default();
        for message in [Message::AddServer, Message::OpenPaste, Message::PasteText(Typed::paste(blocked))] {
            let _ = update(&mut state, message, &data);
        }
        let _ = update(&mut state, Message::FillFromPaste, &data);
        let CustomStep::Paste { problem: Some(problem), .. } = &form(&state).step else {
            panic!("the block stays with its sentence")
        };
        let expected = paste_problem(&ParseError::Blocked { server: "notes".into(), variable: "PATH".into() });
        assert_eq!(problem, &expected);
        assert!(problem.contains("PATH"));
        let _ = update(&mut state, Message::PasteText(Typed::paste("not json")), &data);
        let _ = update(&mut state, Message::FillFromPaste, &data);
        assert!(matches!(&form(&state).step, CustomStep::Paste { problem: Some(_), .. }));
        assert_eq!(escape(&state), Some(Message::BackToForm));
    }

    /// Acceptance: "Add server" sends exactly one `install_extension` with `custom`, the secret as
    /// `Secret`, the plain value as it is; the answer goes on as the catalog does: the reported tools,
    /// "Allow tools" with their fingerprint.
    #[test]
    fn adding_sends_exactly_one_install_with_the_secrets_as_secrets() {
        let data = launch::demo_data();
        let mut state = pasted(BLOCK, &data);
        let (what, request) = asked(update(&mut state, Message::SubmitCustom, &data));
        let env = BTreeMap::from([
            ("WEATHER_API_TOKEN".to_string(), Value::Secret(Secret::new("tok_pasted_123"))),
            ("WEATHER_REGION".to_string(), Value::Plain("eu".into())),
        ]);
        let args = vec!["mcp-weather".to_string(), "--units".into(), "metric".into()];
        let server = ServerSpec::Stdio { command: "uvx".into(), args, env };
        let from = InstallFrom::Custom { id: "weather-api".into(), name: "Weather API".into(), server };
        let expected = Request::InstallExtension { from };
        assert_eq!((what.clone(), request.clone()), (Asked::Install("weather-api".into()), expected));
        assert!(form(&state).busy);
        assert!(matches!(update(&mut state, Message::SubmitCustom, &data), Action::None), "on its way: once");
        assert!(!format!("{request:?}").contains("tok_pasted_123"), "a request shows no secret either");

        let answer = launch::demo_answer(&request, &data).expect("the demo adds it");
        let _ = answered(&mut state, what, &Ok(answer));
        assert!(state.custom.is_none(), "one server: the form has done its job");
        assert_eq!(state.install.as_ref().and_then(Install::installed), Some("weather-api"));
        assert!(overlay(&state, &data, SizeClass::Wide).is_some(), "the tools it reported, with no catalog entry");
        let (what, request) =
            asked(update(&mut state, Message::AllowTools("weather-api".into(), "demo-weather-api".into()), &data));
        let confirm =
            Request::ConfirmExtensionTools { id: "weather-api".into(), fingerprint: "demo-weather-api".into() };
        assert_eq!((what, request), (Asked::Allow("weather-api".into()), confirm));
    }

    /// What only the core can tell comes back as its sentence in the form, with what that means: an
    /// address it does not take, an id in use. Typing on is possible, and the next try goes out.
    #[test]
    fn the_core_s_refusal_shows_in_the_form() {
        let data = launch::demo_data();
        let mut state = State::default();
        for message in [
            Message::AddServer,
            Message::CustomKind(Kind::Address),
            Message::CustomName("Team wiki".into()),
            Message::CustomUrl(Secret::new("http://example.com/mcp")),
        ] {
            let _ = update(&mut state, message, &data);
        }
        let (what, request) = asked(update(&mut state, Message::SubmitCustom, &data));
        let server = ServerSpec::Http { url: "http://example.com/mcp".into(), headers: BTreeMap::new() };
        let from = InstallFrom::Custom { id: "team-wiki".into(), name: "Team wiki".into(), server };
        assert_eq!(request, Request::InstallExtension { from });
        let message = "http:// only for localhost, 127.0.0.1 and ::1".to_string();
        let _ = answered(&mut state, what, &Ok(Response::Error { code: "error".into(), message: message.clone() }));
        let custom = form(&state);
        assert_eq!(custom.problem, Some(Problem { what: Some(tr("It could not be added.")), why: message }));
        assert!(!custom.busy && matches!(custom.step, CustomStep::Form));
        let _ = view(&state, &data, SizeClass::Wide);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());

        let (what, _) = asked(update(&mut state, Message::SubmitCustom, &data));
        let refused = Ok(Response::Error { code: "conflict".into(), message: "in use".into() });
        let _ = answered(&mut state, what, &refused);
        assert_eq!(
            form(&state).problem.as_ref().and_then(|problem| problem.what),
            Some(tr("Another server has this id already."))
        );
        // An answer for another id does not reach the form.
        let _ = asked(update(&mut state, Message::SubmitCustom, &data));
        let _ = answered(&mut state, Asked::Install("other".into()), &refused);
        assert!(form(&state).busy);
    }

    /// What the form checks itself after the first try: a name, a valid id no server has, the
    /// command or the address, row names, names listed twice and an empty secret.
    #[test]
    fn the_form_checks_what_it_can_tell_itself() {
        let data = launch::demo_data();
        let mut state = State::default();
        let _ = update(&mut state, Message::AddServer, &data);
        assert!(matches!(update(&mut state, Message::SubmitCustom, &data), Action::Focus(_)), "nothing goes out");
        let checked = check(form(&state), &data);
        assert_eq!(checked.name, Some(tr("Enter a name.")));
        assert!(checked.id.is_some());
        assert_eq!(checked.target, Some(tr("Enter the program to start.")));
        let _ = update(&mut state, Message::CustomKind(Kind::Address), &data);
        assert_eq!(check(form(&state), &data).target, Some(tr("Enter the address.")));

        let _ = update(&mut state, Message::CustomName("Time".into()), &data);
        assert_eq!(form(&state).id, "time-2", "Time is installed: the proposal moves on");
        let _ = update(&mut state, Message::CustomId("time".into()), &data);
        assert_eq!(check(form(&state), &data).id, Some(tr("Another server has this id already.")));
        let _ = update(&mut state, Message::CustomName("Clock".into()), &data);
        assert_eq!(form(&state).id, "time", "typed: it no longer follows the name");
        let _ = update(&mut state, Message::CustomId("Bad_Id".into()), &data);
        assert!(check(form(&state), &data).id.is_some());
        let _ = update(&mut state, Message::CustomId(String::new()), &data);
        let _ = update(&mut state, Message::CustomName("Clock".into()), &data);
        assert_eq!(form(&state).id, "clock", "typed away: it follows the name again");

        // Headers: an invalid name, one listed twice in another case, an empty secret, an empty row.
        for (at, name) in ["Bad Header", "Authorization", "authorization", ""].into_iter().enumerate() {
            let _ = update(&mut state, Message::AddValue, &data);
            let _ = update(&mut state, Message::ValueName(at, name.into()), &data);
        }
        let _ = update(&mut state, Message::ValueText(0, Secret::new("x")), &data);
        let _ = update(&mut state, Message::ValueText(2, Secret::new("y")), &data);
        let custom = form(&state);
        assert!(custom.values[1].secret && custom.values[2].secret, "Authorization looks like a secret");
        let checked = check(custom, &data);
        assert_eq!(checked.value(0), Some(tr("A header's name has no spaces and no colon.")));
        assert_eq!(checked.value(1), Some(tr("Enter its value.")));
        assert_eq!(checked.value(2), Some(tr("This name is listed twice.")));
        assert_eq!(checked.value(3), None, "an empty row goes nowhere");
        // The switch is the person's once flipped; the name no longer moves it.
        let _ = update(&mut state, Message::ValueSecret(1, false), &data);
        let _ = update(&mut state, Message::ValueName(1, "X-Api-Token".into()), &data);
        assert!(!form(&state).values[1].secret);
    }

    /// Design review of #302: after a try the foot names the fields to check, in the form's order,
    /// since the dialog cannot scroll to the first one itself; nothing while none is marked.
    #[test]
    fn the_foot_names_the_fields_to_check() {
        let data = launch::demo_data();
        let mut state = State::default();
        let _ = update(&mut state, Message::AddServer, &data);
        let tried = update(&mut state, Message::SubmitCustom, &data);
        assert!(
            matches!(&tried, Action::Focus(id) if *id == NAME_FIELD.into()),
            "the first takes the focus: {tried:?}"
        );
        let fields = marked_fields(Kind::Command, &check(form(&state), &data));
        assert_eq!(fields, vec![tr("Name"), tr("Id"), tr("Command")]);
        let sentence = trf(
            "Check {fields} and {last}.",
            &[("fields", &format!("{}, {}", tr("Name"), tr("Id"))), ("last", &tr("Command"))],
        );
        assert_eq!(check_sentence(&fields), Some(sentence));
        let _ = update(&mut state, Message::CustomName("Clock".into()), &data);
        let _ = update(&mut state, Message::CustomKind(Kind::Address), &data);
        let fields = marked_fields(Kind::Address, &check(form(&state), &data));
        assert_eq!(check_sentence(&fields), Some(trf("Check {field}.", &[("field", &tr("Address"))])));
        assert_eq!(check_sentence(&[]), None);
        let checked = check(form(&state), &data);
        assert_eq!(first_marked(Kind::Address, &checked), Some(URL_FIELD.into()));
        let only_a_row = Checked { values: vec![None, Some(RowProblem::name("wrong"))], ..Checked::default() };
        assert_eq!(first_marked(Kind::Command, &only_a_row), Some(value_field(1)));
        let a_missing_value =
            Checked { values: vec![Some(RowProblem { text: "any words", at_value: true })], ..Checked::default() };
        assert_eq!(first_marked(Kind::Command, &a_missing_value), Some(value_text_field(0)), "where the value goes");
        // The check marks a missing secret at its value and a wrong name at its name, by field, not by its words.
        let row = |name: &str, secret| Entry { name: name.into(), secret, ..Entry::default() };
        let missing = entry_problem(Kind::Command, &[], &row("API_KEY", true)).expect("a missing secret");
        assert!(missing.at_value);
        let blocked = entry_problem(Kind::Command, &[], &row("LD_PRELOAD", false)).expect("a blocked name");
        assert!(!blocked.at_value);
        assert_eq!(first_marked(Kind::Command, &Checked::default()), None);
    }

    /// The id proposed from the name, and which names start as secrets.
    #[test]
    fn the_id_follows_the_name_and_secrets_follow_theirs() {
        let none = |_: &str| false;
        assert_eq!(suggested_id("Weather API", none), "weather-api");
        assert_eq!(suggested_id("  2 fast · 4 you! ", none), "fast-4-you");
        assert_eq!(suggested_id("Grüße", none), "gr-e");
        assert_eq!(suggested_id("Ärger", none), "rger");
        assert_eq!(suggested_id("42", none), "");
        let long = suggested_id("A very long name for a server of one's own", none);
        assert!(valid_id(&long) && long.len() <= MAX_ID_CHARS, "{long}");
        let taken = |id: &str| ["time", "time-2"].contains(&id);
        assert_eq!(suggested_id("Time", taken), "time-3");
        let full = "abcdefghijklmnopqrstuvwx";
        assert_eq!(suggested_id(full, |id| id == full), "abcdefghijklmnopqrstuv-2");
        for name in ["GITHUB_TOKEN", "api_key", "Authorization", "DB_PASSWORD", "client_secret", "X-Auth"] {
            assert!(looks_secret(name), "{name}");
        }
        for name in ["REGION", "HOME_DIR", "Accept"] {
            assert!(!looks_secret(name), "{name}");
        }
    }

    /// Several servers in a block: a choice, each into a form of its own; added, the tools it
    /// reported come first, then the choice of the others again.
    #[test]
    fn several_pasted_servers_get_a_form_each() {
        let data = launch::demo_data();
        let mut state = pasted(launch::DEMO_PASTE_TWO, &data);
        assert!(matches!(form(&state).step, CustomStep::Choose));
        assert_eq!(form(&state).pasted.len(), 2);
        assert_eq!(escape(&state), Some(Message::BackToForm));
        let wiki = form(&state).pasted.iter().position(|pasted| pasted.name == "team-wiki").expect("listed");
        let _ = update(&mut state, Message::ChoosePasted(wiki), &data);
        let custom = form(&state);
        assert_eq!(
            (custom.kind, custom.name.as_str(), custom.url.as_str()),
            (Kind::Address, "team-wiki", "https://wiki.example.org/mcp")
        );
        assert!(custom.values.iter().all(|entry| entry.secret), "Authorization is a secret");
        let (what, request) = asked(update(&mut state, Message::SubmitCustom, &data));
        let Request::InstallExtension { from: InstallFrom::Custom { server: ServerSpec::Http { headers, .. }, .. } } =
            &request
        else {
            panic!("an address: {request:?}")
        };
        assert!(matches!(headers.get("Authorization"), Some(Value::Secret(_))));
        let _ = answered(&mut state, what, &Ok(launch::demo_answer(&request, &data).expect("added")));
        assert_eq!(state.install.as_ref().and_then(Install::installed), Some("team-wiki"));
        let custom = form(&state);
        assert!(matches!(custom.step, CustomStep::Choose) && custom.pasted.len() == 1, "the other one waits");
        assert_eq!(custom.pasted[0].name, "weather");
        // "Not now" closes only the tools; the choice comes back.
        let _ = update(&mut state, Message::CloseInstall, &data);
        assert!(state.install.is_none() && state.custom.is_some());
        let _ = update(&mut state, Message::ChoosePasted(0), &data);
        assert_eq!(form(&state).command, "uvx");
        assert_eq!(escape(&state), Some(Message::CloseInstall));
        let _ = update(&mut state, Message::CloseInstall, &data);
        assert!(state.custom.is_none());
    }

    /// "Update…" on the page of one's own server: the form filled in from it, its name and id kept;
    /// a stored secret left empty goes out as `Stored`, a typed one as `Secret`; "Save" sends exactly
    /// one `update_extension`. A catalog server has no such form.
    #[test]
    fn updating_one_s_own_server_keeps_its_stored_secrets() {
        let mut data = launch::demo_data();
        if let Listing::Entries(servers) = &mut data.extensions {
            let notes = servers.iter_mut().find(|server| server.id == "notes").expect("Notes");
            notes.server = ServerSpec::Stdio {
                command: "/usr/local/bin/notes-mcp".into(),
                args: vec!["--stdio".into()],
                env: BTreeMap::from([
                    ("NOTES_DIR".to_string(), Value::Plain("~/Notes".into())),
                    ("NOTES_TOKEN".to_string(), Value::Stored { secret: true }),
                ]),
            };
        }
        let mut state = State::default();
        let _ = update(&mut state, Message::Open("notes".into()), &data);
        let _ = update(&mut state, Message::EditServer("time".into()), &data);
        assert!(state.custom.is_none(), "a catalog server is updated from the catalog");
        let _ = update(&mut state, Message::EditServer("notes".into()), &data);
        let custom = form(&state);
        assert_eq!(
            (custom.updating.as_deref(), custom.id.as_str(), custom.name.as_str()),
            (Some("notes"), "notes", "Notes")
        );
        assert!(custom.values.iter().any(|entry| entry.name == "NOTES_TOKEN" && entry.secret && entry.stored));
        assert!(custom.values.iter().any(|entry| entry.name == "NOTES_DIR" && !entry.secret), "plain stays plain");
        let _ = view(&state, &data, SizeClass::Wide);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());

        let (what, request) = asked(update(&mut state, Message::SubmitCustom, &data));
        let env = BTreeMap::from([
            ("NOTES_DIR".to_string(), Value::Plain("~/Notes".into())),
            ("NOTES_TOKEN".to_string(), Value::Stored { secret: true }),
        ]);
        let server =
            ServerSpec::Stdio { command: "/usr/local/bin/notes-mcp".into(), args: vec!["--stdio".into()], env };
        assert_eq!(
            (what.clone(), request),
            (Asked::Update("notes".into()), Request::UpdateExtension { id: "notes".into(), server: Some(server) })
        );
        let message = "the stored secrets belong to another program: enter them again".to_string();
        let _ = answered(&mut state, what, &Ok(Response::Error { code: "error".into(), message: message.clone() }));
        assert_eq!(form(&state).problem, Some(Problem { what: Some(tr("It could not be updated.")), why: message }));
        assert!(state.server.problem.is_none(), "the form says it, not the page");

        let at = form(&state).values.iter().position(|entry| entry.name == "NOTES_TOKEN").expect("its row");
        let _ = update(&mut state, Message::ValueText(at, Secret::new("tok_new")), &data);
        let (what, request) = asked(update(&mut state, Message::SubmitCustom, &data));
        let Request::UpdateExtension { server: Some(ServerSpec::Stdio { env, .. }), .. } = &request else {
            panic!("an update: {request:?}")
        };
        assert_eq!(env.get("NOTES_TOKEN"), Some(&Value::Secret(Secret::new("tok_new"))));
        let _ = answered(
            &mut state,
            what,
            &Ok(Response::Extension { extension: server_of(&data, "notes").cloned().expect("Notes") }),
        );
        assert!(state.custom.is_none() && state.page == Page::Server("notes".into()), "saved: back on its page");
    }

    /// Acceptance: what is typed or pasted into the form never shows in the state's `Debug`: the
    /// block while it is in its field, its values in the form and in the choice, a typed secret.
    #[test]
    fn secrets_of_the_form_never_show_in_the_debug_of_the_state() {
        let data = launch::demo_data();
        let mut state = State::default();
        for message in
            [Message::AddServer, Message::OpenPaste, Message::PasteText(Typed::paste(launch::DEMO_PASTE_TWO))]
        {
            assert!(!format!("{message:?}").contains("demo-not-a-real"), "{message:?}");
            let _ = update(&mut state, message, &data);
        }
        let mut seen = vec![format!("{state:?}")];
        let _ = update(&mut state, Message::FillFromPaste, &data);
        seen.push(format!("{state:?}"));
        let _ = update(&mut state, Message::ChoosePasted(1), &data);
        let _ = update(&mut state, Message::AddValue, &data);
        let at = form(&state).values.len() - 1;
        let typed = Message::ValueText(at, Secret::new("typed_secret_789"));
        assert!(!format!("{typed:?}").contains("typed_secret_789"));
        let _ = update(&mut state, typed, &data);
        seen.push(format!("{state:?}"));
        for debug in seen {
            assert!(debug.contains("Custom"), "{debug}");
            assert!(!debug.contains("demo-not-a-real") && !debug.contains("typed_secret_789"), "{debug}");
        }
    }

    /// Every state of the form builds in every size, its header action too: empty and tried, filled
    /// in, busy, the pasted block with its sentence, the choice, "Update…" and the tools of a server
    /// of one's own just added.
    #[test]
    fn the_form_of_one_s_own_server_builds_in_every_size() {
        let data = launch::demo_data();
        let mut tried = State::default();
        let _ = update(&mut tried, Message::AddServer, &data);
        let _ = update(&mut tried, Message::AddValue, &data);
        let _ = update(&mut tried, Message::SubmitCustom, &data);
        let filled = pasted(BLOCK, &data);
        let mut busy = pasted(BLOCK, &data);
        let _ = update(&mut busy, Message::SubmitCustom, &data);
        let mut paste = State::default();
        for message in
            [Message::AddServer, Message::OpenPaste, Message::PasteText(Typed::paste("{")), Message::FillFromPaste]
        {
            let _ = update(&mut paste, message, &data);
        }
        let choose = pasted(launch::DEMO_PASTE_TWO, &data);
        let mut editing = State::default();
        let _ = update(&mut editing, Message::Open("notes".into()), &data);
        let _ = update(&mut editing, Message::EditServer("notes".into()), &data);
        let mut tools = pasted(BLOCK, &data);
        let (what, request) = asked(update(&mut tools, Message::SubmitCustom, &data));
        let _ = answered(&mut tools, what, &Ok(launch::demo_answer(&request, &data).expect("added")));
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini, SizeClass::Strip] {
            for state in [&tried, &filled, &busy, &paste, &choose, &editing, &tools] {
                let _ = view(state, &data, class);
                assert!(overlay(state, &data, class).is_some());
            }
            assert_eq!(header(&State::default(), &data, class).actions.len(), 1, "Add server…");
        }
        assert_eq!(
            asked(update(&mut State::default(), Message::Open("notes".into()), &data)).0,
            Asked::Logs("notes".into())
        );
    }
}
