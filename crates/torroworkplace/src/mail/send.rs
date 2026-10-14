//! Sending mail (task M3.3): `mail.send_draft`, the copy in the Sent folder, and the copies that
//! wait for a later try.
//!
//! **Built from the draft in the core.** [`prepare`] reads the stored draft from the drafts
//! folder on the server, never text a client sent along, and builds the message with
//! `mime::build`: the account's address and sender name in `From:`, its signature under the text,
//! the draft's `Message-ID` and `Date:`. Building the same draft twice gives the same bytes, so
//! the bytes name what will go out: the **fingerprint** ([`Prepared::fingerprint`], SHA-256 over
//! the envelope and the MIME bytes) pins an approval to what the person was shown. Saving the draft
//! again gives it a new `Message-ID` (and a changed signature changes the bytes too), so an
//! approval of a draft that changed meanwhile sends nothing (`conflict`).
//!
//! **Bcc.** A draft keeps its Bcc recipients in a `Bcc:` header of its own (task M3.2);
//! `mime::read` does not know it, so [`bcc_of`] reads it from the raw draft. The Bcc recipients go
//! into the SMTP envelope only, never into the bytes that are sent. The **copy in the Sent folder**
//! carries the `Bcc:` header again, as Thunderbird and Apple Mail file it: the person must be able
//! to see later whom they sent a blind copy to, and the Sent folder is theirs alone. So the copy is
//! the sent bytes with that one header in front.
//!
//! **The send rule** (task M2.4, `torro_mail_imap::smtp`): the journal under
//! `<data>/mail/send-journal/` holds an entry per send, named by [`Prepared::send_id`] (a hash of
//! the account and the `Message-ID`, so the same draft is the same send). A send the server took
//! (`accepted`) or may have taken (`unknown`) is never sent again by itself: sending the same draft
//! again answers that outcome without asking the server. Only "not accepted" may be tried again.
//! After an acceptance the draft is removed from the drafts folder; after `unknown` it stays, and
//! the person decides (saving it again makes it a new send). An accepted entry whose draft could
//! not be removed stays until its draft is gone; the core clears such entries when it starts
//! ([`forget_finished`], task M3.5).
//!
//! **Copies that wait.** The journal keeps no message, so the copy's bytes are kept under
//! `<data>/mail/sent-copies/` (directory 0700, files 0600, first line the account) from just
//! before the send until the copy is in the Sent folder. A copy that failed is tried again when the
//! core starts and every minute ([`keep_copying`]); only the copy, never the mail. Before it
//! appends, the copy looks for its `Message-ID` in the Sent folder ([`ImapCopy`]), so an `APPEND`
//! whose answer was lost does not leave two. Providers that file sent mail themselves (Gmail,
//! Outlook) get no copy (`folders::keeps_sent_copies`). Removing an account drops its waiting
//! copies ([`Outbox::forget_account`]).
//!
//! **A refused password** at the SMTP server is noted at the account like a refused IMAP sign-in
//! (one password for both): nothing signs in with it again until the account is connected anew.
//!
//! **Who sends what** (security rule 2 of task M2.1): the app key sends any draft; a client key
//! only drafts it wrote itself ([`DraftAuthor`]); any other is `not-found`, checked before anyone
//! is asked ([`client_refusal`]) and again when it runs.
//!
//! Nothing here logs an address, a subject or text: the log names sends by their id only.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_ipc::Response;
use torro_ipc::mail::Delivery;
use torro_mail::{Flag, MailAddress, Role};
use torro_mail_imap::folders::keeps_sent_copies;
use torro_mail_imap::imap::ImapError;
use torro_mail_imap::mime::{self, Built, MimeError, Outgoing, OutgoingAttachment, Reply};
use torro_mail_imap::smtp::{
    self, CopyPlan, CopyStatus, Envelope, Journal, JournalError, SentCopy, SmtpError, SmtpSubmitter, Submission,
    Submit, Submitted,
};

use super::service::{MailError, MailService};
use super::store::{DraftAuthor, MessageRef};
use super::write::{WriteError, locate};
use super::{MailDirs, MailStoreError, make_private, safe_file_name};
use crate::approvals::{DraftFacts, FileFact, MailFacts};
use crate::pim::Reach;

/// How often the copies that wait are tried again.
const COPY_EVERY: Duration = Duration::from_secs(60);

/// Where the bytes of copies that wait are kept, below `<data>/mail/`.
const COPIES_DIR: &str = "sent-copies";

/// Why a draft was not sent. [`SendError::response`] is the answer: fixed codes, texts for people
/// without addresses, subjects or server words.
#[derive(Debug)]
pub(crate) enum SendError {
    /// `not-found`: no such draft in the drafts folder of an account the caller reaches, or a
    /// draft a client did not write.
    NoDraft,
    /// `conflict`: the drafts folder was renumbered on the server since the store last saw it.
    Renumbered,
    /// `conflict`: the message built now is not the one the person approved.
    Changed,
    /// `error`: the draft cannot be built into a message (no recipient, an address that is none).
    Unbuildable(MimeError),
    /// The SMTP server did not take the message, or was not reached; nothing went out.
    NotAccepted(SmtpError),
    /// The send journal could not be used; nothing went out. `busy` while the same draft is being
    /// sent.
    Journal(JournalError),
    /// The account, its connection or its store failed ([`MailError::code`]).
    Mail(MailError),
}

impl SendError {
    /// The answer for a send that failed with this error.
    pub(crate) fn response(&self) -> Response {
        let refused = |code: &str, message: String| Response::Error { code: code.into(), message };
        match self {
            SendError::NoDraft => refused("not-found", tr("No such draft.").into()),
            SendError::Renumbered => {
                refused("conflict", tr("The folder changed on the server. Try again after the next sync.").into())
            }
            SendError::Changed => refused(
                "conflict",
                tr("The draft changed after the approval was asked for. Nothing was sent; ask again.").into(),
            ),
            SendError::Unbuildable(MimeError::NoRecipients) => {
                refused("error", tr("The draft has no recipient.").into())
            }
            SendError::Unbuildable(_) => {
                refused("error", tr("The draft could not be built: an address is not valid.").into())
            }
            SendError::NotAccepted(error) => smtp_refusal(*error),
            SendError::Journal(JournalError::Busy) => refused("busy", tr("This draft is being sent right now.").into()),
            SendError::Journal(_) => {
                refused("error", tr("Sent mail cannot be kept track of on this device. Nothing was sent.").into())
            }
            SendError::Mail(err) => err.response(),
        }
    }
}

