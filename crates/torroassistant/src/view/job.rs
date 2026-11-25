//! The job of the open conversation in the chat (issue #962, design fixed before the work): a card as the last
//! entry of the history while the job is open, a quiet line once it is over.
//!
//! - **Card** (like the approval card, `style::container::group`): the goal as its title, one line with "…" and
//!   the whole goal as tooltip, the state as a pill (colour only in its icon); "Job · model · 2 steps done"; while
//!   paused the reason in words; then the buttons that fit: "Pause", "Resume" (off while a step is unclear), "Choose
//!   another model" when the model is gone, and "Cancel job…" last, which asks first.
//! - **Unclear step** (`uncertain`): above the buttons, after a line, each step that may or may not have run with
//!   "It happened" / "It didn't happen". Only the person decides; nothing runs again by itself.
//! - **Over:** "Job done", "Job failed" with its reason, "Job cancelled", in the style of "Allowed" and "Stopped".
//!
//! A button whose request is on its way shows busy, the others take no click until the core answered.

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, responsive, row, rule};
use iced::{Alignment, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, Palette, space};
use torro_gui::widgets::{self, Mark};
use torro_gui::{layout, style, typo};
use torro_ipc::assistant::{JobReason, JobState};

use super::{Anchors, tinted};
use crate::app::{App, Message};
use crate::chat;
use crate::job::{JobAction, JobView};

/// The icon of the job's state and the colour it is drawn in: a job that needs the person in attention, one that
/// runs neutral (orange says "your turn" in this chat), done green, failed in the error colour.
pub(super) fn state_mark(view: &JobView) -> (Icon, fn(&Palette) -> iced::Color) {
    match view.job.state {
        JobState::Queued => (Icon::CircleDashed, |p| p.text_3),
        JobState::Running => (Icon::CircleDotDashed, |p| p.text_2),
        JobState::AwaitingApproval => (Icon::ShieldAlert, |p| p.attention_text),
        JobState::Paused if needs_person(view.job.reason) => (Icon::Pause, |p| p.attention_text),
        JobState::Paused => (Icon::Pause, |p| p.text_3),
        JobState::Completed => (Icon::CircleCheck, |p| p.ok),
        JobState::Failed => (Icon::CircleX, |p| p.error_text),
        JobState::Cancelled => (Icon::CircleSlash, |p| p.text_3),
    }
}

/// Whether a paused job waits for the person to do something (decide a step, a model, access): not when it was
/// only paused or stopped by hand.
pub(super) fn needs_person(reason: Option<JobReason>) -> bool {
    !matches!(reason, None | Some(JobReason::PausedByRequest | JobReason::Stopped))
}

impl App {
    /// The mark of a conversation's row whose job waits for the person (issue #962): the pause, or the shield while
    /// it waits for an approval, in attention, with the reason as tooltip; `None` otherwise.
    pub(super) fn waiting_mark<'a>(&self, conversation: &str) -> Option<Element<'a, Message>> {
        let job = self.waiting_jobs.get(conversation)?;
        let (which, tip) = match (job.state, job.reason) {
            (JobState::AwaitingApproval, _) => (Icon::ShieldAlert, tr("Job needs approval").to_owned()),
            (_, Some(reason)) => (Icon::Pause, reason_words(reason, &self.model_name(&job.engine, &job.model))),
            (_, None) => (Icon::Pause, tr("Paused").to_owned()),
        };
        Some(widgets::with_tooltip(tinted(which, icon::SIZE_SM, |p| p.attention_text), tip))
    }
}

/// The word of the state in the pill.
fn state_word(state: JobState) -> &'static str {
    match state {
        JobState::AwaitingApproval => tr("Needs approval"),
        other => crate::job::state_word(other),
    }
}

