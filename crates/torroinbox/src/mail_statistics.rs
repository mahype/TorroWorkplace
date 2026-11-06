//! The statistics page of the mail window (issue #485, `docs/mail.md`): Settings › Statistics, for
//! the moments a person wants to get the flood of mail under control. It shows what the overview
//! (`mail_overview.rs`) leaves out, in four tabs, from the same answer of `mail.overview`:
//!
//! - **Overview:** four figures ([`figures`]) and "What comes in" with "Table" (`mail_overview::day_chart`,
//!   `mail_overview::day_table`).
//! - **Senders:** "Who sends the most" with "Opened x of y" and [`rarely_opened`], "Show mails",
//!   "Unsubscribe…" and "Mute" on hover.
//! - **Inbox:** "What is in your inbox" ([`inbox_parts`], by age and at most one suggestion to tidy up,
//!   [`tidy`]); "Show them" only lists the mail, nothing is archived by itself.
//! - **Categories:** the columns of one category over the span ([`category_columns`], from
//!   `OverviewCategory::days`) above every category with its count, share and how much was opened
//!   ([`category_lines`]); a click chooses the line, at first the largest.
//!
//! Span and account are the overview's ([`mail_overview::controls`]): in the head's buttons in the wide
//! window, under the tabs in the medium and narrow ones, where the figures stand two by two. The settings host scrolls the page and gives it the whole width
//! (#464). Names and addresses come from mail (`untrusted`) and are drawn as plain text.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, responsive, row};
use iced::{Alignment, Element, Fill, Length, Padding, Theme};
use torro_core::i18n::{Lang, lang, tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, ChartColor, palette, size, space};
use torro_gui::units::format_count;
use torro_gui::widgets::{self, Anchor, ChartColumn, ChartSeries, PageAction, RowAction, SharePart, StatFigure};
use torro_gui::{layout, style, typo};
use torro_ipc::StatsRange;
use torro_ipc::inbox::CategoryInfo;
use torro_ipc::mail::{MailOverview, OverviewCategory, OverviewSender, UnsubscribeTarget};
use torro_mail::Category;
use torro_settings_ui::Page;

use crate::app::{Message, UnsubscribeAsk};
use crate::mail_overview::{self, Numbers, OverviewData, Place, Tab, by_age, came_in};
use crate::{insights, list, mail};

/// From how many mails a sender can be "Rarely opened".
const RARELY_FROM: u32 = 5;

/// Below this share in percent of a sender's mail opened, it is "Rarely opened".
const RARELY_BELOW: u32 = 15;

/// What newsletters and ads are: the categories the suggestion to tidy up and the fourth figure
/// count.
pub const NEWSLETTERS_AND_ADS: [Category; 2] = [Category::Newsletter, Category::Promotion];

// ---------------------------------------------------------------------------------------------
// The numbers
// ---------------------------------------------------------------------------------------------

/// How much of what came in is of `categories`.
fn of_categories(overview: &MailOverview, categories: &[Category]) -> u32 {
    overview
        .categories
        .iter()
        .filter(|held| held.category.is_some_and(|category| categories.contains(&category)))
        .map(|held| held.count)
        .sum()
}

/// The four figures: what lies in the inbox (with what is older than a week), what came in (with
/// about how much a day), how much of it came from people ("Personal") and the share of newsletters
/// and ads.
pub fn figures(overview: &MailOverview) -> [StatFigure; 4] {
    let (today, week, older) = by_age(overview);
    let inbox = today + week + older;
    let came = came_in(overview);
    let days = u32::try_from(overview.days.len()).unwrap_or(1).max(1);
    let per_day = (f64::from(came) / f64::from(days)).round() as u32;
    let people = of_categories(overview, &[Category::Personal]);
    let ads = of_categories(overview, &NEWSLETTERS_AND_ADS);
    let count = |n: u32| format_count(u64::from(n));
    [
        StatFigure::new(tr("In your inbox"), count(inbox), trf("{n} older than a week", &[("n", &count(older))])),
        StatFigure::new(tr("Came in"), count(came), trf("about {n} a day", &[("n", &count(per_day))])),
        StatFigure::new(
            tr("From people"),
            count(people),
            trf("{share} of what came in", &[("share", &insights::percent(people, came))]),
        ),
        StatFigure::new(
            tr("Newsletters and ads"),
            insights::percent(ads, came),
            trf("{n} of {total} mails", &[("n", &count(ads)), ("total", &count(came))]),
        ),
    ]
}