/// The answer for a send the SMTP server refused or that never reached it.
fn smtp_refusal(error: SmtpError) -> Response {
    let (code, message) = match error {
        SmtpError::Auth => ("auth", tr("The outgoing mail server refused the password. Nothing was sent.").to_string()),
        SmtpError::Offline | SmtpError::Tls => (
            "offline",
            tr("The outgoing mail server cannot be reached. Nothing was sent; try again later.").to_string(),
        ),
        SmtpError::Unsupported => (
            "unsupported",
            tr("The outgoing mail server does not support what sending needs. Nothing was sent.").to_string(),
        ),
        SmtpError::Rejected { code } => (
            "error",
            trf("The outgoing mail server refused the message ({code}). Nothing was sent.", &[("code", &code)]),
        ),
        SmtpError::InvalidEnvelope => {
            ("error", tr("A recipient is not a valid address. Nothing was sent.").to_string())
        }
        SmtpError::Protocol => {
            ("error", tr("The outgoing mail server answered unexpectedly. Nothing was sent.").to_string())
        }
    };
    Response::Error { code: code.into(), message }
}

impl From<MailError> for SendError {
    fn from(err: MailError) -> SendError {
        SendError::Mail(err)
    }
}

impl From<ImapError> for SendError {
    fn from(err: ImapError) -> SendError {
        SendError::Mail(err.into())
    }
}

impl From<MailStoreError> for SendError {
    fn from(err: MailStoreError) -> SendError {
        SendError::Mail(err.into())
    }
}

impl From<JournalError> for SendError {
    fn from(err: JournalError) -> SendError {
        SendError::Journal(err)
    }
}

// ----- the outbox: journal and copies that wait -----

/// The send journal of the core and the Sent copies that wait to be filed (`<data>/mail/`). One
/// for all accounts, shared by every send ([`MailService::outbox`]): the journal refuses a second
/// send of the same draft while one runs only within one `Journal`.
#[derive(Debug)]
pub(crate) struct Outbox {
    journal: Journal,
    /// `<data>/mail/sent-copies/`.
    copies: PathBuf,
    /// Tests: the SMTP server every send goes to instead of the account's ([`Outbox::use_fake`]).
    #[cfg(test)]
    fake: std::sync::Mutex<Option<Arc<tests::FakeSmtp>>>,
}

impl Outbox {
    /// Opens (and creates) the journal and the folder for copies below `dirs.data` (`<data>/mail/`),
    /// both 0700 on Unix. Blocks: call it on a blocking thread.
    pub(crate) fn open(dirs: &MailDirs) -> Result<Outbox, MailStoreError> {
        // `Journal::open` takes the data directory itself and adds `mail/send-journal`.
        let data = dirs.data.parent().map_or_else(|| dirs.data.clone(), Path::to_path_buf);
        let journal = Journal::open(&data).map_err(|err| MailStoreError::Io(io::Error::other(err.to_string())))?;
        let copies = dirs.data.join(COPIES_DIR);
        crate::clients::create_private_dir(&copies)?;
        Ok(Outbox {
            journal,
            copies,
            #[cfg(test)]
            fake: std::sync::Mutex::new(None),
        })
    }

    /// The file of the copy of send `id`: the id in hex, so any id is a plain file name.
    fn copy_file(&self, id: &str) -> PathBuf {
        self.copies.join(format!("{}.eml", hex(id.as_bytes())))
    }

    /// Keeps `bytes`, the Sent copy of send `id` of `account`, until it is filed: first line the
    /// account (an account id holds no line break), then the bytes. Written to a temporary file
    /// (0600), flushed and renamed, so a crash leaves the whole copy or none.
    fn keep(&self, id: &str, account: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.copy_file(id);
        let temporary = path.with_extension("tmp");
        let mut options = fs::OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(account.as_bytes())?;
        file.write_all(b"\n")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        make_private(&temporary)?;
        fs::rename(&temporary, &path)
    }

    /// The kept copy of send `id`: its account and bytes; `None` when there is none.
    fn kept(&self, id: &str) -> io::Result<Option<(String, Vec<u8>)>> {
        let content = match fs::read(self.copy_file(id)) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let Some(end) = content.iter().position(|&byte| byte == b'\n') else { return Ok(None) };
        let account = String::from_utf8_lossy(&content[..end]).into_owned();
        Ok(Some((account, content[end + 1..].to_vec())))
    }

    /// Deletes the kept copy of send `id`; one that is not there is fine.
    fn discard(&self, id: &str) {
        match fs::remove_file(self.copy_file(id)) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("torroworkplace: cannot delete the kept copy of send {id}: {}", err.kind()),
        }
    }

    /// Drops every copy that waits for `account` and its journal entry: the account was removed
    /// (`MailService::remove`), so there is no Sent folder to file it in, and its drafts are gone
    /// with it, so no send can come again. Blocks briefly (reads the first line of each copy).
    pub(crate) fn forget_account(&self, account: &str) {
        let Ok(entries) = fs::read_dir(&self.copies) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()).and_then(unhex) else { continue };
            let mut first = String::new();
            let read = fs::File::open(&path)
                .and_then(|file| io::BufRead::read_line(&mut io::BufReader::new(file), &mut first));
            if read.is_ok() && first.trim_end_matches('\n') == account {
                self.discard(&id);
                if let Err(err) = self.journal.forget(&id) {
                    eprintln!("torroworkplace: cannot clear send {id} from the journal: {err}");
                }
            }
        }
    }

    /// Tests: every send goes to `fake` from now on.
    #[cfg(test)]
    pub(crate) fn use_fake(&self, fake: Arc<tests::FakeSmtp>) {
        *self.fake.lock().expect("the fake's slot") = Some(fake);
    }

    /// Who hands the mail of `account` to a server: the account's SMTP server
    /// ([`MailService::smtp`]), in tests the fake.
    async fn submitter(&self, mail: &MailService, account: &str) -> Result<Submitter, MailError> {
        #[cfg(test)]
        if let Some(fake) = self.fake.lock().expect("the fake's slot").clone() {
            return Ok(Submitter::Fake(fake));
        }
        Ok(Submitter::Smtp(mail.smtp(account).await?))
    }
}

/// The SMTP side of one send.
enum Submitter {
    Smtp(SmtpSubmitter),
    /// The journal holds a final outcome already: `smtp::send` answers it without asking a
    /// server, so none is looked up (it may well be unreachable now).
    Done,
    #[cfg(test)]
    Fake(Arc<tests::FakeSmtp>),
}

impl Submit for Submitter {
    async fn submit(&self, envelope: &Envelope, message: &[u8]) -> Submitted {
        match self {
            Submitter::Smtp(smtp) => smtp.submit(envelope, message).await,
            // Never asked (see `Done`); "surely not taken" would be the honest answer if it were.
            Submitter::Done => Submitted::NotAccepted(SmtpError::Protocol),
            #[cfg(test)]
            Submitter::Fake(fake) => fake.submit(envelope, message).await,
        }
    }
}

/// Files a Sent copy in the Sent folder of `account` over its command connection: `APPEND` with
/// `\Seen`, unless a message with the same `Message-ID` is there already (an earlier `APPEND`
/// whose answer was lost).
struct ImapCopy<'a> {
    mail: &'a MailService,
    account: String,
}

