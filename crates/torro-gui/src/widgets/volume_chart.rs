//! Mail volume charts: one geometry for the zero-based grid, proportional stacks, labels and hit
//! areas. Opt-in, so the existing column chart and its other callers retain their appearance.

use iced::widget::text::Wrapping;
use iced::widget::{Space, canvas, column, container, pin, responsive, row, stack, tooltip};
use iced::{Element, Fill, Length, Point, Rectangle, Size, Theme, border, mouse};

use super::{ChartColumn, ChartSeries, swatch};
use crate::theme::{self, palette, radius, size, space};
use crate::units::format_count;
use crate::{style, typo};

/// A precise stacked volume chart. Every column keeps a full-height tooltip target, even at zero;
/// missing series values are zero and surplus values are ignored. Colours belong to series, not
/// rank. Offer the same data as a table beside this chart (as on Settings › Statistics).
pub fn volume_chart<'a, M: 'a>(series: Vec<ChartSeries>, columns: Vec<ChartColumn>) -> Element<'a, M> {
    responsive(move |room| {
        let geometry = Geometry::new(room.width, &series, &columns);
        let mut layers: Vec<Element<'a, M>> = vec![
            canvas(Plot { geometry: geometry.clone(), series: series.clone(), columns: columns.clone() })
                .width(room.width)
                .height(geometry.height)
                .into(),
        ];
        let axis = typo::micro().tabular();
        for &tick in &geometry.ticks {
            let text = typo::text(axis, format_count(tick)).style(theme::text_3).wrapping(Wrapping::None);
            layers.push(
                pin(container(text).width(geometry.axis).align_right(geometry.axis))
                    .y(geometry.y(tick) - typo::line_height(axis) / 2.0)
                    .into(),
            );
        }
        for label in &geometry.labels {
            let text = typo::ellipsize(&columns[label.index].label, axis, label.width);
            layers.push(
                pin(container(typo::text(axis, text).style(theme::text_3).wrapping(Wrapping::None))
                    .width(label.width)
                    .center_x(label.width))
                .x(label.x)
                .y(geometry.plot.y + geometry.plot.height + space::s())
                .into(),
            );
        }
        for (index, column) in columns.iter().enumerate() {
            let cell = geometry.cell(index);
            let target = Space::new().width(cell.width).height(cell.height);
            let tip = tooltip(target, tip(&series, column, room.width), tooltip::Position::Top)
                .gap(space::xs())
                .snap_within_viewport(true);
            layers.push(pin(tip).x(cell.x).y(cell.y).into());
        }
        stack(layers).width(Fill).height(geometry.height).into()
    })
    .height(Length::Shrink)
    .into()
}

fn total(series: &[ChartSeries], column: &ChartColumn) -> u64 {
    column.values.iter().take(series.len()).fold(0u64, |sum, value| sum.saturating_add(u64::from(*value)))
}

/// At most four round intervals, without overflowing even when a stack exceeds u32::MAX.
fn ticks(max: u64) -> Vec<u64> {
    if max == 0 {
        return vec![0, 1];
    }
    let mut power = 1u64;
    loop {
        for factor in [1u64, 2, 5] {
            let step = power.saturating_mul(factor);
            let intervals = max.div_ceil(step);
            if intervals <= 4 {
                return (0..=intervals).map(|i| i.saturating_mul(step)).collect();
            }
        }
        power = power.saturating_mul(10);
    }
}

#[derive(Clone, Debug)]
struct Label {
    index: usize,
    x: f32,
    width: f32,
}

#[derive(Clone)]
struct Geometry {
    plot: Rectangle,
    axis: f32,
    height: f32,
    ticks: Vec<u64>,
    top: u64,
    count: usize,
    labels: Vec<Label>,
}