/// A sender is "Rarely opened" with at least [`RARELY_FROM`] mails of which less than
/// [`RARELY_BELOW`] percent were opened.
pub fn rarely_opened(count: u32, opened: u32) -> bool {
    count >= RARELY_FROM && u64::from(opened) * 100 < u64::from(count) * u64::from(RARELY_BELOW)
}

/// How many mails `count` over `days` days are a week, with one decimal in the language's way:
/// "7.5 a week", German "7,5 pro Woche".
pub fn per_week(count: u32, days: usize) -> String {
    let days = days.max(1) as f64;
    let digits = format!("{:.1}", f64::from(count) * 7.0 / days);
    let digits = if lang() == Lang::De { digits.replace('.', ",") } else { digits };
    trf("{n} a week", &[("n", &digits)])
}

/// What lies in the inbox, as parts of a strip: the four main categories with a colour of their own
/// and "Other", each only with mail.
pub fn inbox_parts(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Vec<SharePart> {
    let mut sums = [0u64; 5];
    for held in &overview.inbox {
        sums[mail_overview::slot_index(mail_overview::slot(held.group))] +=
            u64::from(held.today + held.week + held.older);
    }
    let mut parts: Vec<SharePart> = (0..4)
        .filter(|at| sums[*at] > 0)
        .filter_map(|at| {
            let group = mail_overview::group_at(at)?;
            let name = mail_overview::group_name(catalogue, group);
            Some(SharePart::new(name, sums[at], format_count(sums[at]), ChartColor::Series(at)))
        })
        .collect();
    if sums[4] > 0 {
        parts.push(SharePart::new(tr("Other"), sums[4], format_count(sums[4]), ChartColor::Other));
    }
    parts
}

/// The one suggestion to tidy up: how many newsletters and ads in the inbox are older than a week;
/// `None` when none are.
pub fn tidy(overview: &MailOverview) -> Option<u32> {
    let older: u32 = overview
        .inbox
        .iter()
        .filter(|held| held.category.is_some_and(|category| NEWSLETTERS_AND_ADS.contains(&category)))
        .map(|held| held.older)
        .sum();
    (older > 0).then_some(older)
}

/// The categories of the tab "Categories": every one that had mail, in the answer's order (the most
/// first), none cut off and none put under "Other".
pub fn category_lines(overview: &MailOverview) -> Vec<&OverviewCategory> {
    overview.categories.iter().filter(|held| held.count > 0).collect()
}

/// The category the tab charts: the one chosen while it still had mail, else the largest.
pub fn chosen_category(overview: &MailOverview, chosen: Option<Option<Category>>) -> Option<&OverviewCategory> {
    let lines = category_lines(overview);
    chosen
        .and_then(|category| lines.iter().copied().find(|held| held.category == category))
        .or_else(|| lines.first().copied())
}

/// What a person reads for a category of the answer: its name, "Not sorted yet" without one.
fn category_name(catalogue: &[CategoryInfo], category: Option<Category>) -> String {
    match category {
        Some(category) => mail::category_look(catalogue, category).0,
        None => tr("Not sorted yet").to_owned(),
    }
}

/// The columns of `held` over the span: a column per day for 7 and 30 days, per seven days for 90
/// (counted back from today, as the day chart does), from its counts per day. Empty from a core
/// that counts no days.
pub fn category_columns(overview: &MailOverview, held: &OverviewCategory, range: StatsRange) -> Vec<ChartColumn> {
    if held.days.len() != overview.days.len() {
        return Vec::new();
    }
    let size = if range == StatsRange::Quarter { 7 } else { 1 };
    let days: Vec<_> = overview.days.iter().zip(&held.days).collect();
    let mut columns: Vec<ChartColumn> = days
        .rchunks(size)
        .filter_map(|chunk| {
            let (first, last) = (chunk.first()?.0, chunk.last()?.0);
            let bucket = mail_overview::Bucket {
                from: first.day,
                until: last.day,
                values: vec![chunk.iter().map(|(_, count)| **count).sum()],
            };
            Some(ChartColumn {
                label: mail_overview::bucket_label(&bucket, range),
                title: mail_overview::bucket_title(&bucket, overview.until),
                values: bucket.values,
            })
        })
        .collect();
    columns.reverse();
    columns
}

// ---------------------------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------------------------

/// The statistics page as the settings host takes it: the tabs, span and account, and the chosen
/// tab's content – or loading, the error with "Retry", the empty span; while another span or account
/// loads the numbers stay with "Updating…" over them. `data.place` is [`Place::Page`] in the wide
/// window, with span and account in the head's buttons, else [`Place::Narrow`], with them under the
/// tabs and the figures two by two.
pub fn page(data: OverviewData<'_>) -> Page<'_, Message> {
    let state = data.state;
    let labels = Tab::ALL.iter().map(|tab| (tab.label().to_owned(), None)).collect();
    let chosen = Tab::ALL.iter().position(|tab| *tab == state.tab).unwrap_or_default();
    let narrow = data.place == Place::Narrow;
    let mut blocks: Vec<Element<'_, Message>> = vec![widgets::tabs(labels, chosen, Message::MailStatisticsTab)];
    // Below the wide window the header bar has no room for them (issue #485): under the tabs.
    if narrow {
        blocks.push(mail_overview::controls(state, &data.accounts, true));
    }
    blocks.push(typo::text(typo::meta(), mail_overview::head_line(&data)).style(theme::text_3).into());
    match mail_overview::numbers(state) {
        Numbers::Loading => blocks.push(widgets::loading()),
        Numbers::Failed => {
            blocks.push(widgets::error_state(tr("Couldn't load the statistics."), Message::MailOverviewRetry));
        }
        Numbers::Ready { overview, updating } => {
            if updating {
                blocks.push(widgets::refresh_note(tr("Updating…")));
            }
            if mail_overview::is_empty(overview) {
                blocks.push(widgets::empty_state(
                    tr("No mail in this span."),
                    tr("What comes in, who sends it and what you open shows up here."),
                ));
            } else {
                blocks.extend(tab_blocks(&data, overview));
            }
        }
    }
    let body = column(blocks).spacing(layout::section_gap()).width(Fill);
    let page = Page::new(body);
    if narrow { page } else { page.actions(vec![mail_overview::controls(state, &data.accounts, false)]) }
}

/// What the chosen tab shows of `overview`.
fn tab_blocks<'a>(data: &OverviewData<'a>, overview: &MailOverview) -> Vec<Element<'a, Message>> {
    let state = data.state;
    match state.tab {
        Tab::Overview => {
            // Below the wide window two by two: four tiles side by side cut their words (issue #485).
            let [first, second, third, fourth] = figures(overview);
            let mut blocks = match data.place {
                Place::Page => vec![widgets::stat_grid(vec![first, second, third, fourth])],
                Place::Panel | Place::Narrow => {
                    let grids =
                        column![widgets::stat_grid(vec![first, second]), widgets::stat_grid(vec![third, fourth])];
                    vec![grids.spacing(space::m()).into()]
                }
            };
            blocks.push(comes_in_table(overview, state.range, state.table, data.catalogue));
            blocks
        }
        Tab::Senders => vec![senders_card(overview, state.muting.as_deref(), data.catalogue, &data.mute_at)],
        Tab::Inbox => vec![inbox_card(overview, data.catalogue)],
        Tab::Categories => categories_tab(overview, state, data.catalogue),
    }
}

