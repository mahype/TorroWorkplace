//! AI accounts (task KI.1a, `docs/ki.md`): the suite calls a model. The types every part of the
//! suite shares – which kind of server an account talks to ([`AiProvider`]), the models it offers
//! ([`AiModel`]), a conversation with tools ([`ChatRequest`], [`ChatReply`]) – and the
//! [`Provider`] trait a server's protocol implements.
//!
//! Three wire formats: OpenAI-compatible Chat Completions for Ollama, LM Studio, Strata, vLLM,
//! llama.cpp, OpenRouter and most providers ([`openai`]); OpenAI's Responses API
//! ([`responses`]); and Anthropic's Messages API ([`anthropic`]). Each module holds its wire
//! format (request bodies, answers, the server-sent events of a streamed answer, model lists),
//! pure and without a network; the core
//! sends them (`torroworkplace::account::ai`), one implementation of [`Provider`] per protocol. A
//! further protocol is another module beside these, never a change of the types here.
//!
//! Open First (`docs/ki.md`, principle 2): local servers come first, then open models over
//! OpenRouter. A model list keeps only open-weights models unless asked for all
//! ([`open_weights_only`]).
//!
//! The API key of an account is a secret: it lives in the system keychain and travels as a
//! [`crate::account::Secret`]. Nothing here holds or prints one; [`AiError`] never carries a
//! server's words that could repeat it ([`openai::error_of`]).
//!
//! Issue #325 (task MW.7): [`pull`] is the wire format of downloading a model into Ollama.

pub mod anthropic;
pub mod decide_chat;
pub mod hardware;
pub mod openai;
pub mod pull;
pub mod responses;
pub mod strata;
pub mod systemone;

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::i18n::{tr, trf};

/// A boxed future that can be sent to another thread: what [`Provider`]'s methods return, so the
/// trait can be used as `dyn Provider` (one account may be Ollama, the next Anthropic).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which kind of server an AI account talks to. Decides the usual address, whether a key is
/// needed, and where the core learns which models can call tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    /// Ollama, usually on this computer (`http://localhost:11434/v1`). Tells which models can call
    /// tools (`/api/show`).
    Ollama,
    /// LM Studio's local server (`http://localhost:1234/v1`).
    LmStudio,
    /// Strata (`http://127.0.0.1:8080/v1`, issue #322), a local server for large open models. The
    /// OpenAI-compatible protocol; [`strata`] tells it from llama.cpp, which uses the same port.
    Strata,
    /// OpenRouter (`https://openrouter.ai/api/v1`): models of many makers behind one API key. Its
    /// model list says which can call tools and which have open weights. Written `openrouter`.
    #[serde(rename = "openrouter")]
    OpenRouter,
    /// Anthropic's API (`https://api.anthropic.com/v1`), the Claude models. The only provider that
    /// speaks a protocol of its own ([`anthropic`]) instead of the OpenAI-compatible one.
    Anthropic,
    /// Any other server with the OpenAI-compatible protocol: vLLM, llama.cpp, LocalAI, a server of
    /// your own. Its address is always given. Written `openai_compatible`.
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    /// OpenAI's Responses API (task W.3). Written `openai`.
    #[serde(rename = "openai")]
    OpenAi,
    /// Google Gemini through its OpenAI-compatible endpoint.
    Gemini,
    /// Mistral AI, a provider in the EU.
    Mistral,
    /// DeepSeek. Written `deepseek`.
    #[serde(rename = "deepseek")]
    DeepSeek,
    /// Qwen on Alibaba Cloud Model Studio, its international endpoint.
    Qwen,
    /// Kimi by Moonshot AI, its international endpoint.
    Kimi,
    /// GLM by Z.ai (Zhipu), its international endpoint.
    Glm,
    /// MiniMax, its international endpoint. Written `minimax`.
    #[serde(rename = "minimax")]
    MiniMax,
}

/// Where a provider's servers are (task W.3, `docs/einrichtung.md`): shown with each provider, so
/// the choice where data goes is made knowingly. Neutral, never a warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Seat {
    /// On this computer or a machine of the person's own (Ollama, LM Studio, Strata).
    ThisComputer,
    /// Wherever the person's own server is.
    OwnServer,
    /// A cloud service in the United States.
    Us,
    /// A cloud service in the European Union.
    Eu,
    /// A cloud service run from China.
    China,
}

impl Seat {
    /// The words shown with a provider: "This computer", "Your server", "Cloud · US" …
    pub fn label(self) -> &'static str {
        match self {
            Seat::ThisComputer => tr("This computer"),
            Seat::OwnServer => tr("Your server"),
            Seat::Us => tr("Cloud · US"),
            Seat::Eu => tr("Cloud · EU"),
            Seat::China => tr("Cloud · China"),
        }
    }
}

impl AiProvider {
    /// Every provider in the order the suite lists them (Open First): this computer, open models in
    /// the cloud, the providers, those run from China, and any other server last.
    pub const ALL: [AiProvider; 14] = [
        AiProvider::Ollama,
        AiProvider::LmStudio,
        AiProvider::Strata,
        AiProvider::OpenRouter,
        AiProvider::Anthropic,
        AiProvider::OpenAi,
        AiProvider::Gemini,
        AiProvider::Mistral,
        AiProvider::DeepSeek,
        AiProvider::Qwen,
        AiProvider::Kimi,
        AiProvider::Glm,
        AiProvider::MiniMax,
        AiProvider::OpenAiCompatible,
    ];

    /// The providers that run on this computer and that the suite looks for at their usual
    /// address, in the order of [`AiProvider::ALL`].
    pub const LOCAL: [AiProvider; 3] = [AiProvider::Ollama, AiProvider::LmStudio, AiProvider::Strata];

    /// The cloud providers with an API key of their own (task W.3), in the order of [`AiProvider::ALL`].
    pub const CLOUD: [AiProvider; 10] = [
        AiProvider::OpenRouter,
        AiProvider::Anthropic,
        AiProvider::OpenAi,
        AiProvider::Gemini,
        AiProvider::Mistral,
        AiProvider::DeepSeek,
        AiProvider::Qwen,
        AiProvider::Kimi,
        AiProvider::Glm,
        AiProvider::MiniMax,
    ];

