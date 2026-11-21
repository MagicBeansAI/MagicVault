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
//! Loading compacts one entry at a time: an entry's `fields` object is
//! borrowed as raw JSON from the decrypted file and read by a small strict
//! parser, once to size its arena and once into it. Escapes are decoded
//! straight into the arena, so no field passes through serde_json's scratch
//! buffer or any other unwiped one, and a load never grows or trims an arena.
//!
//! Values never appear in a `Debug` impl or a panic message. An error about a
//! fields object names a fixed reason and a position, never the text.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use serde::de;
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

    /// The kind of the full key `prefix` followed by `rest`, without joining
    /// them.
    fn classify_parts(prefix: &str, rest: &str) -> FieldKind {
        Self::PREFIXED
            .into_iter()
            .find(|kind| {
                let wanted = kind.prefix().as_bytes();
                let joined = prefix.bytes().chain(rest.bytes());
                wanted.len() <= prefix.len() + rest.len()
                    && joined.take(wanted.len()).eq(wanted.iter().copied())
            })
            .unwrap_or(FieldKind::Raw)
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

/// The limit on an entry's arena: offsets and lengths are `u32`.
const ARENA_LIMIT: usize = u32::MAX as usize;

impl CapturedFields {
    /// Parse a JSON fields object into an arena of exactly the right size:
    /// one pass adds up the lengths, the second writes the fields.
    ///
    /// The fields object has one shape, an object of strings, so it is read by
    /// [`FieldsParser`] rather than serde_json: escaped text is unescaped
    /// straight into the arena, and keys into one wiped scratch buffer, so no
    /// field passes through a buffer this crate does not wipe. An entry larger
    /// than the arena limit is refused after the sizing pass, before any arena
    /// is allocated. Errors carry a fixed description and a position, never
    /// the text that failed.
    pub(crate) fn from_json_exact(json: &str) -> Result<Self, FieldsError> {
        Self::from_json_exact_with_limit(json, ARENA_LIMIT)
    }

    fn from_json_exact_with_limit(json: &str, limit: usize) -> Result<Self, FieldsError> {
        let mut size = FieldsSize {
            bytes: 0,
            fields: 0,
            limit,
        };
        FieldsParser::new(json).parse(&mut size)?;
        let mut builder = FieldsBuilder::with_capacity(size.bytes, size.fields);
        builder.limit = limit;
        let mut filler = FieldsFiller {
            builder,
            pending: None,
        };
        FieldsParser::new(json).parse(&mut filler)?;
        Ok(filler.builder.finish())
    }
}

/// Why a fields object could not be read: a fixed description and where, in
/// the fields object, it went wrong. Never the text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FieldsError {
    reason: FieldsErrorReason,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldsErrorReason {
    ExpectedObject,
    ExpectedKey,
    ExpectedColon,
    ExpectedStringValue,
    ExpectedCommaOrEnd,
    InvalidEscape,
    LoneSurrogate,
    ControlCharacter,
    UnexpectedEnd,
    TrailingCharacters,
    ArenaFull,
}

impl FieldsError {
    #[cfg(test)]
    pub(crate) fn reason(&self) -> FieldsErrorReason {
        self.reason
    }
}

impl fmt::Display for FieldsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.reason {
            FieldsErrorReason::ExpectedObject => "expected an object of strings",
            FieldsErrorReason::ExpectedKey => "expected a string field name",
            FieldsErrorReason::ExpectedColon => "expected `:` after a field name",
            FieldsErrorReason::ExpectedStringValue => "expected a string field value",
            FieldsErrorReason::ExpectedCommaOrEnd => "expected `,` or `}` after a field",
            FieldsErrorReason::InvalidEscape => "invalid escape in a field string",
            FieldsErrorReason::LoneSurrogate => "unpaired surrogate escape in a field string",
            FieldsErrorReason::ControlCharacter => "control character in a field string",
            FieldsErrorReason::UnexpectedEnd => "unexpected end of the fields object",
            FieldsErrorReason::TrailingCharacters => "trailing characters after the fields object",
            FieldsErrorReason::ArenaFull => "captured entry fields exceed the 4 GiB field arena",
        };
        write!(
            formatter,
            "captured fields: {reason} at line {} column {} of the fields object",
            self.line, self.column
        )
    }
}

