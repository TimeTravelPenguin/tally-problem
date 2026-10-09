//! Keyboard routing and visibility operations for the GUI's manual focus order.
//!
//! [`keyboard_gate`] prevents an unfocused control from consuming keys intended
//! for another control. [`reveal`] inspects widget bounds inside the root
//! `planner-page` scrollable and proposes an absolute vertical offset. The
//! application applies that offset only if its focus revision is still current,
//! so delayed operations cannot undo a newer navigation action.

use iced::advanced::widget::{
    Id, Operation, operate,
    operation::{Focusable, Outcome, Scrollable},
    tree::{self, Tree},
};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Task, Vector, keyboard};

/// Keep hovered controls from handling keys intended for another focused input.
///
/// When disabled, only key presses and releases are withheld. Pointer events,
/// layout, widget state, and overlays continue through the wrapped widget.
pub fn keyboard_gate<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    enabled: bool,
) -> Element<'a, Message> {
    Element::new(KeyboardGate {
        content: content.into(),
        enabled,
    })
}

/// A transparent widget wrapper with an optional keyboard event boundary.
struct KeyboardGate<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    enabled: bool,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for KeyboardGate<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !self.enabled
            && matches!(
                event,
                Event::Keyboard(
                    keyboard::Event::KeyPressed { .. } | keyboard::Event::KeyReleased { .. }
                )
            )
        {
            return;
        }

        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
    }
}

/// Find the page offset needed to bring a widget ID into view with some padding.
///
/// Returns `None` when the target is missing or already visible. This task does
/// not scroll by itself; the caller decides whether the result is still current.
pub fn reveal(target: &'static str) -> Task<Option<f32>> {
    operate(Reveal {
        target: target.into(),
        page: None,
        target_bounds: None,
    })
}

/// Collect the target bounds and root page translation during a widget traversal.
struct Reveal {
    target: Id,
    page: Option<(Rectangle, f32)>,
    target_bounds: Option<Rectangle>,
}

impl Operation<Option<f32>> for Reveal {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<f32>>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.target) {
            self.target_bounds = Some(bounds);
        }
    }

    fn focusable(&mut self, id: Option<&Id>, bounds: Rectangle, _state: &mut dyn Focusable) {
        self.container(id, bounds);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _content: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&Id::from("planner-page")) {
            self.page = Some((bounds, translation.y));
        }
    }

    fn finish(&self) -> Outcome<Option<f32>> {
        let offset = self
            .page
            .zip(self.target_bounds)
            .and_then(|((viewport, offset), target)| visible_offset(viewport, offset, target));

        Outcome::Some(offset)
    }
}

/// Choose the smallest vertical correction, aligning oversized targets at the top.
fn visible_offset(viewport: Rectangle, offset: f32, target: Rectangle) -> Option<f32> {
    const PADDING: f32 = 16.0;
    let visible_top = viewport.y + offset + PADDING;
    let visible_bottom = viewport.y + offset + viewport.height - PADDING;
    let desired =
        if target.y < visible_top || target.height > (viewport.height - PADDING * 2.0).max(0.0) {
            target.y - viewport.y - PADDING
        } else if target.y + target.height > visible_bottom {
            target.y + target.height - viewport.y - viewport.height + PADDING
        } else {
            return None;
        };

    let desired = desired.max(0.0);

    ((desired - offset).abs() > f32::EPSILON).then_some(desired)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(top: f32, height: f32) -> Rectangle {
        Rectangle {
            x: 0.0,
            y: top,
            width: 200.0,
            height,
        }
    }

    #[test]
    fn reveals_controls_and_tall_results_with_padding_without_repeated_scrolling() {
        for (offset, target_top, target_height, expected) in [
            (50.0, 76.0, 20.0, None),
            (50.0, 40.0, 20.0, Some(14.0)),
            (50.0, 160.0, 20.0, Some(86.0)),
            (50.0, 10.0, 20.0, Some(0.0)),
            (0.0, 200.0, 300.0, Some(174.0)),
            (174.0, 200.0, 300.0, None),
        ] {
            assert_eq!(
                visible_offset(rect(10.0, 100.0), offset, rect(target_top, target_height)),
                expected
            );
        }

        assert_eq!(
            visible_offset(rect(0.0, 20.0), 0.0, rect(30.0, 24.0)),
            Some(14.0)
        );
    }

    // Iced's null renderer is available in debug builds, which lets these tests
    // exercise the real slider event path without a desktop or browser window.
    #[cfg(debug_assertions)]
    mod keyboard_gate {
        use super::*;
        use iced::advanced::clipboard;
        use iced::keyboard::{Key, Location, Modifiers, key};

        fn slider_events(enabled: bool, events: &[Event]) -> (Vec<f32>, Vec<bool>) {
            let mut widget: Element<'_, f32, iced::Theme, ()> = Element::new(KeyboardGate {
                content: iced::widget::slider(1.0..=8.0, 3.0, |value| value).into(),
                enabled,
            });
            let mut tree = Tree::new(&widget);
            let node = widget.as_widget_mut().layout(
                &mut tree,
                &(),
                &layout::Limits::new(Size::ZERO, Size::new(100.0, 16.0)),
            );
            let layout = Layout::new(&node);
            let viewport = layout.bounds();
            let cursor = mouse::Cursor::Available(viewport.center());
            let mut clipboard = clipboard::Null;
            let mut messages = Vec::new();
            let mut captured = Vec::new();

            for event in events {
                let mut shell = Shell::new(&mut messages);
                widget.as_widget_mut().update(
                    &mut tree,
                    event,
                    layout,
                    cursor,
                    &(),
                    &mut clipboard,
                    &mut shell,
                    &viewport,
                );
                captured.push(shell.is_event_captured());
            }

            (messages, captured)
        }

        #[test]
        fn hovered_slider_handles_arrow_keys_only_when_keyboard_is_enabled() {
            let arrow = Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(key::Named::ArrowUp),
                modified_key: Key::Named(key::Named::ArrowUp),
                physical_key: key::Physical::Code(key::Code::ArrowUp),
                location: Location::Standard,
                modifiers: Modifiers::empty(),
                text: None,
                repeat: false,
            });

            assert_eq!(
                slider_events(false, std::slice::from_ref(&arrow)),
                (vec![], vec![false])
            );
            assert_eq!(slider_events(true, &[arrow]), (vec![4.0], vec![true]));
        }

        #[test]
        fn disabling_keyboard_keeps_pointer_touch_and_modifier_events_working() {
            let (messages, captured) = slider_events(
                false,
                &[Event::Mouse(mouse::Event::ButtonPressed(
                    mouse::Button::Left,
                ))],
            );

            assert!(!messages.is_empty());
            assert_eq!(captured, vec![true]);

            let (messages, captured) = slider_events(
                false,
                &[Event::Touch(iced::touch::Event::FingerPressed {
                    id: iced::touch::Finger(0),
                    position: iced::Point::new(50.0, 8.0),
                })],
            );

            assert!(!messages.is_empty());
            assert_eq!(captured, vec![true]);

            assert_eq!(
                slider_events(
                    false,
                    &[
                        Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::CTRL)),
                        Event::Mouse(mouse::Event::WheelScrolled {
                            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
                        }),
                    ],
                ),
                (vec![4.0], vec![false, true]),
            );
        }
    }
}
