//! The command table (names, aliases, arguments, options) and the argument
//! parser that turns a command's raw arguments into [`Parsed`].

pub(super) struct Opt {
    pub(super) short: Option<char>,
    pub(super) long: &'static str,
    pub(super) arg: Option<&'static str>,
    pub(super) choices: bool,
    pub(super) desc: &'static str,
}

pub(super) struct Arg {
    pub(super) name: &'static str,
    pub(super) required: bool,
    pub(super) desc: &'static str,
}

pub(super) struct Cmd {
    pub(super) name: &'static str,
    pub(super) alias: Option<&'static str>,
    pub(super) desc: &'static str,
    pub(super) args: &'static [Arg],
    pub(super) opts: &'static [Opt],
}

const TOOL_CHOICES: [&str; 3] = ["rojo", "argon", "none"];

const TOOL: Opt = Opt {
    short: None,
    long: "tool",
    arg: Some("tool"),
    choices: true,
    desc: "sync tool (default: from .loren.json / rokit.toml / aftman.toml / foreman.toml)",
};
const FORCE: Opt = Opt {
    short: Some('f'),
    long: "force",
    arg: None,
    choices: false,
    desc: "replace an existing module of that name (backed up first)",
};
const YES: Opt = Opt {
    short: Some('y'),
    long: "yes",
    arg: None,
    choices: false,
    desc: "do not ask for confirmation",
};

pub(super) static COMMANDS: [Cmd; 10] = [
    Cmd {
        name: "init",
        alias: None,
        desc: "create a new Loren project (Rojo, Argon or Roblox Script Sync)",
        args: &[Arg {
            name: "name",
            required: true,
            desc: "folder to create",
        }],
        opts: &[
            TOOL,
            Opt {
                short: Some('y'),
                long: "yes",
                arg: None,
                choices: false,
                desc: "never prompt; use the defaults (Rojo unless --tool)",
            },
            Opt {
                short: None,
                long: "no-tools",
                arg: None,
                choices: false,
                desc: "skip the toolchain install and the sourcemap",
            },
        ],
    },
    Cmd {
        name: "make",
        alias: None,
        desc: "create a Service or Controller from the template",
        args: &[
            Arg {
                name: "type",
                required: true,
                desc: "service or controller",
            },
            Arg {
                name: "name",
                required: true,
                desc: "module name (a Luau identifier, PascalCase)",
            },
        ],
        opts: &[FORCE, TOOL],
    },
    Cmd {
        name: "inject",
        alias: None,
        desc: "copy a premade module into your project (see --list)",
        args: &[
            Arg {
                name: "type",
                required: false,
                desc: "service, controller or shared",
            },
            Arg {
                name: "name",
                required: false,
                desc: "premade name",
            },
        ],
        opts: &[
            Opt {
                short: Some('l'),
                long: "list",
                arg: None,
                choices: false,
                desc: "list the available premades",
            },
            FORCE,
            TOOL,
        ],
    },
    Cmd {
        name: "add",
        alias: None,
        desc: "download a GitHub repository into loren_packages",
        args: &[
            Arg {
                name: "repo",
                required: true,
                desc: "user/repo[#ref] or a GitHub URL",
            },
            Arg {
                name: "alias",
                required: false,
                desc: "folder name under loren_packages (default: the repo name)",
            },
        ],
        opts: &[
            Opt {
                short: Some('f'),
                long: "force",
                arg: None,
                choices: false,
                desc: "replace an existing package of that name (backed up first)",
            },
            TOOL,
        ],
    },
    Cmd {
        name: "refresh",
        alias: None,
        desc: "regenerate LorenTypes, then the sourcemap",
        args: &[],
        opts: &[
            Opt {
                short: Some('w'),
                long: "watch",
                arg: None,
                choices: false,
                desc: "keep regenerating while Services and Controllers change",
            },
            TOOL,
        ],
    },
    Cmd {
        name: "serve",
        alias: Some("ignite"),
        desc: "start the sync server (rojo serve / argon serve; Script Sync runs inside Studio)",
        args: &[],
        opts: &[TOOL],
    },
    Cmd {
        name: "migrate",
        alias: None,
        desc: "switch this project between Rojo and Argon",
        args: &[],
        opts: &[
            Opt {
                short: None,
                long: "to",
                arg: Some("tool"),
                choices: true,
                desc: "the tool to switch to (default: the other one)",
            },
            YES,
        ],
    },
    Cmd {
        name: "update",
        alias: None,
        desc: "",
        args: &[],
        opts: &[
            Opt {
                short: None,
                long: "dry-run",
                arg: None,
                choices: false,
                desc: "show what would change; change nothing",
            },
            YES,
            Opt {
                short: None,
                long: "no-native",
                arg: None,
                choices: false,
                desc: "install the runtime without @native attributes",
            },
            TOOL,
        ],
    },
    Cmd {
        name: "types",
        alias: None,
        desc: "regenerate LorenTypes (and LorenServerTypes) from your Services and Controllers",
        args: &[],
        opts: &[],
    },
    Cmd {
        name: "doctor",
        alias: None,
        desc: "check the toolchain and the project (including colon-style middleware and names two siblings share)",
        args: &[],
        opts: &[
            Opt {
                short: None,
                long: "fix",
                arg: None,
                choices: false,
                desc: "offer to rewrite colon-style middleware to dot style (with a backup)",
            },
            Opt {
                short: Some('y'),
                long: "yes",
                arg: None,
                choices: false,
                desc: "apply fixes without asking",
            },
        ],
    },
];