    /// The product's name, not translated: `Ollama`, `LM Studio`, `OpenRouter`; the protocol's name
    /// `OpenAI-compatible` for any other server.
    pub fn name(self) -> &'static str {
        match self {
            AiProvider::Ollama => "Ollama",
            AiProvider::LmStudio => "LM Studio",
            AiProvider::Strata => "Strata",
            AiProvider::OpenRouter => "OpenRouter",
            AiProvider::OpenAiCompatible => "OpenAI-compatible",
            AiProvider::Anthropic => "Anthropic",
            AiProvider::OpenAi => "OpenAI",
            AiProvider::Gemini => "Google Gemini",
            AiProvider::Mistral => "Mistral",
            AiProvider::DeepSeek => "DeepSeek",
            AiProvider::Qwen => "Qwen",
            AiProvider::Kimi => "Kimi",
            AiProvider::Glm => "GLM",
            AiProvider::MiniMax => "MiniMax",
        }
    }

    /// How the provider is written in an account id and in the API: `ollama`, `lm_studio`,
    /// `openrouter`, `openai_compatible` (the serde name).
    pub fn id(self) -> &'static str {
        match self {
            AiProvider::Ollama => "ollama",
            AiProvider::LmStudio => "lm_studio",
            AiProvider::Strata => "strata",
            AiProvider::OpenRouter => "openrouter",
            AiProvider::OpenAiCompatible => "openai_compatible",
            AiProvider::Anthropic => "anthropic",
            AiProvider::OpenAi => "openai",
            AiProvider::Gemini => "gemini",
            AiProvider::Mistral => "mistral",
            AiProvider::DeepSeek => "deepseek",
            AiProvider::Qwen => "qwen",
            AiProvider::Kimi => "kimi",
            AiProvider::Glm => "glm",
            AiProvider::MiniMax => "minimax",
        }
    }

    /// The base address of the provider's API when nobody types one; `None` for
    /// [`AiProvider::OpenAiCompatible`], whose address is always given. The OpenAI-compatible one
    /// for every provider but [`AiProvider::Anthropic`], which speaks its own.
    pub fn default_url(self) -> Option<&'static str> {
        match self {
            AiProvider::Ollama => Some("http://localhost:11434/v1"),
            AiProvider::LmStudio => Some("http://localhost:1234/v1"),
            AiProvider::Strata => Some("http://127.0.0.1:8080/v1"),
            AiProvider::OpenRouter => Some("https://openrouter.ai/api/v1"),
            AiProvider::OpenAiCompatible => None,
            // Anthropic's Messages API (task KI.1b), the one address that is no OpenAI-compatible
            // API; `ai::anthropic` says what travels over it.
            AiProvider::Anthropic => Some("https://api.anthropic.com/v1"),
            // The addresses of the OpenAI-compatible APIs, checked against the providers' own
            // documentation on 2026-10-15 (task W.3). Outside China the international ones.
            AiProvider::OpenAi => Some("https://api.openai.com/v1"),
            AiProvider::Gemini => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            AiProvider::Mistral => Some("https://api.mistral.ai/v1"),
            AiProvider::DeepSeek => Some("https://api.deepseek.com/v1"),
            AiProvider::Qwen => Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
            AiProvider::Kimi => Some("https://api.moonshot.ai/v1"),
            AiProvider::Glm => Some("https://api.z.ai/api/paas/v4"),
            AiProvider::MiniMax => Some("https://api.minimax.io/v1"),
        }
    }

    /// The address of the provider's platform for mainland China, where it has one of its own
    /// (decision 3 in `docs/einrichtung.md`: offered under "Advanced"). Keys of one platform do not
    /// work on the other.
    pub fn china_url(self) -> Option<&'static str> {
        match self {
            AiProvider::Qwen => Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            AiProvider::Kimi => Some("https://api.moonshot.cn/v1"),
            AiProvider::Glm => Some("https://open.bigmodel.cn/api/paas/v4"),
            AiProvider::MiniMax => Some("https://api.minimaxi.com/v1"),
            _ => None,
        }
    }

    /// The provider cannot be used without an API key: every cloud provider. The others take one
    /// when the server asks for it.
    pub fn needs_key(self) -> bool {
        AiProvider::CLOUD.contains(&self)
    }

    /// The provider's page for creating an API key, for the "Create a key" link.
    pub fn key_url(self) -> Option<&'static str> {
        match self {
            AiProvider::OpenRouter => Some("https://openrouter.ai/settings/keys"),
            // Checked on 2026-10-19; the older address under `console.anthropic.com` redirects here.
            AiProvider::Anthropic => Some("https://platform.claude.com/settings/keys"),
            AiProvider::OpenAi => Some("https://platform.openai.com/api-keys"),
            AiProvider::Gemini => Some("https://aistudio.google.com/apikey"),
            AiProvider::Mistral => Some("https://console.mistral.ai/api-keys"),
            AiProvider::DeepSeek => Some("https://platform.deepseek.com/api_keys"),
            AiProvider::Qwen => Some("https://modelstudio.console.alibabacloud.com/"),
            AiProvider::Kimi => Some("https://platform.moonshot.ai/console/api-keys"),
            AiProvider::Glm => Some("https://z.ai/manage-apikey/apikey-list"),
            AiProvider::MiniMax => Some("https://platform.minimax.io/user-center/basic-information/interface-key"),
            AiProvider::Ollama | AiProvider::LmStudio | AiProvider::Strata | AiProvider::OpenAiCompatible => None,
        }
    }

    /// Where the provider's servers are ([`Seat`]).
    pub fn seat(self) -> Seat {
        match self {
            AiProvider::Ollama | AiProvider::LmStudio | AiProvider::Strata => Seat::ThisComputer,
            AiProvider::OpenAiCompatible => Seat::OwnServer,
            AiProvider::OpenRouter | AiProvider::Anthropic | AiProvider::OpenAi | AiProvider::Gemini => Seat::Us,
            AiProvider::Mistral => Seat::Eu,
            AiProvider::DeepSeek | AiProvider::Qwen | AiProvider::Kimi | AiProvider::Glm | AiProvider::MiniMax => {
                Seat::China
            }
        }
    }

    /// The provider's own models have open weights (DeepSeek, Qwen, Kimi, GLM, MiniMax, and Strata,
    /// which serves open models only; Mistral in part), so the same model could also run on a server
    /// of one's own. OpenRouter lists open models of many makers and says so per model
    /// ([`AiModel::open_weights`]).
    pub fn open_weights(self) -> bool {
        matches!(
            self,
            AiProvider::Strata
                | AiProvider::DeepSeek
                | AiProvider::Qwen
                | AiProvider::Kimi
                | AiProvider::Glm
                | AiProvider::MiniMax
        )
    }
}

