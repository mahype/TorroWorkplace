//! The API area `contacts` (`docs/kontakte.md`): limits of its commands. The answers carry the
//! types of `torro_contacts` as they are; every contact, book and group says `untrusted: true`.

/// The most bytes of a picture `contacts.set_photo` takes, before Base64: 700 KB. The core takes no
/// request over 1 MB, and Base64 makes a third more of it; an app scales larger pictures down
/// before it sends them (decision 3 in the plan of issue #351).
pub const MAX_PHOTO_BYTES: usize = 700 * 1024;

/// How many days `contacts.upcoming_birthdays` looks ahead when it is not told.
pub const BIRTHDAY_DAYS: u32 = 14;

/// The most days `contacts.upcoming_birthdays` looks ahead.
pub const MAX_BIRTHDAY_DAYS: u32 = 366;
