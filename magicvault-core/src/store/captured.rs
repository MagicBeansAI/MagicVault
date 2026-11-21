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
//! Loading compacts one entry at a time: an entry's `fields` object is read
//! once to size its arena and once into it, straight from the decrypted file,
//! so a load never holds owned field strings and never grows or trims an
//! arena.
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

    /// Where the arena allocation starts, for the wipe-on-free tests.
    #[cfg(test)]
    pub(crate) fn arena_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
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
        let bytes = fields
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum();
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

/// Single pass from any deserializer: the arena grows as fields arrive (each
/// outgrown buffer wiped) and is trimmed to size at the end.
/// [`CapturedEntry`] loads use [`CapturedFields::from_json_exact`] instead.
impl<'de> Deserialize<'de> for CapturedFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(FieldsVisitor(FieldsSize {
            bytes: 256,
            fields: 0,
        }))
    }
}

impl CapturedFields {
    /// Parse a JSON fields object into an arena of exactly the right size:
    /// one pass adds up the lengths, the second writes the fields. Neither
    /// pass copies a value out of `json` except through serde_json's scratch
    /// buffer for escaped strings.
    pub(crate) fn from_json_exact(json: &str) -> Result<Self, serde_json::Error> {
        let size = serde_json::from_str::<FieldsSize>(json)?;
        let mut deserializer = serde_json::Deserializer::from_str(json);
        let fields = deserializer.deserialize_any(FieldsVisitor(size))?;
        deserializer.end()?;
        Ok(fields)
    }
}

/// What a fields object needs: arena bytes (keys without prefix, values) and
/// slot count. A duplicate key is counted each time, so a fields object with
/// one is trimmed after it is built.
#[derive(Clone, Copy)]
struct FieldsSize {
    bytes: usize,
    fields: usize,
}

impl<'de> Deserialize<'de> for FieldsSize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SizeVisitor)
    }
}

struct SizeVisitor;

const NOT_A_FIELDS_MAP: &str = "invalid type: string, expected a map of captured fields";

impl<'de> Visitor<'de> for SizeVisitor {
    type Value = FieldsSize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map of captured field names to string values")
    }

    /// Read through `deserialize_any`, so a string reaches this visitor:
    /// serde_json's own error for a string where a map belongs quotes the
    /// string, which would put a value in the error and from there in a log
    /// line.
    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Err(E::custom(NOT_A_FIELDS_MAP))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut size = FieldsSize {
            bytes: 0,
            fields: 0,
        };
        while let Some(key_len) = map.next_key_seed(LengthSeed { strip_prefix: true })? {
            let value_len = map.next_value_seed(LengthSeed {
                strip_prefix: false,
            })?;
            size.bytes = size.bytes.saturating_add(key_len).saturating_add(value_len);
            size.fields += 1;
        }
        Ok(size)
    }
}

/// The length a key (without its prefix) or a value takes in the arena.
struct LengthSeed {
    strip_prefix: bool,
}

impl<'de> DeserializeSeed<'de> for LengthSeed {
    type Value = usize;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for LengthSeed {
    type Value = usize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a captured field string")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        Ok(if self.strip_prefix {
            FieldKind::classify(text).1.len()
        } else {
            text.len()
        })
    }

    fn visit_string<E: de::Error>(self, mut text: String) -> Result<Self::Value, E> {
        let length = self.visit_str(&text);
        text.zeroize();
        length
    }
}

struct FieldsVisitor(FieldsSize);

impl<'de> Visitor<'de> for FieldsVisitor {
    type Value = CapturedFields;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map of captured field names to string values")
    }

    /// As [`SizeVisitor::visit_str`]: never quote the string.
    fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
        Err(E::custom(NOT_A_FIELDS_MAP))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let FieldsSize { bytes, fields } = self.0;
        let mut builder = FieldsBuilder::with_capacity(bytes, fields);
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

    /// Where the arena allocation starts, for the wipe-on-free tests.
    #[cfg(test)]
    pub(crate) fn arena_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
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

