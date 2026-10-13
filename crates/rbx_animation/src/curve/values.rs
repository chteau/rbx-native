//! The `ValuesAndTimes` binary string a saved `FloatCurve` or `MarkerCurve`
//! keeps its keys in. Roblox does not document it; the layout below was read
//! off real catalog emotes (10 517 `FloatCurve`s and every `MarkerCurve` of
//! 120 `CurveAnimation`s: no parse failure, no leftover byte, and writing the
//! parsed keys back gives the same bytes for every one of them).
//!
//! All little-endian:
//!
//! ```text
//! u32 2                 values section version
//! u32 n                 key count
//! n × value             FloatCurve: u8 interpolation (KeyInterpolationMode),
//!                         u8 tangent flags, f32 value, f32 left tangent,
//!                         f32 right tangent (14 bytes)
//!                       MarkerCurve: the marker's value, NUL-terminated
//! u32 1                 times section version
//! u32 n                 the key count again
//! n × u32               times, in ticks of 1/2400 s
//! ```
//!
//! Tangent flag bit 0 marks a set left tangent and bit 1 a set right one: a
//! first key only ever carries 0, 2 or 3 and a last key 0, 1 or 3. Both
//! slopes are stored even when unset (the engine's own auto slope, often the
//! slope to the next key), in value units per second: a 0→1 face curve over
//! 0.2 s stores 5.0. Those stored auto slopes are kept here only so a curve
//! writes back byte for byte; the model reads an unset tangent as `None`.

use super::{FloatCurve, FloatKey, Interpolation};

pub const TICKS_PER_SECOND: f32 = 2400.;

const VALUES_VERSION: u32 = 2;
const TIMES_VERSION: u32 = 1;
const LEFT_SET: u8 = 1;
const RIGHT_SET: u8 = 2;

/// One `FloatCurve` key exactly as stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawKey {
    pub ticks: u32,
    pub interpolation: u8,
    pub flags: u8,
    pub value: f32,
    pub left: f32,
    pub right: f32,
}

/// One `MarkerCurve` marker exactly as stored.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMarker {
    pub ticks: u32,
    pub value: Vec<u8>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let end = end.ok_or_else(|| format!("ValuesAndTimes ends early at byte {}", self.at))?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn expect(&mut self, what: &str, want: u32) -> Result<(), String> {
        match self.u32()? {
            got if got == want => Ok(()),
            got => Err(format!("ValuesAndTimes {what} is {got}, expected {want}")),
        }
    }

    /// The times section, checked against the values section's count.
    fn times(&mut self, n: usize) -> Result<Vec<u32>, String> {
        self.expect("times version", TIMES_VERSION)?;
        self.expect("time count", n as u32)?;
        let times = (0..n).map(|_| self.u32()).collect::<Result<Vec<_>, _>>()?;
        if self.at != self.bytes.len() {
            return Err(format!(
                "ValuesAndTimes has {} bytes past its keys",
                self.bytes.len() - self.at
            ));
        }
        Ok(times)
    }

    fn count(&mut self) -> Result<usize, String> {
        self.expect("values version", VALUES_VERSION)?;
        let n = self.u32()? as usize;
        // Each key takes at least five bytes, which bounds a corrupt count.
        if n > self.bytes.len() / 5 {
            return Err(format!("ValuesAndTimes claims {n} keys"));
        }
        Ok(n)
    }
}

pub fn read_float(bytes: &[u8]) -> Result<Vec<RawKey>, String> {
    let mut r = Reader { bytes, at: 0 };
    let n = r.count()?;
    let mut keys = Vec::with_capacity(n);
    for _ in 0..n {
        keys.push(RawKey {
            ticks: 0,
            interpolation: r.u8()?,
            flags: r.u8()?,
            value: r.f32()?,
            left: r.f32()?,
            right: r.f32()?,
        });
    }
    for (key, ticks) in keys.iter_mut().zip(r.times(n)?) {
        key.ticks = ticks;
    }
    Ok(keys)
}

pub fn read_markers(bytes: &[u8]) -> Result<Vec<RawMarker>, String> {
    let mut r = Reader { bytes, at: 0 };
    let n = r.count()?;
    let mut markers = Vec::with_capacity(n);
    for _ in 0..n {
        let rest = &r.bytes[r.at..];
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or("a marker value has no terminating NUL")?;
        markers.push(RawMarker {
            ticks: 0,
            value: r.take(len + 1)?[..len].to_vec(),
        });
    }
    for (marker, ticks) in markers.iter_mut().zip(r.times(n)?) {
        marker.ticks = ticks;
    }
    Ok(markers)
}

fn header(out: &mut Vec<u8>, n: usize) {
    out.extend(VALUES_VERSION.to_le_bytes());
    out.extend((n as u32).to_le_bytes());
}

fn times(out: &mut Vec<u8>, ticks: impl ExactSizeIterator<Item = u32>) {
    out.extend(TIMES_VERSION.to_le_bytes());
    out.extend((ticks.len() as u32).to_le_bytes());
    for t in ticks {
        out.extend(t.to_le_bytes());
    }
}

