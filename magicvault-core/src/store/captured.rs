//! The resident form of a captured entry: its fields packed into one wiped
//! arena instead of a hash map of owned strings.
//!
//! A captured partition holds hundreds of thousands of browser-storage fields.
//! As a `HashMap<String, String>` each one cost a hash-table slot and two heap
//! allocations, about 165 bytes for about 100 bytes of payload, resident for
//! the life of the process. Here an entry's keys and values sit back to back in
//! one zeroizing `String`, indexed by a sorted table of 16-byte [`Slot`]s, so a
//! field costs its payload, minus its kind prefix, plus 16 bytes, and an entry
//! costs a few allocations instead of two per field.
//!
//! Keys are stored without their kind prefix (`header:`, `query_param:`,
//! `cookie:`, `local_storage:`, `session_storage:`). Any other key is kept
//! whole as [`FieldKind::Raw`], so every key round-trips exactly. Slots are
//! sorted by the full key, which keeps each kind's fields contiguous and makes
//! the serialized field order deterministic.
//!
//! Serialization writes the same JSON object a `HashMap<String, String>` does,
//! fields in ascending key order, so the vault file format is unchanged.
//! Deserialization compacts each entry as its `fields` object is parsed, so a
//! load never holds the partition as owned strings beside its compact form.
//!
//! Values never appear in a `Debug` impl, a panic message or an error.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::de::{self, DeserializeSeed, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::{Zeroize, Zeroizing};

use crate::policy::SecretPolicy;
use crate::{InjectionTarget, SecretSource};

/// Which family a captured field belongs to, by its key prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FieldKind {
    /// A key with none of the known prefixes, kept whole.
    Raw,
    Header,
    QueryParam,
    Cookie,
    LocalStorage,
    SessionStorage,
}

impl FieldKind {
    const PREFIXED: [FieldKind; 5] = [
        FieldKind::Header,
        FieldKind::QueryParam,
        FieldKind::Cookie,
        FieldKind::LocalStorage,
        FieldKind::SessionStorage,
    ];

    pub(crate) fn prefix(self) -> &'static str {
        match self {
            FieldKind::Raw => "",
            FieldKind::Header => "header:",
            FieldKind::QueryParam => "query_param:",
            FieldKind::Cookie => "cookie:",
            FieldKind::LocalStorage => "local_storage:",
            FieldKind::SessionStorage => "session_storage:",
        }
    }

    /// Split a full key into its kind and the key without the prefix. No
    /// prefix is a prefix of another, so the split is unique, and a `Raw` key
    /// never starts with a known prefix.
    pub(crate) fn classify(key: &str) -> (FieldKind, &str) {
        for kind in Self::PREFIXED {
            if let Some(rest) = key.strip_prefix(kind.prefix()) {
                return (kind, rest);
            }
        }
        (FieldKind::Raw, key)
    }
}

/// One field: its stripped key at `key_off..key_off + key_len` in the arena,
/// immediately followed by its value of `val_len` bytes. 16 bytes.
#[derive(Clone, Copy)]
struct Slot {
    key_off: u32,
    key_len: u32,
    val_len: u32,
    kind: FieldKind,
}

const _: () = assert!(std::mem::size_of::<Slot>() == 16);

impl Slot {
    fn key_range(self) -> std::ops::Range<usize> {
        let start = self.key_off as usize;
        start..start + self.key_len as usize
    }

    fn value_range(self) -> std::ops::Range<usize> {
        let start = self.key_off as usize + self.key_len as usize;
        start..start + self.val_len as usize
    }
}

/// Compare two fields by their full keys, prefix included, without building
/// either key.
fn full_key_cmp(left: (FieldKind, &str), right: (FieldKind, &str)) -> Ordering {
    left.0
        .prefix()
        .bytes()
        .chain(left.1.bytes())
        .cmp(right.0.prefix().bytes().chain(right.1.bytes()))
}

/// The error for an entry whose fields do not fit 32-bit arena offsets. It
/// names no key and no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArenaFull;

impl fmt::Display for ArenaFull {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("captured entry fields exceed the 4 GiB field arena")
    }
}

/// A captured entry's fields. Immutable once built; a write builds a new one.
#[derive(Default)]
pub(crate) struct CapturedFields {
    /// Every stripped key and value, back to back. Wiped on drop, over its
    /// whole capacity.
    bytes: Zeroizing<String>,
    /// Sorted by full key, one per field, no duplicate keys.
    slots: Box<[Slot]>,
}