/// One model an AI account offers, as `ai.list_models` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AiModel {
    /// What a chat names the model by: `qwen3:8b` (Ollama), `qwen/qwen3-14b` (OpenRouter).
    pub id: String,
    /// The name to show; the id when the server gives none.
    pub name: String,
    /// The model can call tools (function calling): `true` or `false` when the server says so
    /// (Ollama's `capabilities`, OpenRouter's `supported_parameters`), `null` when it does not tell
    /// (LM Studio, vLLM). The assistant offers only models that can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// The model's weights are published: `true` or `false` where the server says so (OpenRouter,
    /// from its Hugging Face id), `null` where it does not tell. Models on a local server run on
    /// your own computer either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_weights: Option<bool>,
    /// How many tokens fit into one conversation, when the server tells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// The model thinks before it answers and takes an [`Effort`] (issue #131): `true` or `false`
    /// where the server says so (OpenRouter's `supported_parameters` holds `reasoning`, Ollama's
    /// capabilities hold `thinking`), `null` where it does not tell. Apps offer an effort only for
    /// a model that is not known to lack it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    /// The model runs elsewhere although its server may be local (task S.23): `true` where the
    /// server says it computes somewhere else (Ollama's cloud models, `remote_host` in `/api/tags`,
    /// or a name ending in `:cloud` or `-cloud`; every Ollama model when `/api/tags` does not
    /// answer), `false` for a model of Ollama that runs on its own machine, `null` where nobody
    /// tells. The data rule reads it (`docs/ki.md`, principle 4): local means the model computes on
    /// the person's computer, not that its server does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<bool>,
    /// The model decides instead of writing (task E12.3, `docs/kategorien.md`): it answers named
    /// questions with probabilities at `<base>/systemone` ([`systemone`]). `true` or `false`
    /// where the server says so (Ollama 0.35 and newer list the capability `decision` in
    /// `/api/show`), `null` where it does not tell. Such a model sorts mail directly; one that can
    /// use tools does it through the fallback ([`AiModel::can_decide`], [`decide_chat`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<bool>,
    /// The model is turned on for the whole suite (task S.24, `docs/settings.md` › KI): modules
    /// offer only models that are on. Never from the server: a list read from it has `false`
    /// everywhere, and the core sets it from its switches ([`ModelSwitches`]) before
    /// `ai.list_models` answers. Read as `false` where it is missing.
    #[serde(default)]
    pub enabled: bool,
}

impl AiModel {
    /// A model known by its id only, as when a message names one its account did not list (or the
    /// list failed): the id for its name, nothing known about tools, weights or where it runs, and
    /// not on.
    pub fn named(id: &str) -> AiModel {
        AiModel {
            id: id.to_string(),
            name: id.to_string(),
            tools: None,
            open_weights: None,
            context: None,
            reasoning: None,
            remote: None,
            decision: None,
            enabled: false,
        }
    }

    /// A module of the suite can use the model (#231): it can use tools (`tools` is not `false`,
    /// the assistant and its bots) or it decides ([`AiModel::decision`] is `true`, sorting mail).
    /// Only such a model has a switch and starts on ([`ModelSwitches::starts_on`]); one that can do
    /// neither is of no use to any module.
    pub fn is_usable(&self) -> bool {
        self.tools != Some(false) || self.decision == Some(true)
    }

    /// The model can take the slot Sorting ([`AiSlot::Decide`], a function that needs a decision,
    /// [`AiFunction::needs_decision`]; issue #325, task MW.3): it decides
    /// ([`AiModel::decision`] is `true`), or it can use tools (`tools` is not `false`) and answers the
    /// questions in a fixed format ([`decide_chat`]). What `ai.set_slot` and `ai.set_assignment`
    /// take and what sorting mail picks; a model that can do neither, like an embedding model, is
    /// refused.
    pub fn can_decide(&self) -> bool {
        self.decision == Some(true) || self.tools != Some(false)
    }

    /// The model takes the slot Sorting only through the fallback ([`decide_chat`]): it is no
    /// decision model, but can use tools. The core then asks it with a chat turn instead of
    /// `<base>/systemone`.
    pub fn decides_by_chat(&self) -> bool {
        self.decision != Some(true) && self.tools != Some(false)
    }
}

/// The name of the file the core keeps [`ModelSwitches`] in, next to `accounts.json` in the
/// configuration directory. Not in `settings.json`: the rights of the area `ai` decide who switches
/// models, not those of `settings`, and two switches at once must not overwrite each other through a
/// whole settings object.
pub const MODELS_FILE: &str = "ai-models.json";

/// Which models of the AI accounts are on for the whole suite (task S.24, `docs/settings.md` › KI),
/// as `ai-models.json` holds them: per account (`ai:…`) the models turned on and whether the person
/// ever turned one off there.
///
/// **The rule at the start** ([`ModelSwitches::starts_on`]): on is every model a module can use
/// ([`AiModel::is_usable`]: it can use tools or decides) that computes on this computer; models that compute elsewhere (Ollama's cloud models, OpenRouter,
/// Anthropic, servers on the network) are off until someone turns them on. New models that compute
/// here join while nothing was turned off at their account ([`AccountSwitches::turned_off`]).
/// Whether a model computes here is the caller's to say (the core's data rule,
/// `torroworkplace::assistant::place_of`); nothing here knows addresses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSwitches {
    /// Per account id; an account without an entry follows the rule at the start alone.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub accounts: std::collections::BTreeMap<String, AccountSwitches>,
}

/// The switches of one AI account ([`ModelSwitches`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSwitches {
    /// The models turned on, by id, whatever the rule says.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub on: std::collections::BTreeSet<String>,
    /// The person (or an agent with approval) turned a model of this account off once. From then on
    /// only [`AccountSwitches::on`] counts: the models the rule had on were written into it at that
    /// moment ([`ModelSwitches::turn_off`]), and new ones stay off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub turned_off: bool,
}

impl ModelSwitches {
    /// The rule at the start for `model`: on when a module can use it ([`AiModel::is_usable`]: it
    /// may use tools, or it decides, #231) and it computes on this computer (`here`).
    pub fn starts_on(model: &AiModel, here: bool) -> bool {
        here && model.is_usable()
    }

