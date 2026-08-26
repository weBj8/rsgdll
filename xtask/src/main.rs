use std::error::Error;
use std::path::Path;

use xtask::{Realm, stage_artifact};

const USAGE: &str =
    "Usage: cargo xtask stage <server|client> <name> <target> <artifact> <destination>";

fn main() {
    if let Err(error) = run(std::env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let Some(command) = args.next() else {
        return Err(USAGE.into());
    };
    if command == "--help" || command == "-h" {
        println!("{USAGE}");
        return Ok(());
    }
    if command != "stage" {
        return Err(format!("unsupported xtask command: {command}\n{USAGE}").into());
    }
    let realm = required(&mut args, "realm")?;
    let name = required(&mut args, "name")?;
    let target = required(&mut args, "target")?;
    let artifact = required(&mut args, "artifact")?;
    let destination = required(&mut args, "destination")?;
    if args.next().is_some() {
        return Err(format!("too many arguments\n{USAGE}").into());
    }
    let staged = stage_artifact(
        Realm::parse(&realm)?,
        &name,
        &target,
        Path::new(&artifact),
        Path::new(&destination),
    )?;
    println!("{}", staged.display());
    Ok(())
}

fn required(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, Box<dyn Error>> {
    args.next()
        .ok_or_else(|| format!("missing {name}\n{USAGE}").into())
}
