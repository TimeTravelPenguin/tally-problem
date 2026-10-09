use iced::advanced::widget::{
    Id, Operation, operate,
    operation::{Focusable, Outcome, Scrollable},
};
use iced::{Rectangle, Task, Vector};

pub fn reveal(target: &'static str) -> Task<Option<f32>> {
    operate(Reveal {
        target: target.into(),
        page: None,
        target_bounds: None,
    })
}

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
}