    /// Whether `model` of the account `account` is on: turned on, or on by the rule while nothing
    /// was turned off there. `here`: the model computes on this computer.
    pub fn is_on(&self, account: &str, model: &AiModel, here: bool) -> bool {
        match self.accounts.get(account) {
            None => ModelSwitches::starts_on(model, here),
            Some(held) => held.on.contains(&model.id) || (!held.turned_off && ModelSwitches::starts_on(model, here)),
        }
    }

    /// Whether models the rule turns on count at `account`: nothing was turned off there yet.
    pub fn follows_rule(&self, account: &str) -> bool {
        self.accounts.get(account).is_none_or(|held| !held.turned_off)
    }

    /// Whether the account has a model turned on by id, whatever the server lists.
    pub fn has_turned_on(&self, account: &str) -> bool {
        self.accounts.get(account).is_some_and(|held| !held.on.is_empty())
    }

    /// Turns `model` of `account` on.
    pub fn turn_on(&mut self, account: &str, model: &str) {
        self.accounts.entry(account.to_string()).or_default().on.insert(model.to_string());
    }

    /// Turns `model` of `account` off. The first time at an account, the models the rule has on now
    /// (`on_by_rule`, ids) are written into [`AccountSwitches::on`] first, so they stay on; from then
    /// on new models stay off. A model that was not on changes nothing but that mark.
    pub fn turn_off(&mut self, account: &str, model: &str, on_by_rule: impl IntoIterator<Item = String>) {
        let held = self.accounts.entry(account.to_string()).or_default();
        if !held.turned_off {
            held.on.extend(on_by_rule);
            held.turned_off = true;
        }
        held.on.remove(model);
    }

    /// Forgets the switches of `account` (it was removed): connected again, it starts with the rule.
    pub fn forget(&mut self, account: &str) {
        self.accounts.remove(account);
    }
}

/// A function of the suite that a model does for the person (`docs/ki.md`, "Zuordnung je
/// Funktion", task E12.5): which account and model it uses is set per function, because a small
/// local model is right for sorting mail and a larger one for a summary.
///
/// One function so far. A new one is a variant here plus its row in the module that uses it;
/// nothing else changes, and a settings file naming a function this version does not know is read
/// all the same – the entry is dropped ([`crate::settings::Settings::ai_assignments`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub enum AiFunction {
    /// Sorting incoming mail into categories (task E12.6, `docs/kategorien.md`). Needs a model that
    /// decides – a decision model, or a chat model through the fallback ([`AiModel::can_decide`]) –,
    /// and – while mail may not leave this computer – one that runs here.
    #[serde(rename = "inbox.categorize")]
    InboxCategorize,
}

impl AiFunction {
    /// Every function, in the order the settings list them.
    pub const ALL: [AiFunction; 1] = [AiFunction::InboxCategorize];

    /// How the function is written on the wire and in the settings file: `inbox.categorize`.
    pub fn key(self) -> &'static str {
        match self {
            AiFunction::InboxCategorize => "inbox.categorize",
        }
    }

    /// The function named by `key`; `None` for one this version does not know.
    pub fn from_key(key: &str) -> Option<AiFunction> {
        AiFunction::ALL.into_iter().find(|function| function.key() == key)
    }

    /// What a person reads for the function, in the core's language.
    pub fn label(self) -> &'static str {
        match self {
            AiFunction::InboxCategorize => tr("Sort mail into categories"),
        }
    }

    /// The function only works with a model that decides: a decision model ([`AiModel::decision`])
    /// directly, or a chat model with tools through the fallback (task MW.3,
    /// [`AiModel::can_decide`], [`decide_chat`]).
    pub fn needs_decision(self) -> bool {
        matches!(self, AiFunction::InboxCategorize)
    }

    /// What the function reads may not leave this computer unless the person allowed it
    /// (`docs/ki.md`, principle 4): mail is such data.
    pub fn reads_mail(self) -> bool {
        matches!(self, AiFunction::InboxCategorize)
    }

    /// The slot whose model the function takes while it has no assignment of its own (issue #325):
    /// sorting mail is a decision ([`AiSlot::Decide`]).
    pub fn slot(self) -> AiSlot {
        match self {
            AiFunction::InboxCategorize => AiSlot::Decide,
        }
    }
}

/// One of the two places a model is chosen for (issue #325, `docs/tasks/ki-modellwahl.md`): instead
/// of a model per function, the person picks one for chatting and one for the suite's small
/// decisions. A function takes the model of its slot ([`AiFunction::slot`]) unless it has an
/// assignment of its own ([`AiAssignment`], the exception). Each slot has its own permission for
/// mail to leave this computer ([`crate::settings::Settings::mail_to_cloud_in`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiSlot {
    /// `"chat"`: the assistant and its bots; a new chat starts with its model. Its mail permission
    /// is `mail_to_cloud`.
    Chat,
    /// `"decide"`: the suite's small decisions, sorting mail into categories first. Its mail
    /// permission is `decide_mail_to_cloud`.
    Decide,
}

impl AiSlot {
    /// Both slots, in the order the control centre shows them.
    pub const ALL: [AiSlot; 2] = [AiSlot::Chat, AiSlot::Decide];

    /// How the slot is written on the wire and in the settings file: `chat`, `decide` (the serde
    /// name).
    pub fn key(self) -> &'static str {
        match self {
            AiSlot::Chat => "chat",
            AiSlot::Decide => "decide",
        }
    }

    /// What a person reads for the slot, in the suite's language: "Chat", "Sorting".
    pub fn label(self) -> &'static str {
        match self {
            AiSlot::Chat => tr("Chat"),
            AiSlot::Decide => tr("Sorting"),
        }
    }
}

/// The model chosen for a slot (issue #325), and what a new chat of the assistant starts with
/// (issue #131, `settings.set_setting assistant_default`, the slot [`AiSlot::Chat`]):
/// `{"engine": "ai:ollama@localhost:11434", "model": "qwen3:8b", "effort": "high"}`. The chat
/// window takes it while the engine is offered (`assistant.list_engines`) and otherwise falls back
/// to the one used last; the person can still pick another per message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SlotModel {
    /// The engine's id: an AI account (`ai:…` from `accounts.list_accounts`) or, for the chat, any
    /// engine from `assistant.list_engines`.
    pub engine: String,
    /// The model of that engine; `None` lets the engine pick one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How hard the model thinks; `None` leaves it to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

impl SlotModel {
    /// The longest engine or model id it takes, as `assistant.send_message` does.
    pub const MAX_ID_BYTES: usize = 300;

