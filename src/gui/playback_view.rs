//! Stateless rendering of the solution player's replay snapshots.
//!
//! The playback model owns counter operations and time; these widgets only draw
//! the exact states and eased progress it supplies. Wheel motion uses separate
//! directions for counter digits and the reset index, while instructions fade
//! between the corresponding labels.
//!
//! Short counters fit in a centered row. Long counters stop shrinking at a
//! readable minimum and scroll horizontally, with the reset wheel pinned beside
//! the scrolling digits. Drawing visits only wheels intersecting the viewport.

use iced::advanced::{
    Layout, Widget, layout, mouse, renderer, text as rendered_text, widget::Tree,
};
use iced::advanced::{Renderer as _, text::Renderer as _};
use iced::widget::{container, responsive, row, scrollable};
use iced::{Border, Color, Element, Fill, Length, Point, Rectangle, Size, Theme, alignment};

use crate::playback::{Frame, RollDirection};

const WHEEL_WIDTH: f32 = 46.0;
const WHEEL_HEIGHT: f32 = 68.0;
const WHEEL_GAP: f32 = 6.0;
const RESET_GAP: f32 = 20.0;
const RESET_WIDTH: f32 = 72.0;
const DISPLAY_HEIGHT: f32 = 94.0;
const INSTRUCTION_HEIGHT: f32 = 52.0;
const MINIMUM_SCALE: f32 = 0.75;

/// Lay out a replay frame with readable digits and a continuously visible reset index.
///
/// A fitting counter is centered as one widget. Otherwise, only the digit row
/// scrolls, and a separate reset wheel occupies the fixed column beside it.
pub(crate) fn view<'a, Message: 'a>(frame: Frame<'a>) -> Element<'a, Message> {
    responsive(move |size| {
        let metrics = Metrics::new(frame.current_digits.len(), size.width);

        if metrics.width > size.width + 0.5 {
            let digits = scrollable(Element::new(Wheels {
                frame,
                metrics,
                show_digits: true,
                show_reset: false,
            }))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(5).scroller_width(5),
            ))
            .width(Fill)
            .height(DISPLAY_HEIGHT + 10.0);
            let reset = Element::new(Wheels {
                frame,
                metrics,
                show_digits: false,
                show_reset: true,
            });

            row![digits, reset]
                .spacing(metrics.reset_gap())
                .width(Fill)
                .into()
        } else {
            container(Element::new(Wheels {
                frame,
                metrics,
                show_digits: true,
                show_reset: true,
            }))
            .center_x(Fill)
            .height(DISPLAY_HEIGHT + 10.0)
            .into()
        }
    })
    .height(DISPLAY_HEIGHT + 10.0)
    .into()
}

/// Crossfade instruction labels in a fixed-height slot without shifting controls.
///
/// `progress` is the animation fraction supplied by the playback model. Equal
/// labels draw once; invalid progress falls back to the completed transition.
pub(crate) fn instruction<'a, Message: 'a>(
    previous: &'a str,
    current: &'a str,
    progress: f32,
) -> Element<'a, Message> {
    Element::new(Instruction {
        previous,
        current,
        progress: bounded_progress(progress),
    })
}

/// Shared wheel geometry, scaled no lower than 75% of its natural dimensions.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    scale: f32,
    width: f32,
    digit_count: usize,
}

impl Metrics {
    fn new(digit_count: usize, available_width: f32) -> Self {
        let natural_width = digit_count as f32 * WHEEL_WIDTH
            + digit_count.saturating_sub(1) as f32 * WHEEL_GAP
            + RESET_GAP
            + RESET_WIDTH;
        let scale = (available_width / natural_width).clamp(MINIMUM_SCALE, 1.0);

        Self {
            scale,
            width: natural_width * scale,
            digit_count,
        }
    }

