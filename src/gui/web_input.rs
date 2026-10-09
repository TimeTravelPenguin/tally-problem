//! WASM-only native browser editors positioned over the Iced form fields.
//!
//! Winit 0.30's web IME methods are no-ops, so canvas text inputs cannot open a
//! software keyboard: <https://github.com/rust-windowing/winit/issues/3938>.
//! [`field`] preserves Iced's layout and focus operations, while JavaScript owns
//! the visible editor, selection, paste, and composition. [`subscription`] sends
//! whole values and application keys back to the same form model.
//!
//! [`layer`] reconciles editors with each rendered frame, hiding culled fields
//! and temporarily exposing the Iced fallback underneath overlapping popups.
//! [`reset`] updates browser values synchronously and invalidates queued events.
//! Geometry assumes the three fields live in the root full-window scrollable;
//! it is not a general-purpose bridge for nested forms or arbitrary themes.

use std::cell::{Cell, RefCell};

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::futures::{Stream, channel::mpsc};
use iced::keyboard::{Key, Modifiers, key::Named};
use iced::{Element, Event as IcedEvent, Length, Rectangle, Size, Subscription, Theme, Vector};
use wasm_bindgen::{JsCast, JsValue, prelude::*};

#[wasm_bindgen(module = "/src/gui/web-input.js")]
extern "C" {
    #[wasm_bindgen(js_name = setEventSink)]
    fn set_event_sink(callback: &js_sys::Function);
    #[wasm_bindgen(js_name = syncField)]
    fn sync_field(config: &js_sys::Array, geometry: &js_sys::Array);
    #[wasm_bindgen(js_name = activeField)]
    fn active_field_id() -> String;
    #[wasm_bindgen(js_name = focus)]
    fn focus_element(id: &str);
    #[wasm_bindgen(js_name = resetPending)]
    fn reset_pending(values: &js_sys::Array);
    #[wasm_bindgen(js_name = beginFrame)]
    fn begin_frame();
    #[wasm_bindgen(js_name = endFrame)]
    fn end_frame();
    #[wasm_bindgen(js_name = occludeFields)]
    fn occlude_fields(x: f32, y: f32, width: f32, height: f32);
}

/// A browser event stamped with the form revision when its callback ran.
#[derive(Debug, Clone)]
pub(crate) struct Event {
    /// The edit, focus, navigation, or submit action to apply.
    pub(crate) kind: Kind,
    revision: u64,
}

impl Event {
    /// Reject edits and focus actions queued before the most recent form reset.
    pub(crate) fn is_current(&self) -> bool {
        REVISION.with(|revision| revision.get() == self.revision)
    }
}

/// Browser-owned editing plus the small set of keys handled by the application.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    /// Complete native editor text, preserving zeros and intermediate input.
    Changed(&'static str, String),
    Focused(&'static str),
    /// Origin field, key, and modifiers before JavaScript moves focus for Tab.
    KeyPressed(&'static str, Key, Modifiers),
    /// Enter after the current editor value has been sent through the same queue.
    Submit,
}

type Callback = Closure<dyn FnMut(String, String, String, String, u8)>;

thread_local! {
    static CALLBACK: RefCell<Option<Callback>> = const { RefCell::new(None) };
    static REVISION: Cell<u64> = const { Cell::new(0) };
}

/// Install one retained JavaScript callback and stream browser events into Iced.
pub(crate) fn subscription() -> Subscription<Event> {
    Subscription::run(events)
}

fn events() -> impl Stream<Item = Event> {
    let (sender, receiver) = mpsc::unbounded();
    let callback = Closure::wrap(Box::new(
        move |kind: String, id: String, value: String, key: String, modifiers: u8| {
            let event = match kind.as_str() {
                "changed" => field_id(&id).map(|id| Kind::Changed(id, value)),
                "focused" => field_id(&id).map(Kind::Focused),
                "submit" => Some(Kind::Submit),
                "key" => match key.as_str() {
                    "Tab" => Some(Named::Tab),
                    "Escape" => Some(Named::Escape),
                    _ => None,
                }
                .and_then(|key| {
                    field_id(&id).map(|id| {
                        Kind::KeyPressed(id, Key::Named(key), keyboard_modifiers(modifiers))
                    })
                }),
                _ => None,
            };

            if let Some(kind) = event {
                let revision = REVISION.with(Cell::get);
                let _ = sender.unbounded_send(Event { kind, revision });
            }
        },
    ) as Box<dyn FnMut(String, String, String, String, u8)>);

    set_event_sink(callback.as_ref().unchecked_ref());
    CALLBACK.with(|stored| *stored.borrow_mut() = Some(callback));

    receiver
}

/// Focus a native editor, or the canvas for a non-input control ID.
///
/// Visible inputs are focused synchronously to preserve a browser user gesture.
/// Offscreen requests wait for a subsequent draw to reveal the field; a later
/// pointer action cancels the pending request.
pub(crate) fn focus(id: &'static str) {
    focus_element(id);
}

/// Install target, starting value, and reset-index text after a form reset.
///
/// Applying the values immediately distinguishes a reset from an older model
/// redraw, even when the reset repeats the previous value. New input can arrive
/// before the next draw without being overwritten by it.
pub(crate) fn reset(values: [&str; 3]) {
    REVISION.with(|revision| revision.set(revision.get().wrapping_add(1)));
    let values = values.into_iter().map(JsValue::from_str).collect();
    reset_pending(&values);
}

/// Identify actual DOM focus when an asynchronous Iced focus query may be stale.
pub(crate) fn active_field() -> Option<&'static str> {
    field_id(&active_field_id())
}

fn field_id(id: &str) -> Option<&'static str> {
    match id {
        "target-input" => Some("target-input"),
        "start-input" => Some("start-input"),
        "reset-input" => Some("reset-input"),
        _ => None,
    }
}

