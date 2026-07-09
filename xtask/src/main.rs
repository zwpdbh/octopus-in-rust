use anyhow::Result;

mod apps;
mod args;
mod cargo;
mod deploy;
mod plugins;
mod project;
mod web;

fn main() -> Result<()> {
    match args::Task::parse()? {
        args::Task::App { app, command, rest } => match app {
            args::App::Qqbot => apps::qqbot::run(&command, &rest),
        },
        args::Task::Web {
            command,
            release,
            port,
        } => web::run(&command, release, port),
        args::Task::Global(args::GlobalCommand::Test) => cargo::test_workspace(),
    }
}