    fn wheel(self, origin: Point, idx: usize) -> Rectangle {
        Rectangle {
            x: origin.x + idx as f32 * (WHEEL_WIDTH + WHEEL_GAP) * self.scale,
            y: origin.y + 8.0 + (WHEEL_HEIGHT - WHEEL_HEIGHT * self.scale) / 2.0,
            width: WHEEL_WIDTH * self.scale,
            height: WHEEL_HEIGHT * self.scale,
        }
    }

    fn digit_width(self) -> f32 {
        self.width - self.reset_width() - self.reset_gap()
    }

    fn reset_width(self) -> f32 {
        RESET_WIDTH * self.scale
    }

    fn reset_gap(self) -> f32 {
        RESET_GAP * self.scale
    }

    fn reset_column(self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + bounds.width - self.reset_width(),
            y: bounds.y,
            width: self.reset_width(),
            height: DISPLAY_HEIGHT,
        }
    }

    /// Find intersecting digit slots without traversing the entire counter.
    fn visible_digits(self, origin: Point, viewport: Rectangle) -> std::ops::Range<usize> {
        let stride = (WHEEL_WIDTH + WHEEL_GAP) * self.scale;
        let start = ((viewport.x - origin.x) / stride).floor().max(0.0) as usize;
        let end = ((viewport.x + viewport.width - origin.x) / stride)
            .ceil()
            .max(0.0) as usize;

        start.min(self.digit_count)..end.min(self.digit_count)
    }
}

/// Draw all wheels, only the scrolling digits, or only the pinned reset index.
struct Wheels<'a> {
    frame: Frame<'a>,
    metrics: Metrics,
    show_digits: bool,
    show_reset: bool,
}

impl Wheels<'_> {
    fn width(&self) -> f32 {
        match (self.show_digits, self.show_reset) {
            (true, true) => self.metrics.width,
            (true, false) => self.metrics.digit_width(),
            (false, _) => self.metrics.reset_width(),
        }
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Wheels<'_> {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width()),
            height: Length::Fixed(DISPLAY_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width(), DISPLAY_HEIGHT)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        let palette = theme.extended_palette();
        let progress = bounded_progress(self.frame.progress);

        renderer.with_layer(clip, |renderer| {
            let visible_digits = if self.show_digits {
                self.metrics.visible_digits(bounds.position(), clip)
            } else {
                0..0
            };

            for idx in visible_digits {
                let current = self.frame.current_digits[idx];
                let previous = self
                    .frame
                    .previous_digits
                    .get(idx)
                    .copied()
                    .unwrap_or(current);
                let wheel = self.metrics.wheel(bounds.position(), idx);

                draw_wheel(
                    renderer,
                    wheel,
                    clip,
                    WheelAppearance {
                        previous,
                        current,
                        progress,
                        direction: self.frame.digit_direction,
                        background: palette.background.weak.color,
                        border: palette.background.strong.color,
                        text: theme.palette().text,
                        font_size: 40.0 * self.metrics.scale,
                    },
                );
            }

            let reset_column = self.metrics.reset_column(bounds);

            if self.show_reset && reset_column.intersects(&clip) {
                let mut reset_wheel = self.metrics.wheel(bounds.position(), 0);
                reset_wheel.width = 42.0 * self.metrics.scale;
                reset_wheel.x = reset_column.center_x() - reset_wheel.width / 2.0;

                draw_wheel(
                    renderer,
                    reset_wheel,
                    clip,
                    WheelAppearance {
                        previous: self.frame.previous_reset_index,
                        current: self.frame.reset_index,
                        progress,
                        direction: self.frame.reset_direction,
                        background: palette.background.base.color,
                        border: palette.background.weaker.color,
                        text: palette.secondary.base.color,
                        font_size: 34.0 * self.metrics.scale,
                    },
                );

                draw_text(
                    renderer,
                    "Reset index",
                    Point::new(reset_column.center_x(), bounds.y + 87.0),
                    Size::new(reset_column.width, 15.0),
                    (11.0 * self.metrics.scale).max(10.0),
                    palette.secondary.base.color,
                    clip,
                );
            }
        });
    }
}