impl CapturedFields {
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Bytes of keys and values held, prefixes excluded.
    #[cfg(test)]
    pub(crate) fn arena_len(&self) -> usize {
        self.bytes.len()
    }

    /// Bytes the arena allocation holds.
    #[cfg(test)]
    pub(crate) fn arena_capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Bytes of the slot table.
    #[cfg(test)]
    pub(crate) fn slot_table_bytes(&self) -> usize {
        std::mem::size_of_val(&*self.slots)
    }

    fn text(&self, range: std::ops::Range<usize>) -> &str {
        // Every slot boundary is the end of a whole `&str` pushed by the
        // builder, so this never fails; the message names no content.
        self.bytes
            .get(range)
            .expect("captured field arena slot ends on a char boundary")
    }

    fn key(&self, slot: Slot) -> &str {
        self.text(slot.key_range())
    }

    fn value(&self, slot: Slot) -> &str {
        self.text(slot.value_range())
    }

    /// Every field as `(kind, key without prefix, value)`, in full-key order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (FieldKind, &str, &str)> + '_ {
        self.slots
            .iter()
            .map(move |slot| (slot.kind, self.key(*slot), self.value(*slot)))
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &str> + '_ {
        self.slots.iter().map(move |slot| self.value(*slot))
    }

    /// The contiguous run of slots of one prefixed kind.
    fn kind_slots(&self, kind: FieldKind) -> &[Slot] {
        debug_assert!(kind != FieldKind::Raw, "raw keys are not contiguous");
        let prefix = kind.prefix();
        let start = self.slots.partition_point(|slot| {
            full_key_cmp((slot.kind, self.key(*slot)), (FieldKind::Raw, prefix)) == Ordering::Less
        });
        let rest = &self.slots[start..];
        &rest[..rest.partition_point(|slot| slot.kind == kind)]
    }

    pub(crate) fn has_kind(&self, kind: FieldKind) -> bool {
        !self.kind_slots(kind).is_empty()
    }

    /// The fields of one prefixed kind as `(key without prefix, value)`.
    pub(crate) fn of_kind(&self, kind: FieldKind) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.kind_slots(kind)
            .iter()
            .map(move |slot| (self.key(*slot), self.value(*slot)))
    }

    /// The value of one field, by kind and key without prefix.
    pub(crate) fn get(&self, kind: FieldKind, key: &str) -> Option<&str> {
        self.slots
            .binary_search_by(|slot| full_key_cmp((slot.kind, self.key(*slot)), (kind, key)))
            .ok()
            .map(|index| self.value(self.slots[index]))
    }

    /// The fields as the plain map the entry used to hold, full keys and all.
    #[cfg(test)]
    pub(crate) fn to_field_map(&self) -> HashMap<String, String> {
        self.iter()
            .map(|(kind, key, value)| (format!("{}{key}", kind.prefix()), value.to_string()))
            .collect()
    }

    /// Compact a plain field map, classifying each key by its prefix.
    #[cfg(test)]
    pub(crate) fn from_field_map(fields: &HashMap<String, String>) -> Result<Self, ArenaFull> {
        let bytes = fields.iter().map(|(key, value)| key.len() + value.len()).sum();
        let mut builder = FieldsBuilder::with_capacity(bytes, fields.len());
        for (key, value) in fields {
            let (kind, rest) = FieldKind::classify(key);
            builder.push(kind, &[rest], value)?;
        }
        Ok(builder.finish())
    }
}

/// Content equality: the same keys with the same values. Both sides are in
/// canonical order, so this compares slot by slot.
impl PartialEq for CapturedFields {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other.iter())
                .all(|(left, right)| left == right)
    }
}

impl Eq for CapturedFields {}

