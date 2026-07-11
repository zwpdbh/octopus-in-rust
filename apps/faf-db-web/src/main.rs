mod app;
mod components;
mod pages;
mod route;
mod state;
mod types;
mod utils;

fn main() {
    console_error_panic_hook::set_once();
    dioxus::launch(app::App);
}