impl SentCopy for ImapCopy<'_> {
    type Error = SendError;

    async fn append_sent(&self, message: &[u8]) -> Result<(), SendError> {
        let store = self.mail.store(&self.account).await?;
        let sent = store
            .run(|store| match store.folder_by_role(Role::Sent)? {
                Some(folder) => store.resolve_folder(&folder.id),
                None => Ok(None),
            })
            .await?
            .ok_or_else(|| SendError::Mail(MailError::Server(ImapError::NotFound("no sent folder".into()))))?;
        let mut command = self.mail.command(&self.account).await?;
        let session = command.session();
        if let Some(id) = mime::read(message).ok().and_then(|read| read.message_id) {
            session.select(&sent.path).await?;
            // `TEXT` finds the id in the headers too; the headers then say whether it is this one.
            let uids = session.uid_search_text(&id, None, None).await?;
            if !uids.is_empty() {
                let headers = session.fetch_headers(&uids).await?;
                let filed = headers.iter().any(|header| {
                    mime::read(&header.header).ok().and_then(|read| read.message_id).as_deref() == Some(&id)
                });
                if filed {
                    command.release();
                    return Ok(());
                }
            }
        }
        session.append(&sent.path, message, &[Flag::Seen], false).await?;
        command.release();
        Ok(())
    }
}

// ----- preparing a draft -----

/// A draft made ready to send: where it is, the message built from it as the core stores it, and
/// what its approval shows.
#[derive(Debug)]
pub(crate) struct Prepared {
    draft: MessageRef,
    /// The account's address, for the approval.
    address: String,
    /// The account's IMAP host: whether the provider files sent mail itself.
    imap_host: String,
    /// The `Message-ID` the message goes out with.
    message_id: String,
    built: Built,
    /// The Bcc recipients, for the `Bcc:` header of the Sent copy.
    bcc: Vec<String>,
    facts: DraftFacts,
}

impl Prepared {
    /// What will go out, in 64 hex digits: SHA-256 over the envelope (sender and every recipient,
    /// Bcc too) and the MIME bytes. The same draft gives the same fingerprint; a draft saved again
    /// gets a new `Message-ID` and so another one.
    pub(crate) fn fingerprint(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.built.envelope.from.as_bytes());
        for recipient in &self.built.envelope.recipients {
            hash.update([0u8]);
            hash.update(recipient.as_bytes());
        }
        hash.update([0u8, 0u8]);
        hash.update(&self.built.bytes);
        hex(&hash.finalize())
    }

    /// What the approval of sending it shows (`approvals::MailFacts`, rules there): the account's
    /// address and the draft as stored, counted from the built message.
    pub(crate) fn facts(&self) -> MailFacts {
        MailFacts {
            account: Some(self.address.clone()),
            count: 1,
            draft: Some(self.facts.clone()),
            ..MailFacts::default()
        }
    }

    /// The send's id in the journal: the same draft (account and `Message-ID`) is the same send.
    fn send_id(&self) -> String {
        send_id_of(&self.draft.account, &self.message_id)
    }

    /// The copy for the Sent folder: the sent bytes with the `Bcc:` header in front when there are
    /// Bcc recipients (see the module documentation).
    fn sent_copy(&self) -> Vec<u8> {
        if self.bcc.is_empty() {
            return self.built.bytes.clone();
        }
        // Addresses only, checked by `mime::build` (no white space, no line breaks); folded so no
        // line grows past the limit of RFC 5322, as the draft had it.
        let mut copy = format!("Bcc: {}\r\n", self.bcc.join(",\r\n ")).into_bytes();
        copy.extend_from_slice(&self.built.bytes);
        copy
    }
}

/// Reads the draft `id` from the drafts folder of its account and builds the message to send
/// ([`Prepared`]). `reach` decides which accounts the id is looked up in, `author` whose drafts may
/// be sent ([`find_draft`]). Talks to the IMAP server (the draft's bytes), never to SMTP.
pub(crate) async fn prepare(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    id: &str,
) -> Result<Prepared, SendError> {
    let (draft, stored_date) = find_draft(mail, reach, author, id).await?;
    let raw = fetch_draft(mail, &draft).await?;
    let config = mail.config(&draft.account).await?;
    let settings = mail.settings(&draft.account).await?;
    let read = mime::read(&raw).map_err(SendError::Unbuildable)?;
    let bcc = bcc_of(&raw);
    let mut attachments = Vec::with_capacity(read.attachments.len());
    for info in &read.attachments {
        let found = mime::attachment(&raw, &info.part).map_err(SendError::Unbuildable)?;
        attachments.push(OutgoingAttachment {
            filename: safe_file_name(found.filename.as_deref().unwrap_or("")),
            content_type: found.content_type,
            bytes: found.bytes,
        });
    }
    let domain = config.address.rsplit_once('@').map_or("localhost", |(_, domain)| domain);
    // A draft of another mail program may have none; one made from its bytes stays the same.
    let message_id = read
        .message_id
        .clone()
        .filter(|id| plain_id(id))
        .unwrap_or_else(|| format!("{}@{domain}", &hex(&Sha256::digest(&raw))[..32]));
    let reply = read.in_reply_to.clone().filter(|id| plain_id(id)).map(|in_reply_to| {
        let mut references: Vec<String> = read.references.iter().filter(|id| plain_id(id)).cloned().collect();
        if references.is_empty() {
            references.push(in_reply_to.clone());
        }
        Reply { in_reply_to, references }
    });
    let outgoing = Outgoing {
        from: MailAddress {
            name: Some(config.sender_name.trim().to_string()).filter(|name| !name.is_empty()),
            address: config.address.clone(),
        },
        to: read.to.clone(),
        cc: read.cc.clone(),
        bcc: bcc.clone(),
        subject: read.subject.clone(),
        body: read.text.clone(),
        signature: settings.signature.clone(),
        reply,
        attachments,
        calendar: None,
        message_id: message_id.clone(),
        // The draft's own date, so building it again gives the same bytes.
        date: read.date.unwrap_or(stored_date),
    };
    let built = mime::build(&outgoing).map_err(SendError::Unbuildable)?;
    let first = outgoing.to.iter().chain(&outgoing.cc).chain(&outgoing.bcc).next();
    let facts = DraftFacts {
        subject: read.subject.clone(),
        first_recipient: first.map(|address| address.address.trim().to_string()),
        recipients: built.envelope.recipients.len(),
        attachments: outgoing.attachments.len(),
        text_start: read.snippet(),
        bcc: outgoing.bcc.iter().map(|address| address.address.trim().to_string()).collect(),
        to: outgoing.to.clone(),
        cc: outgoing.cc.clone(),
        files: outgoing
            .attachments
            .iter()
            .map(|file| FileFact { name: file.filename.clone(), size: file.bytes.len() as u64 })
            .collect(),
    };
    Ok(Prepared {
        draft,
        address: config.address,
        imap_host: config.imap.host,
        message_id,
        built,
        bcc: bcc.into_iter().map(|address| address.address.trim().to_string()).collect(),
        facts,
    })
}

