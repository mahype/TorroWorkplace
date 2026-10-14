//! Icons for the apps: Lucide line icons as SVG, bundled in `assets/icons/` (ISC licence, see
//! `assets/icons/LICENSE.txt`).
//!
//! Only the icons listed in [`Icon`] are bundled. To use another one, copy its SVG from the
//! Lucide package into `assets/icons/`, then add a variant and an arm in [`Icon::svg`]. The
//! bundled files are Lucide 0.469.0 with `stroke-width` set to 1.75, the line weight of the
//! design (`torro-design`, section 2); that is the only change made to them.
//!
//! Icons are tinted in the text colour of the current mode ([`crate::theme::palette`]). Restyle
//! one with `.style(…)` for a quieter colour, e.g. `text_3` for a secondary icon. Use [`SIZE`]
//! for icons on their own and [`SIZE_SM`] inside buttons.

use iced::Color;
use iced::widget::{Svg, svg};

use crate::theme::palette;

/// Edge length of a stand-alone icon, in logical pixels.
pub const SIZE: f32 = 16.0;
/// Edge length of an icon inside a button, in logical pixels.
pub const SIZE_SM: f32 = 14.0;

/// The bundled Lucide icons. Names follow the current Lucide names (`ellipsis`, `panel-left`,
/// `square-check`, `chart-gantt`); older aliases from the prototypes map to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Plus,
    Minus,
    RefreshCw,
    Check,
    X,
    Ellipsis,
    Calendar,
    CalendarClock,
    CalendarDays,
    CalendarPlus,
    CalendarRange,
    ListTodo,
    Inbox,
    Sun,
    Moon,
    Settings,
    User,
    Users,
    LayoutGrid,
    AppWindow,
    Search,
    Flag,
    CircleAlert,
    CircleCheck,
    CirclePlus,
    PanelLeft,
    ArrowUpRight,
    /// "Newest first" (task E8.15): sorted descending by arrival.
    ArrowDownWideNarrow,
    /// "Oldest first" (task E8.15): sorted ascending by arrival.
    ArrowUpNarrowWide,
    Bell,
    SquareCheck,
    Square,
    SquarePen,
    Clock,
    ChartGantt,
    Radar,
    List,
    ListFilter,
    MapPin,
    Repeat,
    SlidersHorizontal,
    Video,
    Cloud,
    Layers,
    Sparkles,
    // Added for the tasks redesign (R.1): GitHub items, properties and list hints.
    /// Previous item in a detail header (next is [`Icon::ChevronDown`]).
    ChevronUp,
    /// An open issue (green).
    CircleDot,
    /// Checks that are running (attention colour).
    CircleDotDashed,
    /// Checks that are queued (grey).
    CircleDashed,
    /// Checks that failed (error colour).
    CircleX,
    /// An open pull request (green).
    GitPullRequest,
    /// A draft pull request (grey).
    GitPullRequestDraft,
    /// A closed pull request (error colour).
    GitPullRequestClosed,
    /// A merged pull request (the `merged` colour).
    GitMerge,
    /// A branch name in the properties of a pull request.
    GitBranch,
    /// "2/4" sub-tasks hint in a row, "Sub-tasks" section.
    ListChecks,
    /// Parent task in the properties.
    ListTree,
    /// Drag handle of a row in manual order.
    GripVertical,
    /// A review is requested from you.
    Eye,
    /// Labels / tags in the properties.
    Tag,
    /// Milestone in the properties.
    Milestone,
    /// The GitHub account or source. Lucide 0.469.0 still ships its brand icon.
    Github,
    /// "Copy link".
    Link,
    /// "Copy".
    Copy,
    /// "Delete" (menu entry, with the `danger` look).
    Trash2,
    /// Priority in menus and in the empty priority property (the lit bars are drawn by the app).
    SignalHigh,
    /// Repository in the properties of a GitHub item.
    FolderGit2,
    /// "Has notes" hint in a row.
    AlignLeft,
    /// "Needs me" list of the GitHub area.
    UserCheck,
    /// A saved search such as "Torro bugs" in the GitHub area.
    Bug,
    /// Unpin an item from its filter list (task G.3b).
    PinOff,
    /// A mail account, an unread message, "Mark unread" (task E4.2).
    Mail,
    /// A read message, "Mark read".
    MailOpen,
    /// Reply to a message.
    Reply,
    /// Reply to everyone on a message.
    ReplyAll,
    /// Forward a message.
    Forward,
    /// Archive a message, the archive folder.
    Archive,
    /// Attachments of a message.
    Paperclip,
    /// A mail folder without a special role.
    Folder,
    /// The sent folder.
    Send,
    /// The junk (spam) folder.
    ShieldAlert,
    // Added for the inbox (E8.10): muting, the lane "Info" and the insights.
    /// Mute, "Mute…", "Unmute" – everything that silences; [`Icon::Bell`] stays for notifications.
    BellOff,
    /// The lane "Info": for your information.
    Info,
    /// "Insights" (counts and charts).
    ChartColumn,
    /// "Table view": a chart shown as its table.
    Table,
    // Added for the new inbox sources (E5.15).
    /// Nextcloud Talk: a chat conversation.
    MessageCircle,
    /// ntfy: messages pushed to a subscribed topic.
    RadioTower,
    /// The compose window's toolbar (issue #125): bold, italic, underline, numbered list, quote.
    Bold,
    Italic,
    Underline,
    ListOrdered,
    TextQuote,
}