/// Borrow the entry's raw `fields` JSON from the input and compact it at its
/// exact size. Needs a deserializer that borrows from its input, as
/// `serde_json::from_slice` over the decrypted partition does; any other is
/// refused rather than parsed through owned strings.
fn deserialize_shared_fields<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<CapturedFields>, D::Error> {
    // Mapped to a fixed message: serde_json renders a value it cannot lend
    // into the error, contents included.
    let raw = <&'de serde_json::value::RawValue>::deserialize(deserializer)
        .map_err(|_| de::Error::custom("captured fields must be read from borrowed JSON"))?;
    CapturedFields::from_json_exact(raw.get())
        .map(Arc::new)
        .map_err(de::Error::custom)
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
            fields: Arc::new(
                CapturedFields::from_field_map(&entry.fields).expect("fits the arena"),
            ),
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;
    use crate::{CookieSpec, SecretEntry};

    /// Every kind, unknown prefixes, a prefix-only key, an empty key, empty
    /// values, non-ASCII and JSON-escaped text, a long value, and the same
    /// stripped key under several kinds. Synthetic placeholders only.
    fn every_kind_fields() -> HashMap<String, String> {
        let mut fields = HashMap::from([
            (
                "header:Authorization".to_string(),
                "Bearer PLACEHOLDER-H".to_string(),
            ),
            (
                "header:".to_string(),
                "PLACEHOLDER-EMPTY-HEADER-NAME".to_string(),
            ),
            ("query_param:token".to_string(), "PLACEHOLDER-Q".to_string()),
            ("cookie:0:session".to_string(), "PLACEHOLDER-C0".to_string()),
            (
                "cookie:10:session".to_string(),
                "PLACEHOLDER-C10".to_string(),
            ),
            ("cookie:2:other".to_string(), String::new()),
            (
                "local_storage:token".to_string(),
                "PLACEHOLDER-L".to_string(),
            ),
            (
                "session_storage:token".to_string(),
                "PLACEHOLDER-S".to_string(),
            ),
            (
                "header:token".to_string(),
                "PLACEHOLDER-SAME-KEY".to_string(),
            ),
            ("local_storage:".to_string(), String::new()),
            (
                "local_storage:ключ-🔑".to_string(),
                "значение-🔐-caf\u{e9}".to_string(),
            ),
            (
                "session_storage:tab\ttoken".to_string(),
                "line\nbreak \"quoted\" \\ end".to_string(),
            ),
            ("api_key".to_string(), "PLACEHOLDER-RAW".to_string()),
            (
                "unknown_prefix:thing".to_string(),
                "PLACEHOLDER-UNKNOWN".to_string(),
            ),
            ("cookie".to_string(), "PLACEHOLDER-NO-COLON".to_string()),
            ("Header:Case".to_string(), "PLACEHOLDER-CASE".to_string()),
            (String::new(), "PLACEHOLDER-EMPTY-KEY".to_string()),
            ("\u{1F511}".to_string(), String::new()),
        ]);
        fields.insert(
            "local_storage:long".to_string(),
            "PLACEHOLDER-LONG-".repeat(64 * 1024),
        );
        fields
    }

    fn compact(fields: &HashMap<String, String>) -> CapturedFields {
        CapturedFields::from_field_map(fields).expect("synthetic fields fit the arena")
    }

    #[test]
    fn keys_split_into_kind_and_stripped_key() {
        assert_eq!(FieldKind::classify("header:a"), (FieldKind::Header, "a"));
        assert_eq!(
            FieldKind::classify("query_param:a"),
            (FieldKind::QueryParam, "a")
        );
        assert_eq!(
            FieldKind::classify("cookie:3:a"),
            (FieldKind::Cookie, "3:a")
        );
        assert_eq!(
            FieldKind::classify("local_storage:"),
            (FieldKind::LocalStorage, "")
        );
        assert_eq!(
            FieldKind::classify("session_storage:a:b"),
            (FieldKind::SessionStorage, "a:b")
        );
        assert_eq!(FieldKind::classify("query:a"), (FieldKind::Raw, "query:a"));
        assert_eq!(
            FieldKind::classify("Header:a"),
            (FieldKind::Raw, "Header:a")
        );
        assert_eq!(FieldKind::classify(""), (FieldKind::Raw, ""));
    }

    #[test]
    fn compact_fields_round_trip_the_plain_map_for_every_kind() {
        let fields = every_kind_fields();
        let compacted = compact(&fields);
        assert_eq!(compacted.len(), fields.len());
        assert!(
            compacted.to_field_map() == fields,
            "the compact form lost or changed a field"
        );

        for (key, value) in &fields {
            let (kind, stripped) = FieldKind::classify(key);
            assert!(
                compacted.get(kind, stripped) == Some(value.as_str()),
                "lookup failed for a field of kind {kind:?}"
            );
        }
        assert!(compacted.get(FieldKind::Header, "absent").is_none());
        assert!(compacted.get(FieldKind::Cookie, "1:session").is_none());

        for kind in FieldKind::PREFIXED {
            let expected = fields
                .iter()
                .filter_map(|(key, value)| {
                    key.strip_prefix(kind.prefix())
                        .map(|rest| (rest.to_string(), value.clone()))
                })
                .collect::<HashMap<_, _>>();
            let actual = compacted
                .of_kind(kind)
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<HashMap<_, _>>();
            assert!(
                actual == expected,
                "the {kind:?} range differs from a prefix scan"
            );
            assert_eq!(compacted.has_kind(kind), !expected.is_empty());
        }

        let mut expected_values = fields.values().map(String::as_str).collect::<Vec<_>>();
        let mut values = compacted.values().collect::<Vec<_>>();
        expected_values.sort_unstable();
        values.sort_unstable();
        assert!(
            values == expected_values,
            "values() differs from the map's values"
        );
    }

    #[test]
    fn serialization_is_the_plain_map_json_in_key_order() {
        let fields = every_kind_fields();
        let compacted = compact(&fields);
        let written = serde_json::to_vec(&compacted).unwrap();
        let sorted = fields.iter().collect::<BTreeMap<_, _>>();
        assert!(
            written == serde_json::to_vec(&sorted).unwrap(),
            "compact fields must write the plain map's JSON in ascending key order"
        );
        let parsed = serde_json::from_slice::<HashMap<String, String>>(&written).unwrap();
        assert!(
            parsed == fields,
            "the written JSON parses back to a different map"
        );
    }

    #[test]
    fn deserialization_reads_the_plain_map_json_by_every_string_path() {
        let fields = every_kind_fields();
        let legacy_json = serde_json::to_vec(&fields).unwrap();
        // Borrowed strings and serde_json's scratch buffer for escaped ones.
        let from_slice = serde_json::from_slice::<CapturedFields>(&legacy_json).unwrap();
        assert!(from_slice == compact(&fields));
        // Owned strings.
        let value = serde_json::to_value(&fields).unwrap();
        let from_value = serde_json::from_value::<CapturedFields>(value).unwrap();
        assert!(from_value == compact(&fields));
        // A loaded arena is shrunk to its exact size.
        assert_eq!(from_slice.arena_capacity(), from_slice.arena_len());

        // The sized two-pass parse a load uses, borrowed and escaped strings.
        let exact =
            CapturedFields::from_json_exact(std::str::from_utf8(&legacy_json).unwrap()).unwrap();
        assert!(exact == compact(&fields));
        assert_eq!(exact.arena_capacity(), exact.arena_len());
    }

    /// A load reads an entry's fields from its raw JSON in the decrypted
    /// file; a deserializer that cannot lend its input is refused, not
    /// parsed through owned strings.
    #[test]
    fn a_captured_entry_loads_only_from_borrowed_json() {
        let plain = placeholder_entry(None);
        let json = serde_json::to_vec(&plain).unwrap();
        assert!(
            serde_json::from_slice::<CapturedEntry>(&json)
                .unwrap()
                .to_secret_entry()
                == plain
        );
        let error = serde_json::from_value::<CapturedEntry>(serde_json::to_value(&plain).unwrap())
            .expect_err("an owned Value cannot lend raw JSON");
        assert!(!error.to_string().contains("PLACEHOLDER"));
    }

    #[test]
    fn a_duplicate_key_keeps_the_last_value_like_a_map() {
        let json =
            br#"{"header:a":"PLACEHOLDER-1","raw":"PLACEHOLDER-R","header:a":"PLACEHOLDER-2"}"#;
        let compacted = serde_json::from_slice::<CapturedFields>(json).unwrap();
        let plain = serde_json::from_slice::<HashMap<String, String>>(json).unwrap();
        assert!(compacted.to_field_map() == plain);
        assert_eq!(compacted.len(), 2);
    }

    /// A string where the fields map belongs is refused without quoting it:
    /// serde's default error would carry the value into a log line.
    #[test]
    fn a_string_in_place_of_the_fields_map_is_refused_without_quoting_it() {
        let json = r#""PLACEHOLDER-NOT-A-MAP""#;
        let errors = [
            serde_json::from_str::<CapturedFields>(json).unwrap_err(),
            CapturedFields::from_json_exact(json).unwrap_err(),
        ];
        for error in errors {
            assert!(
                !error.to_string().contains("PLACEHOLDER"),
                "the error quoted the value"
            );
        }
        let mut entry = serde_json::to_value(placeholder_entry(None)).unwrap();
        entry["fields"] = serde_json::Value::String("PLACEHOLDER-NOT-A-MAP".to_string());
        let error = serde_json::from_slice::<CapturedEntry>(&serde_json::to_vec(&entry).unwrap())
            .expect_err("a string is not a fields map");
        assert!(
            !error.to_string().contains("PLACEHOLDER"),
            "the error quoted the value"
        );
    }

    #[test]
    fn a_non_string_value_is_rejected_like_a_map() {
        for json in [
            r#"{"header:a":1}"#,
            r#"{"header:a":null}"#,
            r#"["header:a"]"#,
            "null",
        ] {
            assert!(serde_json::from_str::<HashMap<String, String>>(json).is_err());
            let error = serde_json::from_str::<CapturedFields>(json)
                .expect_err("compact fields reject what the plain map rejects");
            assert!(!error.to_string().contains("PLACEHOLDER"));
        }
    }

    fn placeholder_entry(policy: Option<SecretPolicy>) -> SecretEntry {
        SecretEntry {
            id: "https://placeholder.invalid".to_string(),
            label: "Placeholder \"label\"".to_string(),
            fields: every_kind_fields(),
            source: SecretSource::Captured {
                origin: "https://placeholder.invalid".to_string(),
                generation: 7,
                stale: true,
                expires_hint: Some(1_900_000_000),
            },
            injection: InjectionTarget::Cookies(vec![CookieSpec {
                name: "session".to_string(),
                domain: "placeholder.invalid".to_string(),
                path: "/".to_string(),
                secure: true,
                http_only: false,
                same_site: None,
                expires: Some(1_900_000_001),
            }]),
            policy,
            created_at: 42,
        }
    }

    /// A captured entry reads and writes the `SecretEntry` JSON object, with
    /// and without a policy, and with no `fields` member at all.
    #[test]
    fn a_captured_entry_crosses_the_secret_entry_codec_both_ways() {
        for policy in [None, Some(SecretPolicy::default())] {
            let plain = placeholder_entry(policy);
            let compacted = CapturedEntry::from_secret_entry(&plain);

            let written = serde_json::to_vec(&compacted).unwrap();
            assert!(serde_json::from_slice::<SecretEntry>(&written).unwrap() == plain);
            let read =
                serde_json::from_slice::<CapturedEntry>(&serde_json::to_vec(&plain).unwrap())
                    .unwrap();
            assert!(read == compacted);
            assert!(read.to_secret_entry() == plain);

            let as_value = serde_json::to_value(&compacted).unwrap();
            assert!(as_value == serde_json::to_value(&plain).unwrap());
        }

        let mut without_fields = serde_json::to_value(placeholder_entry(None)).unwrap();
        without_fields.as_object_mut().unwrap().remove("fields");
        let read =
            serde_json::from_slice::<CapturedEntry>(&serde_json::to_vec(&without_fields).unwrap())
                .unwrap();
        assert_eq!(read.fields.len(), 0);
    }

    #[test]
    fn debug_names_no_key_and_no_value() {
        let entry = CapturedEntry::from_secret_entry(&placeholder_entry(None));
        let rendered = format!("{entry:?}");
        assert!(
            !rendered.contains("PLACEHOLDER"),
            "Debug leaked a field value"
        );
        assert!(
            !rendered.contains("Authorization"),
            "Debug leaked a field key"
        );
        assert!(rendered.contains(&format!("fields: {}", every_kind_fields().len())));
    }

    /// The per-field cost is the payload without its prefix plus one 16-byte
    /// slot; the arena is one allocation of exactly the payload.
    #[test]
    fn per_field_overhead_is_one_sixteen_byte_slot() {
        const FIELDS: usize = 10_000;
        let fields = (0..FIELDS)
            .map(|index| {
                (
                    format!("local_storage:placeholder-key-{index}"),
                    format!(
                        "{{\"synthetic\":\"PLACEHOLDER-{index}\",\"pad\":\"{}\"}}",
                        "x".repeat(40)
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        let payload = fields
            .iter()
            .map(|(key, value)| key.len() - "local_storage:".len() + value.len())
            .sum::<usize>();

        let json = serde_json::to_string(&fields).unwrap();
        for compacted in [
            compact(&fields),
            serde_json::from_str::<CapturedFields>(&json).unwrap(),
            CapturedFields::from_json_exact(&json).unwrap(),
        ] {
            assert_eq!(compacted.len(), FIELDS);
            assert_eq!(std::mem::size_of::<Slot>(), 16);
            assert_eq!(compacted.slot_table_bytes(), 16 * FIELDS);
            assert_eq!(compacted.arena_len(), payload);
            assert_eq!(
                compacted.arena_capacity(),
                payload,
                "the arena has no slack"
            );
            let resident = compacted.slot_table_bytes() + compacted.arena_capacity();
            println!(
                "{FIELDS} fields: payload {payload} B, arena {} B, slots {} B, {:.1} B per field ({:.1} B payload)",
                compacted.arena_capacity(),
                compacted.slot_table_bytes(),
                resident as f64 / FIELDS as f64,
                payload as f64 / FIELDS as f64,
            );
            assert!(resident <= payload + 16 * FIELDS);
        }
    }
}