/// "What comes in" with "Table": the columns per day by main category, or the same numbers as a
/// table.
fn comes_in_table<'a>(
    overview: &MailOverview,
    range: StatsRange,
    table: bool,
    catalogue: &[CategoryInfo],
) -> Element<'a, Message> {
    let switch = widgets::choice_chip(
        tr("Table"),
        Some(widgets::Mark::Icon(Icon::Table)),
        table,
        Some(Message::MailOverviewTable(!table)),
    );
    let body: Element<'a, Message> = if table {
        let series = mail_overview::series(overview, catalogue);
        let (head, rows, total) = mail_overview::day_table(overview, range, &series);
        // The names of the main categories head columns as wide as the numbers: cut with "…" so they
        // never run into each other in a narrow window.
        let table = responsive(move |room| {
            let values = head.len().saturating_sub(1);
            let width = widgets::table_value_width(room.width, values);
            let head = head
                .iter()
                .enumerate()
                .map(|(i, name)| if i == 0 { name.clone() } else { typo::ellipsize(name, typo::caption(), width) })
                .collect();
            widgets::settings_table(head, rows.clone(), Some(total.clone()))
        })
        .height(Length::Shrink);
        column![container(switch).width(Fill).align_right(Fill), table].spacing(space::l()).into()
    } else {
        mail_overview::day_chart(overview, range, catalogue, Some(switch))
    };
    widgets::chart_card(tr("What comes in"), mail_overview::chart_note(range), body)
}

