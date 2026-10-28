//! The models Torro suggests for this computer (task MW.5, issue #325): the built-in list
//! `assets/ai/recommendations.toml` – tiers of computers, local models per tier and slot, cloud models per
//! provider and slot – read and checked once ([`List`]), and the pure rules that turn a measured computer
//! ([`Hardware`], task MW.4) and the connected AI accounts into a suggestion per slot ([`suggest`]).
//!
//! The list is data so it can change with each version without touching the rules; nothing is loaded from the
//! network. Its checks ([`List::parse`]) keep a mistake in the file from suggesting a model without open
//! weights, a slot without a model for sorting, or an Ollama name that names another registry host – which
//! matters because `ai.pull_model` downloads only what this list names ([`List::ollama_model`], task MW.7).
//!
//! The rules, in this order per slot: a local model of the tier on a local server (Strata only while it serves
//! the model, else Ollama, with the size of a download while Ollama does not list it); else a cloud model,
//! providers in the order of [`AiProvider::ALL`]; else the local model without an account and the reason "Needs
//! Ollama". Sorting is suggested only while mail is connected. Pure: no network, no settings, nothing stored.

use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;
use torro_core::ai::hardware::Hardware;
use torro_core::ai::{AiModel, AiProvider, AiSlot};
use torro_core::i18n::{tr, trf};
use torro_ipc::{AiRecommendation, SlotSuggestion};

/// The list as built in.
const LIST: &str = include_str!("../../assets/ai/recommendations.toml");

/// The licences a local model may have: open ones only (`docs/ki.md`, principle 2, Open First).
pub const LICENSES: &[&str] = &["apache-2.0", "mit", "qwen", "llama", "gemma"];

/// One GiB: the list's sizes are GB as sold, which are GiB.
const GIB: u128 = 1 << 30;

/// How much less than a minimum still counts, in percent. Systems report a little less than they have: a 12 GB
/// card says 12282 MiB through `nvidia-smi`, a 16 GB computer keeps some memory for itself. 10 % never lets the
/// next smaller size as sold pass (11 GB of 12 does, 10 does not; 14.4 of 16, so 12 does not).
const SLACK_PERCENT: u128 = 10;

/// The checked list of recommendations ([`List::builtin`], or [`List::parse`] for another text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    /// The tiers with conditions, strongest first; [`List::fallback`] follows them.
    tiers: Vec<Tier>,
    /// The last tier, without conditions: everything the others do not take, unknown computers included.
    fallback: Tier,
    /// The local models, as written.
    local: Vec<Local>,
    /// The cloud models, as written.
    cloud: Vec<Cloud>,
}

/// A tier of computers (`[[tier]]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier {
    /// Its id (`large`), what `ai.recommend` answers as `tier`.
    pub id: String,
    /// What a person reads for it, in English; translated with `tr()` when shown ([`suggest`]).
    pub label: &'static str,
    /// It holds when one of these holds; empty only for the last tier, which always holds.
    pub when: Vec<Condition>,
}

/// One condition of a tier: every field given must hold ([`Condition::holds`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// The main memory, in GB as sold.
    #[serde(default)]
    pub min_memory_gb: Option<u64>,
    /// The video memory of the largest card, in GB as sold.
    #[serde(default)]
    pub min_vram_gb: Option<u64>,
    /// `true` asks for memory shared by processor and graphics (Apple silicon), `false` for its absence.
    #[serde(default)]
    pub unified: Option<bool>,
}

/// A model that runs on this computer (`[[local]]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Local {
    /// The tier it is for, by id.
    pub tier: String,
    pub slot: AiSlot,
    /// [`AiProvider::Ollama`] or [`AiProvider::Strata`].
    pub provider: AiProvider,
    /// Ollama: the name to pull (`qwen3:14b`, `nimble` for `nimble:latest`). Strata: the beginning of the id it
    /// serves the model under, upper and lower case alike.
    pub model: String,
    /// What a download loads, in bytes; more than 0.
    pub size_bytes: u64,
    /// One of [`LICENSES`].
    pub license: String,
    /// Always `true`.
    pub open_weights: bool,
    /// A decision model (`docs/kategorien.md`): it sorts mail directly, not through the fallback.
    #[serde(default)]
    pub decision: bool,
}

/// A cloud model (`[[cloud]]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cloud {
    /// One of [`AiProvider::CLOUD`].
    pub provider: AiProvider,
    pub slot: AiSlot,
    /// The beginning of the model's id; the first model of the account that starts with it and fits the slot.
    pub model_prefix: String,
}

/// A connected AI account as the core knows it, the input of [`suggest`] (the core builds it, task MW.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    /// The account's id (`ai:ollama@localhost:11434`).
    pub id: String,
    pub provider: AiProvider,
    /// Its server runs on this computer (the core's `is_this_computer` on its address). Only such an Ollama or
    /// Strata runs a local model of the list: the list fits this computer, not another one.
    pub local: bool,
    /// The models it lists (`AiAccounts::models(id, true)`); `None` when the list could not be read.
    pub models: Option<Vec<AiModel>>,
}

