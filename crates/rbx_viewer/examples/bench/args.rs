//! Command line for the benchmark, parsed by hand the way `crate::cli` parses
//! the viewer's own — one small option set, no argument-parsing dependency
//! pulled in for a developer tool.

use std::path::PathBuf;

/// Where the out-of-repo fixtures live when `--fixture` says nothing else, kept
/// spelled the same as `scripts/shots.sh` so one environment variable points
/// both tools at the same directory.
const FIXTURES: &str = "../rbx-native-fixtures/places";

pub(crate) struct Args {
    /// Place files to measure, in order. A path that does not exist is
    /// reported as skipped rather than failing the run: the interesting
    /// fixture deliberately lives outside this repository.
    pub(crate) fixtures: Vec<PathBuf>,
    pub(crate) json: PathBuf,
    pub(crate) size: (u32, u32),
    /// Quality levels the frame benchmark walks. Not all 21 by default: the
    /// bands only change the renderer at a handful of steps (see
    /// `quality::table`), and twenty-one nearly identical rows hide the three
    /// that differ.
    pub(crate) levels: Vec<u8>,
    /// Downloads (textures, materials, file meshes) on or off. On by default:
    /// with them off a place loads a scene of plain boxes, which is not the
    /// scene anybody waits for. Worth running both ways — on this machine the
    /// two answers differ by more than a factor of two, and only one of them is
    /// steady enough to regression-test against (see BENCHMARKS.md).
    pub(crate) textures: bool,
    pub(crate) load_iters: usize,
    pub(crate) reload_iters: usize,
    pub(crate) patch_iters: usize,
    pub(crate) frame_iters: usize,
    pub(crate) frame_warmup: usize,
}

impl Default for Args {
    fn default() -> Self {
        let fixtures = std::env::var("RBX_FIXTURES").unwrap_or_else(|_| FIXTURES.to_string());
        Args {
            fixtures: vec![
                PathBuf::from("assets/tests/TestPlace.rbxl"),
                PathBuf::from(fixtures).join("marked.rbxl"),
            ],
            json: PathBuf::from("target/bench/bench.json"),
            size: (1280, 720),
            levels: vec![1, 11, 21],
            textures: true,
            load_iters: 5,
            // More than the other phases need, because this one's distribution
            // is not unimodal: resolving a place's assets sometimes costs a
            // fifth again as much as usual, and too few samples let the median
            // land in whichever mode happened to dominate that run.
            reload_iters: 25,
            patch_iters: 50,
            frame_iters: 200,
            frame_warmup: 40,
        }
    }
}

impl Args {
    pub(crate) fn parse(mut raw: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut args = Args::default();
        // Replaced wholesale by the first `--fixture`, so naming one fixture
        // measures that one alone instead of adding to the default pair.
        let mut fixtures = Vec::new();

        while let Some(flag) = raw.next() {
            let mut value = || {
                raw.next()
                    .ok_or_else(|| format!("{flag} needs a value"))
                    .map_err(|err| err.to_string())
            };
            match flag.as_str() {
                "--help" | "-h" => return Ok(None),
                "--fixture" => fixtures.push(PathBuf::from(value()?)),
                "--json" => args.json = PathBuf::from(value()?),
                "--size" => args.size = size(&value()?)?,
                "--levels" => args.levels = levels(&value()?)?,
                "--no-textures" => args.textures = false,
                "--load-iters" => args.load_iters = count(&flag, &value()?)?,
                "--reload-iters" => args.reload_iters = count(&flag, &value()?)?,
                "--patch-iters" => args.patch_iters = count(&flag, &value()?)?,
                "--frame-iters" => args.frame_iters = count(&flag, &value()?)?,
                "--frame-warmup" => args.frame_warmup = number(&flag, &value()?)?,
                other => return Err(format!("unknown option '{other}'")),
            }
        }

        if !fixtures.is_empty() {
            args.fixtures = fixtures;
        }
        Ok(Some(args))
    }

    pub(crate) fn usage() -> String {
        format!(
            "usage: cargo run --release --example bench -p rbx_viewer -- [options]\n\
             \n\
             options:\n\
             \x20 --fixture <path>     place file to measure; repeatable, replaces the default pair\n\
             \x20 --json <path>        machine-readable results (default: target/bench/bench.json)\n\
             \x20 --size <WxH>         frame size (default: 1280x720)\n\
             \x20 --levels <list>      quality levels for the frame benchmark, or 'all' (default: 1,11,21)\n\
             \x20 --no-textures        skip every texture, material and file-mesh resolve\n\
             \x20 --load-iters <n>     cold loads per fixture (default: {})\n\
             \x20 --reload-iters <n>   reloads per fixture (default: {})\n\
             \x20 --patch-iters <n>    patches and edits per fixture (default: {})\n\
             \x20 --frame-iters <n>    measured frames per quality level (default: {})\n\
             \x20 --frame-warmup <n>   discarded frames before each level (default: {})\n\
             \n\
             Fixtures default to assets/tests/TestPlace.rbxl and $RBX_FIXTURES/marked.rbxl\n\
             (RBX_FIXTURES defaults to {FIXTURES}), both resolved from the working\n\
             directory, so run this from the repository root.",
            Args::default().load_iters,
            Args::default().reload_iters,
            Args::default().patch_iters,
            Args::default().frame_iters,
            Args::default().frame_warmup,
        )
    }
}

fn size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("'{text}' is not a WIDTHxHEIGHT size"))?;
    let parse = |part: &str| {
        part.trim()
            .parse::<u32>()
            .ok()
            .filter(|side| *side > 0)
            .ok_or_else(|| format!("'{text}' is not a WIDTHxHEIGHT size"))
    };
    Ok((parse(width)?, parse(height)?))
}

fn levels(text: &str) -> Result<Vec<u8>, String> {
    if text.trim().eq_ignore_ascii_case("all") {
        return Ok((1..=21).collect());
    }
    text.split(',')
        .map(|part| {
            part.trim()
                .parse::<u8>()
                .ok()
                .filter(|level| (1..=21).contains(level))
                .ok_or_else(|| format!("'{part}' is not a quality level between 1 and 21"))
        })
        .collect()
}

/// A count that must actually produce a sample: a zero-iteration phase would
/// print a row of empty statistics rather than saying it measured nothing.
fn count(flag: &str, text: &str) -> Result<usize, String> {
    let value = number(flag, text)?;
    if value == 0 {
        return Err(format!("{flag} must be at least 1"));
    }
    Ok(value)
}

fn number(flag: &str, text: &str) -> Result<usize, String> {
    text.trim()
        .parse::<usize>()
        .map_err(|_| format!("{flag} needs a whole number, not '{text}'"))
}

#[cfg(test)]
mod tests {
    use super::{levels, size, Args};

    #[test]
    fn a_named_fixture_replaces_the_default_pair() {
        let args = Args::parse(["--fixture", "a.rbxl"].into_iter().map(String::from))
            .expect("parses")
            .expect("not --help");
        assert_eq!(args.fixtures.len(), 1);
    }

    #[test]
    fn sizes_and_levels_are_rejected_rather_than_clamped() {
        assert!(size("1280").is_err());
        assert!(size("1280x0").is_err());
        assert_eq!(size("800x600").expect("valid"), (800, 600));
        assert!(levels("22").is_err());
        assert_eq!(levels("1, 21").expect("valid"), vec![1, 21]);
        assert_eq!(levels("all").expect("valid").len(), 21);
    }
}