/// Counts only: no key, no value.
impl fmt::Debug for CapturedFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedFields")
            .field("fields", &self.slots.len())
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// The same JSON object a `HashMap<String, String>` of full keys writes, in
/// ascending key order. Values are written straight from the arena; each full
/// key is assembled in one wiped scratch buffer sized up front, so it never
/// reallocates.
impl Serialize for CapturedFields {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let longest_key = self
            .slots
            .iter()
            .map(|slot| slot.kind.prefix().len() + slot.key_len as usize)
            .max()
            .unwrap_or(0);
        let mut key = Zeroizing::new(String::with_capacity(longest_key));
        let mut map = serializer.serialize_map(Some(self.slots.len()))?;
        for (kind, stripped, value) in self.iter() {
            key.clear();
            key.push_str(kind.prefix());
            key.push_str(stripped);
            map.serialize_entry(key.as_str(), value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CapturedFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(FieldsVisitor)
    }
}

struct FieldsVisitor;

impl<'de> Visitor<'de> for FieldsVisitor {
    type Value = CapturedFields;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map of captured field names to string values")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut builder = FieldsBuilder::with_capacity(256, map.size_hint().unwrap_or(0));
        while let Some(pending) = map.next_key_seed(KeySeed(&mut builder))? {
            let val_len = map.next_value_seed(ValueSeed(&mut builder))?;
            builder.slots.push(Slot {
                key_off: pending.key_off,
                key_len: pending.key_len,
                val_len,
                kind: pending.kind,
            });
        }
        Ok(builder.finish())
    }
}

/// A key appended to the arena whose value is still to come.
struct PendingKey {
    key_off: u32,
    key_len: u32,
    kind: FieldKind,
}

/// Appends a field key, without its prefix, straight into the arena.
struct KeySeed<'builder>(&'builder mut FieldsBuilder);

impl<'de> DeserializeSeed<'de> for KeySeed<'_> {
    type Value = PendingKey;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for KeySeed<'_> {
    type Value = PendingKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a captured field name")
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<Self::Value, E> {
        let (kind, stripped) = FieldKind::classify(key);
        let key_off = self.0.append(&[stripped]).map_err(E::custom)?;
        Ok(PendingKey {
            key_off,
            key_len: stripped.len() as u32,
            kind,
        })
    }

    fn visit_string<E: de::Error>(self, mut key: String) -> Result<Self::Value, E> {
        let pending = self.visit_str(&key);
        key.zeroize();
        pending
    }
}

/// Appends a field value straight into the arena, after its key.
struct ValueSeed<'builder>(&'builder mut FieldsBuilder);

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = u32;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for ValueSeed<'_> {
    type Value = u32;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a captured field value string")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.0.append(&[value]).map_err(E::custom)?;
        Ok(value.len() as u32)
    }

    fn visit_string<E: de::Error>(self, mut value: String) -> Result<Self::Value, E> {
        let length = self.visit_str(&value);
        value.zeroize();
        length
    }
}

/// Builds a [`CapturedFields`]. Its buffer never reallocates in place: when it
/// must grow, the bytes move to a new wiped buffer and the old one is wiped as
/// it drops, so no plaintext reaches the allocator unwiped.
pub(crate) struct FieldsBuilder {
    bytes: Zeroizing<String>,
    slots: Vec<Slot>,
}

impl FieldsBuilder {
    /// `bytes` is the total of every key, prefix excluded, and value to come.
    /// With an exact total the arena is allocated once and never copied.
    pub(crate) fn with_capacity(bytes: usize, fields: usize) -> Self {
        Self {
            bytes: Zeroizing::new(String::with_capacity(bytes)),
            slots: Vec::with_capacity(fields),
        }
    }

    /// Append `parts` back to back and return the offset they start at.
    fn append(&mut self, parts: &[&str]) -> Result<u32, ArenaFull> {
        let extra: usize = parts.iter().map(|part| part.len()).sum();
        let start = self.bytes.len();
        let needed = start.checked_add(extra).ok_or(ArenaFull)?;
        if needed > u32::MAX as usize {
            return Err(ArenaFull);
        }
        if needed > self.bytes.capacity() {
            let mut grown =
                Zeroizing::new(String::with_capacity(needed.max(self.bytes.capacity() * 2)));
            grown.push_str(&self.bytes);
            std::mem::swap(&mut self.bytes, &mut grown);
        }
        for part in parts {
            self.bytes.push_str(part);
        }
        Ok(start as u32)
    }

