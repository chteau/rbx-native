//! Command-line interface for the Roblox Cloud API: whoami, check the key's
//! scopes, list experiences, download a place, or fetch an asset by id.

use std::env::Args;
use std::process::ExitCode;

use rbx_cloud::{ApiKey, Client, Grant, Owner, Visibility};

fn main() -> ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "rbxcloud".to_string());
    let usage = format!("usage: {program} <whoami|check|list|download|asset> [args...]");

    let Some(command) = args.next() else {
        eprintln!("{usage}");
        return ExitCode::FAILURE;
    };

    let client = Client::new(ApiKey::from_env_or_config());

    let result = match command.as_str() {
        "whoami" => run_whoami(&client),
        "check" => run_check(&client),
        "list" => run_list(&client),
        "download" => run_download(&client, &mut args),
        "asset" => run_asset(&client, &mut args),
        other => Err(format!("unknown command '{other}'\n{usage}")),
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
    let experiences = &listing.experiences;
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

fn run_download(client: &Client, args: &mut Args) -> Result<(), String> {
    let place_id = next_u64_arg(args, "placeId")?;
    let out = args.next().ok_or("missing <out.rbxl> argument")?;

    let bytes = client
        .download_place(place_id)
        .map_err(|err| err.to_string())?;
    std::fs::write(&out, &bytes).map_err(|err| format!("failed to write '{out}': {err}"))?;
    println!("wrote {} bytes to {out}", bytes.len());
    Ok(())
}

fn run_asset(client: &Client, args: &mut Args) -> Result<(), String> {
    let asset_id = next_u64_arg(args, "assetId")?;
    let out = args.next().ok_or("missing <out> argument")?;

    let content = client.asset(asset_id).map_err(|err| err.to_string())?;
    std::fs::write(&out, &content.bytes)
        .map_err(|err| format!("failed to write '{out}': {err}"))?;
    match content.asset_type_id {
        Some(id) => println!("assetTypeId: {id}"),
        None => println!("assetTypeId: unknown"),
    }
    println!("wrote {} bytes to {out}", content.bytes.len());
    Ok(())
}

fn next_u64_arg(args: &mut Args, name: &str) -> Result<u64, String> {
    let raw = args
        .next()
        .ok_or_else(|| format!("missing <{name}> argument"))?;
    raw.parse::<u64>()
        .map_err(|_| format!("'{raw}' is not a valid {name}"))
}