    /// The model in `value`, `None` for `null`, or why it does not fit: an object with a non-blank
    /// `engine`, ids of at most [`SlotModel::MAX_ID_BYTES`] on one line, an effort of [`Effort`].
    /// For `settings.set_setting` and the settings file.
    pub fn from_value(value: &serde_json::Value) -> Result<Option<SlotModel>, String> {
        if value.is_null() {
            return Ok(None);
        }
        let chosen: SlotModel = serde_json::from_value(value.clone()).map_err(|err| {
            format!("{value} is not a model to start with: send engine, model and effort, or null ({err})")
        })?;
        let fits = |id: &str| !id.trim().is_empty() && id.len() <= Self::MAX_ID_BYTES && !id.contains(['\n', '\r']);
        if !fits(&chosen.engine) || chosen.model.as_deref().is_some_and(|model| !fits(model)) {
            return Err(format!("an engine or model id is one line of 1 to {} bytes", Self::MAX_ID_BYTES));
        }
        Ok(Some(chosen))
    }

    /// The model as `settings.set_setting` takes it; `null` for none.
    pub fn to_value(chosen: Option<&SlotModel>) -> serde_json::Value {
        chosen.and_then(|chosen| serde_json::to_value(chosen).ok()).unwrap_or(serde_json::Value::Null)
    }
}

/// The model of each slot (issue #325, `settings` › `aiSlots`): `{"chat": {…}, "decide": {…}}`, a
/// slot left out has none. Each slot is read leniently ([`SlotModel::from_value`]): one that does
/// not read is empty and never costs the other or the settings file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AiSlots {
    /// The slot `chat`: what a new chat of the assistant starts with.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_slot")]
    pub chat: Option<SlotModel>,
    /// The slot `decide`: what sorts mail unless `inbox.categorize` has an assignment of its own.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_slot")]
    pub decide: Option<SlotModel>,
}

impl AiSlots {
    /// The model of `slot`; `None` when none is chosen.
    pub fn get(&self, slot: AiSlot) -> Option<&SlotModel> {
        match slot {
            AiSlot::Chat => self.chat.as_ref(),
            AiSlot::Decide => self.decide.as_ref(),
        }
    }

    /// No slot has a model: the settings file leaves `aiSlots` out then.
    pub fn is_empty(&self) -> bool {
        self.chat.is_none() && self.decide.is_none()
    }
}

/// A slot's model as written, or `None` when it does not read ([`SlotModel::from_value`]): a
/// damaged value must not cost the other settings.
pub(crate) fn lenient_slot<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<SlotModel>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(SlotModel::from_value(&value).ok().flatten())
}

/// Which account and model one [`AiFunction`] uses (task E12.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AiAssignment {
    pub function: AiFunction,
    /// The AI account's id (`ai:ollama@localhost:11434`); `None` when none is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// The model of that account; `None` lets the function pick one that fits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How hard it thinks; `None` leaves it to the model. A decision model ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// The function is switched on. `false` leaves everything as it is and asks no model.
    #[serde(default)]
    pub enabled: bool,
}

impl AiAssignment {
    /// Nothing assigned for `function`: what `ai.list_assignments` answers for a function the
    /// person has not set up.
    pub fn none(function: AiFunction) -> AiAssignment {
        AiAssignment { function, engine: None, model: None, effort: None, enabled: false }
    }

    /// The assignment can be used: switched on and with an account.
    pub fn is_ready(&self) -> bool {
        self.enabled && self.engine.is_some()
    }
}

/// How hard a model thinks before it answers (issue #131): sent with a message
/// (`assistant.send_message`), kept as a default (`settings`), and put into a [`ChatRequest`]. Each
/// protocol writes it its own way ([`openai::chat_body_for`]; Claude Code's `--effort`). A model
/// that does not think ignores it or, on some servers, refuses it: apps offer it only where
/// [`AiModel::reasoning`] is not `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    /// Every effort, least first, as menus list them.
    pub const ALL: [Effort; 3] = [Effort::Low, Effort::Medium, Effort::High];

    /// How the effort is written on the wire and on command lines: `low`, `medium`, `high` (the
    /// serde name, and the value OpenAI's `reasoning_effort`, OpenRouter's `reasoning.effort`,
    /// Ollama and Claude Code's `--effort` take).
    pub fn id(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

/// `models` without those known to have closed weights (Open First, decision 4 in
/// `docs/assistent.md`): a model whose [`AiModel::open_weights`] is unknown stays, so the filter
/// changes nothing on a local server. The order stays.
pub fn open_weights_only(models: Vec<AiModel>) -> Vec<AiModel> {
    models.into_iter().filter(|model| model.open_weights != Some(false)).collect()
}

/// Who wrote a message of a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The instructions before the conversation (the skill, task C.3).
    System,
    User,
    Assistant,
    /// The result of a tool call, answering [`ChatMessage::tool_call_id`].
    Tool,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    /// The text; empty for an assistant message that only calls tools.
    pub content: String,
    /// The tools an assistant message called, in its order; empty for the other roles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// The call a [`Role::Tool`] message answers ([`ToolCall::id`]); `None` for the other roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Opaque items that a provider needs back during a tool round: Anthropic's thinking
    /// blocks or OpenAI Responses' output items. Keep them unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thinking: Vec<serde_json::Value>,
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> ChatMessage {
        ChatMessage::plain(Role::System, text)
    }

    pub fn user(text: impl Into<String>) -> ChatMessage {
        ChatMessage::plain(Role::User, text)
    }

    /// What the model answered: its text and the tools it called. The next request of the same
    /// conversation carries it, followed by one [`ChatMessage::tool`] per call.
    pub fn assistant(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage { tool_calls, ..ChatMessage::plain(Role::Assistant, text) }
    }

    /// The same message with the blocks its protocol must get back unchanged
    /// ([`ChatMessage::thinking`]); they come from the answer that wrote it
    /// ([`ChatReply::thinking`]).
    pub fn with_thinking(self, thinking: Vec<serde_json::Value>) -> ChatMessage {
        ChatMessage { thinking, ..self }
    }

    /// The result of the call `call_id`, as text (usually JSON, shortened by the caller).
    pub fn tool(call_id: impl Into<String>, text: impl Into<String>) -> ChatMessage {
        ChatMessage { tool_call_id: Some(call_id.into()), ..ChatMessage::plain(Role::Tool, text) }
    }

    fn plain(role: Role, text: impl Into<String>) -> ChatMessage {
        ChatMessage { role, content: text.into(), tool_calls: Vec::new(), tool_call_id: None, thinking: Vec::new() }
    }
}

