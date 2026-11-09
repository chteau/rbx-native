//! `Lighting.ClockTime` and `TimeOfDay`: one clock, two spellings. A place
//! file only carries `TimeOfDay`, so `ClockTime` is worked out from it, and
//! writing either updates both. The quirks are Studio's, read off its
//! command bar.

use mlua::{Result, Value};
use rbx_dom::{Ref, Variant};

use crate::ctx::Ctx;

const DAY: i64 = 24 * 60 * 60;

/// `ClockTime` as Studio reports it, a 32-bit float of whole seconds.
pub(super) fn get(ctx: &Ctx, referent: Ref) -> Result<Option<Value>> {
    let dom = ctx.dom();
    let Some(instance) = dom.get(referent).filter(|i| i.class() == "Lighting") else {
        return Ok(None);
    };
    let stored = ctx.database().stored_or_default(instance, "TimeOfDay");
    let Some((_, Variant::String(text))) = stored else {
        return Ok(None);
    };
    Ok(seconds_of(text)
        .ok()
        .map(|seconds| Value::Number(f64::from(hours(seconds)))))
}

/// Applies a write to `ClockTime` or `TimeOfDay`. False when it isn't one
/// this module takes, which leaves the caller's usual error to report it.
pub(super) fn set(ctx: &Ctx, referent: Ref, name: &str, value: &Value) -> Result<bool> {
    let is_lighting = ctx
        .dom()
        .get(referent)
        .is_some_and(|instance| instance.class() == "Lighting");
    if !is_lighting {
        return Ok(false);
    }
    let seconds = match (name, value) {
        ("ClockTime", Value::Number(hours)) => seconds_from_hours(*hours),
        ("ClockTime", Value::Integer(hours)) => seconds_from_hours(*hours as f64),
        ("TimeOfDay", Value::String(text)) => {
            seconds_of(&text.to_str()?).map_err(mlua::Error::runtime)?
        }
        _ => return Ok(false),
    };
    let mut dom = ctx.dom_mut();
    let writes = [
        ("TimeOfDay", Variant::String(spelled(seconds))),
        // Stored too: the renderer reads it first.
        ("ClockTime", Variant::Float32(hours(seconds))),
    ];
    for (key, variant) in writes {
        dom.set_property(referent, key, variant)
            .map_err(|error| mlua::Error::runtime(error.to_string()))?;
    }
    Ok(true)
}

/// What a `TimeOfDay` string reads as in hours, or `None` where Studio
/// refuses the string.
pub fn hours_of(text: &str) -> Option<f32> {
    seconds_of(text).ok().map(hours)
}

/// The `TimeOfDay` string a `ClockTime` of `hours` comes out as.
pub fn time_of_day(hours: f64) -> String {
    spelled(seconds_from_hours(hours))
}

fn hours(seconds: i64) -> f32 {
    seconds as f32 / 3600.0
}

/// Anything outside a day is midnight, unlike `TimeOfDay`, which wraps.
fn seconds_from_hours(hours: f64) -> i64 {
    if (0.0..24.0).contains(&hours) {
        (hours * 3600.0).floor() as i64
    } else {
        0
    }
}

/// `"H"`, `"H:M"` or `"H:M:S"`; anything after a third field is ignored and
/// the total wraps around the day, negative hours included.
fn seconds_of(text: &str) -> std::result::Result<i64, &'static str> {
    if text.is_empty() {
        return Err("invalid string position");
    }
    let mut total = 0;
    for (field, unit) in text.split(':').zip([3600, 60, 1]) {
        let value: i64 = field.parse().map_err(|_| {
            "bad lexical cast: source type value could not be interpreted as target"
        })?;
        total += value * unit;
    }
    Ok(total.rem_euclid(DAY))
}

fn spelled(seconds: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use rbx_dom::WeakDom;
    use rbx_reflection::ReflectionDatabase;

    use crate::runtime::Runtime;

    /// Sets `property` to `value` and prints what Studio printed for it.
    fn after(property: &str, value: &str) -> String {
        let mut dom = WeakDom::new();
        dom.new_instance("Lighting", "Lighting", None);
        let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
        let source = format!(
            "local l = game.Lighting l.{property} = {value} print(l.ClockTime, l.TimeOfDay)"
        );
        runtime.run(&source).unwrap().lines().join("")
    }

    #[test]
    fn clock_time_matches_studio() {
        assert_eq!(after("ClockTime", "13.5"), "13.5 13:30:00");
        assert_eq!(after("ClockTime", "6.123"), "6.122777938842773 06:07:22");
        assert_eq!(after("ClockTime", "23.9999"), "23.99972152709961 23:59:59");
    }

    #[test]
    fn a_clock_time_outside_the_day_is_midnight() {
        for value in ["25", "24", "-1"] {
            assert_eq!(after("ClockTime", value), "0 00:00:00", "{value}");
        }
    }

    #[test]
    fn time_of_day_matches_studio() {
        assert_eq!(after("TimeOfDay", "'06:15:00'"), "6.25 06:15:00");
        assert_eq!(after("TimeOfDay", "'25:00:00'"), "1 01:00:00");
        assert_eq!(after("TimeOfDay", "'7'"), "7 07:00:00");
        assert_eq!(after("TimeOfDay", "'7:30'"), "7.5 07:30:00");
        assert_eq!(after("TimeOfDay", "'-3:00:00'"), "21 21:00:00");
        assert_eq!(
            after("TimeOfDay", "'1:2:3:4'"),
            "1.034166693687439 01:02:03"
        );
        assert_eq!(
            after("TimeOfDay", "'12:00:90'"),
            "12.024999618530273 12:01:30"
        );
    }

    #[test]
    fn a_time_of_day_it_cannot_read_fails_with_studios_words() {
        let mut dom = WeakDom::new();
        dom.new_instance("Lighting", "Lighting", None);
        let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
        let output = runtime
            .run(
                "local l = game.Lighting \
                 print(select(2, pcall(function() l.TimeOfDay = 'nope' end))) \
                 print(select(2, pcall(function() l.TimeOfDay = '' end)))",
            )
            .unwrap();
        let lines = output.lines().join("\n");
        assert!(lines.contains("bad lexical cast"), "{lines}");
        assert!(lines.contains("invalid string position"), "{lines}");
    }

    #[test]
    fn clock_time_is_read_from_the_time_of_day_a_file_carries() {
        let mut dom = WeakDom::new();
        let lighting = dom.new_instance("Lighting", "Lighting", None);
        dom.set_property(
            lighting,
            "TimeOfDay",
            rbx_dom::Variant::String("14:30:00".to_owned()),
        )
        .unwrap();
        let mut runtime = Runtime::new(dom, ReflectionDatabase::embedded()).unwrap();
        let output = runtime.run("print(game.Lighting.ClockTime)").unwrap();
        assert_eq!(output.lines(), ["14.5"]);
    }
}