/// The draft `id` and its date as the store holds it: in an account `reach` sees, in that
/// account's drafts folder, and for a client one it wrote itself ([`DraftAuthor`]); else
/// [`SendError::NoDraft`]. Reads the store only.
async fn find_draft(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    id: &str,
) -> Result<(MessageRef, DateTime<Utc>), SendError> {
    let draft = match locate(mail, reach, &[id]).await {
        Ok(found) => found.into_iter().next().ok_or(SendError::NoDraft)?,
        Err(WriteError::Mail(err)) => return Err(SendError::Mail(err)),
        Err(_) => return Err(SendError::NoDraft),
    };
    let store = mail.store(&draft.account).await?;
    let (message, folder, author) = (draft.id.clone(), draft.folder.clone(), author.clone());
    let date = store
        .run(move |store| {
            let in_drafts = store.folder_by_role(Role::Drafts)?.is_some_and(|drafts| drafts.id == folder);
            let allowed = match &author {
                DraftAuthor::App => true,
                client @ DraftAuthor::Client(_) => store.draft_author(&message)?.as_ref() == Some(client),
            };
            match in_drafts && allowed {
                true => Ok(store.summary(&message)?.map(|summary| summary.date)),
                false => Ok(None),
            }
        })
        .await?
        .ok_or(SendError::NoDraft)?;
    Ok((draft, date))
}

/// The raw bytes of `draft` from the server (`BODY.PEEK[]`).
async fn fetch_draft(mail: &MailService, draft: &MessageRef) -> Result<Vec<u8>, SendError> {
    let mut command = mail.command(&draft.account).await?;
    let selected = command.session().select(&draft.folder_path).await?;
    if selected.uid_validity != draft.uidvalidity {
        command.release();
        return Err(SendError::Renumbered);
    }
    let raw = command.session().fetch_message(draft.uid).await?;
    command.release();
    raw.ok_or(SendError::NoDraft)
}

/// The Bcc recipients of the raw draft `raw`: every `Bcc:` header of its header block, unfolded,
/// read as an address list (by `mime::read`, as if it were `To:`, so quoted names with commas
/// stay whole).
fn bcc_of(raw: &[u8]) -> Vec<MailAddress> {
    let end = raw.windows(4).position(|window| window == b"\r\n\r\n").unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..end]).replace("\r\n", "\n");
    let mut values: Vec<String> = Vec::new();
    let mut in_bcc = false;
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if in_bcc && let Some(last) = values.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
            continue;
        }
        in_bcc = false;
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("bcc")
        {
            in_bcc = true;
            values.push(value.trim().to_string());
        }
    }
    values
        .iter()
        .filter(|value| !value.is_empty())
        .filter_map(|value| mime::read(format!("To: {value}\r\n\r\n").as_bytes()).ok())
        .flat_map(|read| read.to)
        .collect()
}

/// A message id `mime::build` takes: not empty, at most 250 bytes, no white space, control
/// characters or angle brackets.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 250
        && !id.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>'))
}

/// The journal's id of sending the draft with `message_id` from `account`: the same draft is the
/// same send ([`Prepared::send_id`]).
fn send_id_of(account: &str, message_id: &str) -> String {
    let digest = Sha256::digest(format!("{account}\n{message_id}").as_bytes());
    format!("send:{}", &hex(&digest)[..40])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The text [`hex`] made of it; `None` for anything else.
fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..text.len())
        .step_by(2)
        .map(|at| text.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect::<Option<Vec<u8>>>()?;
    String::from_utf8(bytes).ok()
}

// ----- the checks of the core -----

/// What the core checks for `mail.send_draft` of the client `client` before anyone is asked to
/// approve it, and again when an approved one runs; `None` when it may go on, and for every other
/// request. The draft must be in an account `reach` sees, in its drafts folder, and written by this
/// client (security rule 2 of task M2.1): else `not-found`, the person's drafts too. Reads the
/// store only.
pub(crate) async fn client_refusal(
    mail: &MailService,
    client: &str,
    reach: Reach<'_>,
    request: &Request,
) -> Option<Response> {
    let Request::SendDraft { id } = request else { return None };
    find_draft(mail, reach, &DraftAuthor::Client(client.to_string()), id).await.err().map(|err| err.response())
}

/// What the approval of sending the draft `id` (written by `author`, in an account `reach` sees)
/// shows ([`Prepared::facts`]) and the fingerprint it is pinned to ([`Prepared::fingerprint`]).
/// The draft is read from the server for it; when that fails the answer is the error, and nobody
/// should be asked about a message that cannot be built.
pub(crate) async fn approval(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    id: &str,
) -> Result<(MailFacts, String), Box<Response>> {
    match prepare(mail, reach, author, id).await {
        Ok(prepared) => Ok((prepared.facts(), prepared.fingerprint())),
        Err(err) => Err(Box::new(err.response())),
    }
}

// ----- sending -----

/// `mail.send_draft`: sends the draft `id` (in an account `reach` sees; a client's only when
/// `author` wrote it) under the send rule, files the Sent copy and removes the draft. With
/// `approved`, the message built now must have that fingerprint (the one the person approved),
/// else `conflict` and nothing is sent.
///
/// Answers [`Response::MailSent`]: `accepted`, also when an earlier send of the same draft was
/// accepted (the server is not asked again, the draft is removed if it is still there), or
/// `unknown` when the server may have it (never sent again by itself; the draft stays). A failed
/// copy does not fail the send: it is filed later ([`keep_copying`]). Errors: `not-found`,
/// `conflict`, `busy`, `auth`, `offline`, `unsupported`, `error` ([`SendError::response`]); none
/// of them sent anything.
pub(crate) async fn send_draft(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    id: &str,
    approved: Option<&str>,
) -> Response {
    let sent = send_with(mail, reach, author, id, approved, |account| ImapCopy { mail, account: account.to_string() });
    sent.await.unwrap_or_else(|err| err.response())
}

/// [`send_draft`] with the Sent copy filed through what `copier` makes for the draft's account
/// (tests put in one that fails).
async fn send_with<C: SentCopy>(
    mail: &MailService,
    reach: Reach<'_>,
    author: &DraftAuthor,
    id: &str,
    approved: Option<&str>,
    copier: impl FnOnce(&str) -> C,
) -> Result<Response, SendError> {
    let prepared = prepare(mail, reach, author, id).await?;
    if approved.is_some_and(|approved| approved != prepared.fingerprint()) {
        return Err(SendError::Changed);
    }
    let parcel = Parcel {
        account: &prepared.draft.account,
        imap_host: &prepared.imap_host,
        send_id: prepared.send_id(),
        built: &prepared.built,
        copy: prepared.sent_copy(),
    };
    match deliver(mail, &parcel, copier).await? {
        Delivered::Accepted { filed } => {
            let removed = match remove_draft(mail, &prepared.draft).await {
                Ok(()) => true,
                Err(err) => {
                    eprintln!("torroworkplace: a sent draft could not be removed: {}", err_code(&err));
                    false
                }
            };
            // Done with: the draft is gone, so the same send cannot come again.
            if filed && removed {
                forget_send(mail, &parcel.send_id).await;
            }
            Ok(Response::MailSent { delivery: Delivery::Accepted })
        }
        Delivered::Unknown => Ok(Response::MailSent { delivery: Delivery::Unknown }),
    }
}

