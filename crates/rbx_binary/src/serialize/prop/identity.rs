//! UniqueId encoding, the encode counterpart of `chunks::prop::identity`.

use rbx_dom::{UniqueId, Variant};

use crate::codec::{interleave_bytes, rotated_i64_bits};
use crate::serialize::prop::map_dense;
use crate::serialize::SerializeError;

// The interleaving unit is the whole 16-byte record, not its individual fields: see
// `chunks::prop::identity` for why writing three separate interleaved blocks would
// scramble any array of more than one instance.
pub(super) fn unique_ids(
    class: &str,
    name: &str,
    values: &[Option<Variant>],
) -> Result<Vec<u8>, SerializeError> {
    let ids = map_dense(class, name, values, |v| match v {
        Variant::UniqueId(id) => Some(*id),
        _ => None,
    })?;

    let records: Vec<[u8; 16]> = ids
        .into_iter()
        .map(|id: UniqueId| {
            // `random` is bit-rotated, not zigzag encoded, so the sign bit stays out
            // of the high interleaved column the same way it does for floats.
            let raw = rotated_i64_bits(id.random);
            let mut record = [0u8; 16];
            record[0..4].copy_from_slice(&id.index.to_be_bytes());
            record[4..8].copy_from_slice(&id.time.to_be_bytes());
            record[8..12].copy_from_slice(&((raw >> 32) as u32).to_be_bytes());
            record[12..16].copy_from_slice(&(raw as u32).to_be_bytes());
            record
        })
        .collect();

    Ok(interleave_bytes(&records))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::prop::{decode, PropHeader};

    fn decoded(type_id: u8, count: usize, payload: &[u8]) -> Vec<Option<Variant>> {
        let header = PropHeader {
            class_id: 0,
            name: "Test".to_owned(),
            type_id,
            payload,
        };
        decode(&header, count, &[])
    }

    #[test]
    fn unique_id_round_trips_two_records_including_a_high_bit_random() {
        let values = vec![
            Some(Variant::UniqueId(UniqueId {
                index: 2,
                time: 0x09e5_50b2,
                random: 0x0038_2dbe_99a6_3c35,
            })),
            Some(Variant::UniqueId(UniqueId {
                index: 0x37d,
                time: 0x09e5_5048,
                random: 0x48eb_8569_9458_3300,
            })),
        ];
        let payload = unique_ids("Test", "Prop", &values).unwrap();
        assert_eq!(decoded(0x1F, 2, &payload), values);
    }
}