/// Why a list was not read. `Display` says it in English, for the log and the `message` of `error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListError {
    /// No valid TOML, or fields that do not fit: the parser's message.
    Toml(String),
    /// The entry `entry` (a tier's id, or `tier/slot/model`) breaks a rule; `reason` says which.
    Invalid { entry: String, reason: &'static str },
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::Toml(message) => write!(f, "the list of recommended models is not valid: {message}"),
            ListError::Invalid { entry, reason } => write!(f, "recommended model {entry:?}: {reason}"),
        }
    }
}

impl std::error::Error for ListError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawList {
    #[serde(default)]
    tier: Vec<RawTier>,
    #[serde(default)]
    local: Vec<Local>,
    #[serde(default)]
    cloud: Vec<Cloud>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTier {
    id: String,
    label: String,
    #[serde(default)]
    when: Vec<Condition>,
}

impl List {
    /// The built-in list, read and checked on first use. `Err` only when the built-in file is broken, which the
    /// tests rule out.
    pub fn builtin() -> Result<&'static List, ListError> {
        static BUILTIN: OnceLock<Result<List, ListError>> = OnceLock::new();
        BUILTIN.get_or_init(|| List::parse(LIST)).as_ref().map_err(Clone::clone)
    }

    /// A list in the format of `recommendations.toml`, checked. Fails on the first rule it breaks; unknown
    /// fields count as broken. The rules: at least one tier, ids unique, only the last tier without `when`
    /// and no condition empty; each local entry for a known tier, by Ollama or Strata, with a size, open
    /// weights and a licence of [`LICENSES`], at most one per tier, slot and provider, an Ollama name without
    /// `/`, a decision model only for `decide`; each tier with a local model for `decide`; each cloud entry by
    /// a cloud provider with a prefix, at most one per provider and slot.
    ///
    /// Each label is kept for the rest of the run (`tr()` takes only text that lives that long); the core
    /// reads the built-in list once.
    pub fn parse(text: &str) -> Result<List, ListError> {
        let raw: RawList = toml::from_str(text).map_err(|err| ListError::Toml(err.message().to_string()))?;
        let mut tiers = raw.tier.into_iter().map(tier_of_raw).collect::<Vec<_>>();
        let fallback = tiers.pop().ok_or_else(|| invalid("-", "the list has no tier"))?;
        if !fallback.when.is_empty() {
            return Err(invalid(&fallback.id, "the last tier must have no conditions"));
        }
        for tier in &tiers {
            if tier.when.is_empty() {
                return Err(invalid(&tier.id, "only the last tier may have no conditions"));
            }
            if tier.when.iter().any(|condition| *condition == Condition::default()) {
                return Err(invalid(&tier.id, "a condition without a field always holds"));
            }
        }
        let ids: Vec<&str> = tiers.iter().chain([&fallback]).map(|tier| tier.id.as_str()).collect();
        for (at, id) in ids.iter().enumerate() {
            if ids[..at].contains(id) {
                return Err(invalid(id, "the tier id is used twice"));
            }
        }
        check_local(&raw.local, &ids)?;
        check_cloud(&raw.cloud)?;
        Ok(List { tiers, fallback, local: raw.local, cloud: raw.cloud })
    }

    /// Every tier, strongest first, the last one (without conditions) at the end. Only the tests read the
    /// whole list; the core asks [`List::ollama_model`] and [`suggest`].
    #[cfg(test)]
    pub fn tiers(&self) -> impl Iterator<Item = &Tier> {
        self.tiers.iter().chain([&self.fallback])
    }

    /// The local models, as written.
    #[cfg(test)]
    pub fn local(&self) -> &[Local] {
        &self.local
    }

    /// The cloud models, as written.
    #[cfg(test)]
    pub fn cloud(&self) -> &[Cloud] {
        &self.cloud
    }

    /// The local Ollama entry for the model `name`, for `ai.pull_model` (task MW.7: only these may be
    /// downloaded) and the size its approval names. `name` as a person, Ollama or the list writes it: a name
    /// without a tag is the tag `latest` (`nimble` and `nimble:latest` are one model). `None` for any name the
    /// list does not hold as an Ollama model.
    pub fn ollama_model(&self, name: &str) -> Option<&Local> {
        self.local.iter().find(|entry| entry.provider == AiProvider::Ollama && same_ollama_model(&entry.model, name))
    }

    /// The entry of `tier` for `slot` at `provider`.
    fn local_for(&self, tier: &str, slot: AiSlot, provider: AiProvider) -> Option<&Local> {
        self.local.iter().find(|entry| entry.tier == tier && entry.slot == slot && entry.provider == provider)
    }
}

/// A tier as read, its label kept for the rest of the run so `tr()` can take it.
fn tier_of_raw(raw: RawTier) -> Tier {
    Tier { id: raw.id, label: raw.label.leak(), when: raw.when }
}

fn invalid(entry: &str, reason: &'static str) -> ListError {
    ListError::Invalid { entry: entry.to_string(), reason }
}