/// What the parser reports, field by field: the full key, unescaped, then the
/// value in unescaped pieces.
trait FieldSink {
    fn key(&mut self, full_key: &str) -> Result<(), FieldsErrorReason>;
    fn value_piece(&mut self, piece: &str) -> Result<(), FieldsErrorReason>;
    fn end_value(&mut self);
}

/// The sizing pass: arena bytes (keys without prefix, values) and slot count.
/// A duplicate key is counted each time; its bytes stay in the arena, wiped
/// with it, until the arena drops.
struct FieldsSize {
    bytes: usize,
    fields: usize,
    limit: usize,
}

impl FieldsSize {
    fn add(&mut self, length: usize) -> Result<(), FieldsErrorReason> {
        self.bytes = self
            .bytes
            .checked_add(length)
            .filter(|bytes| *bytes <= self.limit)
            .ok_or(FieldsErrorReason::ArenaFull)?;
        Ok(())
    }
}

impl FieldSink for FieldsSize {
    fn key(&mut self, full_key: &str) -> Result<(), FieldsErrorReason> {
        self.fields += 1;
        self.add(FieldKind::classify(full_key).1.len())
    }

    fn value_piece(&mut self, piece: &str) -> Result<(), FieldsErrorReason> {
        self.add(piece.len())
    }

    fn end_value(&mut self) {}
}

/// The filling pass: keys and values straight into the arena.
struct FieldsFiller {
    builder: FieldsBuilder,
    /// The current field's kind, key offset and key length.
    pending: Option<(FieldKind, u32, u32)>,
}

impl FieldSink for FieldsFiller {
    fn key(&mut self, full_key: &str) -> Result<(), FieldsErrorReason> {
        let (kind, stripped) = FieldKind::classify(full_key);
        let key_off = self
            .builder
            .append(&[stripped])
            .map_err(|ArenaFull| FieldsErrorReason::ArenaFull)?;
        self.pending = Some((kind, key_off, stripped.len() as u32));
        Ok(())
    }

    fn value_piece(&mut self, piece: &str) -> Result<(), FieldsErrorReason> {
        self.builder
            .append(&[piece])
            .map(|_| ())
            .map_err(|ArenaFull| FieldsErrorReason::ArenaFull)
    }

    fn end_value(&mut self) {
        if let Some((kind, key_off, key_len)) = self.pending.take() {
            let value_start = key_off as usize + key_len as usize;
            self.builder.slots.push(Slot {
                key_off,
                key_len,
                val_len: (self.builder.bytes.len() - value_start) as u32,
                kind,
            });
        }
    }
}

/// A strict reader for the one JSON shape a fields object has, `{}` or an
/// object whose members are all strings, accepting exactly what serde_json
/// accepts for a `HashMap<String, String>` (the tests hold it to that).
struct FieldsParser<'json> {
    json: &'json str,
    position: usize,
    /// Each key, unescaped, so its prefix can be classified. Reused across
    /// keys, grown by moving to a new wiped buffer, wiped on drop.
    key: Zeroizing<String>,
}

impl<'json> FieldsParser<'json> {
    fn new(json: &'json str) -> Self {
        Self {
            json,
            position: 0,
            key: Zeroizing::new(String::new()),
        }
    }

