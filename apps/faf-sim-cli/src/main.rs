//! Launcher for the FAF eco/build simulator.

#![cfg_attr(target_arch = "wasm32", no_main)]

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::net::SocketAddr;
    use std::path::PathBuf;

    use axum::Router;
    use clap::{Parser, Subcommand};
    use tower_http::services::ServeDir;

    #[derive(Parser, Debug)]
    #[command(name = "faf-sim", about = "Interactive FAF eco/build simulator")]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Subcommand, Debug)]
    enum Command {
        /// Launch the interactive simulator GUI natively.
        Run,
        /// Serve the WASM build with an embedded Axum web server.
        Serve {
            /// Port to listen on.
            #[arg(short, long, default_value = "8080")]
            port: u16,
            /// Directory containing the WASM build and `index.html`.
            #[arg(short, long, default_value = "apps/faf-sim-cli/web")]
            dir: PathBuf,
        },
    }

    pub fn main() {
        let cli = Cli::parse();
        match cli.command {
            Command::Run => faf_sim::run_app(),
            Command::Serve { port, dir } => {
                run_server(port, dir);
            }
        }
    }

    fn run_server(port: u16, dir: PathBuf) {
        tracing_subscriber::fmt::init();

        let index_path = dir.join("index.html");
        if !index_path.exists() {
            eprintln!(
                "Could not read {}. Did you run wasm-bindgen first?",
                index_path.display()
            );
            std::process::exit(1);
        }

        let app = Router::new().nest_service("/", ServeDir::new(dir));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .expect("bind port");
            println!(
                "Serving FAF Eco Sim at http://{}",
                listener.local_addr().unwrap()
            );
            axum::serve(listener, app).await.expect("server");
        });
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        faf_sim::run_app();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    native::main();
}