fn check_local(local: &[Local], tiers: &[&str]) -> Result<(), ListError> {
    for (at, entry) in local.iter().enumerate() {
        let name = format!("{}/{}/{}", entry.tier, entry.slot.key(), entry.model);
        if !tiers.contains(&entry.tier.as_str()) {
            return Err(invalid(&name, "the tier is not in the list"));
        }
        if !matches!(entry.provider, AiProvider::Ollama | AiProvider::Strata) {
            return Err(invalid(&name, "a local model runs on Ollama or Strata"));
        }
        if entry.model.is_empty() || entry.model.trim() != entry.model {
            return Err(invalid(&name, "the model has no name"));
        }
        // A `/` names a namespace or another registry host, which `ai.pull_model` would then download from.
        if entry.provider == AiProvider::Ollama && entry.model.contains('/') {
            return Err(invalid(&name, "an Ollama model is a name of Ollama's library, without `/`"));
        }
        if entry.size_bytes == 0 {
            return Err(invalid(&name, "the size is missing"));
        }
        if !entry.open_weights {
            return Err(invalid(&name, "a local model must have open weights"));
        }
        if !LICENSES.contains(&entry.license.as_str()) {
            return Err(invalid(&name, "the licence is not an open one of the list"));
        }
        if entry.decision && entry.slot != AiSlot::Decide {
            return Err(invalid(&name, "a decision model cannot chat"));
        }
        let twice = local[..at]
            .iter()
            .any(|other| other.tier == entry.tier && other.slot == entry.slot && other.provider == entry.provider);
        if twice {
            return Err(invalid(&name, "the tier has two models for the slot at this provider"));
        }
    }
    for tier in tiers {
        if !local.iter().any(|entry| entry.tier == *tier && entry.slot == AiSlot::Decide) {
            return Err(invalid(tier, "the tier has no local model for sorting"));
        }
    }
    Ok(())
}

fn check_cloud(cloud: &[Cloud]) -> Result<(), ListError> {
    for (at, entry) in cloud.iter().enumerate() {
        let name = format!("{}/{}", entry.provider.id(), entry.slot.key());
        if !AiProvider::CLOUD.contains(&entry.provider) {
            return Err(invalid(&name, "a cloud model is at a cloud provider"));
        }
        if entry.model_prefix.trim().is_empty() {
            return Err(invalid(&name, "the model prefix is empty"));
        }
        if cloud[..at].iter().any(|other| other.provider == entry.provider && other.slot == entry.slot) {
            return Err(invalid(&name, "the provider has two models for the slot"));
        }
    }
    Ok(())
}

impl Condition {
    /// Every field given holds for `hardware`. A minimum holds with up to [`SLACK_PERCENT`] less; a minimum of
    /// something not measured never holds, so an unknown computer falls to the last tier. Video memory is that
    /// of the largest card.
    pub fn holds(&self, hardware: &Hardware) -> bool {
        let vram = hardware.gpu.as_ref().and_then(|gpu| gpu.vram_bytes);
        at_least(hardware.memory_bytes, self.min_memory_gb)
            && at_least(vram, self.min_vram_gb)
            && self.unified.is_none_or(|unified| unified == hardware.unified_memory)
    }
}

/// `bytes` reach `min_gb` GiB, less [`SLACK_PERCENT`]; no minimum always holds, no measurement never does.
fn at_least(bytes: Option<u64>, min_gb: Option<u64>) -> bool {
    match (bytes, min_gb) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(bytes), Some(min_gb)) => u128::from(bytes) * 100 >= u128::from(min_gb) * GIB * (100 - SLACK_PERCENT),
    }
}

impl Tier {
    /// The tier takes `hardware`: one of its conditions holds, or it has none (the last tier).
    pub fn holds(&self, hardware: &Hardware) -> bool {
        self.when.is_empty() || self.when.iter().any(|condition| condition.holds(hardware))
    }
}

/// The tier of `hardware`: the first of `list` that takes it, the last one when no other does (unknown
/// computers too).
pub fn tier_of<'a>(hardware: &Hardware, list: &'a List) -> &'a Tier {
    list.tiers.iter().find(|tier| tier.holds(hardware)).unwrap_or(&list.fallback)
}

/// What Torro suggests for `hardware` with the connected `accounts` ([`Connected`], in the core's order), the
/// answer of `ai.recommend` (task MW.6). `mail`: mail the caller may read is connected; without it there is
/// no suggestion for [`AiSlot::Decide`].
///
/// Per slot, in the order of [`AiSlot::ALL`], the first that applies:
/// 1. The tier's Strata model, while a Strata on this computer lists a model whose id starts with it (upper
///    and lower case alike): that id, no download.
/// 2. The tier's Ollama model at the first Ollama on this computer: the id it lists the model by, or the
///    list's name with `download_bytes` while it does not list it (or its list could not be read).
/// 3. A cloud model: providers in the order of [`AiProvider::ALL`], the first account with a `[[cloud]]` entry
///    for the slot whose list holds a model with that beginning that fits the slot (chat: `tools` not
///    `false`; sorting: [`AiModel::can_decide`]) – of several, the one named exactly so, else the shortest
///    id (`gemini-2.5-flash` goes before `gemini-2.5-flash-lite` while the list holds the exact id; one
///    that lists only variants gets the shortest of them, which may be `-lite`); an account whose list could not be read
///    with `model: None`. The reason says whether the tier has no local model for the slot, no AI server
///    runs here, or the servers here offer none of the tier's models (a Strata without it, no Ollama).
/// 4. The tier's Ollama model without an account (`engine: None`), with its size and the reason "Needs
///    Ollama".
///
/// Otherwise the slot has no suggestion (the smallest tier has no local chat model). `by_chat` is set for
/// sorting by a model that is no decision model. Words are in the core's language.
pub fn suggest(hardware: &Hardware, list: &List, accounts: &[Connected], mail: bool) -> AiRecommendation {
    let tier = tier_of(hardware, list);
    let slots = AiSlot::ALL
        .into_iter()
        .filter(|slot| mail || *slot != AiSlot::Decide)
        .filter_map(|slot| suggest_slot(list, &tier.id, slot, accounts))
        .collect();
    AiRecommendation {
        hardware: hardware.clone(),
        tier: tier.id.clone(),
        tier_label: tr(tier.label).to_string(),
        slots,
    }
}