    fn error(&self, reason: FieldsErrorReason) -> FieldsError {
        let before = &self.json.as_bytes()[..self.position.min(self.json.len())];
        let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
        let line_start = before
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        FieldsError {
            reason,
            line,
            column: before.len() - line_start + 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.json.as_bytes().get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, byte: u8, reason: FieldsErrorReason) -> Result<(), FieldsError> {
        self.skip_whitespace();
        match self.peek() {
            Some(found) if found == byte => {
                self.position += 1;
                Ok(())
            }
            None => Err(self.error(FieldsErrorReason::UnexpectedEnd)),
            Some(_) => Err(self.error(reason)),
        }
    }

    fn parse(mut self, sink: &mut impl FieldSink) -> Result<(), FieldsError> {
        self.expect(b'{', FieldsErrorReason::ExpectedObject)?;
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
        } else {
            loop {
                self.expect(b'"', FieldsErrorReason::ExpectedKey)?;
                let mut key = std::mem::take(&mut self.key);
                key.clear();
                let read = self.read_string(&mut |piece| {
                    push_wiping(&mut key, piece);
                    Ok(())
                });
                self.key = key;
                read?;
                sink.key(&self.key).map_err(|reason| self.error(reason))?;
                self.expect(b':', FieldsErrorReason::ExpectedColon)?;
                self.expect(b'"', FieldsErrorReason::ExpectedStringValue)?;
                self.read_string(&mut |piece| sink.value_piece(piece))?;
                sink.end_value();
                self.skip_whitespace();
                match self.peek() {
                    Some(b',') => self.position += 1,
                    Some(b'}') => {
                        self.position += 1;
                        break;
                    }
                    None => return Err(self.error(FieldsErrorReason::UnexpectedEnd)),
                    Some(_) => return Err(self.error(FieldsErrorReason::ExpectedCommaOrEnd)),
                }
            }
        }
        self.skip_whitespace();
        if self.position != self.json.len() {
            return Err(self.error(FieldsErrorReason::TrailingCharacters));
        }
        Ok(())
    }

    /// Read a string whose opening quote was consumed, handing its unescaped
    /// text to `piece` as runs of the input and single decoded characters.
    fn read_string(
        &mut self,
        piece: &mut dyn FnMut(&str) -> Result<(), FieldsErrorReason>,
    ) -> Result<(), FieldsError> {
        let bytes = self.json.as_bytes();
        loop {
            let run_start = self.position;
            while let Some(byte) = self.peek() {
                if byte == b'"' || byte == b'\\' || byte < 0x20 {
                    break;
                }
                self.position += 1;
            }
            if self.position > run_start {
                // The run ends at an ASCII byte or the end, so on a char
                // boundary of the `&str` input.
                piece(&self.json[run_start..self.position]).map_err(|reason| self.error(reason))?;
            }
            match bytes.get(self.position) {
                None => return Err(self.error(FieldsErrorReason::UnexpectedEnd)),
                Some(b'"') => {
                    self.position += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.position += 1;
                    let decoded = self.read_escape()?;
                    let mut utf8 = [0u8; 4];
                    let text: &str = decoded.encode_utf8(&mut utf8);
                    let result = piece(text).map_err(|reason| self.error(reason));
                    utf8.zeroize();
                    result?;
                }
                Some(_) => return Err(self.error(FieldsErrorReason::ControlCharacter)),
            }
        }
    }

    /// Decode one escape whose backslash was consumed.
    fn read_escape(&mut self) -> Result<char, FieldsError> {
        let Some(byte) = self.peek() else {
            return Err(self.error(FieldsErrorReason::UnexpectedEnd));
        };
        self.position += 1;
        let simple = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.read_unicode_escape(),
            _ => return Err(self.error(FieldsErrorReason::InvalidEscape)),
        };
        Ok(simple)
    }

    fn read_hex4(&mut self) -> Result<u16, FieldsError> {
        let digits = self
            .json
            .as_bytes()
            .get(self.position..self.position + 4)
            .ok_or_else(|| self.error(FieldsErrorReason::UnexpectedEnd))?;
        let mut unit = 0u16;
        for digit in digits {
            let value = (*digit as char)
                .to_digit(16)
                .ok_or_else(|| self.error(FieldsErrorReason::InvalidEscape))?;
            unit = unit * 16 + value as u16;
        }
        self.position += 4;
        Ok(unit)
    }

    fn read_unicode_escape(&mut self) -> Result<char, FieldsError> {
        let unit = self.read_hex4()?;
        let code = match unit {
            0xD800..=0xDBFF => {
                if self.json.as_bytes().get(self.position..self.position + 2) != Some(b"\\u") {
                    return Err(self.error(FieldsErrorReason::LoneSurrogate));
                }
                self.position += 2;
                let low = self.read_hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.error(FieldsErrorReason::LoneSurrogate));
                }
                0x10000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(self.error(FieldsErrorReason::LoneSurrogate)),
            _ => u32::from(unit),
        };
        char::from_u32(code).ok_or_else(|| self.error(FieldsErrorReason::InvalidEscape))
    }
}

