//! Reading and building messages (task M2.4): `mail-parser` for reading (text first, HTML only
//! turned into text and never with anything fetched from outside; attachments, `text/calendar`,
//! list traits into `torro_mail::Message`), `mail-builder` for building drafts into MIME (replies
//! with `In-Reply-To`/`References`, signature, attachments, the core's `Message-ID`). Empty until
//! M2.4.