pub fn write_float(keys: &[RawKey]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + 18 * keys.len());
    header(&mut out, keys.len());
    for k in keys {
        out.extend([k.interpolation, k.flags]);
        for f in [k.value, k.left, k.right] {
            out.extend(f.to_le_bytes());
        }
    }
    times(&mut out, keys.iter().map(|k| k.ticks));
    out
}

pub fn write_markers(markers: &[RawMarker]) -> Vec<u8> {
    let mut out = Vec::new();
    header(&mut out, markers.len());
    for m in markers {
        out.extend(&m.value);
        out.push(0);
    }
    times(&mut out, markers.iter().map(|m| m.ticks));
    out
}

pub fn seconds(ticks: u32) -> f32 {
    ticks as f32 / TICKS_PER_SECOND
}

/// The editable curve; unset tangents become `None`.
pub fn to_curve(keys: &[RawKey]) -> FloatCurve {
    FloatCurve::new(
        keys.iter()
            .map(|k| FloatKey {
                time: seconds(k.ticks),
                value: k.value,
                interpolation: Interpolation::from_value(k.interpolation as u32),
                left_tangent: (k.flags & LEFT_SET != 0).then_some(k.left),
                right_tangent: (k.flags & RIGHT_SET != 0).then_some(k.right),
            })
            .collect(),
    )
}

fn ticks_of(time: f32) -> u32 {
    (time * TICKS_PER_SECOND).round().max(0.) as u32
}

/// Raw keys for `curve`. An unset tangent is stored as the slope to the next
/// key (0 on the last), which is what Roblox writes; a set one is kept as is.
pub fn from_curve(curve: &FloatCurve) -> Vec<RawKey> {
    let keys = &curve.keys;
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            let slope = keys.get(i + 1).map_or(0., |n| {
                let dt = (f64::from(ticks_of(n.time)) - f64::from(ticks_of(k.time)))
                    / f64::from(TICKS_PER_SECOND);
                if dt > 0. {
                    ((f64::from(n.value) - f64::from(k.value)) / dt) as f32
                } else {
                    0.
                }
            });
            let mut flags = 0;
            if k.left_tangent.is_some() {
                flags |= LEFT_SET;
            }
            if k.right_tangent.is_some() {
                flags |= RIGHT_SET;
            }
            RawKey {
                ticks: ticks_of(k.time),
                interpolation: k.interpolation.value() as u8,
                flags,
                value: k.value,
                left: k.left_tangent.unwrap_or(slope),
                right: k.right_tangent.unwrap_or(slope),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_curve_stores_auto_slopes_and_ticks() {
        let c = FloatCurve::new(vec![
            FloatKey::new(0., 0., Interpolation::Linear),
            FloatKey::new(0.2, 1., Interpolation::Linear),
        ]);
        let raw = from_curve(&c);
        assert_eq!((raw[0].ticks, raw[1].ticks), (0, 480));
        assert_eq!((raw[0].left, raw[0].right), (5., 5.));
        assert_eq!((raw[1].left, raw[1].right, raw[1].flags), (0., 0., 0));
    }

    #[test]
    fn from_curve_keeps_set_tangents() {
        let mut k = FloatKey::new(0., 0., Interpolation::Cubic);
        k.right_tangent = Some(-2.);
        let raw = from_curve(&FloatCurve::new(vec![k]));
        assert_eq!((raw[0].flags, raw[0].right, raw[0].left), (2, -2., 0.));
    }

    #[test]
    fn from_curve_of_nothing_is_empty() {
        assert!(from_curve(&FloatCurve::default()).is_empty());
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn reads_and_rewrites_a_face_curve_from_a_catalog_emote() {
        // LeftEyeClosed of emote animation 71620783740586: 0 → 1 over 0.2 s.
        let bytes = hex(
            "02000000020000000100000000000000a0400000a04001000000803f00000000\
             00000000010000000200000000000000e0010000",
        );
        let keys = read_float(&bytes).unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!((keys[0].ticks, keys[1].ticks), (0, 480));
        assert_eq!((keys[0].left, keys[0].right), (5., 5.));
        assert_eq!(write_float(&keys), bytes);
        let curve = to_curve(&keys);
        assert_eq!(curve.keys[1].time, 0.2);
        assert_eq!(curve.keys[0].left_tangent, None);
        assert_eq!(curve.keys[1].interpolation, Interpolation::Linear);
    }

    #[test]
    fn reads_and_rewrites_a_named_keyframe_marker() {
        let bytes = hex("0200000001000000456e64000100000001000000d05c0000");
        let markers = read_markers(&bytes).unwrap();
        assert_eq!(markers[0].value, b"End");
        assert_eq!(markers[0].ticks, 23760);
        assert_eq!(write_markers(&markers), bytes);
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        assert!(read_float(&hex("0300000000000000")).is_err());
        assert!(read_float(&hex("0200000001000000")).is_err());
        // A trailing byte past the times.
        assert!(read_float(&hex("02000000000000000100000000000000ff")).is_err());
        assert!(read_markers(&hex("0200000001000000456e64")).is_err());
    }
}
