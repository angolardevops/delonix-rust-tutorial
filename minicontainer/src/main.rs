use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use minicontainer::error::{Error, Result};
use minicontainer::spec::Spec;
use minicontainer::state::{Store, reconcile};
use minicontainer::{container, fsutil, image};
use nix::sys::signal::Signal;

#[derive(Parser)]
#[command(name = "mc", version, about = "A minimal, rootless, daemonless OCI runtime")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

// region: cli
#[derive(Subcommand)]
enum Cmd {
    /// Write a default config.json into BUNDLE (rootfs must be BUNDLE/rootfs)
    Spec {
        bundle: PathBuf,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    /// Unpack an OCI image layout into a bundle (rootfs + config.json)
    Unpack {
        layout: PathBuf,
        bundle: PathBuf,
    },
    /// create + start + wait + delete; exits with the container's exit code
    Run {
        id: String,
        #[arg(short, long)]
        bundle: PathBuf,
    },
    Create {
        id: String,
        #[arg(short, long)]
        bundle: PathBuf,
    },
    Start {
        id: String,
    },
    /// Print the output captured for a container made with `create`
    Logs {
        id: String,
    },
    State {
        id: String,
    },
    Kill {
        id: String,
        #[arg(default_value = "KILL")]
        signal: String,
    },
    Delete {
        id: String,
        #[arg(short, long)]
        force: bool,
    },
    List,
}
// endregion

fn main() -> ExitCode {
    match real_main(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("mc: {e}");
            ExitCode::from(e.exit_code())
        }
    }
}

fn real_main(cli: Cli) -> Result<u8> {
    match cli.cmd {
        Cmd::Spec { bundle, args } => {
            let json = serde_json::to_vec_pretty(&Spec::new_default(args))
                .map_err(|source| Error::Json { path: bundle.clone(), source })?;
            fsutil::write_atomic(&bundle.join("config.json"), &json)?;
        }
        Cmd::Unpack { layout, bundle } => image::unpack(&layout, &bundle)?,
        Cmd::Create { id, bundle } => {
            container::create(&Store::open()?, &id, &bundle, container::Stdio::Log)?
        }
        Cmd::Start { id } => container::start(&Store::open()?, &id)?,
        Cmd::Run { id, bundle } => {
            let store = Store::open()?;
            container::create(&store, &id, &bundle, container::Stdio::Inherit)?;
            container::start(&store, &id)?;
            let code = container::wait_stopped(&store, &id).unwrap_or(255);
            let st = store.load(&id)?;
            container::delete(&store, &id, true)?;
            if st.oom_killed {
                eprintln!("mc: container was OOM-killed");
            }
            return Ok(u8::try_from(code & 0xff).unwrap_or(255));
        }
        Cmd::Logs { id } => {
            let store = Store::open()?;
            store.load(&id)?;
            let path = store.dir(&id)?.join("output.log");
            print!("{}", std::fs::read_to_string(&path).unwrap_or_default());
        }
        Cmd::State { id } => {
            let st = reconcile(Store::open()?.load(&id)?);
            println!("{}", serde_json::to_string_pretty(&st).unwrap_or_default());
        }
        Cmd::Kill { id, signal } => {
            let sig: Signal = format!("SIG{}", signal.trim_start_matches("SIG"))
                .parse()
                .map_err(|_| Error::Spec(format!("unknown signal {signal}")))?;
            container::kill_container(&Store::open()?, &id, sig)?;
        }
        Cmd::Delete { id, force } => container::delete(&Store::open()?, &id, force)?,
        Cmd::List => {
            for st in Store::open()?.list()? {
                let st = reconcile(st);
                println!("{:<20} {:<9} {:>8}  {}", st.id, st.status, st.pid, st.bundle.display());
            }
        }
    }
    Ok(0)
}
