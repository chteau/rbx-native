//! `Lighting`'s two spellings of one clock, kept together when the panel
//! edits either. `TimeOfDay` is the one a place file carries; a `ClockTime`
//! only exists in the DOM once something has set it, but the renderer reads
//! it first, so a stale one would hold the sky still.

use std::collections::BTreeMap;

use rbx_dom::Variant;

/// The other half of a write to `prop_name` on a `class`, and its value.
pub(super) fn synced(
    class: &str,
    prop_name: &str,
    value: &Variant,
    properties: &BTreeMap<String, Variant>,
) -> Option<(&'static str, Variant)> {
    if class != "Lighting" {
        return None;
    }
    match (prop_name, value) {
        ("ClockTime", Variant::Float32(hours)) => Some((
            "TimeOfDay",
            Variant::String(rbx_lua::clock::time_of_day(f64::from(*hours))),
        )),
        ("ClockTime", Variant::Float64(hours)) => Some((
            "TimeOfDay",
            Variant::String(rbx_lua::clock::time_of_day(*hours)),
        )),
        ("TimeOfDay", Variant::String(text)) => {
            let hours = rbx_lua::clock::hours_of(text)?;
            Some((
                "ClockTime",
                match properties.get("ClockTime")? {
                    Variant::Float64(_) => Variant::Float64(f64::from(hours)),
                    _ => Variant::Float32(hours),
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_clock() -> BTreeMap<String, Variant> {
        BTreeMap::from([("ClockTime".to_owned(), Variant::Float32(1.0))])
    }

    #[test]
    fn a_clock_time_edit_moves_the_time_of_day_that_gets_saved() {
        assert_eq!(
            synced(
                "Lighting",
                "ClockTime",
                &Variant::Float32(13.5),
                &BTreeMap::new()
            ),
            Some(("TimeOfDay", Variant::String("13:30:00".to_owned())))
        );
    }

    #[test]
    fn a_time_of_day_edit_moves_a_clock_time_only_where_there_is_one() {
        let value = Variant::String("06:15:00".to_owned());
        assert_eq!(
            synced("Lighting", "TimeOfDay", &value, &with_clock()),
            Some(("ClockTime", Variant::Float32(6.25)))
        );
        assert_eq!(
            synced("Lighting", "TimeOfDay", &value, &BTreeMap::new()),
            None
        );
    }

    #[test]
    fn other_classes_are_left_alone() {
        let value = Variant::Float32(13.5);
        assert_eq!(synced("Part", "ClockTime", &value, &with_clock()), None);
    }
}