/// The suggestion for one slot ([`suggest`], steps 1–4).
fn suggest_slot(list: &List, tier: &str, slot: AiSlot, accounts: &[Connected]) -> Option<SlotSuggestion> {
    let on_this_computer = |provider: AiProvider| accounts.iter().filter(move |a| a.local && a.provider == provider);
    if let Some(entry) = list.local_for(tier, slot, AiProvider::Strata) {
        let served = on_this_computer(AiProvider::Strata).find_map(|account| {
            let model =
                account.models.as_ref()?.iter().find(|model| starts_with_ignoring_case(&model.id, &entry.model))?;
            Some((account, model))
        });
        if let Some((account, model)) = served {
            return Some(local_suggestion(entry, Some(&account.id), &model.id, None));
        }
    }
    let ollama = list.local_for(tier, slot, AiProvider::Ollama);
    if let Some(entry) = ollama
        && let Some(account) = on_this_computer(AiProvider::Ollama).next()
    {
        let listed = account.models.iter().flatten().find(|model| same_ollama_model(&entry.model, &model.id));
        return Some(match listed {
            Some(model) => local_suggestion(entry, Some(&account.id), &model.id, None),
            None => local_suggestion(entry, Some(&account.id), &entry.model, Some(entry.size_bytes)),
        });
    }
    let has_local = list.local_for(tier, slot, AiProvider::Strata).is_some() || ollama.is_some();
    let reason = match (has_local, accounts.iter().any(|account| account.local)) {
        (false, _) => tr("No local model fits this computer"),
        (true, false) => tr("No AI server runs on this computer"),
        (true, true) => tr("No server here offers a model for this computer"),
    };
    if let Some(cloud) = cloud_suggestion(list, slot, accounts, reason) {
        return Some(cloud);
    }
    ollama.map(|entry| SlotSuggestion {
        reason: trf("Needs {program}", &[("program", &AiProvider::Ollama.name())]),
        ..local_suggestion(entry, None, &entry.model, Some(entry.size_bytes))
    })
}

/// A suggestion of the local `entry` as `model` at the account `engine`. Its reason says only what
/// the seat ("This computer") beside it in the row does not: "Ready" or "Downloads first".
fn local_suggestion(entry: &Local, engine: Option<&str>, model: &str, download_bytes: Option<u64>) -> SlotSuggestion {
    let reason = match download_bytes {
        Some(_) => tr("Downloads first"),
        None => tr("Ready"),
    };
    SlotSuggestion {
        slot: entry.slot,
        engine: engine.map(str::to_string),
        provider: entry.provider,
        model: Some(model.to_string()),
        local: true,
        seat: entry.provider.seat().label().to_string(),
        download_bytes,
        by_chat: entry.slot == AiSlot::Decide && !entry.decision,
        reason: reason.to_string(),
    }
}

/// The cloud model for `slot` ([`suggest`], step 3), with `reason`: why no local model is suggested.
fn cloud_suggestion(list: &List, slot: AiSlot, accounts: &[Connected], reason: &'static str) -> Option<SlotSuggestion> {
    let (account, model) = AiProvider::ALL.into_iter().find_map(|provider| {
        let entry = list.cloud.iter().find(|entry| entry.provider == provider && entry.slot == slot)?;
        accounts.iter().filter(|account| account.provider == provider && !account.local).find_map(|account| {
            match &account.models {
                None => Some((account, None)),
                Some(models) => best_of(models, &entry.model_prefix, slot).map(|model| (account, Some(model))),
            }
        })
    })?;
    Some(SlotSuggestion {
        slot,
        engine: Some(account.id.clone()),
        provider: account.provider,
        model: model.map(|model| model.id.clone()),
        local: false,
        seat: account.provider.seat().label().to_string(),
        download_bytes: None,
        // Cloud models are no decision models; one that says it is sorts directly.
        by_chat: slot == AiSlot::Decide && model.is_none_or(AiModel::decides_by_chat),
        reason: reason.to_string(),
    })
}

/// The model of `models` for a `[[cloud]]` entry with `prefix` that fits `slot`: the one whose id is `prefix`,
/// else the shortest id that starts with it – the plain model before its variants (`-lite`, `:nitro`) while
/// the plain one is listed; without it the shortest variant, which may be `-lite`. Of equally long ones the
/// first listed.
fn best_of<'a>(models: &'a [AiModel], prefix: &str, slot: AiSlot) -> Option<&'a AiModel> {
    models
        .iter()
        .filter(|model| model.id.starts_with(prefix) && fits(model, slot))
        .min_by_key(|model| (model.id != prefix, model.id.len()))
}