impl Geometry {
    fn new(width: f32, series: &[ChartSeries], columns: &[ChartColumn]) -> Self {
        let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
        let axis_level = typo::micro().tabular();
        let ticks = ticks(columns.iter().map(|column| total(series, column)).max().unwrap_or(0));
        let axis = ticks.iter().map(|n| typo::text_width(&format_count(*n), axis_level)).fold(0.0, f32::max);
        let left = (axis + space::m()).min(width);
        let plot_height = size::row() * if width < size::panel() { 4.0 } else { 5.0 };
        let plot = Rectangle::new(
            Point::new(left, typo::line_height(axis_level) / 2.0),
            Size::new((width - left).max(0.0), plot_height),
        );
        let mut geometry = Self {
            plot,
            axis: axis.min(width),
            height: plot.y + plot.height + space::s() + typo::line_height(axis_level),
            top: ticks.last().copied().unwrap_or(1).max(1),
            ticks,
            count: columns.len(),
            labels: Vec::new(),
        };
        // Place actual measured rectangles, newest first. Clamping the last label to the plot edge
        // is part of collision detection, not a second pass that could make labels overlap.
        let mut right = width + space::s();
        for (index, column) in columns.iter().enumerate().rev() {
            let label_width = typo::text_width(&column.label, axis_level).min(plot.width);
            let cell = geometry.cell(index);
            let x = (cell.center_x() - label_width / 2.0).clamp(plot.x, (width - label_width).max(plot.x));
            if x + label_width + space::s() <= right && label_width > 0.0 {
                geometry.labels.push(Label { index, x, width: label_width });
                right = x;
            }
        }
        geometry.labels.reverse();
        geometry
    }

    fn y(&self, value: u64) -> f32 {
        self.plot.y + self.plot.height * (1.0 - (value as f64 / self.top as f64).min(1.0) as f32)
    }

    fn cell(&self, index: usize) -> Rectangle {
        let width = self.plot.width / self.count.max(1) as f32;
        Rectangle::new(Point::new(self.plot.x + index as f32 * width, self.plot.y), Size::new(width, self.plot.height))
    }

    fn bar(&self, index: usize) -> Rectangle {
        let cell = self.cell(index);
        let width = (cell.width * 0.7).min(size::control_lg() + space::s());
        Rectangle::new(Point::new(cell.center_x() - width / 2.0, cell.y), Size::new(width, cell.height))
    }

    fn segments(&self, series: &[ChartSeries], column: &ChartColumn, index: usize) -> Vec<(usize, Rectangle)> {
        let bar = self.bar(index);
        let mut sum = 0u64;
        series
            .iter()
            .enumerate()
            .filter_map(|(at, _)| {
                let value = u64::from(column.values.get(at).copied().unwrap_or(0));
                sum = sum.saturating_add(value);
                let top = self.y(sum);
                let height = (f64::from(self.plot.height) * value as f64 / self.top as f64) as f32;
                (value > 0).then(|| (at, Rectangle::new(Point::new(bar.x, top), Size::new(bar.width, height))))
            })
            .collect()
    }
}

struct Plot {
    geometry: Geometry,
    series: Vec<ChartSeries>,
    columns: Vec<ChartColumn>,
}

