//! How the window "Picture large" looks (draft B of "Bild groß" in `docs/prototypes/chat/`): the top
//! row with the file's name, "Download" and "Open in Nextcloud"; the picture fitted into the window
//! between "Previous" and "Next"; under it "2 / 5 · Anna Berg · 10:02". Mini and strip show only the
//! picture.

use iced::widget::{Space, column, container, image, row};
use iced::{Alignment, ContentFit, Element, Fill};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::shell::{self, TopRow};
use torro_gui::theme::{self, space};
use torro_gui::{typo, widgets};

use crate::app::files::Shown;
use crate::app::viewer::{ViewerMsg, ViewerState, wrap};
use crate::app::{App, Message};
use crate::timeline;

impl App {
    /// The window "Picture large".
    pub(crate) fn viewer_window<'a>(&'a self, viewer: &'a ViewerState) -> Element<'a, Message> {
        let current = viewer.current();
        let name = current.map(|item| item.name.as_str()).filter(|name| !name.is_empty()).unwrap_or(tr("Picture"));
        let mut header = row![].spacing(space::s()).width(Fill).align_y(Alignment::Center);
        if !cfg!(target_os = "macos") {
            header = header.push(shell::app_mark(torro_core::AppId::CHAT));
        }
        let save_tip = if viewer.saving { tr("Saving…") } else { tr("Download") };
        let save = (!viewer.saving && current.is_some()).then_some(wrap(ViewerMsg::Save));
        let header = header
            .push(container(widgets::ellipsized(typo::body_strong(), name, theme::text)).width(Fill).clip(true))
            .push(widgets::icon_button(Icon::Download, save_tip, save))
            .push(widgets::icon_button(Icon::Cloud, tr("Open in Nextcloud"), Some(wrap(ViewerMsg::Nextcloud))));
        let top = TopRow::new(self.zoom.factor(), Message::DragWindow);
        let previous = widgets::icon_button(
            Icon::ChevronLeft,
            tr("Previous (←)"),
            (viewer.at > 0).then_some(wrap(ViewerMsg::Step(-1))),
        );
        let next = widgets::icon_button(
            Icon::ChevronRight,
            tr("Next (→)"),
            (viewer.at + 1 < viewer.items.len()).then_some(wrap(ViewerMsg::Step(1))),
        );
        let stage = row![previous, self.viewer_picture(viewer), next]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .height(Fill);
        let mut foot = vec![trf("{at} / {count}", &[("at", &(viewer.at + 1)), ("count", &viewer.items.len())])];
        if let Some(item) = current {
            if !item.author.is_empty() {
                foot.push(item.author.clone());
            }
            if let Some(at) = item.at {
                foot.push(timeline::time_label(at));
            }
        }
        let foot = typo::text(typo::meta().tabular(), foot.join(" · ")).style(theme::text_3);
        let body = column![stage, container(foot).width(Fill).center_x(Fill)]
            .spacing(space::s())
            .padding([space::m(), space::l()])
            .width(Fill)
            .height(Fill);
        shell::shell(SizeClass::Narrow, Space::new(), header, body, None).top_row(top).into()
    }

    /// The picture shown, fitted into its room; a calm placeholder until it came, a sentence when the
    /// server has none.
    pub(crate) fn viewer_picture<'a>(&'a self, viewer: &'a ViewerState) -> Element<'a, Message> {
        let shown = viewer.current().and_then(|item| viewer.pictures.get(&(item.message.clone(), item.file.clone())));
        let content: Element<'a, Message> = match shown {
            Some(Shown::Ready(handle)) => {
                image(handle.clone()).content_fit(ContentFit::Contain).width(Fill).height(Fill).into()
            }
            Some(Shown::Missing) => {
                widgets::empty_state(tr("No preview of this picture"), tr("Open it in Nextcloud to see it."))
            }
            Some(Shown::Loading) | None => widgets::loading(),
        };
        container(content).width(Fill).height(Fill).center(Fill).into()
    }
}