/// A tool the model may call: an API command (task C.2 builds them from the catalogue).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// `calendar_list_events`: letters, digits and `_`, at most 64 characters.
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// The JSON Schema of its arguments, an object schema.
    pub parameters: serde_json::Value,
}

/// A tool the model called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The server's id of the call; the answer names it ([`ChatMessage::tool`]). Made up
    /// (`call_<n>`) when a server sends none.
    pub id: String,
    /// The tool's [`ToolSpec::name`].
    pub name: String,
    /// The arguments as the model wrote them: JSON text, which small models do not always get
    /// right. The caller parses it and tells the model when it is not valid.
    pub arguments: String,
}

/// One turn of a conversation for [`Provider::chat`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChatRequest {
    /// The model's [`AiModel::id`].
    pub model: String,
    /// The conversation so far, oldest first; usually starts with a [`Role::System`] message.
    pub messages: Vec<ChatMessage>,
    /// The tools the model may call; empty for none (then none are sent at all, as some servers
    /// refuse an empty list).
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// The most tokens the answer may have; `None` leaves it to the server.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// How hard the model thinks (issue #131); `None` leaves it to the model. Written per
    /// provider by [`openai::chat_body_for`] or [`responses::body`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    /// It was done.
    #[default]
    Stop,
    /// It called tools ([`ChatReply::tool_calls`]) and waits for their results.
    ToolCalls,
    /// The answer reached `max_tokens` or the context was full.
    Length,
    /// Anything else the server named (a content filter, an error).
    Other,
}

/// Tokens one turn used, as the server counted them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// The model's answer to one [`ChatRequest`], streamed or not.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ChatReply {
    /// The text, whole.
    pub text: String,
    /// The tools it called, in order; empty for none. Look at these, not only at
    /// [`ChatReply::finish`]: some servers say `stop` after calling tools.
    pub tool_calls: Vec<ToolCall>,
    pub finish: Finish,
    /// `None` when the server does not count.
    pub usage: Option<Usage>,
    /// Opaque protocol items to replay during a tool round, in their original order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thinking: Vec<serde_json::Value>,
}

/// Why asking a model or its server failed. The messages are for people, in the core's language
/// ([`fmt::Display`]); none holds an API key or a server's own words that could repeat one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    /// The server could not be reached: not running, no network, no answer in time.
    Offline,
    /// The server refused the API key, or wants one (HTTP 401, 403).
    Auth,
    /// No such account, model or address: a sentence for people that names it.
    NotFound(String),
    /// The account needs an API key and none is stored on this device.
    NoKey,
    /// Too many requests, or the credit is used up (HTTP 429, 402); the text says which.
    Busy(String),
    /// The input was refused before anything was sent: an address that is none, a key that would
    /// travel unencrypted. The text says what to change.
    Invalid(String),
    /// The server answered with an error; its status and a short message without secrets.
    Server { status: u16, message: String },
    /// The server's answer is not what the protocol says.
    Format(String),
}

impl AiError {
    /// The error code of the answer (`torro_ipc::Response::Error`): `offline`, `auth`,
    /// `not-found`, `not-connected`, `busy` or `error`.
    pub fn code(&self) -> &'static str {
        match self {
            AiError::Offline => "offline",
            AiError::Auth => "auth",
            AiError::NotFound(_) => "not-found",
            AiError::NoKey => "not-connected",
            AiError::Busy(_) => "busy",
            AiError::Invalid(_) | AiError::Server { .. } | AiError::Format(_) => "error",
        }
    }
}

impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AiError::Offline => {
                f.write_str(tr("The AI server could not be reached. Check that it is running and try again."))
            }
            AiError::Auth => f.write_str(tr("The AI server did not accept the API key. Check the key and try again.")),
            AiError::NoKey => f.write_str(tr("No API key is stored on this device. Connect the account again.")),
            AiError::NotFound(message)
            | AiError::Busy(message)
            | AiError::Invalid(message)
            | AiError::Format(message) => f.write_str(message),
            AiError::Server { status, message } if message.is_empty() => {
                f.write_str(&trf("The AI server answered with HTTP {status}.", &[("status", status)]))
            }
            AiError::Server { status, message } => f.write_str(&trf(
                "The AI server answered with HTTP {status}: {message}",
                &[("status", status), ("message", message)],
            )),
        }
    }
}

impl std::error::Error for AiError {}

/// A server's protocol: what the suite asks of every AI account. Implemented by the
/// OpenAI-compatible protocol (`torroworkplace::account::ai::OpenAiCompatible`) and by Anthropic's
/// (`torroworkplace::account::ai::Anthropic`). Object safe, so the core can hold
/// `Box<dyn Provider>` per account.
///
/// Implementations must
/// - send the account's API key only in the key header of its protocol (`Authorization` for the
///   OpenAI-compatible one, `x-api-key` for Anthropic's) and only to the account's own server, and
///   never put it into an error, a log line or `Debug`;
/// - map failures to [`AiError`] as its variants say (unreachable → `Offline`, 401/403 → `Auth`,
///   429/402 → `Busy`, anything unreadable → `Format`);
/// - stop sending as soon as the returned future is dropped (that is how a conversation is
///   cancelled).
pub trait Provider: Send + Sync {
    /// The models the server offers, in the server's order, each with what it knows about tools
    /// and open weights. Does not filter: callers apply [`open_weights_only`].
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>>;

    /// One turn of `request`, streamed: every piece of the answer's text goes to `on_text` as it
    /// arrives (in order, nothing twice; their sum is [`ChatReply::text`]); tool calls arrive whole
    /// in the reply. A server that does not stream still calls `on_text` once with the whole text.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>>;

    /// One call to a decision model (task E12.3, [`systemone`]): named questions in, an answer
    /// with probabilities out. No text, nothing streamed.
    ///
    /// The default refuses, so only a server that speaks the protocol need override it – today
    /// Ollama from 0.35 on. Anthropic and every other provider keep the default, and asking the
    /// wrong account is an orderly [`AiError::Invalid`] instead of a request that goes nowhere.
    fn decide<'a>(
        &'a self,
        request: &'a systemone::DecisionRequest,
    ) -> BoxFuture<'a, Result<systemone::DecisionReply, AiError>> {
        let _ = request;
        Box::pin(async { Err(AiError::Invalid(tr("This AI server does not offer decision models.").into())) })
    }
}

