//! The mail card (issue #786, `docs/inhaltskarten.md` "E-Mail"): a mail the assistant found or read, as the mail
//! cache holds it now.
//!
//! The card is read with the rights of the conversation's client ([`Lookup::reach`]), not with those of whoever
//! asks for the cards: a bot's conversation shows only mail the bot may read. It comes from the store alone, never
//! from the server, so looking at a card neither fetches a text nor marks a mail read.

use torro_ipc::assistant::Card;

use super::Lookup;
use crate::mail::read::{locate, stored_message};

/// What the mail `id` of the account `account` shows now: [`Card::Mail`] with the message as the mail cache lists it
/// (`MailService`, the same summary `mail.get_message` starts from) and `untrusted: true`, never a picture from the
/// net.
///
/// Reads nothing unless the conversation's client may read mail of `account` now (`crate::core::reads_mail` with
/// [`Lookup::reach`]), and looks for `id` in that account's store only, so a reference naming one account cannot show
/// the mail of another. Answers [`Card::Unavailable`] for a mail that is gone, moved out of reach, of an account the
/// client may not read, or that cannot be read now – without saying why. Logs no subject, sender or text.
pub async fn look(lookup: &Lookup<'_>, account: &str, id: &str) -> Card {
    let reach = lookup.reach();
    if !crate::core::reads_mail(reach, account) {
        return Card::Unavailable;
    }
    let stores: Vec<_> = lookup.mail.stores(reach).await.into_iter().filter(|(of, _)| of == account).collect();
    let Ok(located) = locate(&stores, id).await else { return Card::Unavailable };
    let Ok(message) = stored_message(&located).await else { return Card::Unavailable };
    let mut summary = message.summary;
    // No picture on a card: the sender's logo would be one more thing shown from a mail.
    summary.logo = None;
    Card::Mail { message: Box::new(summary), untrusted: true }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use chrono::Utc;
    use torro_ipc::ClientInfo;
    use torro_ipc::api::{Area, AreaRights, Rights, RightsTemplate, Selection};

    use super::*;
    use crate::account::github::GithubAccounts;
    use crate::account::github::tests::MemoryTokens;
    use crate::approvals::{Approvals, Notifier};
    use crate::assistant::cards::link::Shown;
    use crate::chat::ChatService;
    use crate::contacts::ContactsService;
    use crate::mail::service::MailService;
    use crate::mail::service::tests::{PASSWORD, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::FakeImap;
    use crate::pim::{PimService, Reach};
    use crate::settings_service::SettingsStore;

    const ANNA: &str = "mail:anna@example.org";
    const SUBJECT: &str = "Quarterly figures";
    const TEXT: &str = "Please find the figures below.";

    /// What a [`Lookup`] borrows, on temp directories and fake servers: no network, no home.
    pub(crate) struct Services {
        pub(crate) mail: MailService,
        pub(crate) pim: PimService,
        pub(crate) contacts: ContactsService,
        pub(crate) chat: Arc<ChatService>,
        pub(crate) github: GithubAccounts,
        pub(crate) approvals: Approvals,
        pub(crate) shown: Shown,
    }

    impl Services {
        /// The services in `dir` with `mail` and `contacts` as given, the others empty.
        pub(crate) fn with(dir: &Path, mail: MailService, contacts: ContactsService) -> Services {
            let settings = SettingsStore::at(dir.join("config").join("settings.json"));
            let github =
                crate::account::github::tests::service(dir, &MemoryTokens::default(), "http://127.0.0.1:9", &[]);
            Services {
                mail,
                pim: PimService::with_settings(settings),
                contacts,
                chat: Arc::new(ChatService::fixed(Vec::new())),
                github,
                approvals: Approvals::new(None, Notifier::asking(|_: &torro_ipc::ApprovalInfo| {})),
                shown: Shown::default(),
            }
        }

        /// The look of the conversation of `client` with `rights`.
        pub(crate) fn lookup<'a>(&'a self, client: &'a ClientInfo, rights: &'a Rights) -> Lookup<'a> {
            Lookup {
                client,
                rights,
                asker: crate::pim::Reach::Client(rights),
                conversation: "c1",
                mail: &self.mail,
                pim: &self.pim,
                contacts: &self.contacts,
                chat: &self.chat,
                github: &self.github,
                approvals: &self.approvals,
                shown: &self.shown,
            }
        }
    }

    /// The assistant's client with `rights`.
    pub(crate) fn client(rights: &Rights) -> ClientInfo {
        ClientInfo {
            id: "a1".into(),
            name: "Assistant".into(),
            kind: "assistant".into(),
            created: Utc::now(),
            last_used: None,
            rights: rights.clone(),
            agent: None,
        }
    }

    /// Anna's account on a fake server holding one mail, synced once, and that mail's id.
    async fn synced(dir: &Path, server: &FakeImap) -> (MailService, String) {
        let date = Utc::now().to_rfc2822();
        server.add(
            "INBOX",
            &format!(
                "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: {SUBJECT}\r\nDate: {date}\r\n\
                 Message-ID: <figures@example.org>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{TEXT}\r\n"
            ),
            &[],
        );
        let secrets = MemoryTokens::default();
        let account = mail_account("anna@example.org", &server.endpoint);
        store_accounts(dir, &secrets, &[(&account, PASSWORD)]);
        let service = service_in(dir, &secrets);
        service.reload().await;
        service.sync_all(Reach::Suite).await.expect("synced");
        let store = service.store(ANNA).await.expect("store");
        let ids = store
            .run(|store| {
                let mut ids = Vec::new();
                for folder in store.folders()? {
                    for (uid, _) in store.folder_uids(&folder.id)? {
                        ids.extend(store.message_id_for_uid(&folder.id, uid)?);
                    }
                }
                Ok(ids)
            })
            .await
            .expect("listed");
        let id = ids.into_iter().next().expect("one mail");
        (service, id)
    }

    fn reading_mail(accounts: Selection) -> Rights {
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.mail_accounts = accounts;
        rights
    }

    #[tokio::test]
    async fn a_mail_within_the_rights_shows_as_untrusted_without_a_picture() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, id) = synced(dir.path(), &server).await;
        let services = Services::with(dir.path(), mail, ContactsService::in_dir(dir.path(), Vec::new()));
        let rights = reading_mail(Selection::Only(vec![ANNA.into()]));
        let client = client(&rights);

        let card = look(&services.lookup(&client, &rights), ANNA, &id).await;
        let Card::Mail { message, untrusted } = card else { panic!("a mail card, got {card:?}") };
        assert!(untrusted && message.untrusted);
        assert_eq!((message.subject.as_str(), message.id.as_str()), (SUBJECT, id.as_str()));
        assert_eq!(message.logo, None, "never a picture");
    }

    #[tokio::test]
    async fn a_mail_out_of_reach_is_unavailable_without_its_text() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let (mail, id) = synced(dir.path(), &server).await;
        let services = Services::with(dir.path(), mail, ContactsService::in_dir(dir.path(), Vec::new()));

        let mut no_mail = reading_mail(Selection::All);
        no_mail.areas.insert(Area::Mail, AreaRights::NONE);
        let other_account = reading_mail(Selection::Only(vec!["mail:bernd@example.com".into()]));
        let within = reading_mail(Selection::All);
        for (rights, account, id, why) in [
            (&no_mail, ANNA, id.as_str(), "no right to read mail"),
            (&other_account, ANNA, id.as_str(), "the account not in the rights"),
            (&within, "mail:bernd@example.com", id.as_str(), "the mail asked for under another account"),
            (&within, ANNA, "m:gone", "a mail that is not there (any more)"),
        ] {
            let client = client(rights);
            let card = look(&services.lookup(&client, rights), account, id).await;
            assert_eq!(card, Card::Unavailable, "{why}");
            let shown = serde_json::to_string(&card).expect("serialised");
            assert!(!shown.contains(SUBJECT) && !shown.contains("bernd"), "{why}: {shown}");
        }
    }
}