impl Icon {
    /// Every bundled icon, for tests and the gallery.
    pub const ALL: &[Icon] = &[
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::Plus,
        Icon::Minus,
        Icon::RefreshCw,
        Icon::Check,
        Icon::X,
        Icon::Ellipsis,
        Icon::Calendar,
        Icon::CalendarClock,
        Icon::CalendarDays,
        Icon::CalendarPlus,
        Icon::CalendarRange,
        Icon::ListTodo,
        Icon::Inbox,
        Icon::Sun,
        Icon::Moon,
        Icon::Settings,
        Icon::User,
        Icon::Users,
        Icon::LayoutGrid,
        Icon::AppWindow,
        Icon::Search,
        Icon::Flag,
        Icon::CircleAlert,
        Icon::CircleCheck,
        Icon::CirclePlus,
        Icon::PanelLeft,
        Icon::ArrowUpRight,
        Icon::ArrowDownWideNarrow,
        Icon::ArrowUpNarrowWide,
        Icon::Bell,
        Icon::SquareCheck,
        Icon::Square,
        Icon::SquarePen,
        Icon::Clock,
        Icon::ChartGantt,
        Icon::Radar,
        Icon::List,
        Icon::ListFilter,
        Icon::MapPin,
        Icon::Repeat,
        Icon::SlidersHorizontal,
        Icon::Video,
        Icon::Cloud,
        Icon::Layers,
        Icon::Sparkles,
        Icon::ChevronUp,
        Icon::CircleDot,
        Icon::CircleDotDashed,
        Icon::CircleDashed,
        Icon::CircleX,
        Icon::GitPullRequest,
        Icon::GitPullRequestDraft,
        Icon::GitPullRequestClosed,
        Icon::GitMerge,
        Icon::GitBranch,
        Icon::ListChecks,
        Icon::ListTree,
        Icon::GripVertical,
        Icon::Eye,
        Icon::Tag,
        Icon::Milestone,
        Icon::Github,
        Icon::Link,
        Icon::Copy,
        Icon::Trash2,
        Icon::SignalHigh,
        Icon::FolderGit2,
        Icon::AlignLeft,
        Icon::UserCheck,
        Icon::Bug,
        Icon::PinOff,
        Icon::Mail,
        Icon::MailOpen,
        Icon::Reply,
        Icon::ReplyAll,
        Icon::Forward,
        Icon::Archive,
        Icon::Paperclip,
        Icon::Folder,
        Icon::Send,
        Icon::ShieldAlert,
        Icon::BellOff,
        Icon::Info,
        Icon::ChartColumn,
        Icon::Table,
        Icon::MessageCircle,
        Icon::RadioTower,
        Icon::Bold,
        Icon::Italic,
        Icon::Underline,
        Icon::ListOrdered,
        Icon::TextQuote,
    ];