/// Sends `outgoing`, a message the core made itself and that is no draft (task E8.3b: the answer to
/// an invitation), from `account` under the send rule, as [`send_draft`] sends a draft: the journal
/// entry named by the account and the `Message-ID` (the same message is never sent twice by
/// itself), the account's SMTP server, the copy in the Sent folder (filed later when that fails).
/// Nothing is checked here: the caller has checked who may send it (`Rights::outbound` and the
/// approval in the core).
///
/// Answers how it went ([`Delivery`]); errors as for [`send_draft`] ([`SendError::response`]),
/// none of them sent anything.
pub(crate) async fn send_message(
    mail: &MailService,
    account: &str,
    outgoing: &Outgoing,
) -> Result<Delivery, SendError> {
    let config = mail.config(account).await?;
    let built = mime::build(outgoing).map_err(SendError::Unbuildable)?;
    let parcel = Parcel {
        account,
        imap_host: &config.imap.host,
        send_id: send_id_of(account, &outgoing.message_id),
        built: &built,
        copy: built.bytes.clone(),
    };
    match deliver(mail, &parcel, |account| ImapCopy { mail, account: account.to_string() }).await? {
        Delivered::Accepted { filed } => {
            // No draft to wait for: once the copy is filed, the send is done with.
            if filed {
                forget_send(mail, &parcel.send_id).await;
            }
            Ok(Delivery::Accepted)
        }
        Delivered::Unknown => Ok(Delivery::Unknown),
    }
}

/// One message on its way out ([`deliver`]): from which account, whether its provider files sent
/// mail itself, its id in the journal, what is sent and the copy for the Sent folder.
struct Parcel<'a> {
    account: &'a str,
    /// The account's IMAP host: whether the provider files sent mail itself.
    imap_host: &'a str,
    send_id: String,
    built: &'a Built,
    copy: Vec<u8>,
}

/// What [`deliver`] did with a message the server did not refuse.
enum Delivered {
    /// The server took it; `filed`: its copy is in the Sent folder or none is needed.
    Accepted { filed: bool },
    /// The server may have it: it is never sent again by itself.
    Unknown,
}

/// Hands `parcel` to the SMTP server of its account under the send rule and files its copy
/// through what `copier` makes: a send the journal knows the outcome of is not sent again, the
/// copy's bytes are kept before the send (so a crash after an acceptance still finds them), and a
/// refused password is noted at the account. "Not accepted" is the error
/// ([`SendError::NotAccepted`]): nothing went out.
async fn deliver<C: SentCopy>(
    mail: &MailService,
    parcel: &Parcel<'_>,
    copier: impl FnOnce(&str) -> C,
) -> Result<Delivered, SendError> {
    let outbox = mail.outbox().await?;
    let (send_id, account) = (&parcel.send_id, parcel.account);
    let plan = match keeps_sent_copies(parcel.imap_host) {
        true => CopyPlan::ProviderKeepsIt,
        false => CopyPlan::Append,
    };
    // A send the journal knows the outcome of needs no server: it is not asked again.
    let settled = outbox
        .journal
        .entry(send_id)?
        .is_some_and(|entry| matches!(entry.submission, Submission::Accepted | Submission::Unknown));
    let smtp = match settled {
        true => Submitter::Done,
        false => outbox.submitter(mail, account).await?,
    };
    if !settled && plan == CopyPlan::Append {
        // Before the send: the journal keeps no message, and a crash after an acceptance must
        // still find the copy's bytes.
        outbox.keep(send_id, account, &parcel.copy).map_err(|err| SendError::Mail(MailError::Store(err.into())))?;
    }
    let envelope = &parcel.built.envelope;
    let report = match smtp::send(&outbox.journal, send_id, envelope, &parcel.built.bytes, &smtp, plan).await {
        Ok(report) => report,
        Err(err) => {
            if !settled {
                outbox.discard(send_id);
            }
            return Err(err.into());
        }
    };
    if report.journal_failed {
        eprintln!("torroworkplace: the send journal could not note the outcome of send {send_id}");
    }
    match report.submission {
        Submission::Accepted => {
            let filed = match report.copy {
                CopyStatus::Pending => file_copy(&outbox, send_id, &parcel.copy, &copier(account), true).await,
                CopyStatus::Saved | CopyStatus::NotNeeded => true,
            };
            if filed {
                outbox.discard(send_id);
            }
            Ok(Delivered::Accepted { filed })
        }
        Submission::NotAccepted => {
            outbox.discard(send_id);
            let error = report.error.unwrap_or(SmtpError::Protocol);
            if error == SmtpError::Auth {
                // One password for IMAP and SMTP: the account shows it, and nothing tries it again
                // until the person connects the account anew, so no server locks it.
                mail.note_password_refused(account).await;
            }
            Err(SendError::NotAccepted(error))
        }
        // "Submitting" is never reported; read as unknown like the journal does.
        Submission::Unknown | Submission::Submitting => {
            outbox.discard(send_id);
            Ok(Delivered::Unknown)
        }
    }
}

/// Clears the finished send `id` from the journal.
async fn forget_send(mail: &MailService, id: &str) {
    let Ok(outbox) = mail.outbox().await else { return };
    if let Err(err) = outbox.journal.forget(id) {
        eprintln!("torroworkplace: cannot clear send {id} from the journal: {err}");
    }
}

/// Files the copy of the accepted send `id` (`bytes`, unless the outbox kept the ones sent before)
/// through `folder`; `true` when it is in the Sent folder or none is needed. `say`: a failure goes
/// to the log (once, when the mail was sent; the tries every minute stay quiet).
async fn file_copy<C: SentCopy>(outbox: &Outbox, id: &str, bytes: &[u8], folder: &C, say: bool) -> bool {
    let kept = outbox.kept(id).ok().flatten().map(|(_, kept)| kept);
    match smtp::save_copy(&outbox.journal, id, kept.as_deref().unwrap_or(bytes), folder).await {
        Ok(report) if report.status != CopyStatus::Pending => true,
        Ok(_) => {
            if say {
                eprintln!("torroworkplace: the sent copy of send {id} waits for a later try");
            }
            false
        }
        Err(err) => {
            if say {
                eprintln!("torroworkplace: the sent copy of send {id} waits for a later try: {err}");
            }
            false
        }
    }
}

