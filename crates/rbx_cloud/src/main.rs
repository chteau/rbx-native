//! Command-line interface for the Roblox Cloud API: whoami, check the key's
//! scopes, list experiences, download a place, save a place file as a new
//! (unpublished) version, or fetch an asset by id.

use std::process::ExitCode;

use rbx_cloud::{ApiKey, Client, Grant, Owner, PublishMode, Visibility};

/// A fully parsed command line. Built before the keyring is touched, so a
/// typo or a missing argument never unlocks the user's credential store.
#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Whoami,
    Check,
    List,
    Download {
        place_id: u64,
        out: String,
    },
    Save {
        universe_id: u64,
        place_id: u64,
        file: String,
    },
    Asset {
        asset_id: u64,
        out: String,
    },
}

fn parse(mut args: impl Iterator<Item = String>, usage: &str) -> Result<Command, String> {
    let command = args.next().ok_or_else(|| usage.to_string())?;
    match command.as_str() {
        "--help" | "-h" => Ok(Command::Help),
        "whoami" => Ok(Command::Whoami),
        "check" => Ok(Command::Check),
        "list" => Ok(Command::List),
        "download" => Ok(Command::Download {
            place_id: next_u64_arg(&mut args, "placeId")?,
            out: args.next().ok_or("missing <out.rbxl> argument")?,
        }),
        "save" => Ok(Command::Save {
            universe_id: next_u64_arg(&mut args, "universeId")?,
            place_id: next_u64_arg(&mut args, "placeId")?,
            file: args.next().ok_or("missing <file.rbxl> argument")?,
        }),
        "asset" => Ok(Command::Asset {
            asset_id: next_u64_arg(&mut args, "assetId")?,
            out: args.next().ok_or("missing <out> argument")?,
        }),
        other => Err(format!("unknown command '{other}'\n{usage}")),
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "rbxcloud".to_string());
    let usage = format!("usage: {program} <whoami|check|list|download|save|asset> [args...]");

    let command = match parse(args, &usage) {
        Ok(Command::Help) => {
            println!("{usage}");
            return ExitCode::SUCCESS;
        }
        Ok(command) => command,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };

    if std::env::var_os("RBX_API_KEY").is_none() {
        ApiKey::install(rbx_cloud::keyring::stored_api_key());
    }
    let client = Client::new(ApiKey::from_env_or_config());

    let result = match command {
        Command::Help => Ok(()),
        Command::Whoami => run_whoami(&client),
        Command::Check => run_check(&client),
        Command::List => run_list(&client),
        Command::Download { place_id, out } => run_download(&client, place_id, &out),
        Command::Save {
            universe_id,
            place_id,
            file,
        } => run_save(&client, universe_id, place_id, &file),
        Command::Asset { asset_id, out } => run_asset(&client, asset_id, &out),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run_whoami(client: &Client) -> Result<(), String> {
    let info = client.introspect().map_err(|err| err.to_string())?;

    println!("name: {}", info.name);
    println!("authorized user id: {}", info.authorized_user_id);
    println!("enabled: {}  expired: {}", info.enabled, info.expired);
    println!("expires: {}", info.expiration_time_utc);
    for scope in &info.scopes {
        let operations = scope.operations.join(",");
        if scope.universe_ids.is_empty() {
            println!("  scope: {} [{operations}]", scope.name);
        } else {
            let ids = scope
                .universe_ids
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            println!("  scope: {} [{operations}] universes: {ids}", scope.name);
        }
    }
    Ok(())
}

/// The setup wizard's pass/fail list, on stdout. Exits non-zero when a
/// required permission is missing, so a script can gate on it.
fn run_check(client: &Client) -> Result<(), String> {
    let info = client.introspect().map_err(|err| err.to_string())?;
    let report = rbx_cloud::check_scopes(&info);
    if !report.usable {
        println!("key is disabled or expired");
    }
    for check in &report.checks {
        let status = match &check.grant {
            Grant::Missing if check.permission.required => "FAIL".to_string(),
            Grant::Missing => "----".to_string(),
            Grant::Everywhere => "PASS".to_string(),
            Grant::Universes(ids) => format!("PASS ({} universes)", ids.len()),
        };
        println!(
            "{status:<20} {:<48} {}",
            check.permission.scope, check.permission.feature
        );
    }
    if report.ready() {
        Ok(())
    } else {
        Err("the key is missing a required permission".to_string())
    }
}

fn run_list(client: &Client) -> Result<(), String> {
    let listing = client.list_experiences().map_err(|err| err.to_string())?;
    let mut experiences = listing.experiences.clone();
    // One group at a time: the group listing is rate-limited per IP, and
    // the client's retry waits out each 429.
    for group in &listing.groups {
        experiences.extend(
            client
                .group_experiences(group.id)
                .map_err(|err| err.to_string())?,
        );
    }
    let experiences = &experiences;
    if experiences.is_empty() {
        println!("no experiences found");
        return Ok(());
    }

    println!(
        "{:<14} {:<16} {:<9} {:<14} NAME",
        "UNIVERSE_ID", "ROOT_PLACE_ID", "VISIBILITY", "OWNER"
    );
    for experience in experiences {
        let owner = match experience.owner {
            Owner::User(id) => format!("user {id}"),
            Owner::Group(id) => format!("group {id}"),
        };
        println!(
            "{:<14} {:<16} {:<9} {:<14} {}",
            experience.universe_id,
            experience.root_place_id,
            format_visibility(&experience.visibility),
            owner,
            experience.name
        );
    }
    Ok(())
}

fn format_visibility(visibility: &Visibility) -> String {
    match visibility {
        Visibility::Public => "PUBLIC".to_string(),
        Visibility::Private => "PRIVATE".to_string(),
        Visibility::Other(raw) => raw.clone(),
    }
}

fn run_download(client: &Client, place_id: u64, out: &str) -> Result<(), String> {
    let bytes = client
        .download_place(place_id)
        .map_err(|err| err.to_string())?;
    std::fs::write(out, &bytes).map_err(|err| format!("failed to write '{out}': {err}"))?;
    println!("wrote {} bytes to {out}", bytes.len());
    Ok(())
}

/// `save <universeId> <placeId> <file>`: uploads `file` as a Saved version,
/// it does not go live, and prints the version number or Roblox's refusal.
fn run_save(client: &Client, universe_id: u64, place_id: u64, file: &str) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|err| format!("failed to read '{file}': {err}"))?;
    let version = client
        .publish_place(universe_id, place_id, &bytes, PublishMode::Saved)
        .map_err(|err| err.to_string())?;
    println!("saved as version {version}");
    Ok(())
}

