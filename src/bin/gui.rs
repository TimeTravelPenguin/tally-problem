//! Iced application entry point shared by desktop and browser builds.
//!
//! The application model owns validation, background jobs, and sequence
//! playback. Platform-specific solver backends keep searches outside the UI
//! thread; browser builds additionally bridge text editing to native DOM inputs.

#[path = "../gui/app.rs"]
mod app;
#[path = "../gui/focus.rs"]
mod focus;
#[path = "../gui/model.rs"]
mod model;
#[path = "../gui/playback.rs"]
mod playback;
#[path = "../gui/playback_view.rs"]
mod playback_view;
#[path = "../gui/progress.rs"]
mod progress;
#[path = "../gui/solver.rs"]
mod solver;
#[cfg(target_arch = "wasm32")]
#[path = "../gui/web_input.rs"]
mod web_input;

/// Start the themed application with its update loop and input subscriptions.
fn main() -> iced::Result {
    iced::application(app::Planner::new, app::Planner::update, app::Planner::view)
        .title("Tally Puzzle Optimizer")
        .theme(iced::Theme::CatppuccinMocha)
        .subscription(app::Planner::subscription)
        .window_size((1040, 900))
        .run()
}