impl<M> canvas::Program<M> for Plot {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let p = palette(theme);
        let g = &self.geometry;
        for &tick in &g.ticks {
            frame.stroke(
                &canvas::Path::line(Point::new(g.plot.x, g.y(tick)), Point::new(g.plot.x + g.plot.width, g.y(tick))),
                canvas::Stroke::default().with_width(size::HAIRLINE).with_color(if tick == 0 {
                    p.line_strong
                } else {
                    p.line
                }),
            );
        }
        for (index, column) in self.columns.iter().enumerate() {
            let segments = g.segments(&self.series, column, index);
            for (n, (at, rect)) in segments.iter().enumerate() {
                let rounded = if n + 1 == segments.len() {
                    radius::xs().min(rect.height / 2.0).min(rect.width / 2.0)
                } else {
                    0.0
                };
                let path = canvas::Path::rounded_rectangle(rect.position(), rect.size(), border::top(rounded));
                frame.fill(&path, p.chart_color(self.series[*at].color));
            }
            // Draw boundaries on top, never add minimum heights or take pixels from small values.
            // Short separators cannot erase a tiny neighbouring segment completely.
            for pair in segments.windows(2) {
                let lower = pair[0].1;
                let upper = pair[1].1;
                let width = 2.0f32.min(lower.height).min(upper.height);
                frame.stroke(
                    &canvas::Path::line(Point::new(lower.x, lower.y), Point::new(lower.x + lower.width, lower.y)),
                    canvas::Stroke::default().with_color(p.panel).with_width(width),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

fn tip<'a, M: 'a>(series: &[ChartSeries], column: &ChartColumn, width: f32) -> Element<'a, M> {
    let head = row![
        typo::text(typo::body_strong(), column.title.clone()).width(Fill),
        typo::text(typo::body_strong().tabular(), format_count(total(series, column))).wrapping(Wrapping::None),
    ]
    .spacing(space::m());
    let mut lines = column![head].spacing(space::s());
    for (at, series) in series.iter().enumerate().rev() {
        lines = lines.push(
            row![
                swatch(series.color),
                typo::text(typo::meta(), series.name.clone()).style(theme::text_2).width(Fill),
                typo::text(
                    typo::meta().tabular(),
                    format_count(u64::from(column.values.get(at).copied().unwrap_or(0)))
                )
                .wrapping(Wrapping::None),
            ]
            .spacing(space::s()),
        );
    }
    container(lines).width(width.min(size::panel())).padding(space::m()).style(style::container::menu).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ChartColor;

    fn series() -> Vec<ChartSeries> {
        (0..4)
            .map(|at| ChartSeries { name: format!("A long series name {at}"), color: ChartColor::Series(at) })
            .collect()
    }

    #[test]
    fn round_axes_are_zero_based_and_overflow_safe() {
        for max in [0, 1, 7, 31, 1001, u64::from(u32::MAX) * 4, u64::MAX] {
            let ticks = ticks(max);
            assert_eq!(ticks[0], 0);
            assert!(ticks.len() <= 5);
            assert!(ticks.last().copied().expect("top") >= max);
            assert!(ticks.windows(2).all(|pair| pair[0] < pair[1]));
        }
    }

    #[test]
    fn all_columns_keep_their_scale_and_labels_never_collide() {
        let series = series();
        for count in [7, 30, 13] {
            let columns: Vec<_> = (0..count)
                .map(|at| ChartColumn {
                    label: format!("{} Oct", at + 1),
                    title: format!("Day {at}"),
                    values: if at == 0 { vec![] } else { vec![u32::MAX, 1, at as u32] },
                })
                .collect();
            for width in [0.0, 80.0, 220.0, 352.0, 800.0] {
                let g = Geometry::new(width, &series, &columns);
                assert!(g.height.is_finite());
                assert!(g.plot.width >= 0.0);
                for (at, column) in columns.iter().enumerate() {
                    let cell = g.cell(at);
                    assert!(cell.width >= 0.0 && cell.width.is_finite());
                    let segments = g.segments(&series, column, at);
                    let height: f32 = segments.iter().map(|(_, rect)| rect.height).sum();
                    let expected = g.plot.height * (total(&series, column) as f64 / g.top as f64) as f32;
                    assert!((height - expected).abs() < 0.001);
                    assert!(segments.iter().all(|(_, rect)| rect.height >= 0.0 && rect.height.is_finite()));
                }
                if g.plot.width > 0.0 {
                    assert_eq!(g.labels.last().expect("newest label").index, count - 1);
                }
                for pair in g.labels.windows(2) {
                    assert!(pair[0].x + pair[0].width + space::s() <= pair[1].x + 0.001);
                }
            }
        }
    }

    #[test]
    fn empty_and_unequal_series_have_no_phantom_values() {
        let series = series();
        let columns = vec![ChartColumn { label: "Today".into(), title: "Today".into(), values: vec![0, 0] }];
        let g = Geometry::new(320.0, &series, &columns);
        assert!(g.segments(&series, &columns[0], 0).is_empty());
        assert_eq!(g.ticks, [0, 1]);
        assert_eq!(total(&series[..1], &ChartColumn { values: vec![2, 99], ..columns[0].clone() }), 2);
        assert_eq!(Geometry::new(320.0, &[], &[]).ticks, [0, 1]);
        let _: Element<'_, ()> = volume_chart(Vec::new(), Vec::new());
    }
}