/// Why the job is paused or failed, in words; `model` names the job's model.
pub(super) fn reason_words(reason: JobReason, model: &str) -> String {
    match reason {
        JobReason::PausedByRequest => tr("Paused on request").to_owned(),
        JobReason::Stopped => tr("Paused with Stop").to_owned(),
        JobReason::Uncertain => tr("A step may have run. Say whether it happened.").to_owned(),
        JobReason::ApprovalPending => tr("A step still waits for your approval.").to_owned(),
        JobReason::ModelUnavailable => trf("{model} doesn't answer or is turned off.", &[("model", &model)]),
        JobReason::Forbidden => tr("The assistant may no longer use this. Change access, then resume.").to_owned(),
        JobReason::NoClient => tr("The assistant has no access set up. Set it up, then resume.").to_owned(),
        JobReason::StepLimit => tr("Reached its step limit.").to_owned(),
        JobReason::ConversationGone => tr("Its conversation is gone.").to_owned(),
        JobReason::Error => tr("Stopped by an error.").to_owned(),
    }
}

impl App {
    /// The last entry of the history for the conversation's job: the card while it is open, the quiet line once
    /// it is over; `None` without a job.
    pub(super) fn job_entry<'a>(&'a self, anchors: &Anchors) -> Option<Element<'a, Message>> {
        let view = self.job.as_ref()?;
        if !view.is_open() {
            return Some(self.job_end_line(view));
        }
        Some(self.job_card(view, anchors))
    }

    /// "Job done", "Job failed · reason", "Job cancelled".
    fn job_end_line<'a>(&'a self, view: &'a JobView) -> Element<'a, Message> {
        let (which, colour) = state_mark(view);
        let word = match (view.job.state, view.job.reason) {
            (JobState::Completed, _) => tr("Job done").to_owned(),
            (JobState::Failed, Some(reason)) => trf(
                "Job failed · {reason}",
                &[("reason", &reason_words(reason, &self.model_name(&view.job.engine, &view.job.model)))],
            ),
            (JobState::Failed, None) => tr("Job failed").to_owned(),
            _ => tr("Job cancelled").to_owned(),
        };
        row![
            tinted(which, icon::SIZE_SM, colour),
            typo::text(typo::meta(), word).style(theme::text_3).wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    }

    fn job_card<'a>(&'a self, view: &'a JobView, anchors: &Anchors) -> Element<'a, Message> {
        let job = &view.job;
        let (which, colour) = state_mark(view);
        let pill = widgets::state_pill(Mark::Tinted(which, colour(&iced_palette(self))), state_word(job.state));
        // The goal as the card's title: one line, "…" with the whole goal as tooltip.
        let goal = job.goal.clone();
        let title = container(responsive(move |size| {
            let shown = typo::ellipsize(&goal, typo::body_strong(), size.width);
            let cut = shown != goal;
            let line: Element<'_, Message> = typo::text(typo::body_strong(), shown).wrapping(Wrapping::None).into();
            if cut { widgets::with_tooltip(line, goal.clone()) } else { line }
        }))
        .width(Fill)
        .height(typo::line_height(typo::body_strong()));
        let head = row![title, pill].spacing(space::s()).align_y(Alignment::Center);
        let model = self.model_name(&job.engine, &job.model);
        let meta = match view.done_steps() {
            1 => trf("Job · {model} · 1 step done", &[("model", &model)]),
            n => trf("Job · {model} · {n} steps done", &[("model", &model), ("n", &n)]),
        };
        let mut card =
            column![head, typo::text(typo::meta(), meta).style(theme::text_3).wrapping(Wrapping::WordOrGlyph)]
                .spacing(space::s());
        if let Some(reason) = job.reason.filter(|_| job.state == JobState::Paused) {
            card = card.push(self.job_reason(reason, &model));
        }
        let unclear = view.uncertain();
        if !unclear.is_empty() {
            card = card.push(rule::horizontal(1).style(style::rule));
            for step in unclear {
                card = card.push(self.unclear_step(step.seq, &step.tool));
            }
        }
        card = card.push(self.job_buttons(view, anchors));
        container(card).width(Fill).padding(layout::panel_padding()).style(style::container::group).into()
    }

    /// The reason of a paused job: with a warning where the person has to act, and "Change access" / "Set up"
    /// where that is the way on.
    fn job_reason<'a>(&self, reason: JobReason, model: &str) -> Element<'a, Message> {
        let words = typo::text(typo::meta(), reason_words(reason, model)).style(theme::text_2);
        let mut line = row![].spacing(space::s()).align_y(Alignment::Center);
        if needs_person(Some(reason)) {
            line = line.push(tinted(Icon::TriangleAlert, icon::SIZE_SM, |p| p.attention_text));
        }
        line = line.push(container(words.wrapping(Wrapping::WordOrGlyph)).width(Fill));
        let way = match reason {
            JobReason::Forbidden => Some(tr("Change access")),
            JobReason::NoClient => Some(tr("Set up access")),
            _ => None,
        };
        if let Some(label) = way {
            line = line.push(widgets::text_button(None, label, style::button::ghost, Some(Message::SetUp)));
        }
        line.into()
    }

    /// A step whose outcome only the person knows: what it did, why it is unclear, and the two answers.
    fn unclear_step<'a>(&'a self, seq: u32, tool: &str) -> Element<'a, Message> {
        let what = trf("Step {n} · {tool}", &[("n", &seq), ("tool", &chat::tool_title(tool))]);
        let head = row![
            tinted(Icon::TriangleAlert, icon::SIZE_SM, |p| p.attention_text),
            typo::text(typo::body(), what).wrapping(Wrapping::WordOrGlyph)
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let why = typo::text(typo::meta(), tr("Unclear whether this ran. It won't run again by itself."))
            .style(theme::text_2)
            .wrapping(Wrapping::WordOrGlyph);
        let buttons = row![
            self.job_button(Some(Icon::Check), tr("It happened"), JobAction::Resolve { step: seq, done: true }),
            self.job_button(Some(Icon::X), tr("It didn't happen"), JobAction::Resolve { step: seq, done: false }),
        ]
        .spacing(space::s())
        .wrap()
        .vertical_spacing(space::s());
        column![head, why, buttons].spacing(space::s()).into()
    }

    /// The buttons of an open job, "Cancel job…" in a group of its own at the end, so a narrow chat wraps it alone.
    fn job_buttons<'a>(&'a self, view: &'a JobView, anchors: &Anchors) -> Element<'a, Message> {
        let mut main: Vec<Element<'a, Message>> = Vec::new();
        if view.can_pause() {
            main.push(self.job_button(Some(Icon::Pause), tr("Pause"), JobAction::Pause));
        }
        if view.job.state == JobState::Paused {
            if view.wants_model() {
                let label = match &self.job_busy {
                    Some(JobAction::ResumeWith(_)) => JobAction::Resume.busy_word(),
                    _ => tr("Choose another model"),
                };
                let open = self.job_busy.is_none().then_some(Message::ToggleMenu(crate::app::Menu::JobModel));
                main.push(match open {
                    Some(open) => widgets::select_button(&anchors.job_model, label, open),
                    None => widgets::busy_button(label, style::button::secondary),
                });
            }
            let resume = self.job_button(Some(Icon::Play), tr("Resume"), JobAction::Resume);
            main.push(match view.can_resume() {
                true => resume,
                false => widgets::with_tooltip(
                    widgets::text_button(Some(Icon::Play), tr("Resume"), style::button::secondary, None),
                    tr("Decide the open step first"),
                ),
            });
        }
        let main = iced::widget::row(main).spacing(space::s()).wrap().vertical_spacing(space::s());
        let cancel = match self.job_busy {
            Some(JobAction::Cancel) => widgets::busy_button(JobAction::Cancel.busy_word(), style::button::danger),
            _ => widgets::text_button(
                Some(Icon::CircleSlash),
                tr("Cancel job…"),
                style::button::danger,
                self.job_busy.is_none().then_some(Message::Job(JobAction::Cancel)),
            ),
        };
        row![main, cancel, Space::new().width(Fill)].spacing(space::s()).wrap().vertical_spacing(space::s()).into()
    }

    /// A `secondary` button of the card for `action`: busy while its request is on its way, without a press while
    /// another one is.
    fn job_button<'a>(&self, which: Option<Icon>, label: &'a str, action: JobAction) -> Element<'a, Message> {
        if self.job_busy.as_ref() == Some(&action) {
            return widgets::busy_button(action.busy_word(), style::button::secondary);
        }
        let press = self.job_busy.is_none().then_some(Message::Job(action));
        widgets::text_button(which, label, style::button::secondary, press)
    }
}

/// The colours of the window's theme, for a mark drawn with a fixed colour (the pill takes a [`Mark`]).
fn iced_palette(app: &App) -> Palette {
    torro_gui::theme::palette(&app.theme())
}