/// Exact numeral endpoints plus independent visual motion and styling.
struct WheelAppearance {
    previous: u8,
    current: u8,
    progress: f32,
    direction: RollDirection,
    background: Color,
    border: Color,
    text: Color,
    font_size: f32,
}

/// Clip outgoing and incoming numerals inside the wheel's rounded face.
///
/// Values never count through intermediate numerals: a reset or decimal wrap
/// animates directly between the two exact states provided by the model.
fn draw_wheel(
    renderer: &mut iced::Renderer,
    bounds: Rectangle,
    viewport: Rectangle,
    appearance: WheelAppearance,
) {
    let Some(clip) = bounds.intersection(&viewport) else {
        return;
    };

    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: Border {
                color: appearance.border,
                width: 1.0,
                radius: 9.0.into(),
            },
            ..renderer::Quad::default()
        },
        appearance.background,
    );

    let inner = Rectangle {
        x: bounds.x + 3.0,
        y: bounds.y + 3.0,
        width: (bounds.width - 6.0).max(0.0),
        height: (bounds.height - 6.0).max(0.0),
    };
    let Some(text_clip) = inner.intersection(&clip) else {
        return;
    };

    renderer.with_layer(text_clip, |renderer| {
        let center = bounds.center();
        let offsets = roll_offsets(appearance.progress, bounds.height, appearance.direction);

        if appearance.previous == appearance.current
            || appearance.direction == RollDirection::Still
            || appearance.progress >= 1.0
        {
            draw_digit(
                renderer,
                appearance.current,
                center,
                bounds,
                text_clip,
                &appearance,
            );
        } else {
            draw_digit(
                renderer,
                appearance.previous,
                Point::new(center.x, center.y + offsets.0),
                bounds,
                text_clip,
                &appearance,
            );
            draw_digit(
                renderer,
                appearance.current,
                Point::new(center.x, center.y + offsets.1),
                bounds,
                text_clip,
                &appearance,
            );
        }
    });

    // A subtle spindle seam lends the display a mechanical appearance without
    // reducing the contrast of the numerals or the moving reset-index wheel.
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle {
                x: bounds.x + 3.0,
                y: bounds.center_y(),
                width: (bounds.width - 6.0).max(0.0),
                height: 1.0,
            },
            ..renderer::Quad::default()
        },
        appearance.border.scale_alpha(0.28),
    );
}

fn draw_digit(
    renderer: &mut iced::Renderer,
    digit: u8,
    center: Point,
    bounds: Rectangle,
    clip: Rectangle,
    appearance: &WheelAppearance,
) {
    draw_text(
        renderer,
        &digit.to_string(),
        center,
        bounds.size(),
        appearance.font_size,
        appearance.text,
        clip,
    );
}

/// A fixed layout slot whose two labels share one clipped drawing layer.
struct Instruction<'a> {
    previous: &'a str,
    current: &'a str,
    progress: f32,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Instruction<'_> {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fixed(INSTRUCTION_HEIGHT),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Fill, INSTRUCTION_HEIGHT)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        let color = theme.palette().text;
        let center = bounds.center();

        renderer.with_layer(clip, |renderer| {
            if self.previous == self.current || self.progress >= 1.0 {
                draw_text(
                    renderer,
                    self.current,
                    center,
                    bounds.size(),
                    16.0,
                    color,
                    clip,
                );
            } else {
                draw_text(
                    renderer,
                    self.previous,
                    Point::new(center.x, center.y - self.progress * 3.0),
                    bounds.size(),
                    16.0,
                    color.scale_alpha(1.0 - self.progress),
                    clip,
                );
                draw_text(
                    renderer,
                    self.current,
                    Point::new(center.x, center.y + (1.0 - self.progress) * 3.0),
                    bounds.size(),
                    16.0,
                    color.scale_alpha(self.progress),
                    clip,
                );
            }
        });
    }
}