    /// The icon as an SVG handle with its stroke in `color` (opaque), for drawing it on a canvas,
    /// where the colour filter of the [`icon`] widget does not reach:
    /// `frame.draw_svg(bounds, &Icon::Moon.tinted(palette(theme).text_3))`. Take the colour from the
    /// current [`palette`] at draw time, so it follows light and dark.
    pub fn tinted(self, color: Color) -> svg::Handle {
        let [r, g, b, _] = color.into_rgba8();
        let svg = String::from_utf8_lossy(self.svg()).replace("currentColor", &format!("#{r:02X}{g:02X}{b:02X}"));
        svg::Handle::from_memory(svg.into_bytes())
    }

    /// The SVG file bytes, compiled into the binary.
    fn svg(self) -> &'static [u8] {
        match self {
            Icon::ChevronLeft => include_bytes!("../assets/icons/chevron-left.svg"),
            Icon::ChevronRight => include_bytes!("../assets/icons/chevron-right.svg"),
            Icon::ChevronDown => include_bytes!("../assets/icons/chevron-down.svg"),
            Icon::Plus => include_bytes!("../assets/icons/plus.svg"),
            Icon::Minus => include_bytes!("../assets/icons/minus.svg"),
            Icon::RefreshCw => include_bytes!("../assets/icons/refresh-cw.svg"),
            Icon::Check => include_bytes!("../assets/icons/check.svg"),
            Icon::X => include_bytes!("../assets/icons/x.svg"),
            Icon::Ellipsis => include_bytes!("../assets/icons/ellipsis.svg"),
            Icon::Calendar => include_bytes!("../assets/icons/calendar.svg"),
            Icon::CalendarClock => include_bytes!("../assets/icons/calendar-clock.svg"),
            Icon::CalendarDays => include_bytes!("../assets/icons/calendar-days.svg"),
            Icon::CalendarPlus => include_bytes!("../assets/icons/calendar-plus.svg"),
            Icon::CalendarRange => include_bytes!("../assets/icons/calendar-range.svg"),
            Icon::ListTodo => include_bytes!("../assets/icons/list-todo.svg"),
            Icon::Inbox => include_bytes!("../assets/icons/inbox.svg"),
            Icon::Sun => include_bytes!("../assets/icons/sun.svg"),
            Icon::Moon => include_bytes!("../assets/icons/moon.svg"),
            Icon::Settings => include_bytes!("../assets/icons/settings.svg"),
            Icon::User => include_bytes!("../assets/icons/user.svg"),
            Icon::Users => include_bytes!("../assets/icons/users.svg"),
            Icon::LayoutGrid => include_bytes!("../assets/icons/layout-grid.svg"),
            Icon::AppWindow => include_bytes!("../assets/icons/app-window.svg"),
            Icon::Search => include_bytes!("../assets/icons/search.svg"),
            Icon::Flag => include_bytes!("../assets/icons/flag.svg"),
            Icon::CircleAlert => include_bytes!("../assets/icons/circle-alert.svg"),
            Icon::CircleCheck => include_bytes!("../assets/icons/circle-check.svg"),
            Icon::CirclePlus => include_bytes!("../assets/icons/circle-plus.svg"),
            Icon::PanelLeft => include_bytes!("../assets/icons/panel-left.svg"),
            Icon::ArrowUpRight => include_bytes!("../assets/icons/arrow-up-right.svg"),
            Icon::ArrowDownWideNarrow => include_bytes!("../assets/icons/arrow-down-wide-narrow.svg"),
            Icon::ArrowUpNarrowWide => include_bytes!("../assets/icons/arrow-up-narrow-wide.svg"),
            Icon::Bell => include_bytes!("../assets/icons/bell.svg"),
            Icon::SquareCheck => include_bytes!("../assets/icons/square-check.svg"),
            Icon::Square => include_bytes!("../assets/icons/square.svg"),
            Icon::SquarePen => include_bytes!("../assets/icons/square-pen.svg"),
            Icon::Clock => include_bytes!("../assets/icons/clock.svg"),
            Icon::ChartGantt => include_bytes!("../assets/icons/chart-gantt.svg"),
            Icon::Radar => include_bytes!("../assets/icons/radar.svg"),
            Icon::List => include_bytes!("../assets/icons/list.svg"),
            Icon::ListFilter => include_bytes!("../assets/icons/list-filter.svg"),
            Icon::MapPin => include_bytes!("../assets/icons/map-pin.svg"),
            Icon::Repeat => include_bytes!("../assets/icons/repeat.svg"),
            Icon::SlidersHorizontal => include_bytes!("../assets/icons/sliders-horizontal.svg"),
            Icon::Video => include_bytes!("../assets/icons/video.svg"),
            Icon::Cloud => include_bytes!("../assets/icons/cloud.svg"),
            Icon::Layers => include_bytes!("../assets/icons/layers.svg"),
            Icon::Sparkles => include_bytes!("../assets/icons/sparkles.svg"),
            Icon::ChevronUp => include_bytes!("../assets/icons/chevron-up.svg"),
            Icon::CircleDot => include_bytes!("../assets/icons/circle-dot.svg"),
            Icon::CircleDotDashed => include_bytes!("../assets/icons/circle-dot-dashed.svg"),
            Icon::CircleDashed => include_bytes!("../assets/icons/circle-dashed.svg"),
            Icon::CircleX => include_bytes!("../assets/icons/circle-x.svg"),
            Icon::GitPullRequest => include_bytes!("../assets/icons/git-pull-request.svg"),
            Icon::GitPullRequestDraft => include_bytes!("../assets/icons/git-pull-request-draft.svg"),
            Icon::GitPullRequestClosed => include_bytes!("../assets/icons/git-pull-request-closed.svg"),
            Icon::GitMerge => include_bytes!("../assets/icons/git-merge.svg"),
            Icon::GitBranch => include_bytes!("../assets/icons/git-branch.svg"),
            Icon::ListChecks => include_bytes!("../assets/icons/list-checks.svg"),
            Icon::ListTree => include_bytes!("../assets/icons/list-tree.svg"),
            Icon::GripVertical => include_bytes!("../assets/icons/grip-vertical.svg"),
            Icon::Eye => include_bytes!("../assets/icons/eye.svg"),
            Icon::Tag => include_bytes!("../assets/icons/tag.svg"),
            Icon::Milestone => include_bytes!("../assets/icons/milestone.svg"),
            Icon::Github => include_bytes!("../assets/icons/github.svg"),
            Icon::Link => include_bytes!("../assets/icons/link.svg"),
            Icon::Copy => include_bytes!("../assets/icons/copy.svg"),
            Icon::Trash2 => include_bytes!("../assets/icons/trash-2.svg"),
            Icon::SignalHigh => include_bytes!("../assets/icons/signal-high.svg"),
            Icon::FolderGit2 => include_bytes!("../assets/icons/folder-git-2.svg"),
            Icon::AlignLeft => include_bytes!("../assets/icons/align-left.svg"),
            Icon::UserCheck => include_bytes!("../assets/icons/user-check.svg"),
            Icon::Bug => include_bytes!("../assets/icons/bug.svg"),
            Icon::PinOff => include_bytes!("../assets/icons/pin-off.svg"),
            Icon::Mail => include_bytes!("../assets/icons/mail.svg"),
            Icon::MailOpen => include_bytes!("../assets/icons/mail-open.svg"),
            Icon::Reply => include_bytes!("../assets/icons/reply.svg"),
            Icon::ReplyAll => include_bytes!("../assets/icons/reply-all.svg"),
            Icon::Forward => include_bytes!("../assets/icons/forward.svg"),
            Icon::Archive => include_bytes!("../assets/icons/archive.svg"),
            Icon::Paperclip => include_bytes!("../assets/icons/paperclip.svg"),
            Icon::Folder => include_bytes!("../assets/icons/folder.svg"),
            Icon::Send => include_bytes!("../assets/icons/send.svg"),
            Icon::ShieldAlert => include_bytes!("../assets/icons/shield-alert.svg"),
            Icon::BellOff => include_bytes!("../assets/icons/bell-off.svg"),
            Icon::Info => include_bytes!("../assets/icons/info.svg"),
            Icon::ChartColumn => include_bytes!("../assets/icons/chart-column.svg"),
            Icon::Table => include_bytes!("../assets/icons/table.svg"),
            Icon::MessageCircle => include_bytes!("../assets/icons/message-circle.svg"),
            Icon::RadioTower => include_bytes!("../assets/icons/radio-tower.svg"),
            Icon::Bold => include_bytes!("../assets/icons/bold.svg"),
            Icon::Italic => include_bytes!("../assets/icons/italic.svg"),
            Icon::Underline => include_bytes!("../assets/icons/underline.svg"),
            Icon::ListOrdered => include_bytes!("../assets/icons/list-ordered.svg"),
            Icon::TextQuote => include_bytes!("../assets/icons/text-quote.svg"),
        }
    }
}