/// `model` can take `slot`: chat needs tools (not known to lack them), sorting [`AiModel::can_decide`].
fn fits(model: &AiModel, slot: AiSlot) -> bool {
    match slot {
        AiSlot::Chat => model.tools != Some(false),
        AiSlot::Decide => model.can_decide(),
    }
}

/// Two Ollama names are one model: equal once a name without a tag has the tag `latest`.
fn same_ollama_model(a: &str, b: &str) -> bool {
    with_tag(a) == with_tag(b)
}

/// `name` with its tag: `nimble` → `nimble:latest`. Only the part after the last `/` holds a tag, since a
/// registry host may have a port (`host:5000/x`).
fn with_tag(name: &str) -> std::borrow::Cow<'_, str> {
    let last = name.rsplit('/').next().unwrap_or(name);
    if last.contains(':') { name.into() } else { format!("{name}:latest").into() }
}

fn starts_with_ignoring_case(id: &str, prefix: &str) -> bool {
    id.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use torro_core::ai::hardware::Gpu;

    use super::*;

    const MIB: u64 = 1 << 20;
    const GIB_U64: u64 = 1 << 30;

    fn builtin() -> &'static List {
        List::builtin().expect("the built-in list is valid")
    }

    fn pc(memory_bytes: u64, vram_bytes: Option<u64>) -> Hardware {
        Hardware {
            cpu: Some("AMD Ryzen 9 7950X".into()),
            memory_bytes: Some(memory_bytes),
            gpu: vram_bytes.map(|vram| Gpu { name: Some("NVIDIA GeForce RTX 4070".into()), vram_bytes: Some(vram) }),
            unified_memory: false,
        }
    }

    fn mac(memory_bytes: u64) -> Hardware {
        Hardware {
            cpu: Some("Apple M3 Max".into()),
            memory_bytes: Some(memory_bytes),
            gpu: Some(Gpu { name: Some("Apple M3 Max".into()), vram_bytes: None }),
            unified_memory: true,
        }
    }

    fn model(id: &str, tools: Option<bool>, decision: Option<bool>) -> AiModel {
        AiModel { tools, decision, ..AiModel::named(id) }
    }

    fn account(id: &str, provider: AiProvider, local: bool, models: Option<Vec<AiModel>>) -> Connected {
        Connected { id: id.into(), provider, local, models }
    }

    const OLLAMA: &str = "ai:ollama@localhost:11434";
    const STRATA: &str = "ai:strata@127.0.0.1:8080";

    fn slot(recommendation: &AiRecommendation, slot: AiSlot) -> Option<&SlotSuggestion> {
        recommendation.slots.iter().find(|suggestion| suggestion.slot == slot)
    }

    #[test]
    fn the_built_in_list_reads_and_keeps_its_rules() {
        let list = builtin();
        let tiers: Vec<&str> = list.tiers().map(|tier| tier.id.as_str()).collect();
        assert_eq!(tiers, ["mac_large", "gpu", "large", "medium", "small"]);
        assert!(list.tiers().all(|tier| !tier.label.is_empty()));
        for entry in list.local() {
            assert!(tiers.contains(&entry.tier.as_str()), "{entry:?}: known tier");
            assert!(entry.size_bytes > 0, "{entry:?}: a size");
            assert!(entry.open_weights, "{entry:?}: open weights");
            assert!(LICENSES.contains(&entry.license.as_str()), "{entry:?}: an open licence");
            assert!(matches!(entry.provider, AiProvider::Ollama | AiProvider::Strata));
        }
        for tier in &tiers {
            let decide = list.local().iter().any(|entry| entry.tier == *tier && entry.slot == AiSlot::Decide);
            assert!(decide, "{tier} has a model for sorting");
        }
        assert!(list.cloud().iter().all(|entry| AiProvider::CLOUD.contains(&entry.provider)));
        let decides: Vec<&str> =
            list.local().iter().filter(|entry| entry.decision).map(|entry| entry.model.as_str()).collect();
        assert!(decides.iter().all(|name| name.starts_with("nimble")), "nimble is the decision model: {decides:?}");
        let small_chat = list.local().iter().any(|entry| entry.tier == "small" && entry.slot == AiSlot::Chat);
        assert!(!small_chat, "the smallest tier has no local chat model");
    }

    #[test]
    fn a_broken_list_is_refused_with_its_reason() {
        let tier = "[[tier]]\nid = \"small\"\nlabel = \"Small\"\n";
        let nimble = "[[local]]\ntier = \"small\"\nslot = \"decide\"\nprovider = \"ollama\"\nmodel = \"nimble\"\n\
                      size_bytes = 1\nlicense = \"apache-2.0\"\nopen_weights = true\ndecision = true\n";
        assert!(List::parse(&format!("{tier}{nimble}")).is_ok());
        let reason = |text: String| match List::parse(&text) {
            Err(ListError::Invalid { reason, .. }) => reason,
            other => panic!("refused: {other:?}"),
        };
        assert_eq!(reason(tier.into()), "the tier has no local model for sorting");
        assert_eq!(reason(nimble.into()), "the list has no tier");
        assert_eq!(
            reason(format!("{tier}{}", nimble.replace("\"nimble\"", "\"host.example/x/y\""))),
            "an Ollama model is a name of Ollama's library, without `/`"
        );
        assert_eq!(
            reason(format!("{tier}{}", nimble.replace("apache-2.0", "proprietary"))),
            "the licence is not an open one of the list"
        );
        assert_eq!(
            reason(format!("{tier}{}", nimble.replace("open_weights = true", "open_weights = false"))),
            "a local model must have open weights"
        );
        assert_eq!(
            reason(format!("{tier}{}", nimble.replace("size_bytes = 1", "size_bytes = 0"))),
            "the size is missing"
        );
        assert_eq!(
            reason(format!("{tier}{}", nimble.replace("\"small\"\nslot", "\"huge\"\nslot"))),
            "the tier is not in the list"
        );
        let first = "[[tier]]\nid = \"big\"\nlabel = \"Big\"\n";
        assert_eq!(reason(format!("{first}{tier}{nimble}")), "only the last tier may have no conditions");
        let empty = "[[tier]]\nid = \"big\"\nlabel = \"Big\"\nwhen = [{}]\n";
        assert_eq!(reason(format!("{empty}{tier}{nimble}")), "a condition without a field always holds");
        assert!(matches!(List::parse(&format!("{tier}{nimble}size = 3\n")), Err(ListError::Toml(_))), "unknown field");
    }

    #[test]
    fn unknown_hardware_is_the_smallest_tier() {
        assert_eq!(tier_of(&Hardware::default(), builtin()).id, "small");
        let gpu_only =
            Hardware { gpu: Some(Gpu { name: None, vram_bytes: Some(24 * GIB_U64) }), ..Hardware::default() };
        assert_eq!(tier_of(&gpu_only, builtin()).id, "small", "memory unknown");
    }

    #[test]
    fn tiers_allow_a_little_less_memory_than_sold() {
        let list = builtin();
        // nvidia-smi: a 12 GB RTX 4070 reports 12282 MiB, about 11.99 GiB.
        assert_eq!(tier_of(&pc(64 * GIB_U64, Some(12282 * MIB)), list).id, "gpu");
        // A computer sold with 16 GB that keeps some for itself: 15.5 GiB.
        assert_eq!(tier_of(&pc(15 * GIB_U64 + 512 * MIB, Some(8188 * MIB)), list).id, "large");
        assert_eq!(tier_of(&pc(64 * GIB_U64, None), list).id, "medium", "a PC needs a graphics card for large");
        assert_eq!(tier_of(&pc(32 * GIB_U64, Some(6 * GIB_U64)), list).id, "medium", "6 GB is not 8");
        assert_eq!(tier_of(&pc(7 * GIB_U64 + 700 * MIB, None), list).id, "medium", "8 GB sold");
        assert_eq!(tier_of(&pc(12 * GIB_U64, None), list).id, "medium", "12 GB is not 16");
        assert_eq!(tier_of(&pc(32 * GIB_U64, Some(10 * GIB_U64)), list).id, "large", "10 GB is not 12");
        assert_eq!(tier_of(&pc(4 * GIB_U64, None), list).id, "small");
        assert_eq!(tier_of(&mac(64 * GIB_U64), list).id, "mac_large");
        assert_eq!(tier_of(&mac(36 * GIB_U64), list).id, "large");
        assert_eq!(tier_of(&mac(16 * GIB_U64), list).id, "medium", "a Mac needs 32 for large");
    }

    #[test]
    fn eight_gb_without_ollama_but_with_a_cloud_account_goes_to_the_cloud() {
        let anthropic = account(
            "ai:anthropic",
            AiProvider::Anthropic,
            false,
            Some(vec![
                model("claude-opus-5", Some(true), None),
                model("claude-sonnet-4-5", Some(true), None),
                model("claude-haiku-4-5", Some(true), None),
            ]),
        );
        let got = suggest(&pc(8 * GIB_U64, None), builtin(), &[anthropic], true);
        assert_eq!((got.tier.as_str(), got.tier_label.as_str()), ("medium", tr("Medium")));
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!((chat.engine.as_deref(), chat.model.as_deref()), (Some("ai:anthropic"), Some("claude-sonnet-4-5")));
        assert!(!chat.local && !chat.by_chat && chat.download_bytes.is_none());
        assert_eq!(
            (chat.seat.as_str(), chat.reason.as_str()),
            (tr("Cloud · US"), tr("No AI server runs on this computer"))
        );
        let decide = slot(&got, AiSlot::Decide).expect("sorting");
        assert_eq!(decide.model.as_deref(), Some("claude-haiku-4-5"));
        assert!(decide.by_chat, "a cloud model sorts through the fallback");
    }

    #[test]
    fn without_any_server_the_local_model_needs_ollama() {
        let got = suggest(&pc(8 * GIB_U64, None), builtin(), &[], true);
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!((chat.engine.as_deref(), chat.model.as_deref()), (None, Some("qwen3:4b")));
        assert_eq!(chat.reason, trf("Needs {program}", &[("program", &"Ollama")]));
        assert_eq!((chat.provider, chat.local, chat.seat.as_str()), (AiProvider::Ollama, true, tr("This computer")));
        assert_eq!(chat.download_bytes, Some(2_497_293_931));
        let decide = slot(&got, AiSlot::Decide).expect("sorting");
        assert_eq!(decide.model.as_deref(), Some("nimble:9b-q4_K_M"));
        assert!(!decide.by_chat, "nimble decides");
    }

    #[test]
    fn rtx_with_ollama_suggests_what_is_there_and_downloads_the_rest() {
        let ollama = account(OLLAMA, AiProvider::Ollama, true, Some(vec![model("qwen3:14b", Some(true), Some(false))]));
        let got = suggest(&pc(64 * GIB_U64, Some(12282 * MIB)), builtin(), std::slice::from_ref(&ollama), true);
        assert_eq!((got.tier.as_str(), got.tier_label.as_str()), ("gpu", tr("Graphics card")));
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!((chat.engine.as_deref(), chat.model.as_deref()), (Some(OLLAMA), Some("qwen3:14b")));
        assert_eq!((chat.download_bytes, chat.reason.as_str()), (None, tr("Ready")));
        let decide = slot(&got, AiSlot::Decide).expect("sorting");
        assert_eq!((decide.engine.as_deref(), decide.model.as_deref()), (Some(OLLAMA), Some("nimble")));
        assert_eq!((decide.download_bytes, decide.by_chat), (Some(9_527_504_198), false));
        assert_eq!(decide.reason, tr("Downloads first"));

        // Ollama lists a model without a tag as `:latest`: it is there, by the id Ollama lists.
        let pulled =
            account(OLLAMA, AiProvider::Ollama, true, Some(vec![model("nimble:latest", Some(false), Some(true))]));
        let got = suggest(&pc(64 * GIB_U64, Some(12282 * MIB)), builtin(), &[pulled], true);
        let decide = slot(&got, AiSlot::Decide).expect("sorting");
        assert_eq!((decide.model.as_deref(), decide.download_bytes), (Some("nimble:latest"), None));

        // Strata serving the tier's model wins for chat; Strata serving another falls back to Ollama.
        let strata = |id: &str| account(STRATA, AiProvider::Strata, true, Some(vec![model(id, None, None)]));
        let got = suggest(
            &pc(64 * GIB_U64, Some(12282 * MIB)),
            builtin(),
            &[ollama.clone(), strata("Qwen3.6-35B-A3B-UD-Q4_K_XL")],
            true,
        );
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!((chat.engine.as_deref(), chat.provider), (Some(STRATA), AiProvider::Strata));
        assert_eq!((chat.model.as_deref(), chat.download_bytes), (Some("Qwen3.6-35B-A3B-UD-Q4_K_XL"), None));
        let got = suggest(&pc(64 * GIB_U64, Some(12282 * MIB)), builtin(), &[ollama, strata("deepseek-v4.1")], true);
        assert_eq!(slot(&got, AiSlot::Chat).expect("chat").engine.as_deref(), Some(OLLAMA));
    }

    #[test]
    fn mac_with_64_gb_takes_the_large_models() {
        let ollama = account(OLLAMA, AiProvider::Ollama, true, Some(Vec::new()));
        let got = suggest(&mac(64 * GIB_U64), builtin(), &[ollama], true);
        assert_eq!((got.tier.as_str(), got.tier_label.as_str()), ("mac_large", tr("Mac with large memory")));
        assert_eq!(got.hardware, mac(64 * GIB_U64));
        let models: Vec<(AiSlot, Option<&str>, Option<u64>)> =
            got.slots.iter().map(|s| (s.slot, s.model.as_deref(), s.download_bytes)).collect();
        assert_eq!(
            models,
            [
                (AiSlot::Chat, Some("qwen3:32b"), Some(20_201_253_829)),
                (AiSlot::Decide, Some("nimble"), Some(9_527_504_198))
            ]
        );
    }

    #[test]
    fn without_mail_there_is_no_suggestion_for_sorting() {
        let got = suggest(&mac(64 * GIB_U64), builtin(), &[], false);
        assert_eq!(got.slots.iter().map(|s| s.slot).collect::<Vec<_>>(), [AiSlot::Chat]);
    }

    #[test]
    fn a_small_computer_chats_in_the_cloud_or_not_at_all() {
        let ollama = account(OLLAMA, AiProvider::Ollama, true, None);
        let got = suggest(&pc(4 * GIB_U64, None), builtin(), std::slice::from_ref(&ollama), true);
        assert!(slot(&got, AiSlot::Chat).is_none(), "no local chat model, no cloud account");
        let decide = slot(&got, AiSlot::Decide).expect("sorting");
        assert_eq!(decide.download_bytes, Some(5_629_111_625), "the list could not be read: download");

        let mistral = account("ai:mistral", AiProvider::Mistral, false, None);
        let got = suggest(&pc(4 * GIB_U64, None), builtin(), &[ollama, mistral], true);
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!(
            (chat.engine.as_deref(), chat.model.as_deref()),
            (Some("ai:mistral"), None),
            "unread list: it picks"
        );
        assert_eq!(chat.reason, tr("No local model fits this computer"));
        assert_eq!(slot(&got, AiSlot::Decide).expect("sorting").engine.as_deref(), Some(OLLAMA), "local first");
    }

    #[test]
    fn cloud_providers_come_in_torros_order_and_models_must_fit() {
        let anthropic = account(
            "ai:anthropic",
            AiProvider::Anthropic,
            false,
            Some(vec![model("claude-sonnet-4-5", Some(true), None)]),
        );
        let openrouter = account(
            "ai:openrouter",
            AiProvider::OpenRouter,
            false,
            Some(vec![model("qwen/qwen3-32b", Some(false), None), model("qwen/qwen3-32b:nitro", Some(true), None)]),
        );
        let got = suggest(&Hardware::default(), builtin(), &[anthropic.clone(), openrouter], false);
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!(chat.engine.as_deref(), Some("ai:openrouter"), "OpenRouter comes before Anthropic");
        assert_eq!(chat.model.as_deref(), Some("qwen/qwen3-32b:nitro"), "the first one that can use tools");

        let none_fits =
            account("ai:openrouter", AiProvider::OpenRouter, false, Some(vec![model("meta/llama", None, None)]));
        let got = suggest(&Hardware::default(), builtin(), &[none_fits, anthropic], false);
        assert_eq!(slot(&got, AiSlot::Chat).expect("chat").engine.as_deref(), Some("ai:anthropic"));
    }

    /// Review of #325 (F7): a Strata here without the tier's model and no Ollama sends the slot to the cloud
    /// with the reason that no server here offers a model, not that none runs.
    #[test]
    fn a_strata_without_the_model_says_no_server_here_offers_one() {
        let strata = account(STRATA, AiProvider::Strata, true, Some(vec![model("other-model", Some(true), None)]));
        let anthropic = account(
            "ai:anthropic",
            AiProvider::Anthropic,
            false,
            Some(vec![model("claude-sonnet-4-5", Some(true), None)]),
        );
        let got = suggest(&pc(64 * GIB_U64, Some(12282 * MIB)), builtin(), &[strata, anthropic], false);
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!(chat.engine.as_deref(), Some("ai:anthropic"));
        assert_eq!(chat.reason, tr("No server here offers a model for this computer"));
    }

    /// Review of #325 (F7): of several models with the entry's beginning, the one named exactly so, else
    /// the shortest – `gemini-2.5-flash` goes before `gemini-2.5-flash-lite`, whatever the order, while the
    /// list holds the exact id.
    #[test]
    fn the_cloud_model_named_exactly_goes_before_its_variants() {
        let gemini = |ids: &[&str]| {
            let models = ids.iter().map(|id| model(id, Some(true), None)).collect();
            account("ai:gemini", AiProvider::Gemini, false, Some(models))
        };
        let chat_of = |ids: &[&str]| {
            let got = suggest(&pc(64 * GIB_U64, None), builtin(), &[gemini(ids)], false);
            slot(&got, AiSlot::Chat).and_then(|chat| chat.model.clone())
        };
        let chat_prefix = builtin()
            .cloud
            .iter()
            .find(|entry| entry.provider == AiProvider::Gemini && entry.slot == AiSlot::Chat)
            .map(|entry| entry.model_prefix.clone())
            .expect("a Gemini entry for chat");
        assert_eq!(chat_prefix, "gemini-2.5-flash");
        assert_eq!(chat_of(&["gemini-2.5-flash-lite", "gemini-2.5-flash"]).as_deref(), Some("gemini-2.5-flash"));
        let variants = ["gemini-2.5-flash-lite-preview", "gemini-2.5-flash-001"];
        assert_eq!(chat_of(&variants).as_deref(), Some("gemini-2.5-flash-001"), "the shortest");
        assert_eq!(
            best_of(
                &[model("x-long", None, None), model("x-b", None, None), model("x-a", None, None)],
                "x",
                AiSlot::Chat
            )
            .map(|model| model.id.as_str()),
            Some("x-b"),
            "equally long: the first listed"
        );
    }

    #[test]
    fn an_ollama_on_another_computer_runs_no_local_model() {
        let elsewhere =
            account("ai:ollama@nas:11434", AiProvider::Ollama, false, Some(vec![model("qwen3:4b", None, None)]));
        let got = suggest(&pc(8 * GIB_U64, None), builtin(), &[elsewhere], false);
        let chat = slot(&got, AiSlot::Chat).expect("chat");
        assert_eq!(
            (chat.engine.as_deref(), chat.reason.clone()),
            (None, trf("Needs {program}", &[("program", &"Ollama")]))
        );
    }

    #[test]
    fn only_ollama_models_of_the_list_can_be_looked_up() {
        let list = builtin();
        assert_eq!(list.ollama_model("nimble:latest").map(|entry| entry.size_bytes), Some(9_527_504_198));
        assert_eq!(list.ollama_model("nimble").map(|entry| entry.model.as_str()), Some("nimble"));
        assert_eq!(list.ollama_model("qwen3:14b").map(|entry| entry.size_bytes), Some(9_276_198_565));
        assert!(list.ollama_model("qwen3").is_none(), "qwen3:latest is not in the list");
        assert!(list.ollama_model("host.example/x/y").is_none());
        assert!(list.ollama_model("qwen3.6-35b-a3b").is_none(), "a Strata entry is never downloaded");
        assert_eq!(with_tag("host:5000/x"), "host:5000/x:latest", "a port is no tag");
    }
}