fn run_asset(client: &Client, asset_id: u64, out: &str) -> Result<(), String> {
    let content = client.asset(asset_id).map_err(|err| err.to_string())?;
    std::fs::write(out, &content.bytes).map_err(|err| format!("failed to write '{out}': {err}"))?;
    match content.asset_type_id {
        Some(id) => println!("assetTypeId: {id}"),
        None => println!("assetTypeId: unknown"),
    }
    println!("wrote {} bytes to {out}", content.bytes.len());
    Ok(())
}

fn next_u64_arg(args: &mut impl Iterator<Item = String>, name: &str) -> Result<u64, String> {
    let raw = args
        .next()
        .ok_or_else(|| format!("missing <{name}> argument"))?;
    raw.parse::<u64>()
        .map_err(|_| format!("'{raw}' is not a valid {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &[&str]) -> Result<Command, String> {
        parse(line.iter().map(|s| s.to_string()), "usage")
    }

    #[test]
    fn help_flags_ask_for_the_usage_text() {
        assert_eq!(parsed(&["--help"]), Ok(Command::Help));
        assert_eq!(parsed(&["-h"]), Ok(Command::Help));
    }

    #[test]
    fn unknown_command_fails_before_any_key_is_needed() {
        let err = parsed(&["bogus"]).unwrap_err();
        assert!(err.starts_with("unknown command 'bogus'"));
        assert_eq!(parsed(&[]).unwrap_err(), "usage");
    }

    #[test]
    fn known_commands_check_their_arguments_up_front() {
        assert_eq!(parsed(&["whoami"]), Ok(Command::Whoami));
        assert_eq!(
            parsed(&["download", "7", "o.rbxl"]),
            Ok(Command::Download {
                place_id: 7,
                out: "o.rbxl".into()
            })
        );
        assert!(parsed(&["download", "x", "o"])
            .unwrap_err()
            .contains("placeId"));
        assert!(parsed(&["save", "1", "2"])
            .unwrap_err()
            .contains("file.rbxl"));
        assert!(parsed(&["asset", "5"]).unwrap_err().contains("<out>"));
    }
}