/// The base address of an OpenAI-compatible API from what a person typed: `localhost:11434` →
/// `http://localhost:11434/v1`, `https://ai.example.org/` → `https://ai.example.org/v1`,
/// `https://openrouter.ai/api/v1/` → `https://openrouter.ai/api/v1`.
///
/// Without a scheme, `http://` for this computer ([`is_this_computer`]) and `https://` for any
/// other host; an address without a path gets `/v1`, the path every such server uses; a path that
/// is given stays, without the trailing `/`. Query and fragment are refused. Errors are
/// [`AiError::Invalid`] with a sentence for people.
pub fn normalize_base(input: &str) -> Result<String, AiError> {
    let input = input.trim();
    let invalid = || AiError::Invalid(tr("Enter the address of the server, like http://localhost:11434.").into());
    let (scheme, rest) = match input.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => (String::new(), input),
    };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(authority, path)| (authority, path));
    if authority.is_empty() || authority.contains(['@', ' ']) || path.contains(['?', '#', ' ']) {
        return Err(invalid());
    }
    let scheme = match scheme.as_str() {
        "" if is_this_computer_host(host_of(authority)) => "http",
        "" => "https",
        "http" | "https" => scheme.as_str(),
        _ => return Err(invalid()),
    };
    let path = path.trim_end_matches('/');
    let path = if path.is_empty() { "v1" } else { path };
    Ok(format!("{scheme}://{}/{path}", authority.to_ascii_lowercase()))
}

/// Whether the server at the base address `url` runs on this computer: `localhost` (or a name
/// under it), `127.0.0.0/8` or `::1`. Only there may an API key travel without TLS.
pub fn is_this_computer(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    is_this_computer_host(host_of(authority))
}

/// `[::1]:8080` → `::1`, `localhost:11434` → `localhost`.
fn host_of(authority: &str) -> &str {
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => authority.rsplit_once(':').map_or(authority, |(host, _)| host),
    }
}