    /// Add one field. `key` is the key without its prefix, as parts (a cookie
    /// key is `index:name`); `kind` must be the kind its full key classifies as.
    pub(crate) fn push(
        &mut self,
        kind: FieldKind,
        key: &[&str],
        value: &str,
    ) -> Result<(), ArenaFull> {
        let key_off = self.append(key)?;
        let key_len = (self.bytes.len() - key_off as usize) as u32;
        self.append(&[value])?;
        let slot = Slot {
            key_off,
            key_len,
            val_len: value.len() as u32,
            kind,
        };
        debug_assert_eq!(
            FieldKind::classify(&format!("{}{}", kind.prefix(), self.text(slot.key_range()))).0,
            kind,
            "a field's kind must be the one its full key classifies as"
        );
        self.slots.push(slot);
        Ok(())
    }

    fn text(&self, range: std::ops::Range<usize>) -> &str {
        self.bytes
            .get(range)
            .expect("captured field arena slot ends on a char boundary")
    }

    /// Sort the slots by full key and keep the last of any duplicate key, as
    /// inserting into a map would. A buffer with spare room is copied into an
    /// exact one, and the larger one is wiped as it drops.
    pub(crate) fn finish(mut self) -> CapturedFields {
        let bytes = &self.bytes;
        let key_of = |slot: &Slot| -> (FieldKind, &str) {
            let key = bytes
                .get(slot.key_range())
                .expect("captured field arena slot ends on a char boundary");
            (slot.kind, key)
        };
        self.slots
            .sort_by(|left, right| full_key_cmp(key_of(left), key_of(right)));
        self.slots.dedup_by(|later, kept| {
            let duplicate = full_key_cmp(key_of(later), key_of(kept)) == Ordering::Equal;
            if duplicate {
                *kept = *later;
            }
            duplicate
        });
        if self.bytes.capacity() > self.bytes.len() {
            let mut exact = Zeroizing::new(String::with_capacity(self.bytes.len()));
            exact.push_str(&self.bytes);
            std::mem::swap(&mut self.bytes, &mut exact);
        }
        CapturedFields {
            bytes: self.bytes,
            slots: self.slots.into_boxed_slice(),
        }
    }
}

/// A captured entry as the store holds it: a `SecretEntry`'s metadata, with
/// the fields compact and shared, so copying the entry for a lease flip copies
/// metadata and a pointer.
///
/// It serializes and deserializes as exactly the `SecretEntry` JSON object:
/// the same field names in the same order, `policy` omitted when absent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CapturedEntry {
    pub(crate) id: String,
    pub(crate) label: String,
    #[serde(
        default,
        serialize_with = "serialize_shared_fields",
        deserialize_with = "deserialize_shared_fields"
    )]
    pub(crate) fields: Arc<CapturedFields>,
    pub(crate) source: SecretSource,
    pub(crate) injection: InjectionTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) policy: Option<SecretPolicy>,
    pub(crate) created_at: i64,
}

fn serialize_shared_fields<S: Serializer>(
    fields: &Arc<CapturedFields>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    fields.as_ref().serialize(serializer)
}

fn deserialize_shared_fields<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<CapturedFields>, D::Error> {
    CapturedFields::deserialize(deserializer).map(Arc::new)
}

/// Metadata and field counts; never a value.
impl fmt::Debug for CapturedEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedEntry")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("fields", &self.fields)
            .field("source", &self.source)
            .field("injection", &self.injection)
            .field("policy", &self.policy)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[cfg(test)]
impl CapturedEntry {
    /// The plain entry this stands for.
    pub(crate) fn to_secret_entry(&self) -> crate::SecretEntry {
        crate::SecretEntry {
            id: self.id.clone(),
            label: self.label.clone(),
            fields: self.fields.to_field_map(),
            source: self.source.clone(),
            injection: self.injection.clone(),
            policy: self.policy.clone(),
            created_at: self.created_at,
        }
    }

    pub(crate) fn from_secret_entry(entry: &crate::SecretEntry) -> Self {
        Self {
            id: entry.id.clone(),
            label: entry.label.clone(),
            fields: Arc::new(CapturedFields::from_field_map(&entry.fields).expect("fits the arena")),
            source: entry.source.clone(),
            injection: entry.injection.clone(),
            policy: entry.policy.clone(),
            created_at: entry.created_at,
        }
    }
}

/// Wipe every value of a temporary field map before it is freed.
pub(crate) fn wipe_values(map: &mut HashMap<String, String>) {
    for value in map.values_mut() {
        value.zeroize();
    }
}