/// `which` as a square of `size` logical pixels, in the text colour of the current mode.
///
/// Iced's SVG colour filter keeps the icon's alpha and replaces its colour, so the stroke
/// follows the theme. The result is a widget: turn it into an element with `.into()`, or
/// restyle it with `.style(…)` before that.
pub fn icon(which: Icon, size: f32) -> Svg<'static> {
    svg(svg::Handle::from_memory(which.svg()))
        .width(size)
        .height(size)
        .style(|theme, _status| svg::Style { color: Some(palette(theme).text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_a_lucide_svg_with_the_design_stroke() {
        for &which in Icon::ALL {
            let svg = std::str::from_utf8(which.svg()).expect("icon files are UTF-8");
            assert!(svg.contains("<svg"), "{which:?}: no <svg element");
            assert!(svg.trim_end().ends_with("</svg>"), "{which:?}: <svg> is not closed");
            assert!(svg.contains(r#"viewBox="0 0 24 24""#), "{which:?}: unexpected viewBox");
            assert!(svg.contains(r#"stroke-width="1.75""#), "{which:?}: stroke width is not 1.75");
            assert!(!svg.contains("<script"), "{which:?}: scripts are not allowed in icons");
            assert!(svg.contains("lucide-static v0.469.0"), "{which:?}: not from Lucide 0.469.0");
        }
    }

    #[test]
    fn every_bundled_file_has_exactly_one_icon() {
        let mut files: Vec<String> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons"))
            .expect("the icons directory exists")
            .map(|entry| entry.expect("readable entry").file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".svg"))
            .collect();
        files.sort();
        // A file without a variant (or a variant listed twice) would be dead weight or a typo.
        assert_eq!(files.len(), Icon::ALL.len(), "files in assets/icons and variants in Icon::ALL differ");
        let mut bytes: Vec<&[u8]> = Icon::ALL.iter().map(|which| which.svg()).collect();
        bytes.sort();
        bytes.dedup();
        assert_eq!(bytes.len(), Icon::ALL.len(), "two variants share one file, or one is listed twice");
    }

    #[test]
    fn every_variant_loads_the_lucide_icon_of_its_name() {
        // `BellOff` → `bell-off`, `FolderGit2` → `folder-git-2`: the class Lucide writes into each
        // file, so an arm in `Icon::svg` that points at a neighbouring file fails here.
        let kebab = |name: &str| {
            let mut out = String::new();
            let mut previous: Option<char> = None;
            for c in name.chars() {
                let starts_word =
                    c.is_ascii_uppercase() || (c.is_ascii_digit() && !previous.is_some_and(|p| p.is_ascii_digit()));
                if starts_word && previous.is_some() {
                    out.push('-');
                }
                out.push(c.to_ascii_lowercase());
                previous = Some(c);
            }
            out
        };
        for &which in Icon::ALL {
            let svg = std::str::from_utf8(which.svg()).expect("icon files are UTF-8");
            let class = format!(r#"class="lucide lucide-{}""#, kebab(&format!("{which:?}")));
            assert!(svg.contains(&class), "{which:?}: the file is not {class}");
        }
    }

    #[test]
    fn every_icon_builds_at_both_sizes() {
        for &which in Icon::ALL {
            let _ = icon(which, SIZE);
            let _ = icon(which, SIZE_SM);
        }
        assert_eq!((SIZE, SIZE_SM), (16.0, 14.0));
    }

    #[test]
    fn a_tinted_icon_strokes_in_the_given_colour() {
        let svg = std::str::from_utf8(Icon::Moon.svg()).expect("UTF-8");
        assert!(svg.contains("currentColor"), "the files leave the colour to the user");
        let tinted = format!("{:?}", Icon::Moon.tinted(Color::from_rgb8(0x8A, 0x8F, 0x98)));
        assert_ne!(tinted, format!("{:?}", Icon::Moon.tinted(Color::BLACK)), "another colour, another image");
    }
}