pub(super) fn opt_flags(opt: &Opt) -> String {
    let mut s = match opt.short {
        Some(c) => format!("-{c}, --{}", opt.long),
        None => format!("--{}", opt.long),
    };
    if let Some(arg) = opt.arg {
        s.push_str(&format!(" <{arg}>"));
    }
    s
}

pub(super) enum Stop {
    Help,
    Error(String),
}

#[derive(Default)]
pub(super) struct Parsed {
    args: Vec<String>,
    flags: Vec<&'static str>,
    values: Vec<(&'static str, String)>,
}

impl Parsed {
    pub(super) fn flag(&self, long: &str) -> bool {
        self.flags.contains(&long)
    }
    pub(super) fn value(&self, long: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| *k == long)
            .map(|(_, v)| v.as_str())
    }
    pub(super) fn arg(&self, i: usize) -> Option<&str> {
        self.args.get(i).map(String::as_str)
    }
}

fn take_value(opt: &'static Opt, value: String, out: &mut Parsed) -> Result<(), Stop> {
    if opt.choices && !TOOL_CHOICES.contains(&value.as_str()) {
        return Err(Stop::Error(format!(
            "option '{}' argument '{value}' is invalid. Allowed choices are {}.",
            opt_flags(opt),
            TOOL_CHOICES.join(", ")
        )));
    }
    out.values.push((opt.long, value));
    Ok(())
}

pub(super) fn parse_command(cmd: &'static Cmd, rest: &[String]) -> Result<Parsed, Stop> {
    let mut out = Parsed::default();
    let mut only_args = false;
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        i += 1;
        if only_args || a == "-" || !a.starts_with('-') {
            out.args.push(a.clone());
            continue;
        }
        if a == "--" {
            only_args = true;
            continue;
        }
        let missing =
            |opt: &Opt| Stop::Error(format!("option '{}' argument missing", opt_flags(opt)));
        if let Some(body) = a.strip_prefix("--") {
            let (name, inline) = match body.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (body, None),
            };
            if name == "help" {
                return Err(Stop::Help);
            }
            let Some(opt) = cmd.opts.iter().find(|o| o.long == name) else {
                return Err(Stop::Error(format!("unknown option '{a}'")));
            };
            if opt.arg.is_some() {
                let value = match inline {
                    Some(v) => v,
                    None if i < rest.len() && !rest[i].starts_with('-') => {
                        i += 1;
                        rest[i - 1].clone()
                    }
                    None => return Err(missing(opt)),
                };
                take_value(opt, value, &mut out)?;
            } else {
                out.flags.push(opt.long);
            }
        } else {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            while k < chars.len() {
                let c = chars[k];
                k += 1;
                if c == 'h' {
                    return Err(Stop::Help);
                }
                let Some(opt) = cmd.opts.iter().find(|o| o.short == Some(c)) else {
                    return Err(Stop::Error(format!("unknown option '-{c}'")));
                };
                if opt.arg.is_some() {
                    let tail: String = chars[k..].iter().collect();
                    let value = if !tail.is_empty() {
                        tail
                    } else if i < rest.len() && !rest[i].starts_with('-') {
                        i += 1;
                        rest[i - 1].clone()
                    } else {
                        return Err(missing(opt));
                    };
                    take_value(opt, value, &mut out)?;
                    break;
                }
                out.flags.push(opt.long);
            }
        }
    }
    if let Some(arg) = cmd
        .args
        .iter()
        .enumerate()
        .find(|(n, a)| a.required && *n >= out.args.len())
        .map(|(_, a)| a)
    {
        return Err(Stop::Error(format!(
            "missing required argument '{}'",
            arg.name
        )));
    }
    if out.args.len() > cmd.args.len() {
        let n = cmd.args.len();
        return Err(Stop::Error(format!(
            "too many arguments for '{}'. Expected {n} argument{} but got {}.",
            cmd.name,
            if n == 1 { "" } else { "s" },
            out.args.len()
        )));
    }
    Ok(out)
}

pub(super) fn find_command(name: &str) -> Option<&'static Cmd> {
    COMMANDS
        .iter()
        .find(|c| c.name == name || c.alias == Some(name))
}