/// Append to a wiped buffer that, when it must grow, moves to a new wiped
/// buffer and wipes the old one as it drops, rather than reallocating.
fn push_wiping(buffer: &mut Zeroizing<String>, text: &str) {
    let needed = buffer.len() + text.len();
    if needed > buffer.capacity() {
        let mut grown = Zeroizing::new(String::with_capacity(needed.max(buffer.capacity() * 2)));
        grown.push_str(buffer);
        std::mem::swap(buffer, &mut grown);
    }
    buffer.push_str(text);
}

/// Builds a [`CapturedFields`]. Its buffer never reallocates in place: when it
/// must grow, the bytes move to a new wiped buffer and the old one is wiped as
/// it drops, so no plaintext reaches the allocator unwiped.
pub(crate) struct FieldsBuilder {
    bytes: Zeroizing<String>,
    slots: Vec<Slot>,
    /// Most arena bytes; [`ARENA_LIMIT`] except in tests.
    limit: usize,
}

impl FieldsBuilder {
    /// `bytes` is the total of every key, prefix excluded, and value to come.
    /// With an exact total the arena is allocated once and never copied.
    pub(crate) fn with_capacity(bytes: usize, fields: usize) -> Self {
        Self {
            bytes: Zeroizing::new(String::with_capacity(bytes)),
            slots: Vec::with_capacity(fields),
            limit: ARENA_LIMIT,
        }
    }

    /// A builder that refuses more than `limit` arena bytes, so the refusal
    /// can be tested without gigabytes.
    #[cfg(test)]
    pub(crate) fn with_limit(limit: usize) -> Self {
        let mut builder = Self::with_capacity(0, 0);
        builder.limit = limit;
        builder
    }