/// Removes the sent `draft` from its folder on the server and from the store, and forgets who
/// wrote it. Nothing is removed when the folder was renumbered meanwhile.
async fn remove_draft(mail: &MailService, draft: &MessageRef) -> Result<(), SendError> {
    let mut command = mail.command(&draft.account).await?;
    let selected = command.session().select(&draft.folder_path).await?;
    if selected.uid_validity != draft.uidvalidity {
        command.release();
        return Err(SendError::Renumbered);
    }
    command.session().delete_messages(&[draft.uid]).await?;
    command.release();
    let store = mail.store(&draft.account).await?;
    let id = draft.id.clone();
    store
        .run(move |store| {
            store.delete_message(&id)?;
            store.forget_draft(&id)?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// The error code of `err`, for the log.
fn err_code(err: &SendError) -> String {
    match err.response() {
        Response::Error { code, .. } => code,
        _ => "error".into(),
    }
}

// ----- copies that wait -----

/// Files every Sent copy that waits (accepted, copy still missing) and returns how many are filed
/// now. Only copies: no mail is sent. A copy whose send runs right now, or whose account cannot be
/// reached, waits for the next round.
pub(crate) async fn copy_pending(mail: &MailService) -> usize {
    let Ok(outbox) = mail.outbox().await else { return 0 };
    let pending = match outbox.journal.pending_copies() {
        Ok(pending) => pending,
        Err(err) => {
            eprintln!("torroworkplace: cannot read the send journal: {err}");
            return 0;
        }
    };
    let mut filed = 0;
    for id in pending {
        // Without its bytes a copy cannot be made; the entry stays, nothing else is harmed.
        let Ok(Some((account, bytes))) = outbox.kept(&id) else { continue };
        let folder = ImapCopy { mail, account };
        if file_copy(&outbox, &id, &bytes, &folder, false).await {
            outbox.discard(&id);
            filed += 1;
        }
    }
    filed
}

/// Forgets the journal entries of sends that are done with but were left behind (task M3.5):
/// accepted, the Sent copy filed or not needed, and no draft of theirs left in any account. They
/// stay when removing the draft or forgetting the entry failed after the send, and nothing else
/// would ever clear them. Returns how many were forgotten.
///
/// An entry names its send by a hash of the account and the draft's `Message-ID` only, so the
/// drafts of every account are looked up in the stores ([`send_id_of`]). Whenever that cannot be
/// told for sure, nothing is forgotten: a mail account in `accounts.json` without a store (mail
/// off, not loaded), a store without a drafts folder (not synced yet) or a draft without a usable
/// `Message-ID`. Forgetting the entry of a draft that is still there would let it be sent twice.
pub(crate) async fn forget_finished(mail: &MailService) -> usize {
    let Ok(outbox) = mail.outbox().await else { return 0 };
    let finished: Vec<String> = match outbox.journal.entries() {
        Ok(entries) => entries
            .into_iter()
            .filter(|entry| entry.submission == Submission::Accepted && entry.copy != CopyStatus::Pending)
            .map(|entry| entry.id)
            .collect(),
        Err(err) => {
            eprintln!("torroworkplace: cannot read the send journal: {err}");
            return 0;
        }
    };
    if finished.is_empty() {
        return 0;
    }
    let Some(drafts) = draft_sends(mail).await else { return 0 };
    let mut forgotten = 0;
    for id in finished.into_iter().filter(|id| !drafts.contains(id)) {
        match outbox.journal.forget(&id) {
            Ok(()) => forgotten += 1,
            Err(err) => eprintln!("torroworkplace: cannot clear send {id} from the journal: {err}"),
        }
    }
    forgotten
}

/// The send ids of every draft of every mail account ([`send_id_of`]); `None` when one of them
/// cannot be told (see [`forget_finished`]).
async fn draft_sends(mail: &MailService) -> Option<std::collections::HashSet<String>> {
    let stored = mail.stored_accounts().await.ok()?;
    let stores = mail.stores(Reach::Suite).await;
    let every_store = stored
        .iter()
        .filter(|account| account.kind == torro_core::account::AccountKind::Mail)
        .all(|account| stores.iter().any(|(id, _)| *id == account.id));
    if !every_store {
        return None;
    }
    let mut sends = std::collections::HashSet::new();
    for (account, store) in stores {
        let ids = store
            .run(|store| {
                let Some(drafts) = store.folder_by_role(Role::Drafts)? else { return Ok(None) };
                let mut ids = Vec::new();
                for summary in store.list_messages(&drafts.id, u32::MAX, 0)? {
                    let message_id = store.resolve_message(&summary.id)?.and_then(|found| found.rfc_message_id);
                    match message_id.filter(|id| plain_id(id)) {
                        Some(id) => ids.push(id),
                        None => return Ok(None),
                    }
                }
                Ok(Some(ids))
            })
            .await
            .ok()??;
        sends.extend(ids.iter().map(|id| send_id_of(&account, id)));
    }
    Some(sends)
}

/// [`forget_finished`] once, then [`copy_pending`] now and every minute, for as long as the core
/// runs (`MailService::start`).
pub(crate) async fn keep_copying(mail: Arc<MailService>) {
    let forgotten = forget_finished(&mail).await;
    if forgotten > 0 {
        eprintln!("torroworkplace: cleared {forgotten} finished sends from the send journal");
    }
    loop {
        copy_pending(&mail).await;
        tokio::time::sleep(COPY_EVERY).await;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::mail::service::tests::service_in;
    use crate::mail::sync::fake::FakeImap;
    use crate::mail::write::save_draft;
    use crate::mail::write::tests::{ANNA, draft, drafts_on_server, mail_server, on_server, saved};

    /// An SMTP server in memory: answers every send with what it was told and keeps what it got.
    #[derive(Debug)]
    pub(crate) struct FakeSmtp {
        answer: Mutex<Submitted>,
        sent: Mutex<Vec<(Envelope, Vec<u8>)>>,
    }

    impl FakeSmtp {
        pub(crate) fn answering(answer: Submitted) -> Arc<FakeSmtp> {
            Arc::new(FakeSmtp { answer: Mutex::new(answer), sent: Mutex::new(Vec::new()) })
        }

        /// Every message handed over so far, with its envelope.
        pub(crate) fn sent(&self) -> Vec<(Envelope, Vec<u8>)> {
            self.sent.lock().expect("the fake's record").clone()
        }

        pub(crate) fn answer(&self, answer: Submitted) {
            *self.answer.lock().expect("the fake's answer") = answer;
        }

        pub(crate) async fn submit(&self, envelope: &Envelope, message: &[u8]) -> Submitted {
            self.sent.lock().expect("the fake's record").push((envelope.clone(), message.to_vec()));
            *self.answer.lock().expect("the fake's answer")
        }
    }

    /// A Sent folder that is never reached.
    struct NoCopy;

    impl SentCopy for NoCopy {
        type Error = ();

        async fn append_sent(&self, _message: &[u8]) -> Result<(), ()> {
            Err(())
        }
    }

    struct Room {
        _dir: tempfile::TempDir,
        server: FakeImap,
        mail: MailService,
        smtp: Arc<FakeSmtp>,
    }

    /// Anna's account on a fake IMAP server, synced, with a fake SMTP server that accepts.
    async fn room() -> Room {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = mail_server(dir.path(), &secrets).await;
        let mail = service_in(dir.path(), &secrets);
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let smtp = FakeSmtp::answering(Submitted::Accepted);
        mail.outbox().await.expect("an outbox").use_fake(Arc::clone(&smtp));
        Room { _dir: dir, server, mail, smtp }
    }

    /// Anna's draft "Lunch" to Bernd, Cc Dora and Bcc Carla, saved by `author`; its id.
    async fn lunch(room: &Room, author: DraftAuthor) -> String {
        let mut lunch = draft("Lunch");
        lunch.cc = vec![MailAddress { name: None, address: "dora@example.com".into() }];
        lunch.bcc = vec![MailAddress { name: Some("Carla".into()), address: "carla@example.net".into() }];
        saved(save_draft(&room.mail, Reach::Suite, lunch, author).await).id.expect("an id")
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "ok",
        }
    }

    fn sent_copies(server: &FakeImap) -> Vec<(Vec<String>, String)> {
        server.messages("Sent").into_iter().map(|(_, flags, raw)| (flags, raw)).collect()
    }

    #[tokio::test]
    async fn the_person_sends_a_draft_bcc_in_the_envelope_only_and_the_copy_in_sent() {
        let room = room().await;
        let id = lunch(&room, DraftAuthor::App).await;

        let sent = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(sent, Response::MailSent { delivery: Delivery::Accepted });
        let handed = room.smtp.sent();
        assert_eq!(handed.len(), 1);
        let (envelope, data) = &handed[0];
        assert_eq!(envelope.from, "anna@example.org");
        assert_eq!(envelope.recipients, ["bernd@example.com", "dora@example.com", "carla@example.net"]);
        let data = String::from_utf8_lossy(data);
        assert!(!data.contains("carla") && !data.to_ascii_lowercase().contains("bcc:"), "no Bcc in DATA: {data}");
        assert!(data.contains("Subject: Lunch") && data.contains("Dear Bernd,"), "{data}");

        let copies = sent_copies(&room.server);
        assert_eq!(copies.len(), 1, "one copy in Sent");
        assert_eq!(copies[0].0, ["\\Seen"]);
        assert!(copies[0].1.starts_with("Bcc: carla@example.net\r\n"), "the copy keeps the Bcc: {}", copies[0].1);
        assert!(copies[0].1.ends_with(data.as_ref()), "the copy is the sent message");
        assert!(drafts_on_server(&room.server).is_empty(), "the draft is gone");
        let store = room.mail.store(ANNA).await.expect("store");
        let gone = id.clone();
        let (summary, author) =
            store.run(move |store| Ok((store.summary(&gone)?, store.draft_author(&gone)?))).await.expect("read");
        assert_eq!((summary, author), (None, None));
        let outbox = room.mail.outbox().await.expect("outbox");
        assert!(outbox.journal.entries().expect("journal").is_empty(), "done with");
        assert_eq!(fs::read_dir(&outbox.copies).expect("copies").count(), 0, "no copy waits");

        let again = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(code(&again), "not-found");
        assert_eq!(room.smtp.sent().len(), 1);
    }

    #[tokio::test]
    async fn an_unknown_outcome_is_never_sent_again_and_the_draft_stays() {
        let room = room().await;
        room.smtp.answer(Submitted::Unknown(SmtpError::Offline));
        let id = lunch(&room, DraftAuthor::App).await;

        let first = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(first, Response::MailSent { delivery: Delivery::Unknown });
        room.smtp.answer(Submitted::Accepted);
        let second = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(second, Response::MailSent { delivery: Delivery::Unknown });
        assert_eq!(room.smtp.sent().len(), 1, "the server was asked once");
        assert!(sent_copies(&room.server).is_empty(), "no copy of a send that may not be out");
        assert_eq!(on_server(&room.server, "Drafts"), ["Lunch"], "the person decides");
    }

    #[tokio::test]
    async fn a_refused_send_may_be_tried_again() {
        let room = room().await;
        room.smtp.answer(Submitted::NotAccepted(SmtpError::Rejected { code: 550 }));
        let id = lunch(&room, DraftAuthor::App).await;
        let refused = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(code(&refused), "error");
        let outbox = room.mail.outbox().await.expect("outbox");
        assert_eq!(fs::read_dir(&outbox.copies).expect("copies").count(), 0, "no copy of what did not go");
        room.smtp.answer(Submitted::Accepted);
        let sent = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(sent, Response::MailSent { delivery: Delivery::Accepted });
        assert_eq!(room.smtp.sent().len(), 2);
        assert_eq!(sent_copies(&room.server).len(), 1);
    }

    #[tokio::test]
    async fn a_failed_copy_is_filed_later_without_sending_again() {
        let room = room().await;
        let id = lunch(&room, DraftAuthor::App).await;
        let sent = send_with(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None, |_| NoCopy).await;
        assert_eq!(sent.expect("sent"), Response::MailSent { delivery: Delivery::Accepted });
        assert!(sent_copies(&room.server).is_empty());
        assert!(drafts_on_server(&room.server).is_empty(), "the draft goes, its copy is kept");
        let outbox = room.mail.outbox().await.expect("outbox");
        assert_eq!(outbox.journal.pending_copies().expect("journal").len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
            let kept = fs::read_dir(&outbox.copies).expect("copies").next().expect("a copy").expect("entry");
            assert_eq!((mode(&outbox.copies), mode(&kept.path())), (0o700, 0o600));
        }

        assert_eq!(copy_pending(&room.mail).await, 1);
        let copies = sent_copies(&room.server);
        assert_eq!(copies.len(), 1);
        assert!(copies[0].1.contains("Subject: Lunch"));
        assert_eq!(room.smtp.sent().len(), 1, "only the copy was made again");
        assert!(outbox.journal.pending_copies().expect("journal").is_empty());
        assert_eq!(copy_pending(&room.mail).await, 0, "nothing more to do");
        assert_eq!(sent_copies(&room.server).len(), 1);
    }

    #[tokio::test]
    async fn a_refused_password_shows_at_the_account_and_is_not_tried_again() {
        let room = room().await;
        room.smtp.answer(Submitted::NotAccepted(SmtpError::Auth));
        let id = lunch(&room, DraftAuthor::App).await;
        let refused = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None).await;
        assert_eq!(code(&refused), "auth");
        assert!(matches!(room.mail.smtp(ANNA).await, Err(MailError::Auth)), "no new sign-in");
    }

    #[tokio::test]
    async fn removing_the_account_drops_its_waiting_copies() {
        let room = room().await;
        let id = lunch(&room, DraftAuthor::App).await;
        send_with(&room.mail, Reach::Suite, &DraftAuthor::App, &id, None, |_| NoCopy).await.expect("sent");
        let outbox = room.mail.outbox().await.expect("outbox");
        assert_eq!(outbox.journal.pending_copies().expect("journal").len(), 1);
        room.mail.remove(ANNA).await.expect("removed");
        assert_eq!(fs::read_dir(&outbox.copies).expect("copies").count(), 0, "no mail is kept for it");
        assert!(outbox.journal.pending_copies().expect("journal").is_empty());
        assert_eq!(copy_pending(&room.mail).await, 0);
    }

    #[tokio::test]
    async fn a_copy_whose_append_answer_was_lost_is_not_filed_twice() {
        let room = room().await;
        let id = lunch(&room, DraftAuthor::App).await;
        let prepared = prepare(&room.mail, Reach::Suite, &DraftAuthor::App, &id).await.expect("prepared");
        let copy = ImapCopy { mail: &room.mail, account: ANNA.to_string() };
        copy.append_sent(&prepared.sent_copy()).await.expect("filed");
        copy.append_sent(&prepared.sent_copy()).await.expect("found");
        assert_eq!(sent_copies(&room.server).len(), 1);
    }

    #[tokio::test]
    async fn the_fingerprint_follows_the_draft_and_pins_the_send() {
        let room = room().await;
        let id = lunch(&room, DraftAuthor::App).await;
        let (facts, fingerprint) =
            approval(&room.mail, Reach::Suite, &DraftAuthor::App, &id).await.expect("an approval");
        let again = prepare(&room.mail, Reach::Suite, &DraftAuthor::App, &id).await.expect("prepared");
        assert_eq!(again.fingerprint(), fingerprint, "the same draft, the same message");
        let shown = facts.draft.expect("the draft");
        assert_eq!((shown.recipients, shown.attachments), (3, 0), "Bcc counted");
        assert_eq!(shown.first_recipient.as_deref(), Some("bernd@example.com"));
        assert_eq!((shown.subject.as_str(), shown.text_start.as_str()), ("Lunch", "Dear Bernd, see you."));
        assert_eq!(facts.account.as_deref(), Some("anna@example.org"));
        assert_eq!(shown.bcc, ["carla@example.net"], "every blind copy is named");

        let changed = torro_mail::Draft { id: Some(id.clone()), ..draft("Lunch") };
        saved(save_draft(&room.mail, Reach::Suite, changed, DraftAuthor::App).await);
        let refused = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, Some(&fingerprint)).await;
        assert_eq!(code(&refused), "conflict");
        assert!(room.smtp.sent().is_empty(), "nothing sent");
        let now = prepare(&room.mail, Reach::Suite, &DraftAuthor::App, &id).await.expect("prepared");
        let sent = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &id, Some(&now.fingerprint())).await;
        assert_eq!(sent, Response::MailSent { delivery: Delivery::Accepted });
    }

    #[tokio::test]
    async fn a_client_sends_only_its_own_drafts() {
        let room = room().await;
        let persons = lunch(&room, DraftAuthor::App).await;
        let agent = DraftAuthor::Client("c1".into());
        let request = Request::SendDraft { id: persons.clone() };
        let refused = client_refusal(&room.mail, "c1", Reach::Suite, &request).await.expect("refused");
        assert_eq!(code(&refused), "not-found");
        assert_eq!(code(&send_draft(&room.mail, Reach::Suite, &agent, &persons, None).await), "not-found");
        let hello = crate::mail::write::tests::id_of(&room.mail, "Hello").await;
        let no_draft = send_draft(&room.mail, Reach::Suite, &DraftAuthor::App, &hello, None).await;
        assert_eq!(code(&no_draft), "not-found", "only drafts are sent");
        assert!(room.smtp.sent().is_empty());

        let own = lunch(&room, agent.clone()).await;
        assert_eq!(client_refusal(&room.mail, "c1", Reach::Suite, &Request::SendDraft { id: own.clone() }).await, None);
        let sent = send_draft(&room.mail, Reach::Suite, &agent, &own, None).await;
        assert_eq!(sent, Response::MailSent { delivery: Delivery::Accepted });
    }

    /// Writes a journal entry for `id` as a send the fake answers with `answer` would leave it.
    async fn journal_entry(room: &Room, id: &str, answer: Submitted, plan: CopyPlan) {
        let outbox = room.mail.outbox().await.expect("outbox");
        let envelope = Envelope { from: "anna@example.org".into(), recipients: vec!["bernd@example.com".into()] };
        room.smtp.answer(answer);
        let smtp = Submitter::Fake(Arc::clone(&room.smtp));
        smtp::send(&outbox.journal, id, &envelope, b"Subject: x\r\n\r\nx\r\n", &smtp, plan).await.expect("noted");
    }

    /// Task M3.5: at start, the journal forgets accepted sends whose copy is filed (or not needed)
    /// and whose draft is gone; a draft still there, a copy still missing and an unknown outcome
    /// keep their entries, and so does everything while an account's drafts cannot be told.
    #[tokio::test]
    async fn finished_sends_without_a_draft_leave_the_journal_at_start() {
        let room = room().await;
        let live = lunch(&room, DraftAuthor::App).await;
        let live = prepare(&room.mail, Reach::Suite, &DraftAuthor::App, &live).await.expect("prepared").send_id();
        journal_entry(&room, "send:orphan", Submitted::Accepted, CopyPlan::ProviderKeepsIt).await;
        journal_entry(&room, &live, Submitted::Accepted, CopyPlan::ProviderKeepsIt).await;
        journal_entry(&room, "send:copy-missing", Submitted::Accepted, CopyPlan::Append).await;
        journal_entry(&room, "send:unknown", Submitted::Unknown(SmtpError::Offline), CopyPlan::Append).await;
        let outbox = room.mail.outbox().await.expect("outbox");
        let ids = || {
            let mut ids: Vec<String> = outbox.journal.entries().expect("journal").into_iter().map(|e| e.id).collect();
            ids.sort();
            ids
        };

        // An account whose drafts the core cannot see (mail switched off) keeps everything.
        let off = room.mail.update(ANNA, |account| {
            account.mail_settings.get_or_insert_with(Default::default).services.mail = false;
        });
        off.await.expect("stored").expect("an account");
        assert_eq!(forget_finished(&room.mail).await, 0);
        assert_eq!(ids().len(), 4);
        let on = room.mail.update(ANNA, |account| {
            account.mail_settings.get_or_insert_with(Default::default).services.mail = true;
        });
        on.await.expect("stored").expect("an account");

        assert_eq!(forget_finished(&room.mail).await, 1);
        let mut kept = vec![live, "send:copy-missing".to_string(), "send:unknown".to_string()];
        kept.sort();
        assert_eq!(ids(), kept);
        assert_eq!(forget_finished(&room.mail).await, 0, "nothing more to clear");
    }

    #[test]
    fn bcc_is_read_from_the_raw_draft() {
        let raw = b"Bcc: carla@example.net,\r\n \"Doe, John\" <john@example.org>\r\nSubject: x\r\nbcc: eve@example.com\r\n\r\nBcc: not@header.org\r\n";
        let found: Vec<String> = bcc_of(raw).into_iter().map(|address| address.address).collect();
        assert_eq!(found, ["carla@example.net", "john@example.org", "eve@example.com"]);
        assert!(bcc_of(b"Subject: none\r\n\r\n").is_empty());
        assert_eq!(unhex(&hex("send:ab".as_bytes())).as_deref(), Some("send:ab"));
        assert_eq!(unhex("zz"), None);
    }
}