fn keyboard_modifiers(bits: u8) -> Modifiers {
    let mut modifiers = Modifiers::empty();

    for (bit, modifier) in [
        (1, Modifiers::SHIFT),
        (2, Modifiers::CTRL),
        (4, Modifiers::ALT),
        (8, Modifiers::LOGO),
    ] {
        if bits & bit != 0 {
            modifiers.insert(modifier);
        }
    }

    modifiers
}

/// Wrap the root page so each draw reconciles native editors and Iced overlays.
///
/// This wrapper must surround the full-window scrollable, where its own draw
/// cannot be culled together with the individual form fields.
pub(crate) fn layer<'a, Message: 'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    Element::new(Layer { content })
}

/// A transparent root widget that brackets drawing and wraps popup overlays.
struct Layer<'a, Message> {
    content: Element<'a, Message>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Layer<'_, Message> {
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
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &IcedEvent,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        begin_frame();
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);

        // Iced culls entirely offscreen children. Hide editors that were not
        // visited this frame instead of retaining their previous hit area.
        end_frame();
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(tree, layout, renderer, viewport, translation)
            .map(|content| wrap_overlay(content, *viewport))
    }
}

fn wrap_overlay<'a, Message: 'a>(
    content: overlay::Element<'a, Message, Theme, iced::Renderer>,
    viewport: Rectangle,
) -> overlay::Element<'a, Message, Theme, iced::Renderer> {
    overlay::Element::new(Box::new(Overlay { content, viewport }))
}

/// Hide only browser editors underneath a canvas popup, retaining their focus.
struct Overlay<'a, Message> {
    content: overlay::Element<'a, Message, Theme, iced::Renderer>,
    viewport: Rectangle,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for Overlay<'_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        self.content.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        occlude(layout, self.viewport);
        self.content
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &IcedEvent,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        self.content
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, clipboard, shell);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }

    fn overlay<'a>(
        &'a mut self,
        layout: Layout<'a>,
        renderer: &iced::Renderer,
    ) -> Option<overlay::Element<'a, Message, Theme, iced::Renderer>> {
        self.content
            .as_overlay_mut()
            .overlay(layout, renderer)
            .map(|content| wrap_overlay(content, self.viewport))
    }

    fn index(&self) -> f32 {
        self.content.as_overlay().index()
    }
}

/// Find painted popup bounds beneath Iced's full-window overlay group nodes.
fn occlude(layout: Layout<'_>, viewport: Rectangle) {
    let bounds = layout.bounds();
    let children = layout.children();

    if bounds == viewport && children.len() > 0 {
        // Iced's overlay groups span the window; their popup children carry
        // the actual painted bounds. Leave unrelated native editors visible.
        for child in children {
            occlude(child, viewport);
        }
    } else {
        occlude_fields(bounds.x, bounds.y, bounds.width, bounds.height);
    }
}

/// Preserve an Iced editor's geometry and focus while browser text sits over it.
///
/// The wrapped editor is still drawn as a fallback when an overlapping tooltip
/// hides its DOM counterpart. Its ID must be one of the three form field IDs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn field<'a, Message: 'a>(
    content: Element<'a, Message>,
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    invalid: bool,
    help: &'static str,
    error: Option<&str>,
) -> Element<'a, Message> {
    Element::new(Field {
        content,
        id,
        label,
        placeholder,
        value,
        invalid,
        help,
        error: error.map(str::to_owned),
    })
}

/// An editor's native metadata with its original Iced widget retained as a child.
struct Field<'a, Message> {
    content: Element<'a, Message>,
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    value: &'a str,
    invalid: bool,
    help: &'static str,
    error: Option<String>,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Field<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content]);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);

        layout::Node::with_children(child.size(), vec![child])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.children().next().unwrap(),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &IcedEvent,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.children().next().unwrap(),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // An overlay temporarily hides an overlapping browser editor, exposing
        // this matching canvas input underneath without obscuring the popup.
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.children().next().unwrap(),
            cursor,
            viewport,
        );

        let bounds = layout.bounds();
        let clip = bounds.intersection(viewport).unwrap_or_default();

        // These fields belong to the root, full-window scrollable. Its viewport
        // starts at the window origin, so subtracting it removes page scrolling.
        // This positioning is not intended for nested scrollable fields.
        let geometry: js_sys::Array = [
            bounds.x - viewport.x,
            bounds.y - viewport.y,
            bounds.width,
            bounds.height,
            clip.x - viewport.x,
            clip.y - viewport.y,
            clip.width,
            clip.height,
        ]
        .into_iter()
        .map(|value| JsValue::from_f64(f64::from(value)))
        .collect();
        let config: js_sys::Array = [
            JsValue::from_str(self.id),
            JsValue::from_str(self.label),
            JsValue::from_str(self.placeholder),
            JsValue::from_str(self.value),
            JsValue::from_bool(self.invalid),
            JsValue::from_str(self.help),
            JsValue::from_str(self.error.as_deref().unwrap_or("")),
        ]
        .into_iter()
        .collect();

        // Keep the Iced input's layout and focus operation, while the browser
        // owns editing, selection, composition, and its software keyboard.
        sync_field(&config, &geometry);
    }
}