/// "Categories": the columns of the chosen category over the span (without counts per day none),
/// above a line per category; a click on a line chooses it.
fn categories_tab<'a>(
    overview: &MailOverview,
    state: &mail_overview::State,
    catalogue: &[CategoryInfo],
) -> Vec<Element<'a, Message>> {
    let Some(chosen) = chosen_category(overview, state.category) else { return Vec::new() };
    let mut blocks = Vec::new();
    let columns = category_columns(overview, chosen, state.range);
    if !columns.is_empty() {
        let name = category_name(catalogue, chosen.category);
        let series = vec![ChartSeries { name: name.clone(), color: mail_overview::slot(chosen.group) }];
        let note = if state.range == StatsRange::Quarter { tr("per week") } else { tr("per day") };
        blocks.push(widgets::chart_card(name, note, widgets::column_chart(series, columns)));
    }
    let lines = category_lines(overview);
    let largest = lines.iter().map(|held| held.count).max().unwrap_or(0);
    let whole = came_in(overview);
    let shares: Vec<String> = lines.iter().map(|held| insights::percent(held.count, whole)).collect();
    let widths =
        mail_overview::NumberWidths::of(lines.iter().zip(&shares).map(|(held, share)| (held.count, share.as_str())));
    let rows = lines
        .into_iter()
        .map(|held| category_row(held, catalogue, (largest, whole, widths), held.category == chosen.category));
    let note = trf("{n} mails", &[("n", &format_count(u64::from(whole)))]);
    // The chosen line is filled beside its words: the rows reach into the card's padding (issue #485).
    blocks.push(widgets::chart_card_rows(tr("By category"), note, column(rows).spacing(space::xs())));
    blocks
}

/// One line of "Categories": mark and name with "Opened x of y" under it, a bar against the largest,
/// the count and the share in the columns of `widths`; the whole line chooses the category, the chosen
/// one `selected`.
fn category_row<'a>(
    held: &OverviewCategory,
    catalogue: &[CategoryInfo],
    (largest, whole, widths): (u32, u32, mail_overview::NumberWidths),
    selected: bool,
) -> Element<'a, Message> {
    let mark = held.category.map(|category| {
        let (_, group) = mail::category_look(catalogue, category);
        let (_, colour) = mail::group_look(catalogue, held.group.unwrap_or(group));
        let symbol = catalogue
            .iter()
            .find(|info| info.category == category)
            .map_or_else(|| category.default_icon(), |info| info.icon);
        (mail::category_symbol(symbol), colour)
    });
    let opened = trf(
        "Opened {n} of {total}",
        &[("n", &format_count(u64::from(held.opened))), ("total", &format_count(u64::from(held.count)))],
    );
    // What was opened under the name, lined up with it, so the line fits a narrow window too.
    let name = column![
        row![
            mail_overview::category_mark(mark),
            widgets::ellipsized(typo::body(), category_name(catalogue, held.category), theme::text)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center),
        container(widgets::ellipsized(typo::meta(), opened, theme::text_3))
            .padding(Padding::ZERO.left(icon::SIZE_SM + space::s())),
    ];
    let [bar, count, share] =
        mail_overview::category_numbers(held.count, largest, insights::percent(held.count, whole), widths);
    let line = row![container(name).width(Fill), bar, count, share].spacing(space::m()).align_y(Alignment::Center);
    button(line)
        .width(Fill)
        .padding([space::xs(), space::s()])
        .style(style::button::sidebar_item(selected))
        .on_press(Message::MailStatisticsCategory(held.category))
        .into()
}

