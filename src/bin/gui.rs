#[path = "../gui/app.rs"]
mod app;
#[path = "../gui/focus.rs"]
mod focus;
#[path = "../gui/model.rs"]
mod model;

fn main() -> iced::Result {
    iced::application(app::Planner::new, app::Planner::update, app::Planner::view)
        .title("Tally Puzzle Optimizer")
        .theme(iced::Theme::CatppuccinMocha)
        .subscription(app::Planner::subscription)
        .window_size((1040, 900))
        .run()
}
