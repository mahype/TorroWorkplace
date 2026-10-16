//! Changing mail (task M3.2): `mail.set_flags`, `mail.move_messages`, `mail.delete_messages` and
//! `mail.save_draft`, on the account's command connection ([`MailService::command`]) and in its
//! store.
//!
//! **Ids.** Message and folder ids (`m:…`, `f:…`) name no account. [`locate`] looks every id of a
//! call up in the stores of the accounts the caller reaches ([`MailService::accounts_within`]); one
//! that no reachable store knows makes the whole call `not-found`. The core checks this for a
//! client before anyone is asked to approve ([`client_refusal`]) and again when an approved call
//! runs; the handlers look the ids up once more, so a message the sync dropped meanwhile is
//! `not-found` too. M3.1 (reading) and M3.3 (sending) resolve their ids with the same [`locate`].
//!
//! **Into the trash is deleting.** A move to a folder whose role is trash is the same as a move to
//! the role (`MoveTarget::Role(Trash)`, delete access): [`with_trash_role`] rewrites the request
//! before the rights are checked, so approvals and rights see what really runs.
//!
//! **Cache.** Flags change in the store at once and are put back when the server refuses
//! ("optimistic"). Moves, deletions and drafts change the store as soon as the server confirmed:
//! only then is the new UID known. A moved message keeps its id ([`MailStore::relocate_message`])
//! when its new UID can be found by its `Message-ID`; otherwise it leaves the store and the next
//! sync brings it back under a new id.
//!
//! **Drafts** (security rules 1 and 2 of task M2.1): the core remembers who wrote each draft
//! ([`DraftAuthor`]); a client key replaces only its own (others are `not-found`), and attaches
//! files only from the managed attachment folder ([`AttachmentCache::contains`], else
//! `forbidden`). No file is read through a symbolic link, none over 25 MiB. A draft is built
//! with `mime::build` (the core's `Message-ID`, no signature: that is added when it is sent) and
//! its Bcc recipients are kept in a `Bcc:` header of the stored draft, as mail programs do, since
//! the built message carries them in the envelope only. Replacing appends the new draft first and
//! removes the old one after, so a failure never loses the draft; the id stays. The HTML of a
//! formatted draft (issue #125) is cleaned before anything else (`torro_mail::html::sanitize`), and
//! its plain text is made from that HTML, so both forms of the stored draft say the same.
//!
//! **Connections.** A command connection is released only when every call on it worked, and never
//! held while another account's connection is taken: attachments of other messages and the headers
//! for a reply are fetched before the draft's account is asked.
//!
//! **Answering an invitation** (task E8.3b, [`send_invitation_reply`]): the iTIP `REPLY` to the
//! organizer is built here from the invitation as read from the server ([`reply_ics`]) and sent
//! like a draft, but without one ([`super::send::send_message`]); the calendar side is the core's.
//!
//! Nothing here logs a subject, an address, a file name or a folder name.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use chrono::{DateTime, Utc};
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::mail::{Delivery, MAX_FILE_BYTES, MoveTarget};
use torro_mail::{Draft, DraftAttachment, Flag, InvitationAnswer, MailAddress, Role, html};
use torro_mail_imap::imap::{FetchedHeader, ImapError, Search, Session};
use torro_mail_imap::mime::{self, CalendarPart, Outgoing, OutgoingAttachment, ReadMessage, Reply};

use super::read::Invitation;
use super::service::{MailChange, MailError, MailService, SharedStore};
use super::store::{Body, DraftAuthor, FolderRef, MailStore, MessageRef, NewMessage};
use super::{AttachmentCache, MailStoreError, random_hex, safe_file_name};
use crate::approvals::{DraftFacts, MailFacts};
use crate::pim::Reach;
use torro_ipc::api::{Access, Area};

const NO_MESSAGE: &str = "No such message.";
const NO_ACCOUNT: &str = "No such mail account.";
const TRASH_IS_DELETING: &str = "Moving into the trash is deleting, which this client may not do.";
const NO_FOLDER: &str = "No such folder.";
const NO_DRAFT: &str = "No such draft.";
const NO_ROLE_FOLDER: &str = "The account has no folder with this role.";
const NO_ATTACHMENT: &str = "The message has no such attachment.";
const NO_FILE: &str = "The file to attach does not exist.";
const ONE_ACCOUNT: &str = "Messages can only be moved within one account.";
const FILE_OUTSIDE: &str = "Only files from the attachment folder of Torro Workplace can be attached.";
const NO_LINKS: &str = "A file behind a symbolic link cannot be attached.";
const CHANGED: &str = "The folder changed on the server. Try again after the next sync.";
const BOTH_WAYS: &str = "A flag cannot be added and removed at once.";
const NOT_IN_TRASH: &str = "Only messages in the trash can be deleted for good.";
const TOO_BIG: &str = "An attachment may be at most 25 MiB.";
const NOT_ABSOLUTE: &str = "Give the full path of the file to attach.";
const NOT_A_FILE: &str = "Only files can be attached.";
const UNBUILDABLE: &str = "The draft could not be built: an address is not valid.";
const NOT_ANSWERABLE: &str = "This mail holds no invitation that can be answered.";
const NOT_LOCATED: &str = "The draft was saved, but the server did not say where. It shows up after the next sync.";