/// "Who sends the most": a line per sender with how much it sends, how much of it is opened and –
/// on hover – "Show mails", "Unsubscribe…" and "Mute". Below a form's width the column of what is
/// opened goes: the second line says it after the category, without the address.
fn senders_card<'a>(
    overview: &MailOverview,
    muting: Option<&str>,
    catalogue: &[CategoryInfo],
    mute_at: &Anchor,
) -> Element<'a, Message> {
    let senders = overview.senders.clone();
    let catalogue = catalogue.to_vec();
    let days = overview.days.len();
    let muting = muting.map(str::to_owned);
    let mute_at = mute_at.clone();
    let body: Element<'a, Message> = if senders.is_empty() {
        typo::text(typo::body(), tr("No sender stands out yet.")).style(theme::text_2).into()
    } else {
        responsive(move |room| {
            let wide = room.width >= size::form_max();
            // Words or icons once for the whole card, by the widest bar a row can have, so the bars do
            // not switch between rows; measured at the row's real width.
            let words = widgets::row_actions_words(&widest_actions(), room.width);
            let lines = senders.iter().map(|sender| {
                let muting = muting.as_deref() == Some(sender.address.as_str());
                sender_line(sender, &catalogue, days, (wide, words), muting, &mute_at)
            });
            column(lines).spacing(space::xs()).into()
        })
        .height(Length::Shrink)
        .into()
    };
    widgets::chart_card(tr("Who sends the most"), tr("and how much of it you open"), body)
}

/// One sender: avatar, name, address and usual category; "Opened x of y" with its bar and "Rarely
/// opened" (wide only); how many and how many a week; on hover the bar with "Show mails",
/// "Unsubscribe…" (issue #416, only for a sender with a mail that offers it) and "Mute", with words
/// as the card decided (`words`).
fn sender_line<'a>(
    sender: &OverviewSender,
    catalogue: &[CategoryInfo],
    days: usize,
    (wide, words): (bool, bool),
    muting: bool,
    mute_at: &Anchor,
) -> Element<'a, Message> {
    let name = sender
        .name
        .as_deref()
        .map(list::one_line)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| list::one_line(&sender.address));
    let category = sender.category.map(|category| mail::category_look(catalogue, category).0);
    let opened = trf(
        "Opened {n} of {total}",
        &[("n", &format_count(u64::from(sender.opened))), ("total", &format_count(u64::from(sender.count)))],
    );
    // Narrow, what was opened stands in the second line, as the card's note promises (issue #485).
    let detail = match (wide, category) {
        (true, Some(category)) => format!("{} · {category}", list::one_line(&sender.address)),
        (true, None) => list::one_line(&sender.address),
        (false, Some(category)) => format!("{category} · {opened}"),
        (false, None) => opened.clone(),
    };
    let who = column![
        widgets::ellipsized(typo::body_strong(), name.clone(), theme::text),
        widgets::ellipsized(typo::meta(), detail, theme::text_3),
    ];
    let mut line = row![list::avatar(&name, None, size::tile()), container(who).width(Length::FillPortion(3))]
        .spacing(space::m())
        .align_y(Alignment::Center);
    if wide {
        // "Rarely opened" beside the bar, so every row keeps two lines (issue #485); cut with "…" where
        // the column is narrow.
        let mut bar =
            row![widgets::share_bar(sender.opened, sender.count)].spacing(space::s()).align_y(Alignment::Center);
        if rarely_opened(sender.count, sender.opened) {
            bar = bar.push(
                row![
                    icon::icon(Icon::EyeOff, icon::SIZE_SM)
                        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) }),
                    container(widgets::ellipsized(typo::meta(), tr("Rarely opened"), theme::text_3)).width(Fill),
                ]
                .spacing(space::xs())
                .align_y(Alignment::Center),
            );
        }
        let read =
            column![typo::text(typo::meta().tabular(), opened).style(theme::text_2).wrapping(Wrapping::None), bar]
                .spacing(space::xs());
        line = line.push(container(read).width(Length::FillPortion(2)));
    }
    let number = column![
        typo::text(typo::body_strong().tabular(), format_count(u64::from(sender.count))),
        typo::text(typo::meta(), per_week(sender.count, days)).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .align_x(Alignment::End);
    line = line.push(number);
    // No surface of its own (the bar floats over it): the row stays on the axis of the card's title.
    let row_ = container(line).width(Fill).padding(Padding::ZERO.top(space::xs()).bottom(space::xs()));
    let actions = sender_actions(&sender.address, sender.unsubscribe.as_ref(), &name, muting, mute_at);
    widgets::row_actions(row_, actions, muting, words)
}