fn is_this_computer_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// The base address `url` may carry an API key: over https, or to this computer.
pub fn key_may_travel(url: &str) -> bool {
    url.starts_with("https://") || is_this_computer(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, open_weights: Option<bool>) -> AiModel {
        AiModel {
            id: id.into(),
            name: id.into(),
            tools: None,
            open_weights,
            context: None,
            reasoning: None,
            remote: None,
            decision: None,
            enabled: false,
        }
    }

    #[test]
    fn every_provider_is_written_by_its_id_and_every_cloud_one_has_an_address_a_key_page_and_a_seat() {
        for provider in AiProvider::ALL {
            let json = serde_json::to_value(provider).expect("json");
            assert_eq!(json, serde_json::json!(provider.id()), "{provider:?}");
            assert_eq!(serde_json::from_value::<AiProvider>(json).expect("back"), provider);
        }
        for provider in AiProvider::CLOUD {
            assert!(provider.needs_key() && provider.key_url().is_some(), "{provider:?}");
            let url = provider.default_url().expect("an address");
            assert!(url.starts_with("https://") && !url.ends_with('/'), "{provider:?}: {url}");
            assert!(!matches!(provider.seat(), Seat::ThisComputer | Seat::OwnServer), "{provider:?}");
        }
        for local in [AiProvider::Ollama, AiProvider::LmStudio, AiProvider::Strata, AiProvider::OpenAiCompatible] {
            assert!(!local.needs_key() && local.china_url().is_none(), "{local:?}");
        }
        assert_eq!(AiProvider::Mistral.seat(), Seat::Eu);
        assert_eq!(AiProvider::Kimi.china_url(), Some("https://api.moonshot.cn/v1"));
        assert_eq!(serde_json::to_value(AiProvider::MiniMax).expect("json"), "minimax");
    }

    #[test]
    fn the_filter_drops_only_models_known_to_be_closed() {
        let models =
            vec![model("qwen/qwen3-14b", Some(true)), model("openai/gpt-4o", Some(false)), model("qwen3:8b", None)];
        let kept: Vec<String> = open_weights_only(models).into_iter().map(|m| m.id).collect();
        assert_eq!(kept, ["qwen/qwen3-14b", "qwen3:8b"]);
    }

    #[test]
    fn addresses_get_a_scheme_and_the_usual_path() {
        let base = |input: &str| normalize_base(input).expect(input);
        assert_eq!(base("localhost:11434"), "http://localhost:11434/v1");
        assert_eq!(base(" 127.0.0.1:1234/ "), "http://127.0.0.1:1234/v1");
        assert_eq!(base("[::1]:8000"), "http://[::1]:8000/v1");
        assert_eq!(base("ai.example.org"), "https://ai.example.org/v1");
        assert_eq!(base("HTTP://GPU.local:8000/"), "http://gpu.local:8000/v1");
        assert_eq!(base("https://openrouter.ai/api/v1/"), "https://openrouter.ai/api/v1");
        for wrong in ["", "   ", "ftp://x.org", "https://", "https://a b.org", "https://x.org/v1?key=1", "anna@x.org"] {
            assert!(matches!(normalize_base(wrong), Err(AiError::Invalid(_))), "{wrong:?}");
        }
    }

    #[test]
    fn keys_travel_over_tls_or_to_this_computer_only() {
        assert!(key_may_travel("https://openrouter.ai/api/v1"));
        assert!(key_may_travel("http://localhost:11434/v1"));
        assert!(key_may_travel("http://127.0.0.1:1234/v1"));
        assert!(key_may_travel("http://[::1]:8000/v1"));
        assert!(!key_may_travel("http://gpu.local:8000/v1"));
        assert!(!key_may_travel("http://192.168.1.20:8000/v1"));
        assert!(!key_may_travel("http://localhost.example.org/v1"));
    }

    #[test]
    fn efforts_are_written_as_their_ids() {
        for effort in Effort::ALL {
            assert_eq!(serde_json::to_value(effort).expect("json"), effort.id());
        }
        assert!(Effort::Low < Effort::High);
    }

    #[test]
    fn providers_have_their_names_addresses_and_ids() {
        for provider in AiProvider::ALL {
            let json = serde_json::to_value(provider).expect("json");
            assert_eq!(json, provider.id(), "the id is the serde name");
            assert_eq!(provider.default_url().is_none(), provider == AiProvider::OpenAiCompatible);
        }
        assert!(AiProvider::OpenRouter.needs_key() && !AiProvider::Ollama.needs_key());
        assert_eq!(AiProvider::LmStudio.name(), "LM Studio");
    }

    /// Issue #322: Strata is a local provider like Ollama and LM Studio – no key, this computer,
    /// open weights, and found at its usual address.
    #[test]
    fn strata_is_a_local_provider_with_open_weights() {
        let strata = AiProvider::Strata;
        assert_eq!((strata.name(), strata.id()), ("Strata", "strata"));
        assert_eq!(serde_json::to_value(strata).expect("json"), "strata");
        assert_eq!(strata.default_url(), Some("http://127.0.0.1:8080/v1"));
        assert_eq!(strata.seat(), Seat::ThisComputer);
        assert!(!strata.needs_key() && strata.key_url().is_none() && strata.open_weights());
        assert!(AiProvider::ALL.contains(&strata) && !AiProvider::CLOUD.contains(&strata));
        for local in AiProvider::LOCAL {
            assert!(AiProvider::ALL.contains(&local) && local.seat() == Seat::ThisComputer, "{local:?}");
        }
        assert_eq!(AiProvider::LOCAL, [AiProvider::Ollama, AiProvider::LmStudio, AiProvider::Strata]);
    }

    /// Task S.24: at the start the models with tools (or deciding ones, #231) that compute here are on; a model turned on
    /// stays on, and the first one turned off keeps the others the rule had on, while new ones
    /// stay off from then on.
    #[test]
    fn models_start_on_here_and_turning_one_off_keeps_the_others() {
        let with_tools = |id: &str, tools: Option<bool>| AiModel { tools, ..AiModel::named(id) };
        let (local, unsure, toolless) =
            (with_tools("qwen3:8b", Some(true)), with_tools("lm", None), with_tools("gemma3:4b", Some(false)));
        let cloud = with_tools("glm-5.3:cloud", Some(true));
        let mut switches = ModelSwitches::default();
        let account = "ai:ollama@localhost:11434";
        assert!(switches.is_on(account, &local, true) && switches.is_on(account, &unsure, true));
        assert!(!switches.is_on(account, &toolless, true), "no tools, no decisions: no module can use it");
        // #231: a decision model has no tools, but sorting mail uses it.
        let nimble = AiModel { decision: Some(true), ..with_tools("nimble", Some(false)) };
        assert!(nimble.is_usable() && !toolless.is_usable() && unsure.is_usable());
        assert!(switches.is_on(account, &nimble, true), "a decision model here starts on");
        assert!(!switches.is_on(account, &nimble, false), "elsewhere it is off like any other");
        assert!(!switches.is_on(account, &cloud, false), "elsewhere: off until someone turns it on");
        assert!(switches.follows_rule(account) && !switches.has_turned_on(account));

        switches.turn_on(account, &cloud.id);
        assert!(switches.is_on(account, &cloud, false) && switches.has_turned_on(account));
        switches.turn_off(account, &local.id, ["qwen3:8b".to_string(), "lm".to_string()]);
        assert!(!switches.is_on(account, &local, true), "off");
        assert!(switches.is_on(account, &unsure, true), "the rule's other model stays on");
        assert!(!switches.is_on(account, &with_tools("new:1b", Some(true)), true), "new ones stay off now");
        assert!(!switches.follows_rule(account));

        let json = serde_json::to_string(&switches).expect("json");
        assert_eq!(serde_json::from_str::<ModelSwitches>(&json).expect("back"), switches);
        assert_eq!(serde_json::from_str::<ModelSwitches>("{}").expect("empty"), ModelSwitches::default());
        switches.forget(account);
        assert!(switches.is_on(account, &local, true), "a removed account starts with the rule again");
    }

    /// Task MW.3: the slot Sorting takes a decision model directly and a model with tools through
    /// the fallback; one that can do neither is no model for it.
    #[test]
    fn sorting_takes_a_decision_model_or_one_with_tools() {
        let with = |decision: Option<bool>, tools: Option<bool>| AiModel { decision, tools, ..AiModel::named("m") };
        let nimble = with(Some(true), Some(false));
        assert!(nimble.can_decide() && !nimble.decides_by_chat(), "directly");
        let qwen = with(Some(false), Some(true));
        assert!(qwen.can_decide() && qwen.decides_by_chat(), "through the fallback");
        let unsure = with(None, None);
        assert!(unsure.can_decide() && unsure.decides_by_chat(), "a server that does not tell, like LM Studio");
        let embedding = with(Some(false), Some(false));
        assert!(!embedding.can_decide() && !embedding.decides_by_chat());
    }

    /// Issue #325: each slot is written by its key, sorting mail takes the slot `decide`, and the
    /// slots of a file read each on its own.
    #[test]
    fn slots_are_written_by_their_key_and_functions_name_theirs() {
        for slot in AiSlot::ALL {
            assert_eq!(serde_json::to_value(slot).expect("json"), serde_json::json!(slot.key()));
        }
        assert_eq!((AiSlot::Chat.label(), AiSlot::Decide.label()), (tr("Chat"), tr("Sorting")));
        assert_eq!(AiFunction::InboxCategorize.slot(), AiSlot::Decide);
        let slots: AiSlots =
            serde_json::from_value(serde_json::json!({"chat": {"engine": ""}, "decide": {"engine": "ai:x"}}))
                .expect("lenient");
        assert_eq!(
            (slots.get(AiSlot::Chat), slots.get(AiSlot::Decide).map(|chosen| chosen.engine.as_str())),
            (None, Some("ai:x"))
        );
        assert!(!slots.is_empty() && AiSlots::default().is_empty());
        assert_eq!(serde_json::to_value(AiSlots::default()).expect("json"), serde_json::json!({}));
    }

    #[test]
    fn a_model_from_the_server_is_never_on_by_itself() {
        let read: AiModel =
            serde_json::from_value(serde_json::json!({"id": "qwen3:8b", "name": "Qwen3"})).expect("model");
        assert!(!read.enabled);
        let shown = serde_json::to_value(AiModel { enabled: true, ..AiModel::named("qwen3:8b") }).expect("json");
        assert_eq!(shown["enabled"], serde_json::json!(true));
    }

    #[test]
    fn errors_have_codes_and_say_nothing_secret() {
        assert_eq!(AiError::NoKey.code(), "not-connected");
        assert_eq!(AiError::Busy("x".into()).code(), "busy");
        assert_eq!(AiError::Auth.code(), "auth");
        let server = AiError::Server { status: 500, message: String::new() };
        assert_eq!(server.to_string(), trf("The AI server answered with HTTP {status}.", &[("status", &500)]));
    }
}
