//! `loren serve` (alias `ignite`). Upstream runs `rojo serve` or `argon serve`
//! for the Studio plugin; rbx-native's live sync is its built-in Argon
//! client, so a Rojo or Argon project is served with Argon (which reads Rojo
//! project files) and picked up from the editor's Argon dock.

use crate::commands::refresh::resolve_project_tool;
use crate::ctx::{CliError, CmdResult, Ctx, Tool};
use crate::project::require_project;
use crate::run::{describe_failure, RunOpts, Stdio};

pub fn serve_command(ctx: &Ctx, tool: Option<&str>) -> CmdResult {
    let root = require_project(&ctx.cwd)?;
    let tool = resolve_project_tool(ctx, &root, tool)?;
    if tool == Tool::None {
        ctx.log.info("This project uses Roblox Script Sync, which runs inside Studio: there is nothing to serve.");
        ctx.log
            .info("In Studio, sync the four Folders once (see README.md), then press Play.");
        return Ok(0);
    }
    let args = ctx.lanes.serve_args(Tool::Argon).map_err(CliError::new)?;
    let cmd = "argon";
    let line: Vec<&str> = std::iter::once(cmd)
        .chain(args.iter().map(String::as_str))
        .collect();
    ctx.log.info(&format!(
        "Serving with Argon (rbx-native live sync): {}  (Ctrl+C to stop)",
        line.join(" ")
    ));
    if tool == Tool::Rojo {
        ctx.log.info(
            "This project uses Rojo; rbx-native syncs through Argon, which reads the same default.project.json.",
        );
    }
    ctx.log.info("In RbxNative, open your place and press Connect in the Argon dock (or turn on Auto Connect).");
    let result = ctx.run.run(
        cmd,
        &args,
        &RunOpts {
            cwd: Some(root),
            stdio: Stdio::Inherit,
        },
    );
    if result.error.is_some() {
        ctx.log
            .error(&format!("{}.", describe_failure(cmd, &result)));
        ctx.log
            .info("Install Argon with: rokit add argon-rbx/argon");
        return Ok(1);
    }
    match result.status {
        Some(status) if status != 0 => {
            ctx.log
                .error(&format!("{}.", describe_failure(cmd, &result)));
            Ok(if (1..256).contains(&status) {
                status
            } else {
                1
            })
        }
        _ => Ok(0),
    }
}