/// The bar of a sender's row: "Show mails", "Unsubscribe…" (issue #416, only with a mail to
/// unsubscribe by, asked about under the sender's `name`) and "Mute", whose menu hangs at its button
/// while `muting`.
fn sender_actions<'a>(
    address: &str,
    unsubscribe: Option<&UnsubscribeTarget>,
    name: &str,
    muting: bool,
    mute_at: &Anchor,
) -> Vec<RowAction<'a, Message>> {
    let mute = RowAction::new(Icon::BellOff, tr("Mute"), "", Message::MailOverviewMute(address.to_owned()));
    let mut actions =
        vec![RowAction::new(Icon::Search, tr("Show mails"), "", Message::MailOverviewSender(address.to_owned()))];
    if let Some(target) = unsubscribe {
        let ask = UnsubscribeAsk {
            account: target.account.clone(),
            id: target.id.clone(),
            sender: name.to_owned(),
            address: address.to_owned(),
        };
        actions.push(RowAction::new(Icon::MailX, tr("Unsubscribe…"), "", Message::AskUnsubscribe(ask)));
    }
    actions.push(if muting { mute.at(mute_at) } else { mute });
    actions
}

/// The widest bar a sender's row can have: with "Unsubscribe…", for [`widgets::row_actions_words`].
fn widest_actions<'a>() -> Vec<RowAction<'a, Message>> {
    let target = UnsubscribeTarget { account: String::new(), id: String::new() };
    sender_actions("", Some(&target), "", false, &Anchor::new())
}