    /// Append `parts` back to back and return the offset they start at.
    fn append(&mut self, parts: &[&str]) -> Result<u32, ArenaFull> {
        let extra: usize = parts.iter().map(|part| part.len()).sum();
        let start = self.bytes.len();
        let needed = start.checked_add(extra).ok_or(ArenaFull)?;
        if needed > self.limit.min(ARENA_LIMIT) {
            return Err(ArenaFull);
        }
        for part in parts {
            push_wiping(&mut self.bytes, part);
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
        // Compared in place: formatting the full key would copy it into an
        // unwiped buffer, and the message names no key.
        debug_assert!(
            FieldKind::classify_parts(kind.prefix(), self.text(slot.key_range())) == kind,
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
    /// inserting into a map would. A duplicate's bytes are not removed: they
    /// stay in the arena, wiped with it, until it drops. A buffer with spare
    /// room is copied into an exact one, and the larger one is wiped as it
    /// drops.
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
    // into the error, contents included. This covers both a syntax error in
    // the fields object and a deserializer that cannot lend its input; the
    // outer parser adds the position.
    let raw = <&'de serde_json::value::RawValue>::deserialize(deserializer)
        .map_err(|_| de::Error::custom("captured fields object could not be read"))?;
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

    fn exact(json: &str) -> Result<CapturedFields, FieldsError> {
        CapturedFields::from_json_exact(json)
    }

    #[test]
    fn the_load_parser_reads_the_plain_map_json_exactly() {
        let fields = every_kind_fields();
        let legacy_json = serde_json::to_string(&fields).unwrap();
        let loaded = exact(&legacy_json).unwrap();
        assert!(loaded == compact(&fields));
        assert_eq!(
            loaded.arena_capacity(),
            loaded.arena_len(),
            "the arena is sized exactly"
        );
        let pretty = serde_json::to_string_pretty(&fields).unwrap();
        assert!(exact(&pretty).unwrap() == compact(&fields));
    }

    /// The load parser accepts exactly what serde_json accepts for a
    /// `HashMap<String, String>`, and reads the same map: every escape, hex
    /// digits in both cases, surrogate pairs and every way to break one,
    /// escaped key prefixes, whitespace, duplicates, truncation and a range of
    /// malformed input.
    ///
    /// Both directions matter. A false acceptance would load bytes the old
    /// codec refused. A false rejection is worse: a load parse error
    /// quarantines the whole captured partition, so it would take the vault
    /// offline. The accept side needs in-repo coverage as much as the reject
    /// side.
    #[test]
    fn the_load_parser_agrees_with_serde_json_on_every_case() {
        let deep_array = format!(r#"{{"a":{}{}}}"#, "[".repeat(10_000), "]".repeat(10_000));
        let deep_object = format!(r#"{{"a":{}"#, r#"{"b":"#.repeat(10_000));
        let cases: Vec<&str> = vec![
            "{}",
            " { } ",
            "\n{\r\n\t\"a\" : \"b\" ,\"c\":\"d\"\t}\n",
            r#"{"a":"\"\\\/\b\f\n\r\t"}"#,
            r#"{"a":"\u0041\u00e9\u00E9\u4e2d\uABCD\uabcd\ud83d\udd11\uD83D\uDD11\u001b\uFFFF\u0000"}"#,
            r#"{"\u0068eader:x":"PLACEHOLDER-ESCAPED-PREFIX"}"#,
            r#"{"header\u003Ax":"PLACEHOLDER-ESCAPED-COLON"}"#,
            r#"{"header:a":"PLACEHOLDER-1","raw":"R","header:a":"PLACEHOLDER-2"}"#,
            r#"{"header:a":"PLACEHOLDER-PLAIN","\u0068eader:a":"PLACEHOLDER-ESCAPED"}"#,
            r#"{"\u0068eader:a":"PLACEHOLDER-ESCAPED","header:a":"PLACEHOLDER-PLAIN"}"#,
            "{\"caf\u{e9}\":\"\u{1F511}\"}",
            "{\"a\":\"del\u{7f}byte\"}",
            r#"{"":""}"#,
            // Surrogates: lone high, lone low, high then a non-low escape,
            // high then another high, high then a plain character, a
            // reversed pair, high at the end of the string.
            r#"{"a":"\ud800"}"#,
            r#"{"a":"\udc00"}"#,
            r#"{"a":"\ud800\u0041"}"#,
            r#"{"a":"\ud800\ud800"}"#,
            r#"{"a":"\ud800A"}"#,
            r#"{"a":"\udc00\ud800"}"#,
            r#"{"a":"\ud83d"}"#,
            r#"{"a":"\x"}"#,
            r#"{"a":"\u12"}"#,
            r#"{"a":"\u12G4"}"#,
            "{\"a\":\"\u{1}\"}",
            "{\"a\":\"line\nbreak\"}",
            // Truncation inside an escape, inside a unicode escape, after a comma.
            r#"{"a":"\"#,
            r#"{"a":"\u00"#,
            r#"{"a":"\ud83d\u"#,
            r#"{"a":"b","#,
            r#"{"a":"b",}"#,
            r#"{,}"#,
            r#"{"a" "b"}"#,
            r#"{"a":"b" "c":"d"}"#,
            // Bytes after the closing brace.
            r#"{"a":"b"} x"#,
            r#"{"a":"b"}}"#,
            "{}x",
            "{} {}",
            r#"{"a":"b""#,
            r#"{"a":"b"#,
            r#"{"a":"#,
            r#"{"a""#,
            "{",
            "",
            // A leading byte-order mark.
            "\u{feff}{}",
            "\u{feff}{\"a\":\"b\"}",
            r#""string""#,
            "[]",
            "null",
            r#"{"a":1}"#,
            r#"{"a":1.5}"#,
            r#"{"a":true}"#,
            r#"{"a":null}"#,
            r#"{"a":["b"]}"#,
            r#"{"a":{"b":"c"}}"#,
            r#"{a:"b"}"#,
            r#"{"a":'b'}"#,
            // Deep nesting as a value: refused at its first byte, without
            // recursion (serde_json refuses it at its depth limit).
            &deep_array,
            &deep_object,
        ];
        assert_eq!(cases.len(), 58, "case count");
        for json in cases {
            let plain = serde_json::from_str::<HashMap<String, String>>(json);
            let loaded = exact(json);
            let shown = json.chars().take(80).collect::<String>();
            assert_eq!(
                plain.is_ok(),
                loaded.is_ok(),
                "the load parser and serde_json disagree on case {shown:?}"
            );
            if let (Ok(plain), Ok(loaded)) = (plain, loaded) {
                assert!(
                    loaded.to_field_map() == plain,
                    "different map for case {shown:?}"
                );
                assert_eq!(loaded.arena_capacity(), loaded.arena_len());
            }
        }

        // Escaped prefixes classify as their kind, and the last spelling of a
        // duplicate wins whichever spelling it uses.
        let escaped = exact(r#"{"\u0068eader:x":"PLACEHOLDER-ESCAPED-PREFIX"}"#).unwrap();
        assert!(escaped.get(FieldKind::Header, "x") == Some("PLACEHOLDER-ESCAPED-PREFIX"));
        let colon = exact(r#"{"header\u003Ax":"PLACEHOLDER-ESCAPED-COLON"}"#).unwrap();
        assert!(colon.get(FieldKind::Header, "x") == Some("PLACEHOLDER-ESCAPED-COLON"));
        let last_escaped =
            exact(r#"{"header:a":"PLACEHOLDER-PLAIN","\u0068eader:a":"PLACEHOLDER-ESCAPED"}"#)
                .unwrap();
        assert_eq!(last_escaped.len(), 1);
        assert!(last_escaped.get(FieldKind::Header, "a") == Some("PLACEHOLDER-ESCAPED"));
        let last_plain =
            exact(r#"{"\u0068eader:a":"PLACEHOLDER-ESCAPED","header:a":"PLACEHOLDER-PLAIN"}"#)
                .unwrap();
        assert!(last_plain.get(FieldKind::Header, "a") == Some("PLACEHOLDER-PLAIN"));
        let decoded = exact(
            r#"{"a":"\u0041\u00e9\u00E9\u4e2d\uABCD\uabcd\ud83d\udd11\uD83D\uDD11\u001b\uFFFF\u0000"}"#,
        )
        .unwrap();
        let expected = [
            'A',
            '\u{e9}',
            '\u{e9}',
            '\u{4e2d}',
            '\u{abcd}',
            '\u{abcd}',
            '\u{1F511}',
            '\u{1F511}',
            '\u{1b}',
            '\u{ffff}',
            '\u{0}',
        ]
        .iter()
        .collect::<String>();
        assert!(decoded.get(FieldKind::Raw, "a") == Some(expected.as_str()));
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
            r#"{"header:a":"PLACEHOLDER-1","raw":"PLACEHOLDER-R","header:a":"PLACEHOLDER-2"}"#;
        let compacted = exact(json).unwrap();
        let plain = serde_json::from_str::<HashMap<String, String>>(json).unwrap();
        assert!(compacted.to_field_map() == plain);
        assert_eq!(compacted.len(), 2);
    }

    /// Fields that cannot be read are refused with a fixed description and a
    /// position. Each case carries a marker in the offending value; serde's
    /// rendering of that value would contain it.
    #[test]
    fn a_fields_error_never_quotes_the_offending_value() {
        let cases = [
            (
                r#"{"header:a":4815162342}"#,
                "4815162342",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#"{"header:a":-271828.5e3}"#,
                "271828",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#"{"header:a":true}"#,
                "true",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#"{"header:a":false}"#,
                "false",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#"{"header:a":null}"#,
                "null",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#""PLACEHOLDER-NOT-A-MAP""#,
                "PLACEHOLDER",
                FieldsErrorReason::ExpectedObject,
            ),
            (
                r#"["PLACEHOLDER-IN-ARRAY"]"#,
                "PLACEHOLDER",
                FieldsErrorReason::ExpectedObject,
            ),
            (
                r#"{"header:a":["PLACEHOLDER-IN-ARRAY"]}"#,
                "PLACEHOLDER",
                FieldsErrorReason::ExpectedStringValue,
            ),
            (
                r#"{"header:a":"PLACEHOLDER-\q"}"#,
                "PLACEHOLDER",
                FieldsErrorReason::InvalidEscape,
            ),
            (
                r#"{"header:a":"PLACEHOLDER-UNTERMINATED"#,
                "PLACEHOLDER",
                FieldsErrorReason::UnexpectedEnd,
            ),
            (
                r#"{"header:a":"PLACEHOLDER-OK"} PLACEHOLDER-TRAILING"#,
                "PLACEHOLDER",
                FieldsErrorReason::TrailingCharacters,
            ),
        ];
        for (json, marker, reason) in cases {
            // The marker is in the input, so the check below can fail.
            assert!(json.contains(marker));
            let error = exact(json).expect_err("malformed fields are refused");
            assert_eq!(error.reason(), reason);
            let rendered = error.to_string();
            assert!(
                !rendered.contains(marker),
                "the error quoted the offending value"
            );
            assert!(
                rendered.contains("line 1 column "),
                "the error gives a position"
            );
        }

        // Through a whole entry, as a load reads it, each scalar kind included.
        for fields in [
            serde_json::json!("PLACEHOLDER-NOT-A-MAP"),
            serde_json::json!({ "header:a": 4815162342u64 }),
            serde_json::json!({ "header:a": -271828.5 }),
            serde_json::json!({ "header:a": true }),
            serde_json::json!({ "header:a": ["PLACEHOLDER-IN-ARRAY"] }),
        ] {
            let mut entry = serde_json::to_value(placeholder_entry(None)).unwrap();
            entry["fields"] = fields;
            let error =
                serde_json::from_slice::<CapturedEntry>(&serde_json::to_vec(&entry).unwrap())
                    .expect_err("malformed fields are refused")
                    .to_string();
            for marker in ["PLACEHOLDER", "4815162342", "271828", "true"] {
                assert!(
                    !error.contains(marker),
                    "the entry error quoted the offending value"
                );
            }
        }

        // A syntax error inside the fields object, reported by the outer parse.
        let mut text = serde_json::to_string(&placeholder_entry(None)).unwrap();
        let at = text.find(r#""fields":{"#).unwrap() + r#""fields":{"#.len();
        text.insert_str(at, r#""header:z":"PLACEHOLDER-BROKEN" "#);
        let error = serde_json::from_str::<CapturedEntry>(&text)
            .expect_err("a syntax error in fields is refused")
            .to_string();
        assert!(
            !error.contains("PLACEHOLDER"),
            "the syntax error quoted a value"
        );
        assert!(
            error.contains("could not be read"),
            "the wording is neutral"
        );

        // The outer parse adds the file position to a fields error.
        let mut entry = serde_json::to_value(placeholder_entry(None)).unwrap();
        entry["fields"] = serde_json::json!({ "header:a": true });
        let error = serde_json::from_slice::<CapturedEntry>(&serde_json::to_vec(&entry).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("of the fields object at line 1 column "),
            "both positions are reported"
        );
    }

    /// An entry over the arena limit is refused after the sizing pass, before
    /// an arena is allocated; the limit is lowered so no gigabytes are needed.
    #[test]
    fn an_entry_over_the_arena_limit_is_refused() {
        let json = r#"{"local_storage:key":"PLACEHOLDER-VALUE"}"#;
        let payload = "key".len() + "PLACEHOLDER-VALUE".len();
        assert!(CapturedFields::from_json_exact_with_limit(json, payload).is_ok());
        let error = CapturedFields::from_json_exact_with_limit(json, payload - 1)
            .expect_err("one byte over the limit");
        assert_eq!(error.reason(), FieldsErrorReason::ArenaFull);
        assert!(!error.to_string().contains("PLACEHOLDER"));

        let mut builder = FieldsBuilder::with_limit(payload);
        assert!(builder
            .push(FieldKind::LocalStorage, &["key"], "PLACEHOLDER-VALUE")
            .is_ok());
        assert_eq!(
            builder.push(FieldKind::LocalStorage, &["k"], ""),
            Err(ArenaFull),
            "the builder refuses a field past the limit"
        );
    }

    #[test]
    fn classifying_split_keys_matches_classifying_joined_ones() {
        for (prefix, rest) in [
            ("header:", "a"),
            ("", "header:a"),
            ("hea", "der:a"),
            ("cookie:", "0:x"),
            ("", "cookie"),
            ("", "Header:a"),
            ("local_storage:", ""),
            ("", ""),
        ] {
            assert_eq!(
                FieldKind::classify_parts(prefix, rest),
                FieldKind::classify(&format!("{prefix}{rest}")).0
            );
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