/// Why a change of mail was refused or failed. [`WriteError::response`] is the answer: fixed
/// codes, texts for people without subjects, addresses, paths or server words.
#[derive(Debug)]
pub(crate) enum WriteError {
    /// `not-found`: an id no reachable store knows, a draft of someone else, a missing folder.
    NotFound(&'static str),
    /// `unsupported`: messages of several accounts, or a folder of another account.
    Unsupported(&'static str),
    /// `forbidden`: a file a client key may not attach (security rule 1).
    Forbidden(&'static str),
    /// `conflict`: the folder was renumbered on the server since the store last saw it.
    Conflict,
    /// `error`: the request cannot work as given.
    Invalid(&'static str),
    /// The account, its connection or its store failed ([`MailError::code`]).
    Mail(MailError),
}

impl WriteError {
    /// The answer for a request that failed with this error.
    pub(crate) fn response(&self) -> Response {
        let refused =
            |code: &str, message: &'static str| Response::Error { code: code.into(), message: tr(message).into() };
        match self {
            WriteError::NotFound(message) => refused("not-found", message),
            WriteError::Unsupported(message) => refused("unsupported", message),
            WriteError::Forbidden(message) => refused("forbidden", message),
            WriteError::Conflict => refused("conflict", CHANGED),
            WriteError::Invalid(message) => refused("error", message),
            WriteError::Mail(err) => err.response(),
        }
    }
}

impl From<MailError> for WriteError {
    fn from(err: MailError) -> WriteError {
        WriteError::Mail(err)
    }
}

impl From<ImapError> for WriteError {
    fn from(err: ImapError) -> WriteError {
        WriteError::Mail(err.into())
    }
}

impl From<MailStoreError> for WriteError {
    fn from(err: MailStoreError) -> WriteError {
        WriteError::Mail(err.into())
    }
}

/// The requests of this module: the ones the core routes here and checks with
/// [`client_refusal`].
pub(crate) fn changes_mail(request: &Request) -> bool {
    matches!(
        request,
        Request::SetMailFlags { .. }
            | Request::MoveMail { .. }
            | Request::DeleteMail { .. }
            | Request::SaveDraft { .. }
    )
}

// ----- looking ids up -----

/// Where each of `ids` is, in their order: looked up in the stores of the accounts `reach` sees,
/// one store after the other (a primary-key lookup each). [`WriteError::NotFound`] when any id is
/// in no reachable store: then the whole call is `not-found`, whatever the others are. The shared
/// "id → account" lookup of the mail commands (M3.1 to M3.3).
pub(crate) async fn locate(mail: &MailService, reach: Reach<'_>, ids: &[&str]) -> Result<Vec<MessageRef>, WriteError> {
    let found = locate_each(mail, reach, ids).await?;
    found.into_iter().collect::<Option<Vec<_>>>().ok_or(WriteError::NotFound(NO_MESSAGE))
}

/// [`locate`], but `None` for an id no reachable store knows instead of failing.
async fn locate_each(
    mail: &MailService,
    reach: Reach<'_>,
    ids: &[&str],
) -> Result<Vec<Option<MessageRef>>, WriteError> {
    let mut found: Vec<Option<MessageRef>> = vec![None; ids.len()];
    for account in mail.accounts_within(reach).await {
        let wanted: Vec<(usize, String)> = ids
            .iter()
            .enumerate()
            .filter(|(at, _)| found[*at].is_none())
            .map(|(at, id)| (at, id.to_string()))
            .collect();
        if wanted.is_empty() {
            break;
        }
        // An account whose store did not open holds nothing to find.
        let Ok(store) = mail.store(&account).await else { continue };
        let refs = store
            .run(move |store| {
                let mut refs = Vec::new();
                for (at, id) in wanted {
                    if let Some(message) = store.resolve_message(&id)? {
                        refs.push((at, message));
                    }
                }
                Ok(refs)
            })
            .await?;
        for (at, message) in refs {
            found[at] = Some(message);
        }
    }
    Ok(found)
}

/// A folder as a command works with it: where it is on the server, its role and its name.
#[derive(Debug, Clone)]
struct Place {
    folder: FolderRef,
    role: Option<Role>,
    name: String,
}

/// The folder `id` in the stores of the accounts `reach` sees; `None` when none has it.
async fn locate_folder(mail: &MailService, reach: Reach<'_>, id: &str) -> Result<Option<Place>, WriteError> {
    for account in mail.accounts_within(reach).await {
        let Ok(store) = mail.store(&account).await else { continue };
        let id = id.to_string();
        if let Some(place) = store.run(move |store| place_of(store, &id)).await? {
            return Ok(Some(place));
        }
    }
    Ok(None)
}

/// The folder with `role` in `store`, if the account has one.
async fn folder_with_role(store: &SharedStore, role: Role) -> Result<Option<Place>, WriteError> {
    Ok(store
        .run(move |store| match store.folder_by_role(role)? {
            Some(folder) => place_of(store, &folder.id),
            None => Ok(None),
        })
        .await?)
}

fn place_of(store: &MailStore, id: &str) -> Result<Option<Place>, MailStoreError> {
    let (Some(folder), Some(shown)) = (store.resolve_folder(id)?, store.folder(id)?) else { return Ok(None) };
    Ok(Some(Place { folder, role: shown.role, name: shown.name }))
}

// ----- checks before anything runs -----

/// What the core checks for a changing mail request of the client `client` before anyone is
/// asked to approve it, and again when an approved one runs (`Core::handle_client`,
/// `Core::run_approved`); `None` when it may go on. Other requests are not looked at.
///
/// - every message id is in an account `reach` sees, and so is the folder of a move: else
///   `not-found` for the whole call, before anything is read;
/// - `mail.save_draft` with an id replaces only a draft of this client, in the drafts folder of
///   the draft's account: else `not-found` (security rule 2); files only from the managed
///   attachment folder: else `forbidden` (security rule 1).
pub(crate) async fn client_refusal(
    mail: &MailService,
    client: &str,
    reach: Reach<'_>,
    request: &Request,
) -> Option<Response> {
    if !changes_mail(request) {
        return None;
    }
    if let Err(err) = locate(mail, reach, &request.message_ids()).await {
        return Some(err.response());
    }
    let checked = match request {
        Request::MoveMail { to: MoveTarget::Folder(folder), .. } => match locate_folder(mail, reach, folder).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(WriteError::NotFound(NO_FOLDER)),
            Err(err) => Err(err),
        },
        Request::SaveDraft { draft } => {
            check_draft(mail, &DraftAuthor::Client(client.to_string()), draft, mail.attachment_cache().as_ref())
                .await
                .map(drop)
        }
        _ => Ok(()),
    };
    checked.err().map(|err| err.response())
}

/// `request` with a move into a folder whose role is trash written as a move to the role trash
/// (`MoveTarget::Role(MoveRole::Trash)`), which needs delete access (`Request::operation`): the
/// core calls it before it checks the rights of a client, so approvals and rights see the
/// deleting. Only when the folder is in the account of the messages; anything else stays as it
/// is (and the handler answers it). A lookup that fails leaves the request as it is too; the
/// handler checks delete access again for a client ([`move_messages`]).
pub(crate) async fn with_trash_role(mail: &MailService, reach: Reach<'_>, request: Request) -> Request {
    let Request::MoveMail { ids, to: MoveTarget::Folder(folder) } = request else { return request };
    let to_trash = async {
        let place = locate_folder(mail, reach, &folder).await.ok()??;
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let refs = locate(mail, reach, &id_refs).await.ok()?;
        let same = refs.iter().all(|message| message.account == place.folder.account);
        Some(place.role == Some(Role::Trash) && same)
    };
    let to = match to_trash.await {
        Some(true) => MoveTarget::Role(torro_ipc::mail::MoveRole::Trash),
        _ => MoveTarget::Folder(folder),
    };
    Request::MoveMail { ids, to }
}

/// The draft `draft.id` replaces, after the rules for `author`: `draft.account` is a mail account
/// with mail on (else `not-found`), the draft is in its drafts folder, and a client replaces only
/// its own (else `not-found`, security rule 2). A
/// client's files must be in the managed folder `cache` (else `forbidden`, security rule 1).
/// `None` for a new draft.
async fn check_draft(
    mail: &MailService,
    author: &DraftAuthor,
    draft: &Draft,
    cache: Option<&AttachmentCache>,
) -> Result<Option<MessageRef>, WriteError> {
    // An account without mail (or none at all) is `not-found` before anyone is asked.
    let store = mail.store(&draft.account).await.map_err(|_| WriteError::NotFound(NO_ACCOUNT))?;
    let old = match &draft.id {
        None => None,
        Some(id) => {
            let (id, author) = (id.clone(), author.clone());
            let found = store
                .run(move |store| {
                    let Some(message) = store.resolve_message(&id)? else { return Ok(None) };
                    let in_drafts = store.folder_by_role(Role::Drafts)?.is_some_and(|f| f.id == message.folder);
                    let written = store.draft_author(&id)?;
                    let allowed = match &author {
                        DraftAuthor::App => true,
                        client @ DraftAuthor::Client(_) => written.as_ref() == Some(client),
                    };
                    Ok((in_drafts && allowed).then_some(message))
                })
                .await?;
            Some(found.ok_or(WriteError::NotFound(NO_DRAFT))?)
        }
    };
    if matches!(author, DraftAuthor::Client(_)) {
        for attachment in &draft.attachments {
            if let DraftAttachment::File { path } = attachment
                && !cache.is_some_and(|cache| cache.contains(Path::new(path)))
            {
                return Err(WriteError::Forbidden(FILE_OUTSIDE));
            }
        }
    }
    Ok(old)
}

// ----- mail.set_flags -----

/// `mail.set_flags`: adds `add` and clears `remove` on the messages `ids`, folder by folder.
/// Each folder's messages show the change in the store at once and get their old flags back when
/// the server refuses; folders done before stay done. Answers [`Response::Ok`].
pub(crate) async fn set_flags(
    mail: &MailService,
    reach: Reach<'_>,
    ids: &[String],
    add: &[Flag],
    remove: &[Flag],
) -> Response {
    answer(set_flags_of(mail, reach, ids, add, remove).await.map(|()| Response::Ok))
}

async fn set_flags_of(
    mail: &MailService,
    reach: Reach<'_>,
    ids: &[String],
    add: &[Flag],
    remove: &[Flag],
) -> Result<(), WriteError> {
    if add.iter().any(|flag| remove.contains(flag)) {
        return Err(WriteError::Invalid(BOTH_WAYS));
    }
    let refs = locate(mail, reach, &as_strs(ids)).await?;
    if add.is_empty() && remove.is_empty() {
        return Ok(());
    }
    for group in by_folder(refs) {
        flag_group(mail, &group, add, remove).await?;
    }
    Ok(())
}

/// Sets the flags of `group`, messages of one folder.
async fn flag_group(mail: &MailService, group: &[MessageRef], add: &[Flag], remove: &[Flag]) -> Result<(), WriteError> {
    let Some(first) = group.first() else { return Ok(()) };
    let store = mail.store(&first.account).await?;
    let ids: Vec<String> = group.iter().map(|message| message.id.clone()).collect();
    let (more, less) = (add.to_vec(), remove.to_vec());
    // Optimistic: lists show the change at once; undone below when the server refuses.
    let before = store
        .run(move |store| {
            let mut before = Vec::with_capacity(ids.len());
            for id in ids {
                let Some(summary) = store.summary(&id)? else { continue };
                store.set_flags(&id, &changed(&summary.flags, &more, &less))?;
                before.push((id, summary.flags));
            }
            Ok(before)
        })
        .await?;
    let uids: Vec<u32> = group.iter().map(|message| message.uid).collect();
    let done = async {
        let mut command = mail.command(&first.account).await?;
        open(command.session(), &first.folder_path, first.uidvalidity).await?;
        command.session().store_flags(&uids, add, remove).await?;
        command.release();
        Ok::<(), WriteError>(())
    }
    .await;
    if let Err(err) = done {
        // The connection went with the error (a dropped `Command` is never used again).
        let undo =
            store.run(move |store| before.iter().try_for_each(|(id, flags)| store.set_flags(id, flags).map(drop)));
        if let Err(undo) = undo.await {
            eprintln!("torroworkplace: cannot put back mail flags: {undo}");
        }
        return Err(err);
    }
    Ok(())
}

/// `flags` with `add` set and `remove` cleared, in the order of [`Flag`].
fn changed(flags: &[Flag], add: &[Flag], remove: &[Flag]) -> Vec<Flag> {
    let mut set: BTreeSet<Flag> = flags.iter().copied().collect();
    set.extend(add.iter().copied());
    set.retain(|flag| !remove.contains(flag));
    set.into_iter().collect()
}

// ----- mail.move_messages and mail.delete_messages -----

/// `mail.move_messages`: moves the messages `ids` into the folder `to` of their account (a role
/// is the account's folder with that role). Messages of several accounts, or a folder of another
/// account, are `unsupported`; a role the account has no folder for is `not-found`. Messages in
/// the target already stay. Answers [`Response::Ok`].
pub(crate) async fn move_messages(mail: &MailService, reach: Reach<'_>, ids: &[String], to: &MoveTarget) -> Response {
    answer(move_of(mail, reach, ids, to).await.map(|()| Response::Ok))
}

async fn move_of(mail: &MailService, reach: Reach<'_>, ids: &[String], to: &MoveTarget) -> Result<(), WriteError> {
    let refs = locate(mail, reach, &as_strs(ids)).await?;
    let Some(account) = refs.first().map(|message| message.account.clone()) else { return Ok(()) };
    if refs.iter().any(|message| message.account != account) {
        return Err(WriteError::Unsupported(ONE_ACCOUNT));
    }
    let store = mail.store(&account).await?;
    let target = match to {
        MoveTarget::Role(role) => {
            folder_with_role(&store, role.role()).await?.ok_or(WriteError::NotFound(NO_ROLE_FOLDER))?
        }
        MoveTarget::Folder(folder) => match locate_folder(mail, reach, folder).await? {
            Some(place) if place.folder.account == account => place,
            Some(_) => return Err(WriteError::Unsupported(ONE_ACCOUNT)),
            None => return Err(WriteError::NotFound(NO_FOLDER)),
        },
    };
    // The core asks for delete access before (`with_trash_role`); checked again where it runs, so
    // a folder that became the trash meanwhile (or a lookup that failed then) changes nothing.
    if let Reach::Client(rights) = reach
        && target.role == Some(Role::Trash)
        && !rights.area(Area::Mail).allows(Access::Delete)
    {
        return Err(WriteError::Forbidden(TRASH_IS_DELETING));
    }
    move_into(mail, &store, refs, &target.folder).await
}

/// `mail.delete_messages`: moves the messages `ids` into the trash of their accounts (those in the
/// trash already stay there), or with `permanent` removes them for good, which works only when all
/// of them are in the trash (else `error` and nothing is removed). The core forgets who wrote a
/// deleted draft. An account without a trash folder is `not-found`. Answers [`Response::Ok`].
pub(crate) async fn delete_messages(mail: &MailService, reach: Reach<'_>, ids: &[String], permanent: bool) -> Response {
    answer(delete_of(mail, reach, ids, permanent).await.map(|()| Response::Ok))
}

async fn delete_of(mail: &MailService, reach: Reach<'_>, ids: &[String], permanent: bool) -> Result<(), WriteError> {
    let refs = locate(mail, reach, &as_strs(ids)).await?;
    // Every account's trash first: nothing is removed when one message is refused.
    let mut accounts: Vec<(SharedStore, Place, Vec<MessageRef>)> = Vec::new();
    for message in refs {
        match accounts.iter_mut().find(|(_, trash, _)| trash.folder.account == message.account) {
            Some((_, _, messages)) => messages.push(message),
            None => {
                let store = mail.store(&message.account).await?;
                let trash = folder_with_role(&store, Role::Trash).await?.ok_or(WriteError::NotFound(NO_ROLE_FOLDER))?;
                accounts.push((store, trash, vec![message]));
            }
        }
    }
    if permanent && accounts.iter().any(|(_, trash, messages)| messages.iter().any(|m| m.folder != trash.folder.id)) {
        return Err(WriteError::Invalid(NOT_IN_TRASH));
    }
    for (store, trash, messages) in accounts {
        let ids: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
        if permanent {
            remove_for_good(mail, &store, &messages).await?;
        } else {
            move_into(mail, &store, messages, &trash.folder).await?;
        }
        store.run(move |store| ids.iter().try_for_each(|id| store.forget_draft(id).map(drop))).await?;
    }
    Ok(())
}

/// Moves `refs` (all of one account, whose store is `store`) into `target`, folder by folder, and
/// then finds where they landed, so they keep their ids in the store. Messages of folders that
/// were moved before a failure are moved in the store too; the failure is the answer.
async fn move_into(
    mail: &MailService,
    store: &SharedStore,
    refs: Vec<MessageRef>,
    target: &FolderRef,
) -> Result<(), WriteError> {
    let refs: Vec<MessageRef> = refs.into_iter().filter(|message| message.folder != target.id).collect();
    if refs.is_empty() {
        return Ok(());
    }
    let mut command = mail.command(&target.account).await?;
    // The first UID the moved messages can get: the new ones from here on are theirs or new mail.
    let next = command.session().status(&target.path).await?.uid_next;
    let mut moved: Vec<MessageRef> = Vec::new();
    let mut failure = None;
    for group in by_folder(refs) {
        let Some(first) = group.first() else { continue };
        let uids: Vec<u32> = group.iter().map(|message| message.uid).collect();
        let step = async {
            open(command.session(), &first.folder_path, first.uidvalidity).await?;
            command.session().move_messages(&uids, &target.path).await?;
            Ok::<(), WriteError>(())
        };
        match step.await {
            Ok(()) => moved.extend(group),
            Err(err) => {
                failure = Some(err);
                break;
            }
        }
    }
    let landed = match (&failure, next, moved.is_empty()) {
        (None, Some(next), false) => landed(command.session(), &target.path, next, &moved).await.ok(),
        _ => None,
    };
    // Released only when every call worked; else the connection goes (see `Command`).
    if failure.is_none() && landed.is_some() {
        command.release();
    } else {
        drop(command);
    }
    let target_id = target.id.clone();
    let account = target.account.clone();
    let left_inbox = store
        .run(move |store| {
            let mut kept: Vec<String> = Vec::new();
            if let Some((uidvalidity, found)) = landed {
                store.note_uidvalidity(&target_id, uidvalidity)?;
                for (id, header) in found {
                    if store.relocate_message(&id, &target_id, uidvalidity, header.uid)? {
                        kept.push(id);
                        continue;
                    }
                    // The sync of the old folder saw the message go and dropped it before it was
                    // moved here: stored anew under its id from the headers just read (its text
                    // comes back when someone opens it).
                    if let Ok(read) = mime::read(&header.header) {
                        let message = stored_message(&id, &target_id, uidvalidity, &header, &read);
                        put(store, &target_id, uidvalidity, &message)?;
                        kept.push(id);
                    }
                }
            }
            // Not found in the target: out of the store; the next sync brings it back.
            for message in moved.iter().filter(|message| !kept.contains(&message.id)) {
                store.delete_message(&message.id)?;
            }
            let Some(inbox) = store.folder_by_role(Role::Inbox)? else { return Ok(None) };
            let left: Vec<String> =
                moved.iter().filter(|message| message.folder == inbox.id).map(|message| message.id.clone()).collect();
            Ok(Some((inbox.id, left)))
        })
        .await?;
    // The inbox folder's sync never sees these go (their rows moved with them), so its entries are
    // settled from here (issue #205). Only mail that left the inbox: one moved back in (undo)
    // must not close the entry opened again.
    if let Some((inbox, left)) = left_inbox {
        mail.announce(MailChange {
            account,
            folder: inbox,
            role: Some(Role::Inbox),
            new: Vec::new(),
            flags: Vec::new(),
            removed: left,
            initial: false,
        });
    }
    failure.map_or(Ok(()), Err)
}

/// Where the `moved` messages are in the folder `path` now: its UIDVALIDITY and, for each moved
/// message found by its `Message-ID` among the UIDs from `next` on, its id and its headers there
/// (with the new UID). Messages without a `Message-ID`, or not found, are left out.
async fn landed(
    session: &mut Session,
    path: &str,
    next: u32,
    moved: &[MessageRef],
) -> Result<(u32, Vec<(String, FetchedHeader)>), WriteError> {
    let selected = session.select(path).await?;
    let uids = session.uid_search(&Search::UidsFrom(next)).await?;
    let mut free: Vec<(FetchedHeader, Option<String>)> = match uids.is_empty() {
        true => Vec::new(),
        false => session
            .fetch_headers(&uids)
            .await?
            .into_iter()
            .map(|header| {
                let id = mime::read(&header.header).ok().and_then(|read| read.message_id);
                (header, id)
            })
            .collect(),
    };
    free.sort_by_key(|(header, _)| header.uid);
    let mut found = Vec::new();
    for message in moved {
        let Some(wanted) = message.rfc_message_id.as_deref() else { continue };
        if let Some(at) = free.iter().position(|(_, id)| id.as_deref() == Some(wanted)) {
            found.push((message.id.clone(), free.remove(at).0));
        }
    }
    Ok((selected.uid_validity, found))
}

/// Stores `message` under its id ([`NewMessage::id`]) in `folder`: a row a sync made for the same
/// UID under an id of its own meanwhile gives way, so the id clients hold stays.
fn put(store: &MailStore, folder: &str, uidvalidity: u32, message: &NewMessage) -> Result<(), MailStoreError> {
    store.upsert_message(folder, uidvalidity, message)?;
    if let Some(other) =
        store.message_id_for_uid(folder, message.uid)?.filter(|other| Some(other) != message.id.as_ref())
    {
        store.delete_message(&other)?;
        store.upsert_message(folder, uidvalidity, message)?;
    }
    Ok(())
}

/// Removes `messages` (all in the trash of one account, whose store is `store`) on the server and
/// then from the store.
async fn remove_for_good(mail: &MailService, store: &SharedStore, messages: &[MessageRef]) -> Result<(), WriteError> {
    let Some(first) = messages.first() else { return Ok(()) };
    let uids: Vec<u32> = messages.iter().map(|message| message.uid).collect();
    let mut command = mail.command(&first.account).await?;
    open(command.session(), &first.folder_path, first.uidvalidity).await?;
    command.session().delete_messages(&uids).await?;
    command.release();
    let ids: Vec<String> = messages.iter().map(|message| message.id.clone()).collect();
    store.run(move |store| ids.iter().try_for_each(|id| store.delete_message(id).map(drop))).await?;
    Ok(())
}

// ----- mail.save_draft -----

/// `mail.save_draft`: builds `draft` and files it in the drafts folder of its account, written by
/// `author` (the app key: [`DraftAuthor::App`]; a client: its id). With an id it replaces that
/// draft ([`check_draft`] decides whose) and keeps the id; else the core gives a new id and
/// remembers `author`. Answers [`Response::MailDraft`] with the draft and its id.
///
/// `reach` decides which messages the draft may answer and forward parts of. Errors: `not-found`
/// for an account without mail or drafts folder, a foreign draft or a message, part or file that
/// is not there; `forbidden` for files outside the managed folder (clients) or behind a link;
/// `error` for an attachment over 25 MiB or an address that is no address.
pub(crate) async fn save_draft(mail: &MailService, reach: Reach<'_>, draft: Draft, author: DraftAuthor) -> Response {
    answer(save(mail, reach, draft, author).await.map(|draft| Response::MailDraft { draft }))
}

async fn save(mail: &MailService, reach: Reach<'_>, draft: Draft, author: DraftAuthor) -> Result<Draft, WriteError> {
    let draft = cleaned(draft);
    let config = mail.config(&draft.account).await?;
    let store = mail.store(&draft.account).await?;
    let drafts = folder_with_role(&store, Role::Drafts).await?.ok_or(WriteError::NotFound(NO_ROLE_FOLDER))?;
    let cache = mail.attachment_cache();
    let old = check_draft(mail, &author, &draft, cache.as_ref()).await?;
    let reply = match &draft.in_reply_to {
        Some(id) => reply_to(mail, reach, id).await?,
        None => None,
    };
    let mut attachments = Vec::with_capacity(draft.attachments.len());
    for attachment in &draft.attachments {
        attachments.push(attachment_of(mail, reach, attachment, &author, cache.as_ref()).await?);
    }
    let domain = config.address.rsplit_once('@').map_or("localhost", |(_, domain)| domain);
    let outgoing = Outgoing {
        from: MailAddress {
            name: Some(config.sender_name.trim().to_string()).filter(|name| !name.is_empty()),
            address: config.address.clone(),
        },
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        bcc: draft.bcc.clone(),
        subject: draft.subject.clone(),
        body: draft.body.clone(),
        html: draft.html.clone(),
        signature: None,
        reply,
        attachments,
        calendar: None,
        message_id: format!("{}@{domain}", random_hex(16)?),
        date: Utc::now(),
    };
    let bytes = build_draft(&outgoing)?;
    let read = mime::read(&bytes).map_err(|_| WriteError::Invalid(UNBUILDABLE))?;

    let id = match &old {
        Some(old) => old.id.clone(),
        None => {
            let author = author.clone();
            store.run(move |store| store.new_draft_id(&author)).await?
        }
    };
    let filed = file_draft(mail, &drafts.folder, &bytes, &outgoing.message_id, old.as_ref()).await;
    let (uidvalidity, uid) = match filed {
        Ok(filed) => filed,
        Err(err) => {
            if old.is_none() {
                let id = id.clone();
                let _ = store.run(move |store| store.forget_draft(&id)).await;
            }
            return Err(err);
        }
    };
    let (folder, id_kept) = (drafts.folder.id.clone(), id.clone());
    let replacing = old.is_some();
    store
        .run(move |store| {
            store.note_uidvalidity(&folder, uidvalidity)?;
            if replacing {
                store.relocate_message(&id_kept, &folder, uidvalidity, uid)?;
            }
            let mut message = draft_message(&id_kept, &folder, uidvalidity, uid, &read);
            message.flags = vec![Flag::Seen];
            put(store, &folder, uidvalidity, &message)?;
            store.set_body(&id_kept, &Body::of(read, None))?;
            Ok(())
        })
        .await?;
    // A draft the person rewrote is the person's now: no agent sends what the person wrote.
    if author == DraftAuthor::App && replacing {
        let id = id.clone();
        store.run(move |store| store.forget_draft(&id).map(drop)).await?;
    }
    Ok(Draft { id: Some(id), ..draft })
}

/// `draft` with its HTML cleaned (`html::sanitize`) and its plain text made from it; HTML that
/// holds nothing after cleaning is none. A plain draft stays as it is.
fn cleaned(draft: Draft) -> Draft {
    match draft.html.as_deref().map(html::sanitize).filter(|markup| !markup.trim().is_empty()) {
        Some(markup) => Draft { body: html::to_text(&markup), html: Some(markup), ..draft },
        None => Draft { html: None, ..draft },
    }
}

/// The draft's MIME bytes: `mime::build` (which checks every address), with the Bcc recipients in
/// a `Bcc:` header of their own, since the built message carries them in the envelope only. A
/// draft may have no recipient yet; `mime::build` wants one for the envelope, which a draft does
/// not use, so the sender stands in there and shows nowhere in the bytes.
fn build_draft(outgoing: &Outgoing) -> Result<Vec<u8>, WriteError> {
    let mut building = outgoing.clone();
    if building.to.is_empty() && building.cc.is_empty() && building.bcc.is_empty() {
        building.bcc = vec![outgoing.from.clone()];
    }
    let built = mime::build(&building).map_err(|_| WriteError::Invalid(UNBUILDABLE))?;
    if outgoing.bcc.is_empty() {
        return Ok(built.bytes);
    }
    // Addresses only: `mime::build` checked them (no white space, no line breaks); folded so no
    // line grows past the limit of RFC 5322.
    let list: Vec<&str> = outgoing.bcc.iter().map(|address| address.address.trim()).collect();
    let mut bytes = format!("Bcc: {}\r\n", list.join(",\r\n ")).into_bytes();
    bytes.extend(built.bytes);
    Ok(bytes)
}

/// Files `bytes` in the drafts folder `drafts` (`\Seen`, `\Draft`) and removes `old` after; answers
/// the folder's UIDVALIDITY and the new draft's UID (from `APPENDUID`, else found by its
/// `message_id` among the UIDs that came after).
async fn file_draft(
    mail: &MailService,
    drafts: &FolderRef,
    bytes: &[u8],
    message_id: &str,
    old: Option<&MessageRef>,
) -> Result<(u32, u32), WriteError> {
    let mut command = mail.command(&drafts.account).await?;
    let session = command.session();
    let selected = session.select(&drafts.path).await?;
    let appended = session.append(&drafts.path, bytes, &[Flag::Seen], true).await?;
    let uid = match appended {
        Some(appended) if appended.uid_validity == selected.uid_validity => appended.uid,
        _ => {
            let next = selected.uid_next.ok_or(WriteError::Invalid(NOT_LOCATED))?;
            let (_, found) = landed(session, &drafts.path, next, &[draft_ref(drafts, message_id)]).await?;
            found.first().map(|(_, header)| header.uid).ok_or(WriteError::Invalid(NOT_LOCATED))?
        }
    };
    if let Some(old) = old.filter(|old| old.folder == drafts.id && old.uidvalidity == selected.uid_validity) {
        session.delete_messages(&[old.uid]).await?;
    }
    command.release();
    Ok((selected.uid_validity, uid))
}

/// A stand-in for the draft just appended, to find it with [`landed`] by its `Message-ID`.
fn draft_ref(drafts: &FolderRef, message_id: &str) -> MessageRef {
    MessageRef {
        id: String::new(),
        account: drafts.account.clone(),
        folder: drafts.id.clone(),
        folder_path: drafts.path.clone(),
        uidvalidity: 0,
        uid: 0,
        rfc_message_id: Some(message_id.to_string()),
    }
}

/// The headers of the stored draft `id` at `uid` in `folder`, from its built bytes.
fn draft_message(id: &str, folder: &str, uidvalidity: u32, uid: u32, read: &ReadMessage) -> NewMessage {
    NewMessage {
        id: Some(id.to_string()),
        uid,
        rfc_message_id: read.message_id.clone(),
        thread_id: super::sync::thread_id(read).unwrap_or_else(|| format!("t:{folder}:{uidvalidity}:{uid}")),
        from: read.from.clone(),
        to: read.to.clone(),
        cc: read.cc.clone(),
        reply_to: read.reply_to.clone(),
        subject: read.subject.clone(),
        date: read.date.unwrap_or_else(Utc::now),
        snippet: read.snippet(),
        flags: Vec::new(),
        has_attachments: read.has_attachments(),
        traits: read.traits.clone(),
        invitation: read.invitation,
    }
}

/// The headers of message `id` as fetched in `folder` (`header`, read as `read`), as the sync
/// would store them, keeping the id.
fn stored_message(id: &str, folder: &str, uidvalidity: u32, header: &FetchedHeader, read: &ReadMessage) -> NewMessage {
    let mut message = draft_message(id, folder, uidvalidity, header.uid, read);
    message.date = read.date.or(header.internal_date).unwrap_or(message.date);
    message.flags = header.flags.clone();
    message.has_attachments = header.has_attachments || message.has_attachments;
    message
}

/// The headers a draft answering the message `id` carries, from the message's own headers on the
/// server (so the draft joins its conversation); `None` when it has no `Message-ID`.
async fn reply_to(mail: &MailService, reach: Reach<'_>, id: &str) -> Result<Option<Reply>, WriteError> {
    let answered = locate(mail, reach, &[id]).await?.remove(0);
    let mut command = mail.command(&answered.account).await?;
    open(command.session(), &answered.folder_path, answered.uidvalidity).await?;
    let headers = command.session().fetch_headers(&[answered.uid]).await?;
    command.release();
    let header =
        headers.into_iter().find(|header| header.uid == answered.uid).ok_or(WriteError::NotFound(NO_MESSAGE))?;
    Ok(mime::read(&header.header).ok().and_then(|read| read.reply()))
}

/// One attachment of a draft with its content: a part of a message `reach` sees, or a file
/// ([`read_file`]).
async fn attachment_of(
    mail: &MailService,
    reach: Reach<'_>,
    attachment: &DraftAttachment,
    author: &DraftAuthor,
    cache: Option<&AttachmentCache>,
) -> Result<OutgoingAttachment, WriteError> {
    match attachment {
        DraftAttachment::File { path } => {
            let (path, client, cache) = (path.clone(), matches!(author, DraftAuthor::Client(_)), cache.cloned());
            tokio::task::spawn_blocking(move || read_file(&path, client, cache.as_ref())).await.map_err(|err| {
                WriteError::Mail(MailError::Store(MailStoreError::Io(std::io::Error::other(err.to_string()))))
            })?
        }
        DraftAttachment::MailPart { message, part } => {
            let source = locate(mail, reach, &[message.as_str()]).await?.remove(0);
            let mut command = mail.command(&source.account).await?;
            open(command.session(), &source.folder_path, source.uidvalidity).await?;
            let raw = command.session().fetch_message(source.uid).await?;
            command.release();
            let raw = raw.ok_or(WriteError::NotFound(NO_MESSAGE))?;
            let found = mime::attachment(&raw, part).map_err(|_| WriteError::NotFound(NO_ATTACHMENT))?;
            if found.bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(WriteError::Invalid(TOO_BIG));
            }
            Ok(OutgoingAttachment {
                filename: safe_file_name(found.filename.as_deref().unwrap_or("")),
                content_type: found.content_type,
                bytes: found.bytes,
            })
        }
    }
}

/// The file at `path` to attach (security rule 1): for a `client` only inside the managed folder
/// `cache` (else `forbidden`, also for a file that is not there, so nothing about other files
/// shows); for everyone an absolute path, a regular file reached without a symbolic link at the
/// end, at most 25 MiB. Blocks: call it on a blocking thread.
fn read_file(path: &str, client: bool, cache: Option<&AttachmentCache>) -> Result<OutgoingAttachment, WriteError> {
    let path = Path::new(path);
    if client && !cache.is_some_and(|cache| cache.contains(path)) {
        return Err(WriteError::Forbidden(FILE_OUTSIDE));
    }
    if !path.is_absolute() {
        return Err(WriteError::Invalid(NOT_ABSOLUTE));
    }
    let meta = fs::symlink_metadata(path).map_err(|_| WriteError::NotFound(NO_FILE))?;
    if meta.file_type().is_symlink() {
        return Err(WriteError::Forbidden(NO_LINKS));
    }
    if !meta.is_file() {
        return Err(WriteError::Invalid(NOT_A_FILE));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(WriteError::Invalid(TOO_BIG));
    }
    let mut bytes = Vec::new();
    let file = fs::File::open(path).map_err(|_| WriteError::NotFound(NO_FILE))?;
    // The file may have grown since it was measured.
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| WriteError::NotFound(NO_FILE))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(WriteError::Invalid(TOO_BIG));
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(OutgoingAttachment { content_type: content_type_of(path).to_string(), filename: safe_file_name(&name), bytes })
}

/// The MIME type of a file by its extension, for the types mail usually carries; else
/// `application/octet-stream`, which every mail program can save.
fn content_type_of(path: &Path) -> &'static str {
    let extension = path.extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "ics" => "text/calendar",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

// ----- approvals -----

/// What the mail store holds about what a changing mail `request` names, for its approval's
/// summary (`crate::approvals::MailFacts`): the account's address, how many messages, the
/// subjects the store has (in the order of `ids`), the target folder's name, and for replacing a
/// draft its subject as stored. Looked up by the ids the command runs with, never taken from the
/// client's text; call it after the reach was checked. `None` for other requests.
pub(crate) async fn facts(mail: &MailService, request: &Request) -> Option<MailFacts> {
    match request {
        Request::SetMailFlags { ids, .. } | Request::DeleteMail { ids, .. } => Some(message_facts(mail, ids).await.0),
        Request::MoveMail { ids, to } => {
            let (mut facts, account) = message_facts(mail, ids).await;
            facts.folder = match (to, account) {
                (MoveTarget::Role(role), Some(account)) => match mail.store(&account).await {
                    Ok(store) => folder_with_role(&store, role.role()).await.ok().flatten().map(|place| place.name),
                    Err(_) => None,
                },
                (MoveTarget::Folder(folder), _) => {
                    locate_folder(mail, Reach::Suite, folder).await.ok().flatten().map(|place| place.name)
                }
                (MoveTarget::Role(_), None) => None,
            };
            Some(facts)
        }
        Request::SaveDraft { draft } => {
            let account = mail.config(&draft.account).await.ok().map(|config| config.address);
            let draft = match &draft.id {
                Some(id) => draft_subject(mail, &draft.account, id)
                    .await
                    .map(|subject| DraftFacts { subject, ..Default::default() }),
                None => None,
            };
            Some(MailFacts { account, draft, ..MailFacts::default() })
        }
        _ => None,
    }
}

/// The facts of the messages `ids` and the account of the first one the store holds.
async fn message_facts(mail: &MailService, ids: &[String]) -> (MailFacts, Option<String>) {
    let found = locate_each(mail, Reach::Suite, &as_strs(ids)).await.unwrap_or_default();
    let known: Vec<MessageRef> = found.into_iter().flatten().collect();
    let account = known.first().map(|message| message.account.clone());
    let mut subjects = Vec::new();
    for message in &known {
        let Ok(store) = mail.store(&message.account).await else { continue };
        let id = message.id.clone();
        if let Ok(Some(summary)) = store.run(move |store| store.summary(&id)).await {
            subjects.push(summary.subject);
        }
    }
    let address = match &account {
        Some(account) => mail.config(account).await.ok().map(|config| config.address),
        None => None,
    };
    (MailFacts { account: address, count: ids.len(), subjects, ..MailFacts::default() }, account)
}

/// The subject of the draft `id` of `account` as the store holds it.
async fn draft_subject(mail: &MailService, account: &str, id: &str) -> Option<String> {
    let store = mail.store(account).await.ok()?;
    let id = id.to_string();
    store.run(move |store| store.summary(&id)).await.ok().flatten().map(|summary| summary.subject)
}

// ----- helpers -----

/// Opens `path` on `session` and checks that its UIDVALIDITY is still `uidvalidity`: else the
/// stored UIDs name other messages now ([`WriteError::Conflict`]).
async fn open(session: &mut Session, path: &str, uidvalidity: u32) -> Result<(), WriteError> {
    let selected = session.select(path).await?;
    if selected.uid_validity != uidvalidity {
        return Err(WriteError::Conflict);
    }
    Ok(())
}

/// `refs` grouped by folder, in the order each folder is first named.
fn by_folder(refs: Vec<MessageRef>) -> Vec<Vec<MessageRef>> {
    let mut groups: Vec<Vec<MessageRef>> = Vec::new();
    for message in refs {
        match groups.iter_mut().find(|group| group.first().is_some_and(|first| first.folder == message.folder)) {
            Some(group) => group.push(message),
            None => groups.push(vec![message]),
        }
    }
    groups
}

fn as_strs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

// ----- answering an invitation (task E8.3b) -----

/// The longest line of an iCalendar object, in bytes without the line end (RFC 5545, 3.1).
const ICS_LINE_MAX: usize = 75;

/// Sends the answer `answer` to `invitation`, which came in the message `read` of `account`, to its
/// organizer (`mail.respond_to_invitation`): from the account's address and sender name, as an
/// answer to the message (`In-Reply-To`), with a short text in the core's language and the
/// iCalendar `REPLY` beside it ([`reply_ics`], `text/calendar; method=REPLY`). No signature: it is
/// an answer of the calendar, not a letter. Sent with [`super::send::send_message`] under the send
/// rule, a copy in the Sent folder. The caller checked who may send it and that the invitation can
/// be answered ([`Invitation::answerable`]).
///
/// Answers how the send went, or the error to answer with (codes as for `mail.send_draft`).
pub(crate) async fn send_invitation_reply(
    mail: &MailService,
    account: &str,
    read: &ReadMessage,
    invitation: &Invitation,
    answer: InvitationAnswer,
    now: DateTime<Utc>,
) -> Result<Delivery, Response> {
    let config = mail.config(account).await.map_err(|err| err.response())?;
    let Some(organizer) = invitation.organizer.clone() else {
        return Err(WriteError::Invalid(NOT_ANSWERABLE).response());
    };
    let name = Some(config.sender_name.trim().to_string()).filter(|name| !name.is_empty());
    let me = attendee(invitation, &config.address, name.as_deref());
    let title = invitation.event.summary.trim();
    let shown_name = name.clone().unwrap_or_else(|| config.address.clone());
    let domain = config.address.rsplit_once('@').map_or("localhost", |(_, domain)| domain);
    let message_id = random_hex(16).map_err(|err| WriteError::from(err).response())?;
    let outgoing = Outgoing {
        from: MailAddress { name, address: config.address.clone() },
        to: vec![MailAddress { name: organizer.name.clone(), address: organizer.address.clone() }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: reply_subject(answer, title),
        body: reply_text(answer, &shown_name),
        html: None,
        signature: None,
        reply: read.reply(),
        attachments: Vec::new(),
        calendar: Some(CalendarPart {
            method: "REPLY".into(),
            ics: reply_ics(invitation, &organizer, &me, answer, now),
        }),
        message_id: format!("{message_id}@{domain}"),
        date: now,
    };
    super::send::send_message(mail, account, &outgoing).await.map_err(|err| err.response())
}

/// Who answers: the attendee of `invitation` with the account's `address` (any case), as the
/// invitation spells it, so the organizer's calendar finds its own entry; else the address itself.
/// The name is the account's sender name, else the one the invitation gives.
fn attendee(invitation: &Invitation, address: &str, name: Option<&str>) -> MailAddress {
    let listed = invitation.attendees.iter().find(|attendee| attendee.address.eq_ignore_ascii_case(address));
    MailAddress {
        name: name.map(str::to_string).or_else(|| listed.and_then(|attendee| attendee.name.clone())),
        address: listed.map_or_else(|| address.to_string(), |attendee| attendee.address.clone()),
    }
}

/// The subject of the answer, as calendars write it: "Accepted: Planning".
fn reply_subject(answer: InvitationAnswer, title: &str) -> String {
    let title = if title.is_empty() { tr("(no title)").to_string() } else { title.to_string() };
    match answer {
        InvitationAnswer::Accept => trf("Accepted: {title}", &[("title", &title)]),
        InvitationAnswer::Tentative => trf("Tentative: {title}", &[("title", &title)]),
        InvitationAnswer::Decline => trf("Declined: {title}", &[("title", &title)]),
    }
}

/// The text beside the calendar part, for mail programs that show no calendar.
fn reply_text(answer: InvitationAnswer, name: &str) -> String {
    match answer {
        InvitationAnswer::Accept => trf("{name} has accepted this invitation.", &[("name", &name)]),
        InvitationAnswer::Tentative => trf("{name} has tentatively accepted this invitation.", &[("name", &name)]),
        InvitationAnswer::Decline => trf("{name} has declined this invitation.", &[("name", &name)]),
    }
}

/// The iCalendar object of the answer (iTIP `REPLY`, RFC 5546 3.2.3): the event's `UID`,
/// `SEQUENCE`, times and title as the invitation has them, the organizer, the time of the answer
/// (`DTSTAMP`) and one `ATTENDEE`, `me`, with the answer as `PARTSTAT`. Values from outside are
/// made harmless: text escaped, line breaks and control characters removed from the UID and the
/// addresses, quotes from names; lines folded at 75 bytes, CRLF line ends.
pub(crate) fn reply_ics(
    invitation: &Invitation,
    organizer: &MailAddress,
    me: &MailAddress,
    answer: InvitationAnswer,
    now: DateTime<Utc>,
) -> String {
    let event = &invitation.event;
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//Torro Workplace//Torro Workplace//EN".to_string(),
        "METHOD:REPLY".to_string(),
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", one_value(&event.uid)),
        format!("SEQUENCE:{}", invitation.sequence),
        format!("DTSTAMP:{}", now.format("%Y%m%dT%H%M%SZ")),
        ics_time("DTSTART", event.start),
    ];
    lines.extend(event.end.map(|end| ics_time("DTEND", end)));
    lines.push(format!("SUMMARY:{}", ics_text(&event.summary)));
    lines.push(format!("ORGANIZER{}:mailto:{}", cn(organizer), one_value(&organizer.address)));
    lines.push(format!("ATTENDEE;PARTSTAT={}{}:mailto:{}", answer.partstat(), cn(me), one_value(&me.address)));
    lines.push("END:VEVENT".to_string());
    lines.push("END:VCALENDAR".to_string());
    lines.iter().map(|line| fold(line)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

/// `NAME:20261015T080000Z`, or `NAME;VALUE=DATE:20261015` for a day.
fn ics_time(name: &str, time: torro_pim::EventTime) -> String {
    match time {
        torro_pim::EventTime::Date(day) => format!("{name};VALUE=DATE:{}", day.format("%Y%m%d")),
        torro_pim::EventTime::DateTime(at) => format!("{name}:{}", at.format("%Y%m%dT%H%M%SZ")),
    }
}

/// A text value escaped as RFC 5545 3.3.11 says.
fn ics_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A value that must stay on its line: control characters (line breaks among them) removed.
fn one_value(value: &str) -> String {
    value.trim().chars().filter(|c| !c.is_control()).collect()
}

/// `;CN="Anna Berg"` for a name, empty without: quotes and control characters removed, so a name
/// cannot end the parameter.
fn cn(who: &MailAddress) -> String {
    let name: String =
        who.name.as_deref().unwrap_or_default().chars().filter(|c| *c != '"' && !c.is_control()).collect();
    let name = name.trim();
    if name.is_empty() { String::new() } else { format!(";CN=\"{name}\"") }
}

/// `line` folded into lines of at most [`ICS_LINE_MAX`] bytes, each continuation after CRLF and a
/// space, never inside a character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / ICS_LINE_MAX * 3);
    let mut length = 0;
    for c in line.chars() {
        if length + c.len_utf8() > ICS_LINE_MAX {
            out.push_str("\r\n ");
            length = 1;
        }
        out.push(c);
        length += c.len_utf8();
    }
    out
}

fn answer(result: Result<Response, WriteError>) -> Response {
    result.unwrap_or_else(|err| err.response())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::store::SearchQuery;
    use crate::mail::sync::fake::{self, FakeImap};

    pub(crate) const ANNA: &str = "mail:anna@example.org";

    /// A fake server with a trash folder and mail in the inbox, archive and trash, and Anna's
    /// account on it in `dir`, synced once.
    pub(crate) struct Mailroom {
        pub(crate) dir: tempfile::TempDir,
        pub(crate) server: FakeImap,
        pub(crate) mail: MailService,
    }

    /// The server of a [`Mailroom`] with Anna's account stored in `dir` (keychain `secrets`).
    pub(crate) async fn mail_server(dir: &Path, secrets: &MemoryTokens) -> FakeImap {
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Trash", "\\HasNoChildren \\Trash", 15);
        server.add("INBOX", &fake::mail("Hello", 1, None), &[]);
        server.add("INBOX", &fake::mail("Invoice", 2, None), &[]);
        server.add("Archive", &fake::mail("Old news", 30, None), &["\\Seen"]);
        server.add("Trash", &fake::mail("Spam offer", 3, None), &["\\Seen"]);
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir, secrets, &[(&account, PASSWORD)]);
        server
    }

    async fn mailroom() -> Mailroom {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = mail_server(dir.path(), &secrets).await;
        let mail = service_in(dir.path(), &secrets);
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        Mailroom { dir, server, mail }
    }

    /// The id of the message with `subject` in Anna's store.
    pub(crate) async fn id_of(mail: &MailService, subject: &str) -> String {
        let store = mail.store(ANNA).await.expect("store");
        let subject = subject.to_string();
        store
            .run(move |store| {
                let all = store.search(&SearchQuery::default(), 100, 0)?;
                Ok(all.into_iter().find(|summary| summary.subject == subject).map(|summary| summary.id))
            })
            .await
            .expect("searched")
            .expect("a message with this subject")
    }

    /// The path of the folder message `id` is in, as the store has it; `None` when it is gone.
    pub(crate) async fn folder_of(mail: &MailService, id: &str) -> Option<String> {
        let store = mail.store(ANNA).await.expect("store");
        let id = id.to_string();
        store.run(move |store| Ok(store.resolve_message(&id)?.map(|m| m.folder_path))).await.expect("read")
    }

    async fn flags_of(mail: &MailService, id: &str) -> Vec<Flag> {
        let store = mail.store(ANNA).await.expect("store");
        let id = id.to_string();
        store.run(move |store| Ok(store.summary(&id)?.map(|s| s.flags))).await.expect("read").expect("there")
    }

    /// The subjects of `folder` on the server.
    pub(crate) fn on_server(server: &FakeImap, folder: &str) -> Vec<String> {
        server
            .messages(folder)
            .into_iter()
            .filter_map(|(_, _, raw)| mime::read(raw.as_bytes()).ok().map(|read| read.subject))
            .collect()
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    /// A draft from Anna to Bernd.
    pub(crate) fn draft(subject: &str) -> Draft {
        Draft {
            id: None,
            account: ANNA.into(),
            to: vec![MailAddress { name: Some("Bernd".into()), address: "bernd@example.com".into() }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: subject.into(),
            body: "Dear Bernd,\nsee you.".into(),
            html: None,
            in_reply_to: None,
            attachments: Vec::new(),
        }
    }

    pub(crate) fn saved(response: Response) -> Draft {
        match response {
            Response::MailDraft { draft } => draft,
            other => panic!("expected a saved draft, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn flags_change_on_the_server_and_in_the_store() {
        let room = mailroom().await;
        let hello = id_of(&room.mail, "Hello").await;
        let ids = [hello.clone()];
        let set = set_flags(&room.mail, Reach::Suite, &ids, &[Flag::Seen, Flag::Flagged], &[]).await;
        assert_eq!(set, Response::Ok);
        assert_eq!(flags_of(&room.mail, &hello).await, [Flag::Seen, Flag::Flagged]);
        let (_, flags, _) = room.server.messages("INBOX").into_iter().find(|(uid, _, _)| *uid == 1).expect("there");
        assert_eq!(flags, ["\\Flagged", "\\Seen"]);

        assert_eq!(set_flags(&room.mail, Reach::Suite, &ids, &[], &[Flag::Flagged]).await, Response::Ok);
        assert_eq!(flags_of(&room.mail, &hello).await, [Flag::Seen]);
        let both = set_flags(&room.mail, Reach::Suite, &ids, &[Flag::Seen], &[Flag::Seen]).await;
        assert_eq!(code(&both), "error");
        let unknown = set_flags(&room.mail, Reach::Suite, &["m:nothing".into()], &[Flag::Seen], &[]).await;
        assert_eq!(code(&unknown), "not-found");
    }

    #[tokio::test]
    async fn a_refused_flag_change_is_put_back_in_the_store() {
        let room = mailroom().await;
        let old = id_of(&room.mail, "Old news").await;
        // The archive is not watched: the store still holds the old numbering.
        room.server.renumber("Archive");
        let refused = set_flags(&room.mail, Reach::Suite, std::slice::from_ref(&old), &[Flag::Flagged], &[]).await;
        assert_eq!(code(&refused), "conflict");
        assert_eq!(flags_of(&room.mail, &old).await, [Flag::Seen], "back as it was");
    }

    #[tokio::test]
    async fn moved_messages_keep_their_ids() {
        let room = mailroom().await;
        let (hello, invoice) = (id_of(&room.mail, "Hello").await, id_of(&room.mail, "Invoice").await);
        let to = MoveTarget::Role(torro_ipc::mail::MoveRole::Archive);
        assert_eq!(move_messages(&room.mail, Reach::Suite, &[hello.clone(), invoice.clone()], &to).await, Response::Ok);
        assert!(on_server(&room.server, "INBOX").is_empty());
        let mut archived = on_server(&room.server, "Archive");
        archived.sort();
        assert_eq!(archived, ["Hello", "Invoice", "Old news"]);
        assert_eq!(folder_of(&room.mail, &hello).await.as_deref(), Some("Archive"));
        assert_eq!(folder_of(&room.mail, &invoice).await.as_deref(), Some("Archive"));

        // Back by the folder's id; a message there already stays.
        let store = room.mail.store(ANNA).await.expect("store");
        let inbox = store
            .run(|store| Ok(store.folder_by_role(Role::Inbox)?.map(|f| f.id)))
            .await
            .expect("read")
            .expect("inbox");
        let back = MoveTarget::Folder(inbox);
        assert_eq!(move_messages(&room.mail, Reach::Suite, std::slice::from_ref(&hello), &back).await, Response::Ok);
        assert_eq!(folder_of(&room.mail, &hello).await.as_deref(), Some("INBOX"));
        assert_eq!(move_messages(&room.mail, Reach::Suite, std::slice::from_ref(&hello), &back).await, Response::Ok);
        assert_eq!(on_server(&room.server, "INBOX"), ["Hello"]);
        let nowhere = move_messages(&room.mail, Reach::Suite, &[hello], &MoveTarget::Folder("f:none".into())).await;
        assert_eq!(code(&nowhere), "not-found");
    }

    #[tokio::test]
    async fn mail_that_leaves_the_inbox_is_announced_as_removed() {
        let room = mailroom().await;
        let (hello, invoice) = (id_of(&room.mail, "Hello").await, id_of(&room.mail, "Invoice").await);
        let mut events = room.mail.subscribe();
        // The inbox's own sync (IDLE) may announce as well; what matters is the move's own change.
        let removed_from_inbox = |events: &mut tokio::sync::broadcast::Receiver<MailChange>| {
            let mut removed = Vec::new();
            while let Ok(change) = events.try_recv() {
                assert_eq!(change.account, ANNA);
                if change.role == Some(Role::Inbox) {
                    removed.extend(change.removed);
                }
            }
            removed
        };
        let archive = MoveTarget::Role(torro_ipc::mail::MoveRole::Archive);
        assert_eq!(move_messages(&room.mail, Reach::Suite, std::slice::from_ref(&hello), &archive).await, Response::Ok);
        assert!(removed_from_inbox(&mut events).contains(&hello), "the archived mail is announced");

        // Into the trash is the same; back into the inbox (undo) does not settle anything.
        assert_eq!(
            delete_messages(&room.mail, Reach::Suite, std::slice::from_ref(&invoice), false).await,
            Response::Ok
        );
        assert!(removed_from_inbox(&mut events).contains(&invoice), "the deleted mail is announced");
        let inbox = MoveTarget::Role(torro_ipc::mail::MoveRole::Inbox);
        assert_eq!(move_messages(&room.mail, Reach::Suite, std::slice::from_ref(&hello), &inbox).await, Response::Ok);
        assert_eq!(folder_of(&room.mail, &hello).await.as_deref(), Some("INBOX"));
        assert!(!removed_from_inbox(&mut events).contains(&hello), "a move into the inbox is no removal");
    }

    #[tokio::test]
    async fn moving_between_accounts_is_unsupported() {
        let room = mailroom().await;
        let other = FakeImap::start(PASSWORD).await;
        other.add("INBOX", &fake::mail("For Bernd", 0, None), &[]);
        let secrets = MemoryTokens::default();
        let anna = mail_account("anna@example.org", &room.server.endpoint);
        let bernd = mail_account("bernd@example.org", &other.endpoint);
        store_accounts(room.dir.path(), &secrets, &[(&anna, PASSWORD), (&bernd, PASSWORD)]);
        let mail = service_in(room.dir.path(), &secrets);
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let hello = id_of(&mail, "Hello").await;
        let store = mail.store(&bernd.id).await.expect("Bernd's store");
        let (bernds, bernds_inbox) = store
            .run(|store| {
                let found = store.search(&SearchQuery::default(), 10, 0)?;
                let inbox = store.folder_by_role(Role::Inbox)?.map(|f| f.id);
                Ok((found[0].id.clone(), inbox))
            })
            .await
            .expect("read");
        let archive = MoveTarget::Role(torro_ipc::mail::MoveRole::Archive);
        let mixed = move_messages(&mail, Reach::Suite, &[hello.clone(), bernds], &archive).await;
        assert_eq!(code(&mixed), "unsupported");
        let elsewhere = MoveTarget::Folder(bernds_inbox.expect("inbox"));
        assert_eq!(
            code(&move_messages(&mail, Reach::Suite, std::slice::from_ref(&hello), &elsewhere).await),
            "unsupported"
        );

        // A client that may only write never moves into the trash, whatever the core checked before.
        let mut rights = torro_ipc::api::RightsTemplate::ReadAndWrite.rights();
        rights
            .areas
            .insert(Area::Mail, torro_ipc::api::AreaRights { level: torro_ipc::api::Level::Write, admin: false });
        let store = mail.store(ANNA).await.expect("Anna's store");
        let trash = store.run(|store| Ok(store.folder_by_role(Role::Trash)?.map(|f| f.id))).await.expect("read");
        let into_trash = MoveTarget::Folder(trash.expect("a trash folder"));
        let refused = move_messages(&mail, Reach::Client(&rights), &[hello], &into_trash).await;
        assert_eq!(code(&refused), "forbidden");
        assert_eq!(on_server(&room.server, "INBOX").len(), 2, "nothing moved");
    }

    #[tokio::test]
    async fn deleting_goes_to_the_trash_and_for_good_only_from_there() {
        let room = mailroom().await;
        let (hello, spam) = (id_of(&room.mail, "Hello").await, id_of(&room.mail, "Spam offer").await);
        let for_good = delete_messages(&room.mail, Reach::Suite, &[hello.clone(), spam.clone()], true).await;
        assert_eq!(code(&for_good), "error");
        assert_eq!(on_server(&room.server, "Trash"), ["Spam offer"], "nothing removed");

        assert_eq!(delete_messages(&room.mail, Reach::Suite, std::slice::from_ref(&hello), false).await, Response::Ok);
        assert_eq!(folder_of(&room.mail, &hello).await.as_deref(), Some("Trash"));
        assert_eq!(on_server(&room.server, "INBOX"), ["Invoice"]);
        let both = [hello.clone(), spam.clone()];
        assert_eq!(delete_messages(&room.mail, Reach::Suite, &both, true).await, Response::Ok);
        assert!(on_server(&room.server, "Trash").is_empty());
        assert_eq!((folder_of(&room.mail, &hello).await, folder_of(&room.mail, &spam).await), (None, None));
    }

    /// The raw drafts in the drafts folder of the fake, with their flags.
    pub(crate) fn drafts_on_server(server: &FakeImap) -> Vec<(Vec<String>, String)> {
        server.messages("Drafts").into_iter().map(|(_, flags, raw)| (flags, raw)).collect()
    }

    #[tokio::test]
    async fn a_draft_is_filed_with_its_author_and_replaced_under_the_same_id() {
        let room = mailroom().await;
        let mut first = draft("Lunch");
        first.bcc = vec![MailAddress { name: Some("Carla".into()), address: "carla@example.net".into() }];
        let filed_first = saved(save_draft(&room.mail, Reach::Suite, first.clone(), DraftAuthor::App).await);
        let id = filed_first.id.clone().expect("an id");
        assert_eq!(Draft { id: None, ..filed_first }, first);
        let filed = drafts_on_server(&room.server);
        assert_eq!(filed.len(), 1);
        assert_eq!(filed[0].0, ["\\Draft", "\\Seen"]);
        assert!(filed[0].1.contains("Bcc: carla@example.net\r\n"), "the Bcc is kept: {}", filed[0].1);
        assert!(filed[0].1.contains("Subject: Lunch") && !filed[0].1.contains("\n-- \r\n"), "no signature");
        let store = room.mail.store(ANNA).await.expect("store");
        let read = id.clone();
        let (author, summary) =
            store.run(move |store| Ok((store.draft_author(&read)?, store.summary(&read)?))).await.expect("read");
        assert_eq!(author, Some(DraftAuthor::App));
        assert_eq!(summary.map(|s| s.subject).as_deref(), Some("Lunch"));

        let replaced = Draft { id: Some(id.clone()), subject: "Dinner".into(), ..first };
        let again = saved(save_draft(&room.mail, Reach::Suite, replaced, DraftAuthor::App).await);
        assert_eq!(again.id.as_deref(), Some(id.as_str()), "the id stays");
        assert_eq!(on_server(&room.server, "Drafts"), ["Dinner"], "the old draft is gone");
        let subject = store.run(move |store| Ok(store.summary(&id)?.map(|s| s.subject))).await.expect("read");
        assert_eq!(subject.as_deref(), Some("Dinner"));
    }

    /// Issue #125: a formatted draft is stored cleaned, as HTML and the plain text made from it.
    #[tokio::test]
    async fn a_formatted_draft_is_cleaned_and_its_plain_text_made_from_it() {
        let room = mailroom().await;
        let dirty = "<p>Dear <b>Bernd</b>,<script>steal()</script></p><p onclick=\"x()\">see <a href=\"javascript:x()\">you</a>.</p>";
        let formatted = Draft { html: Some(dirty.into()), body: "not used".into(), ..draft("Lunch") };
        let filed = saved(save_draft(&room.mail, Reach::Suite, formatted, DraftAuthor::App).await);
        assert_eq!(filed.html.as_deref(), Some("<p>Dear <b>Bernd</b>,</p><p>see you.</p>"));
        assert_eq!(filed.body, "Dear Bernd,\n\nsee you.", "the plain text is the HTML's");
        let stored = &drafts_on_server(&room.server)[0].1;
        assert!(stored.contains("multipart/alternative") && stored.contains("text/html"), "{stored}");
        assert!(!stored.contains("steal") && !stored.contains("javascript"), "{stored}");
        let read = mime::read(stored.as_bytes()).expect("read");
        assert_eq!(read.text.trim_end(), "Dear Bernd,\n\nsee you.");
        assert_eq!(read.html.as_deref().map(|html| html.contains("<b>Bernd</b>")), Some(true));

        // HTML that holds nothing after cleaning is a plain draft.
        let nothing = Draft { html: Some("<script>x()</script>".into()), ..draft("Plain") };
        let plain = saved(save_draft(&room.mail, Reach::Suite, nothing, DraftAuthor::App).await);
        assert_eq!((plain.html, plain.body.as_str()), (None, "Dear Bernd,\nsee you."));
    }

    #[tokio::test]
    async fn a_draft_without_recipients_shows_no_sender_as_recipient() {
        let room = mailroom().await;
        let empty = Draft { to: Vec::new(), ..draft("Ideas") };
        saved(save_draft(&room.mail, Reach::Suite, empty, DraftAuthor::App).await);
        let raw = &drafts_on_server(&room.server)[0].1;
        assert!(!raw.contains("To:") && !raw.contains("Bcc:"), "{raw}");
    }

    #[tokio::test]
    async fn a_client_replaces_only_its_own_drafts_and_attaches_only_managed_files() {
        let room = mailroom().await;
        let person = saved(save_draft(&room.mail, Reach::Suite, draft("Mine"), DraftAuthor::App).await);
        let agent = DraftAuthor::Client("c1".into());
        let takeover = Draft { id: person.id.clone(), ..draft("Hijacked") };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, takeover, agent.clone()).await), "not-found");
        let own = saved(save_draft(&room.mail, Reach::Suite, draft("Agent's"), agent.clone()).await);
        let foreign = Draft { id: own.id.clone(), ..draft("Not yours") };
        let other = DraftAuthor::Client("c2".into());
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, foreign, other).await), "not-found");
        let own_again = Draft { id: own.id.clone(), ..draft("Agent's, better") };
        saved(save_draft(&room.mail, Reach::Suite, own_again, agent.clone()).await);
        // The person rewrites the agent's draft: it is the person's now.
        let rewritten = Draft { id: own.id.clone(), ..draft("Rewritten by the person") };
        saved(save_draft(&room.mail, Reach::Suite, rewritten, DraftAuthor::App).await);
        let back = Draft { id: own.id.clone(), ..draft("Agent again") };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, back, agent.clone()).await), "not-found");
        let nowhere = Draft { account: "mail:nobody@example.org".into(), ..draft("Nowhere") };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, nowhere, DraftAuthor::App).await), "not-found");
        // A message that is no draft cannot be replaced, not even by the person.
        let hello = id_of(&room.mail, "Hello").await;
        let no_draft = Draft { id: Some(hello), ..draft("Gone") };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, no_draft, DraftAuthor::App).await), "not-found");

        let outside = room.dir.path().join("secret.txt");
        fs::write(&outside, b"secret").expect("written");
        let with_file = |path: &PathBuf| Draft {
            attachments: vec![DraftAttachment::File { path: path.display().to_string() }],
            ..draft("With a file")
        };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, with_file(&outside), agent.clone()).await), "forbidden");
        let cache = room.mail.attachment_cache().expect("a managed folder");
        let inside = cache.store("notes.txt", b"from the cache").expect("stored");
        saved(save_draft(&room.mail, Reach::Suite, with_file(&inside), agent).await);
        assert!(on_server(&room.server, "Drafts").contains(&"With a file".to_string()));
        // The person may attach any file, but never through a link.
        saved(save_draft(&room.mail, Reach::Suite, with_file(&outside), DraftAuthor::App).await);
        #[cfg(unix)]
        {
            let link = room.dir.path().join("link.txt");
            std::os::unix::fs::symlink(&outside, &link).expect("link");
            let linked = save_draft(&room.mail, Reach::Suite, with_file(&link), DraftAuthor::App).await;
            assert_eq!(code(&linked), "forbidden");
        }
        let missing = room.dir.path().join("missing.txt");
        let missing = save_draft(&room.mail, Reach::Suite, with_file(&missing), DraftAuthor::App).await;
        assert_eq!(code(&missing), "not-found");
    }

    #[tokio::test]
    async fn a_reply_joins_its_conversation_and_a_forward_carries_the_part() {
        let room = mailroom().await;
        let raw = "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: Contract\r\n\
                   Message-ID: <contract@example.com>\r\nMIME-Version: 1.0\r\n\
                   Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain\r\n\r\n\
                   See attached.\r\n--b1\r\nContent-Type: application/pdf\r\n\
                   Content-Disposition: attachment; filename=\"contract.pdf\"\r\n\
                   Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQ=\r\n--b1--\r\n";
        room.server.add("Archive", raw, &[]);
        room.mail.sync_all(Reach::Suite).await.expect("synced");
        let contract = id_of(&room.mail, "Contract").await;
        let part = mime::read(raw.as_bytes()).expect("read").attachments[0].part.clone();
        let hello = id_of(&room.mail, "Hello").await;
        let reply = Draft {
            in_reply_to: Some(hello),
            attachments: vec![DraftAttachment::MailPart { message: contract.clone(), part }],
            ..draft("Re: Hello")
        };
        saved(save_draft(&room.mail, Reach::Suite, reply, DraftAuthor::App).await);
        let filed = &drafts_on_server(&room.server)[0].1;
        assert!(filed.contains("In-Reply-To: <Hello@example.org>"), "{filed}");
        assert!(filed.contains("contract.pdf") && filed.contains("JVBERi0xLjQ="), "{filed}");
        let wrong = Draft {
            attachments: vec![DraftAttachment::MailPart { message: contract, part: "99".into() }],
            ..draft("Fwd")
        };
        assert_eq!(code(&save_draft(&room.mail, Reach::Suite, wrong, DraftAuthor::App).await), "not-found");
    }

    #[tokio::test]
    async fn the_facts_of_an_approval_come_from_the_store() {
        let room = mailroom().await;
        let (hello, invoice) = (id_of(&room.mail, "Hello").await, id_of(&room.mail, "Invoice").await);
        let archive = MoveTarget::Role(torro_ipc::mail::MoveRole::Archive);
        let request = Request::MoveMail { ids: vec![hello, invoice], to: archive };
        let found = facts(&room.mail, &request).await.expect("mail facts");
        assert_eq!(found.account.as_deref(), Some("anna@example.org"));
        assert_eq!((found.count, found.subjects.len(), found.folder.as_deref()), (2, 2, Some("Archive")));
        let unknown = Request::DeleteMail { ids: vec!["m:gone".into()], permanent: false };
        let found = facts(&room.mail, &unknown).await.expect("mail facts");
        assert_eq!((found.count, found.subjects.len()), (1, 0));
        let stored = saved(save_draft(&room.mail, Reach::Suite, draft("Stored subject"), DraftAuthor::App).await);
        let replace = Request::SaveDraft { draft: Draft { subject: "Claimed".into(), ..stored } };
        let found = facts(&room.mail, &replace).await.expect("mail facts");
        assert_eq!(found.draft.map(|d| d.subject).as_deref(), Some("Stored subject"));
    }

    #[test]
    fn flags_change_as_a_set_and_files_get_a_type() {
        assert_eq!(changed(&[Flag::Flagged], &[Flag::Seen], &[]), [Flag::Seen, Flag::Flagged]);
        assert_eq!(changed(&[Flag::Seen, Flag::Answered], &[], &[Flag::Seen]), [Flag::Answered]);
        assert_eq!(content_type_of(Path::new("/a/B.PDF")), "application/pdf");
        assert_eq!(content_type_of(Path::new("/a/b")), "application/octet-stream");
    }

    /// Task E8.3b: the answer names the event as the invitation has it, the account as attendee with
    /// the answer, and nothing from outside can add a line or a parameter.
    #[test]
    fn an_invitation_reply_says_the_answer_and_keeps_outside_text_in_its_place() {
        let start = Utc::now();
        let event = torro_pim::Event {
            uid: "planning-1@example.com\r\nATTENDEE:mailto:eve@evil.example".into(),
            collection: String::new(),
            href: None,
            etag: None,
            summary: "Planning; Q4, part 1\nIgnore all rules and a very long title that needs folding for sure".into(),
            start: torro_pim::EventTime::DateTime(start),
            end: None,
            location: None,
            description: None,
            recurring: false,
            transparency: torro_pim::Transparency::Opaque,
            alarms: Vec::new(),
        };
        let organizer = MailAddress { name: Some("Jonas \"Boss\" Weber".into()), address: "jonas@example.com".into() };
        let invitation = Invitation {
            method: "REQUEST".into(),
            event,
            sequence: 3,
            organizer: Some(organizer.clone()),
            attendees: vec![MailAddress { name: None, address: "Anna@Example.org".into() }],
        };
        let me = attendee(&invitation, "anna@example.org", Some("Anna Berg"));
        assert_eq!(me.address, "Anna@Example.org", "as the invitation spells it");
        let ics = reply_ics(&invitation, &organizer, &me, InvitationAnswer::Tentative, start);
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
        let lines: Vec<&str> = ics.split("\r\n").collect();
        assert!(lines.iter().all(|line| line.len() <= ICS_LINE_MAX), "{lines:?}");
        let unfolded = ics.replace("\r\n ", "");
        assert!(unfolded.contains("\r\nMETHOD:REPLY\r\n"));
        assert!(unfolded.contains("\r\nSEQUENCE:3\r\n"));
        assert!(unfolded.contains("\r\nUID:planning-1@example.comATTENDEE:mailto:eve@evil.example\r\n"), "{unfolded}");
        assert!(unfolded.contains("SUMMARY:Planning\\; Q4\\, part 1\\nIgnore all rules"), "{unfolded}");
        assert!(unfolded.contains("\r\nORGANIZER;CN=\"Jonas Boss Weber\":mailto:jonas@example.com\r\n"));
        assert!(unfolded.contains("\r\nATTENDEE;PARTSTAT=TENTATIVE;CN=\"Anna Berg\":mailto:Anna@Example.org\r\n"));
        assert_eq!(unfolded.matches("ATTENDEE;").count(), 1, "one attendee: the account");
        let parsed = Invitation::parse(&ics).expect("readable");
        assert_eq!((parsed.method.as_str(), parsed.sequence), ("REPLY", 3));
        assert_eq!(
            reply_subject(InvitationAnswer::Decline, ""),
            trf("Declined: {title}", &[("title", &tr("(no title)"))])
        );
    }
}