/// "What is in your inbox": the inbox by main category as a strip, by age, and at most one
/// suggestion to tidy up, which only lists the mail.
fn inbox_card<'a>(overview: &MailOverview, catalogue: &[CategoryInfo]) -> Element<'a, Message> {
    let (today, week, older) = by_age(overview);
    let total = today + week + older;
    let mut body = column![].spacing(space::m());
    if total == 0 {
        body = body.push(typo::text(typo::body(), tr("Your inbox is empty.")).style(theme::text_2));
    } else {
        body = body.push(widgets::share_strip(inbox_parts(overview, catalogue), None));
        let age = |label: &'static str, n: u32| {
            row![
                typo::text(typo::meta(), label).style(theme::text_3),
                typo::text(typo::meta().tabular(), format_count(u64::from(n))).style(theme::text),
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center)
        };
        let ages = row![
            typo::text(typo::meta(), tr("By age")).style(theme::text_3),
            age(tr("Today"), today),
            age(tr("This week"), week),
            age(tr("Older"), older),
        ]
        .spacing(space::l())
        .align_y(Alignment::Center)
        .wrap();
        body = body.push(ages);
    }
    if let Some(old) = tidy(overview) {
        let title = trf("{n} newsletters and ads are older than a week", &[("n", &format_count(u64::from(old)))]);
        let show = PageAction::new(tr("Show them"), Some(Message::MailOverviewTidy));
        body =
            body.push(widgets::question_card(Icon::Archive, title, tr("Nothing is archived by itself."), vec![show]));
    }
    let note = trf("{n} mails", &[("n", &format_count(u64::from(total)))]);
    widgets::chart_card(tr("What is in your inbox"), note, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_overview::tests::{day, overview};
    use crate::mail_overview::{demo, nothing};

    #[test]
    fn the_figures_count_the_inbox_what_came_people_and_ads() {
        let [inbox, came, people, ads] = figures(&overview());
        assert_eq!((inbox.label.as_str(), inbox.value.as_str()), (tr("In your inbox"), "17"));
        assert_eq!(inbox.note, trf("{n} older than a week", &[("n", &"10")]));
        assert_eq!(came.value, "91");
        assert_eq!(came.note, trf("about {n} a day", &[("n", &"13")]));
        assert_eq!(people.value, "14");
        assert_eq!(people.note, trf("{share} of what came in", &[("share", &insights::percent(14, 91))]));
        assert_eq!(ads.value, insights::percent(70, 91));
        assert_eq!(ads.note, trf("{n} of {total} mails", &[("n", &"70"), ("total", &"91")]));
    }

    /// Issue #416: a sender's bar offers "Unsubscribe" between "Show mails" and "Mute" only with a
    /// mail to unsubscribe by, which it asks about under the sender's name; the demo's newsletters
    /// have one, people do not.
    #[test]
    fn unsubscribe_stands_only_for_a_sender_with_a_mail_to_unsubscribe_by() {
        let today = day(30);
        let source = torro_inbox::demo::DemoSource::new(chrono::Utc::now());
        let inbox = source.by_role(torro_mail::Role::Inbox, None, None, torro_mail::Order::NewestFirst);
        let demo = demo(today, StatsRange::Month, false, &inbox);
        let words = |sender: &OverviewSender| {
            sender_actions(&sender.address, sender.unsubscribe.as_ref(), "Shop", false, &Anchor::new())
                .iter()
                .map(|action| action.label.to_string())
                .collect::<Vec<_>>()
        };
        let with = demo.senders.iter().find(|sender| sender.unsubscribe.is_some()).expect("a newsletter");
        assert_eq!(words(with), [tr("Show mails"), tr("Unsubscribe…"), tr("Mute")]);
        assert_eq!(widest_actions().len(), 3, "the card measures its bars with Unsubscribe…");
        let actions = sender_actions(&with.address, with.unsubscribe.as_ref(), "Shop", false, &Anchor::new());
        let Message::AskUnsubscribe(ask) = &actions[1].on_press else { panic!("it asks") };
        let target = with.unsubscribe.clone().expect("a target");
        assert_eq!((&ask.account, &ask.id, ask.sender.as_str()), (&target.account, &target.id, "Shop"));
        assert_eq!(ask.address, with.address);
        let without = demo.senders.iter().find(|sender| sender.unsubscribe.is_none()).expect("a person");
        assert_eq!(words(without), [tr("Show mails"), tr("Mute")]);
    }

    #[test]
    fn a_sender_is_rarely_opened_from_five_mails_under_fifteen_percent() {
        assert!(rarely_opened(30, 2));
        assert!(!rarely_opened(4, 0), "too few to say");
        assert!(!rarely_opened(20, 3), "15% is not rarely");
        assert!(rarely_opened(20, 2));
        assert_eq!(per_week(30, 30), trf("{n} a week", &[("n", &if lang() == Lang::De { "7,0" } else { "7.0" })]));
    }

    #[test]
    fn the_inbox_shows_its_groups_ages_and_one_suggestion() {
        let o = overview();
        assert_eq!(by_age(&o), (3, 4, 10));
        let parts = inbox_parts(&o, &[]);
        assert_eq!(parts.iter().map(|part| part.value).collect::<Vec<_>>(), [2, 15]);
        assert_eq!(parts[1].color, ChartColor::Series(2));
        assert_eq!(tidy(&o), Some(10));
        let mut tidy_already = o.clone();
        tidy_already.inbox.retain(|held| held.category == Some(Category::Personal));
        assert_eq!(tidy(&tidy_already), None, "no suggestion without old newsletters and ads");
        assert!(!mail_overview::is_empty(&o));
        assert!(mail_overview::is_empty(&nothing(day(30), StatsRange::Month)));
    }

    /// Issue #485: "Categories" lists every category with mail in the answer's order, the largest
    /// chosen at first and a chosen one while it has mail; its columns are its days, by week over 90
    /// days, and none without counts per day.
    #[test]
    fn the_categories_tab_charts_the_chosen_category() {
        let o = overview();
        let lines: Vec<Option<Category>> = category_lines(&o).iter().map(|held| held.category).collect();
        assert_eq!(lines, [Some(Category::Newsletter), Some(Category::Promotion), Some(Category::Personal), None]);
        assert_eq!(chosen_category(&o, None).map(|held| held.category), Some(Some(Category::Newsletter)));
        assert_eq!(chosen_category(&o, Some(None)).map(|held| held.category), Some(None), "not sorted yet");
        let gone = Some(Some(Category::Invoice));
        assert_eq!(chosen_category(&o, gone).map(|held| held.category), Some(Some(Category::Newsletter)));
        let newsletter = chosen_category(&o, None).expect("the largest");
        let columns = category_columns(&o, newsletter, StatsRange::Week);
        assert_eq!(columns.iter().map(|column| column.values[0]).collect::<Vec<_>>(), [5, 5, 10, 5, 5, 10, 10]);
        let promotion = chosen_category(&o, Some(Some(Category::Promotion))).expect("promotion");
        assert!(category_columns(&o, promotion, StatsRange::Week).is_empty(), "a core without days");
        let quarter = demo(day(30), StatsRange::Quarter, false, &[]);
        let largest = chosen_category(&quarter, None).expect("a category");
        let weeks = category_columns(&quarter, largest, StatsRange::Quarter);
        assert_eq!(weeks.len(), 13, "90 days: 12 weeks and what is left over first");
        assert_eq!(weeks.iter().map(|column| column.values[0]).sum::<u32>(), largest.days.iter().sum::<u32>());
        assert!(category_lines(&nothing(day(30), StatsRange::Week)).is_empty());
    }
}