fn draw_text(
    renderer: &mut iced::Renderer,
    content: &str,
    center: Point,
    bounds: Size,
    size: f32,
    color: Color,
    clip: Rectangle,
) {
    renderer.fill_text(
        rendered_text::Text {
            content: content.to_owned(),
            bounds,
            size: size.into(),
            line_height: rendered_text::LineHeight::Relative(1.0),
            font: renderer.default_font(),
            align_x: rendered_text::Alignment::Center,
            align_y: alignment::Vertical::Center,
            shaping: rendered_text::Shaping::Basic,
            wrapping: rendered_text::Wrapping::Word,
        },
        center,
        color,
        clip,
    );
}

/// Clamp animation progress, treating nonfinite input as already complete.
fn bounded_progress(progress: f32) -> f32 {
    if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Vertical offsets for the outgoing and incoming numerals, respectively.
///
/// Forward motion carries the old numeral up and introduces the new one from
/// below; backward motion reverses that travel. The new numeral ends centered.
fn roll_offsets(progress: f32, height: f32, direction: RollDirection) -> (f32, f32) {
    let progress = bounded_progress(progress);
    let sign = match direction {
        RollDirection::Forward => -1.0,
        RollDirection::Backward => 1.0,
        RollDirection::Still => 0.0,
    };

    (sign * height * progress, -sign * height * (1.0 - progress))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheels_keep_a_readable_minimum_and_long_counters_scroll() {
        let short = Metrics::new(4, 340.0);
        assert_eq!(short.scale, 1.0);
        assert!(short.width < 340.0);

        let long = Metrics::new(100, 340.0);
        assert_eq!(long.scale, MINIMUM_SCALE);
        assert!(long.width > 340.0);
        assert!(WHEEL_WIDTH * long.scale >= 34.0);
    }

    #[test]
    fn wide_counters_reserve_a_pinned_reset_wheel_inside_the_viewport() {
        let viewport_width = 340.0;
        let metrics = Metrics::new(1_000, viewport_width);
        let digit_viewport = viewport_width - metrics.reset_gap() - metrics.reset_width();
        let reset_bounds = Rectangle {
            x: digit_viewport + metrics.reset_gap(),
            y: 0.0,
            width: metrics.reset_width(),
            height: DISPLAY_HEIGHT,
        };
        let reset_column = metrics.reset_column(reset_bounds);
        assert_eq!(reset_column.x + reset_column.width, viewport_width);
        assert!(digit_viewport > 0.0);
        assert!(metrics.digit_width() > digit_viewport);
        assert_eq!(reset_column.width, 54.0);
    }

    #[test]
    fn only_visible_digits_are_drawn_after_horizontal_scrolling() {
        let metrics = Metrics::new(1_000, 340.0);
        let viewport = Rectangle {
            x: 390.0,
            y: 0.0,
            width: 340.0,
            height: DISPLAY_HEIGHT,
        };
        let visible = metrics.visible_digits(Point::ORIGIN, viewport);
        assert_eq!(visible, 10..19);
        assert!(visible.len() < 12);
    }

    #[test]
    fn rolling_wraps_have_correct_endpoints_in_both_directions() {
        for direction in [RollDirection::Forward, RollDirection::Backward] {
            let start = roll_offsets(0.0, WHEEL_HEIGHT, direction);
            let end = roll_offsets(1.0, WHEEL_HEIGHT, direction);
            assert_eq!(start.0, 0.0);
            assert_eq!(end.1, 0.0);
            assert_eq!(start.1.abs(), WHEEL_HEIGHT);
            assert_eq!(end.0.abs(), WHEEL_HEIGHT);
        }

        assert_eq!(
            roll_offsets(0.5, WHEEL_HEIGHT, RollDirection::Still),
            (0.0, 0.0)
        );
        assert_eq!(bounded_progress(f32::NAN), 1.0);
    }
}
