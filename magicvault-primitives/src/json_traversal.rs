//! Stack-safe traversal helpers for externally supplied JSON values.
//!
//! `serde_json::Value` is recursive, but tool/provider payload depth is not a
//! reason to grow the native call stack.  These helpers keep traversal state on
//! the heap and provide one process-wide depth contract for values that must be
//! retained after the traversal.

use serde_json::{Map, Value};

/// Deep enough for legitimate provider/tool payloads while keeping subsequent
/// serde consumers and `Value` destruction comfortably inside ordinary stacks.
pub const MAX_RETAINED_JSON_DEPTH: usize = 64;
pub const DEPTH_LIMIT_SENTINEL: &str = "[TRUNCATED: JSON depth limit exceeded]";

/// Check only the maximum encoded container depth. Unlike
/// [`json_bytes_nesting_is_bounded`], this deliberately leaves structural
/// validity to the parser so callers with malformed-response recovery can
/// retain that behavior without exposing Serde to adversarial depth.
pub fn json_bytes_depth_is_bounded(data: &[u8], max_depth: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in data {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                if depth >= max_depth {
                    return false;
                }
                depth = depth.saturating_add(1);
            },
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {},
        }
    }
    true
}

/// Check JSON container nesting directly on encoded bytes without parsing or
/// allocating a `Value`. This is an admission preflight, not a syntax
/// validator: callers still use Serde after the depth contract is established.
/// Brackets inside strings and escaped quotes are ignored.
pub fn json_bytes_nesting_is_bounded(data: &[u8], max_depth: usize) -> bool {
    let mut stack = Vec::with_capacity(max_depth.min(64));
    let mut in_string = false;
    let mut escaped = false;
    for byte in data {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                if stack.len() >= max_depth {
                    return false;
                }
                stack.push(*byte);
            },
            b'}' => {
                if stack.pop() != Some(b'{') {
                    return false;
                }
            },
            b']' => {
                if stack.pop() != Some(b'[') {
                    return false;
                }
            },
            _ => {},
        }
    }
    !in_string && !escaped && stack.is_empty()
}

/// Count encoded JSON value nodes before Serde allocates a `Value` tree.
///
/// This is deliberately an admission preflight rather than a syntax parser:
/// [`json_bytes_nesting_is_bounded`] establishes the structural stack bound
/// and Serde remains authoritative for grammar and scalar validation. For
/// valid JSON, containers and scalar values count exactly like
/// [`inspect_json_bounded`], while object keys and structural characters do
/// not count. Strings are scanned in place, so brackets or primitive-looking
/// text inside one large string still consume exactly one node. The scanner
/// also applies [`MAX_RETAINED_JSON_DEPTH`] to its own worklist so callers that
/// use node admission directly remain memory-bounded on malformed input.
pub fn json_bytes_nodes_are_bounded(data: &[u8], max_nodes: usize) -> bool {
    let mut containers = Vec::<u8>::with_capacity(MAX_RETAINED_JSON_DEPTH);
    let mut expects_value = true;
    let mut nodes = 0usize;
    let mut index = 0usize;

    let admit_node = |nodes: &mut usize| {
        if *nodes >= max_nodes {
            return false;
        }
        *nodes = nodes.saturating_add(1);
        true
    };

    while index < data.len() {
        match data[index] {
            b' ' | b'\n' | b'\r' | b'\t' => index += 1,
            b'{' | b'[' => {
                // This helper is also used as a standalone preflight in a few
                // compatibility readers. Bound its structural worklist even
                // for malformed input such as `{{{{...`; syntax remains
                // Serde's responsibility, but node counting must never retain
                // one heap entry per hostile input byte before that parser.
                if containers.len() >= MAX_RETAINED_JSON_DEPTH {
                    return false;
                }
                if expects_value && !admit_node(&mut nodes) {
                    return false;
                }
                let container = data[index];
                containers.push(container);
                expects_value = container == b'[';
                index += 1;
            },
            b'}' | b']' => {
                containers.pop();
                expects_value = false;
                index += 1;
            },
            b':' => {
                expects_value = true;
                index += 1;
            },
            b',' => {
                expects_value = containers.last().copied() == Some(b'[');
                index += 1;
            },
            b'"' => {
                if expects_value && !admit_node(&mut nodes) {
                    return false;
                }
                expects_value = false;
                index += 1;
                let mut escaped = false;
                while index < data.len() {
                    let byte = data[index];
                    index += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
            },
            _ => {
                if expects_value && !admit_node(&mut nodes) {
                    return false;
                }
                expects_value = false;
                while index < data.len()
                    && !matches!(
                        data[index],
                        b' ' | b'\n' | b'\r' | b'\t' | b',' | b']' | b'}'
                    )
                {
                    index += 1;
                }
            },
        }
    }
    true
}

/// Incremental depth/node admission for JSON stored in files. This scanner
/// retains only the structural stack (bounded by `max_depth`) and lexical
/// state across chunks; it never retains the body. Serde remains responsible
/// for complete JSON grammar after [`Self::finish`] succeeds.
pub struct EncodedJsonStreamAdmission {
    containers: Vec<u8>,
    max_depth: usize,
    max_nodes: usize,
    nodes: usize,
    expects_value: bool,
    in_string: bool,
    escaped: bool,
    rejected: bool,
}

impl EncodedJsonStreamAdmission {
    pub fn new(max_depth: usize, max_nodes: usize) -> Self {
        Self {
            containers: Vec::with_capacity(max_depth.min(64)),
            max_depth,
            max_nodes,
            nodes: 0,
            expects_value: true,
            in_string: false,
            escaped: false,
            rejected: false,
        }
    }

    fn admit_node(&mut self) {
        if self.nodes >= self.max_nodes {
            self.rejected = true;
        } else {
            self.nodes = self.nodes.saturating_add(1);
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> bool {
        if self.rejected {
            return false;
        }
        for byte in data {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if *byte == b'\\' {
                    self.escaped = true;
                } else if *byte == b'"' {
                    self.in_string = false;
                }
                continue;
            }

            match *byte {
                b' ' | b'\n' | b'\r' | b'\t' => {},
                b'{' | b'[' => {
                    if self.expects_value {
                        self.admit_node();
                    }
                    if self.containers.len() >= self.max_depth {
                        self.rejected = true;
                    } else {
                        self.containers.push(*byte);
                    }
                    self.expects_value = *byte == b'[';
                },
                b'}' => {
                    if self.containers.pop() != Some(b'{') {
                        self.rejected = true;
                    }
                    self.expects_value = false;
                },
                b']' => {
                    if self.containers.pop() != Some(b'[') {
                        self.rejected = true;
                    }
                    self.expects_value = false;
                },
                b':' => self.expects_value = true,
                b',' => {
                    self.expects_value = self.containers.last().copied() == Some(b'[');
                },
                b'"' => {
                    if self.expects_value {
                        self.admit_node();
                    }
                    self.expects_value = false;
                    self.in_string = true;
                },
                _ => {
                    if self.expects_value {
                        self.admit_node();
                    }
                    self.expects_value = false;
                },
            }
            if self.rejected {
                return false;
            }
        }
        true
    }

    pub fn finish(&self) -> bool {
        !self.rejected
            && !self.in_string
            && !self.escaped
            && self.containers.is_empty()
            && self.nodes > 0
    }

    #[cfg(test)]
    pub fn admitted_nodes(&self) -> usize {
        self.nodes
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonMetrics {
    pub max_depth: usize,
    pub array_entries: usize,
    pub maximum_scalar_bytes: usize,
    pub nodes: usize,
}

/// Inspect a JSON tree without recursive calls.
pub fn inspect_json(root: &Value) -> JsonMetrics {
    inspect_json_impl(root, None).expect("unbounded inspection cannot reach a node limit")
}

/// Inspect until `max_nodes` values have been admitted. Returns `None` before
/// visiting or queueing another child when the limit is exceeded.
pub fn inspect_json_bounded(root: &Value, max_nodes: usize) -> Option<JsonMetrics> {
    inspect_json_impl(root, Some(max_nodes))
}

/// Compare two JSON trees without recursive `Value::eq` calls.
pub fn json_values_equal_iteratively(left: &Value, right: &Value) -> bool {
    let mut pending = vec![(left, right)];
    while let Some((left, right)) = pending.pop() {
        match (left, right) {
            (Value::Null, Value::Null) => {},
            (Value::Bool(left), Value::Bool(right)) if left == right => {},
            (Value::Number(left), Value::Number(right)) if left == right => {},
            (Value::String(left), Value::String(right)) if left == right => {},
            (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
                pending.extend(left.iter().zip(right));
            },
            (Value::Object(left), Value::Object(right)) if left.len() == right.len() => {
                for (key, left_value) in left {
                    let Some(right_value) = right.get(key) else {
                        return false;
                    };
                    pending.push((left_value, right_value));
                }
            },
            _ => return false,
        }
    }
    true
}

enum InspectFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        child_parent_depth: usize,
    },
    Object {
        remaining: serde_json::map::Values<'a>,
        child_parent_depth: usize,
    },
}

fn inspect_json_impl(root: &Value, max_nodes: Option<usize>) -> Option<JsonMetrics> {
    let mut metrics = JsonMetrics::default();
    let mut frames = Vec::<InspectFrame<'_>>::new();
    let mut current = Some((root, 0usize));
    loop {
        let (value, parent_depth) = current.take().expect("root or pending child");
        if max_nodes.is_some_and(|limit| metrics.nodes >= limit) {
            return None;
        }
        metrics.nodes = metrics.nodes.saturating_add(1);
        match value {
            Value::Array(values) => {
                let depth = parent_depth.saturating_add(1);
                metrics.max_depth = metrics.max_depth.max(depth);
                metrics.array_entries = metrics.array_entries.saturating_add(values.len());
                frames.push(InspectFrame::Array {
                    remaining: values.iter(),
                    child_parent_depth: depth,
                });
            },
            Value::Object(values) => {
                let depth = parent_depth.saturating_add(1);
                metrics.max_depth = metrics.max_depth.max(depth);
                frames.push(InspectFrame::Object {
                    remaining: values.values(),
                    child_parent_depth: depth,
                });
            },
            Value::String(text) => {
                metrics.maximum_scalar_bytes = metrics.maximum_scalar_bytes.max(text.len());
            },
            Value::Null => {},
            Value::Bool(value) => {
                metrics.maximum_scalar_bytes =
                    metrics.maximum_scalar_bytes.max(if *value { 4 } else { 5 });
            },
            Value::Number(number) => {
                metrics.maximum_scalar_bytes =
                    metrics.maximum_scalar_bytes.max(number.to_string().len());
            },
        }

        loop {
            let Some(frame) = frames.last_mut() else {
                return Some(metrics);
            };
            let next = match frame {
                InspectFrame::Array {
                    remaining,
                    child_parent_depth,
                } => remaining.next().map(|child| (child, *child_parent_depth)),
                InspectFrame::Object {
                    remaining,
                    child_parent_depth,
                } => remaining.next().map(|child| (child, *child_parent_depth)),
            };
            if let Some(next) = next {
                current = Some(next);
                break;
            }
            frames.pop();
        }
    }
}

enum BuildFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        output: Vec<Value>,
    },
    Object {
        remaining: std::vec::IntoIter<(&'a String, &'a Value)>,
        output: Map<String, Value>,
        active_key: Option<String>,
    },
}

enum CloneFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        output: Vec<Value>,
    },
    Object {
        remaining: serde_json::map::Iter<'a>,
        output: Map<String, Value>,
        active_key: Option<String>,
    },
}

enum OwnedCanonicalFrame {
    Array {
        remaining: std::vec::IntoIter<Value>,
        output: Vec<Value>,
    },
    Object {
        remaining: std::vec::IntoIter<(String, Value)>,
        output: Map<String, Value>,
        active_key: Option<String>,
    },
}

/// Clone and deterministically sort a JSON value without recursive clone or
/// traversal. Callers retaining the result must first enforce
/// [`MAX_RETAINED_JSON_DEPTH`].
pub fn canonicalize_json(root: &Value) -> Value {
    let mut frames = Vec::<BuildFrame<'_>>::new();
    let mut current = root;
    let mut produced: Option<Value> = None;

    loop {
        if produced.is_none() {
            match current {
                Value::Array(values) if !values.is_empty() => {
                    let mut remaining = values.iter();
                    current = remaining.next().expect("non-empty array");
                    frames.push(BuildFrame::Array {
                        remaining,
                        output: Vec::with_capacity(values.len()),
                    });
                    continue;
                },
                Value::Object(values) if !values.is_empty() => {
                    let mut entries = values.iter().collect::<Vec<_>>();
                    entries.sort_by(|left, right| left.0.cmp(right.0));
                    let mut remaining = entries.into_iter();
                    let (key, child) = remaining.next().expect("non-empty object");
                    current = child;
                    frames.push(BuildFrame::Object {
                        remaining,
                        output: Map::new(),
                        active_key: Some(key.clone()),
                    });
                    continue;
                },
                Value::Array(_) => produced = Some(Value::Array(Vec::new())),
                Value::Object(_) => produced = Some(Value::Object(Map::new())),
                scalar => produced = Some(scalar.clone()),
            }
        }

        let value = produced.take().expect("a scalar or completed container");
        let Some(frame) = frames.last_mut() else {
            return value;
        };
        match frame {
            BuildFrame::Array { remaining, output } => {
                output.push(value);
                if let Some(child) = remaining.next() {
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Array(output));
                }
            },
            BuildFrame::Object {
                remaining,
                output,
                active_key,
            } => {
                output.insert(
                    active_key.take().expect("object child has an active key"),
                    value,
                );
                if let Some((key, child)) = remaining.next() {
                    *active_key = Some(key.clone());
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Object(output));
                }
            },
        }
    }
}

/// Clone a JSON value while retaining source object order and keeping traversal
/// state on the heap. Callers retaining the clone must first enforce
/// [`MAX_RETAINED_JSON_DEPTH`].
pub fn clone_json_iteratively(root: &Value) -> Value {
    let mut frames = Vec::<CloneFrame<'_>>::new();
    let mut current = root;
    let mut produced: Option<Value> = None;

    loop {
        if produced.is_none() {
            match current {
                Value::Array(values) if !values.is_empty() => {
                    let mut remaining = values.iter();
                    current = remaining.next().expect("non-empty array");
                    frames.push(CloneFrame::Array {
                        remaining,
                        output: Vec::with_capacity(values.len()),
                    });
                    continue;
                },
                Value::Object(values) if !values.is_empty() => {
                    let mut remaining = values.iter();
                    let (key, child) = remaining.next().expect("non-empty object");
                    current = child;
                    frames.push(CloneFrame::Object {
                        remaining,
                        output: Map::new(),
                        active_key: Some(key.clone()),
                    });
                    continue;
                },
                Value::Array(_) => produced = Some(Value::Array(Vec::new())),
                Value::Object(_) => produced = Some(Value::Object(Map::new())),
                scalar => produced = Some(scalar.clone()),
            }
        }

        let value = produced.take().expect("a scalar or completed container");
        let Some(frame) = frames.last_mut() else {
            return value;
        };
        match frame {
            CloneFrame::Array { remaining, output } => {
                output.push(value);
                if let Some(child) = remaining.next() {
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Array(output));
                }
            },
            CloneFrame::Object {
                remaining,
                output,
                active_key,
            } => {
                output.insert(
                    active_key.take().expect("object child has an active key"),
                    value,
                );
                if let Some((key, child)) = remaining.next() {
                    *active_key = Some(key.clone());
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Object(output));
                }
            },
        }
    }
}

/// Admit and clone an externally derived value in one shared contract. The
/// returned clone is exact; `None` means retaining it would exceed at least one
/// heap/stack/encoded-size boundary.
pub fn clone_json_bounded(
    root: &Value,
    max_nodes: usize,
    max_bytes: usize,
    max_depth: usize,
) -> Option<Value> {
    let metrics = inspect_json_bounded(root, max_nodes)?;
    if metrics.max_depth > max_depth || exact_json_encoded_len(root) > max_bytes {
        return None;
    }
    Some(clone_json_iteratively(root))
}

/// Deserialize a retained JSON tree only after proving that generic Serde's
/// recursive traversal is bounded. The one required owned input is copied
/// with heap frames rather than `Value::clone`.
pub fn deserialize_json_bounded<T>(
    root: &Value,
    max_nodes: usize,
    max_depth: usize,
) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned,
{
    let metrics = inspect_json_bounded(root, max_nodes).ok_or_else(|| {
        serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON value exceeds {max_nodes} nodes"),
        ))
    })?;
    if metrics.max_depth > max_depth {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("JSON value exceeds depth {max_depth}"),
        )));
    }
    serde_json::from_value(clone_json_iteratively(root))
}

#[derive(Default)]
struct BoundedBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
    exceeded: bool,
}

struct PrefixBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
    truncated: bool,
}

impl std::io::Write for PrefixBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = self.max_bytes.saturating_sub(self.bytes.len());
        if buffer.len() > remaining {
            if remaining > 0 {
                self.bytes.extend_from_slice(&buffer[..remaining]);
            }
            self.truncated = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "JSON prefix byte ceiling reached",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Write for BoundedBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if !self.exceeded {
            let remaining = self.max_bytes.saturating_sub(self.bytes.len());
            if buffer.len() <= remaining {
                self.bytes.extend_from_slice(buffer);
            } else {
                self.exceeded = true;
                self.bytes.clear();
            }
        }
        // Continue the bounded-depth serializer after crossing the limit so an
        // ordinary size rejection is not surfaced as an I/O failure.
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Pretty-print an admitted JSON value without ever retaining more than
/// `max_bytes` of output. `None` means the pretty representation exceeded the
/// cap; serialization failures remain distinguishable as errors.
pub fn pretty_json_bytes_bounded(
    root: &Value,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, serde_json::Error> {
    let mut output = BoundedBuffer {
        bytes: Vec::with_capacity(max_bytes.min(64 * 1024)),
        max_bytes,
        exceeded: false,
    };
    write_pretty_json(root, &mut output)?;
    Ok((!output.exceeded).then_some(output.bytes))
}

/// Pretty-serialize any already shape-admitted value while retaining at most
/// `max_bytes`. Unlike `serde_json::to_value` followed by pretty rendering,
/// this does not allocate an intermediate JSON tree or a second full string.
pub fn pretty_serialized_bytes_bounded<T: serde::Serialize + ?Sized>(
    value: &T,
    max_bytes: usize,
) -> Result<Option<Vec<u8>>, serde_json::Error> {
    let mut output = BoundedBuffer {
        bytes: Vec::with_capacity(max_bytes.min(64 * 1024)),
        max_bytes,
        exceeded: false,
    };
    serde_json::to_writer_pretty(&mut output, value)?;
    Ok((!output.exceeded).then_some(output.bytes))
}

/// Measure pretty JSON without allocating its rendered body.
pub fn pretty_serialized_len<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<usize, serde_json::Error> {
    let mut output = CountingWriter::default();
    serde_json::to_writer_pretty(&mut output, value)?;
    Ok(output.bytes)
}

enum PrettyWriteFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        depth: usize,
        first: bool,
    },
    Object {
        remaining: serde_json::map::Iter<'a>,
        depth: usize,
        first: bool,
    },
}

fn write_json_bytes<W: std::io::Write + ?Sized>(
    output: &mut W,
    bytes: &[u8],
) -> Result<(), serde_json::Error> {
    output.write_all(bytes).map_err(serde_json::Error::io)
}

fn write_pretty_indent<W: std::io::Write>(
    output: &mut W,
    depth: usize,
) -> Result<(), serde_json::Error> {
    for _ in 0..depth {
        write_json_bytes(output, b"  ")?;
    }
    Ok(())
}

/// Write the exact default `serde_json` pretty representation of a `Value`
/// while retaining traversal state on the heap.
pub fn write_pretty_json<W: std::io::Write>(
    root: &Value,
    output: &mut W,
) -> Result<(), serde_json::Error> {
    let mut frames = Vec::<PrettyWriteFrame<'_>>::new();
    let mut current = Some((root, 0usize));
    loop {
        if let Some((value, depth)) = current.take() {
            match value {
                Value::Null => write_json_bytes(output, b"null")?,
                Value::Bool(value) => {
                    if *value {
                        write_json_bytes(output, b"true")?;
                    } else {
                        write_json_bytes(output, b"false")?;
                    }
                },
                Value::Number(number) => write_json_bytes(output, number.to_string().as_bytes())?,
                Value::String(text) => serde_json::to_writer(&mut *output, text)?,
                Value::Array(values) if values.is_empty() => write_json_bytes(output, b"[]")?,
                Value::Array(values) => {
                    write_json_bytes(output, b"[")?;
                    frames.push(PrettyWriteFrame::Array {
                        remaining: values.iter(),
                        depth,
                        first: true,
                    });
                },
                Value::Object(values) if values.is_empty() => write_json_bytes(output, b"{}")?,
                Value::Object(values) => {
                    write_json_bytes(output, b"{")?;
                    frames.push(PrettyWriteFrame::Object {
                        remaining: values.iter(),
                        depth,
                        first: true,
                    });
                },
            }
        }

        loop {
            let Some(frame) = frames.last_mut() else {
                return Ok(());
            };
            match frame {
                PrettyWriteFrame::Array {
                    remaining,
                    depth,
                    first,
                } => {
                    if let Some(value) = remaining.next() {
                        if *first {
                            write_json_bytes(output, b"\n")?;
                        } else {
                            write_json_bytes(output, b",\n")?;
                        }
                        *first = false;
                        write_pretty_indent(output, depth.saturating_add(1))?;
                        current = Some((value, depth.saturating_add(1)));
                        break;
                    }
                    write_json_bytes(output, b"\n")?;
                    write_pretty_indent(output, *depth)?;
                    write_json_bytes(output, b"]")?;
                    frames.pop();
                },
                PrettyWriteFrame::Object {
                    remaining,
                    depth,
                    first,
                } => {
                    if let Some((key, value)) = remaining.next() {
                        if *first {
                            write_json_bytes(output, b"\n")?;
                        } else {
                            write_json_bytes(output, b",\n")?;
                        }
                        *first = false;
                        write_pretty_indent(output, depth.saturating_add(1))?;
                        serde_json::to_writer(&mut *output, key)?;
                        write_json_bytes(output, b": ")?;
                        current = Some((value, depth.saturating_add(1)));
                        break;
                    }
                    write_json_bytes(output, b"\n")?;
                    write_pretty_indent(output, *depth)?;
                    write_json_bytes(output, b"}")?;
                    frames.pop();
                },
            }
        }
    }
}

/// Render only a compact UTF-8 prefix of a JSON value after iterative
/// depth/node admission. This is for non-authoritative prompt previews: the
/// returned text may be syntactically incomplete when `truncated` is true.
pub fn compact_json_prefix_bounded(
    root: &Value,
    max_bytes: usize,
    max_nodes: usize,
    max_depth: usize,
) -> Result<Option<(String, bool)>, serde_json::Error> {
    let Some(metrics) = inspect_json_bounded(root, max_nodes) else {
        return Ok(None);
    };
    if metrics.max_depth > max_depth {
        return Ok(None);
    }
    let mut output = PrefixBuffer {
        bytes: Vec::with_capacity(max_bytes.min(64 * 1024)),
        max_bytes,
        truncated: false,
    };
    // The value has been structurally admitted, but this helper can still run
    // on an already-deep execution future. Keep the serializer's own traversal
    // frames on the heap too; admission alone should not make a recursive
    // `Value::serialize` call chain part of the runtime stack budget.
    if let Err(error) = write_json(root, &mut output) {
        // `PrefixBuffer` is an in-memory writer, so its only I/O failure is
        // the private early-stop signal above. Preserve genuine serializer
        // failures while treating the signalled prefix ceiling as successful
        // truncation.
        if !output.truncated {
            return Err(error);
        }
    }
    if let Err(error) = std::str::from_utf8(&output.bytes) {
        output.bytes.truncate(error.valid_up_to());
        output.truncated = true;
    }
    // Serde produced valid UTF-8; after trimming a partial final codepoint the
    // retained prefix is valid by construction.
    let text = String::from_utf8(output.bytes).expect("trimmed JSON prefix is UTF-8");
    Ok(Some((text, output.truncated)))
}

/// Deterministically order an owned JSON value without retaining a second
/// payload-sized tree and without recursive traversal or drop.
pub fn canonicalize_json_owned(root: Value) -> Value {
    let mut frames = Vec::<OwnedCanonicalFrame>::new();
    let mut current = root;
    let mut produced: Option<Value> = None;

    loop {
        if produced.is_none() {
            let value = std::mem::replace(&mut current, Value::Null);
            match value {
                Value::Array(values) if !values.is_empty() => {
                    let capacity = values.len();
                    let mut remaining = values.into_iter();
                    current = remaining.next().expect("non-empty array");
                    frames.push(OwnedCanonicalFrame::Array {
                        remaining,
                        output: Vec::with_capacity(capacity),
                    });
                    continue;
                },
                Value::Object(values) if !values.is_empty() => {
                    let mut entries = values.into_iter().collect::<Vec<_>>();
                    entries.sort_by(|left, right| left.0.cmp(&right.0));
                    let mut remaining = entries.into_iter();
                    let (key, child) = remaining.next().expect("non-empty object");
                    current = child;
                    frames.push(OwnedCanonicalFrame::Object {
                        remaining,
                        output: Map::new(),
                        active_key: Some(key),
                    });
                    continue;
                },
                scalar => produced = Some(scalar),
            }
        }

        let value = produced.take().expect("scalar or completed container");
        let Some(frame) = frames.last_mut() else {
            return value;
        };
        match frame {
            OwnedCanonicalFrame::Array { remaining, output } => {
                output.push(value);
                if let Some(child) = remaining.next() {
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Array(output));
                }
            },
            OwnedCanonicalFrame::Object {
                remaining,
                output,
                active_key,
            } => {
                output.insert(
                    active_key.take().expect("object child has an active key"),
                    value,
                );
                if let Some((key, child)) = remaining.next() {
                    *active_key = Some(key);
                    current = child;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Object(output));
                }
            },
        }
    }
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Measure one JSON string, including quotes and escaping, without allocating
/// the encoded representation. This is useful before constructing a retained
/// envelope whose raw text can expand substantially when escaped.
pub fn json_string_encoded_len(text: &str) -> Result<usize, serde_json::Error> {
    let mut output = CountingWriter::default();
    serde_json::to_writer(&mut output, text)?;
    Ok(output.bytes)
}

enum CountingFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        first: bool,
    },
    Object {
        remaining: serde_json::map::Iter<'a>,
        first: bool,
    },
}

enum CanonicalWriteFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        first: bool,
    },
    Object {
        remaining: std::vec::IntoIter<(&'a String, &'a Value)>,
        first: bool,
    },
}

enum JsonWriteFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        first: bool,
    },
    Object {
        remaining: serde_json::map::Iter<'a>,
        first: bool,
    },
}

/// Measure compact JSON encoding without allocating a second payload-sized
/// byte buffer or invoking recursive serde traversal.
pub fn json_encoded_len(root: &Value) -> Result<usize, serde_json::Error> {
    let mut output = CountingWriter::default();
    let mut frames = Vec::<CountingFrame<'_>>::new();
    let mut current = Some(root);
    loop {
        if let Some(value) = current.take() {
            match value {
                Value::Null => output.bytes = output.bytes.saturating_add(4),
                Value::Bool(value) => {
                    output.bytes = output.bytes.saturating_add(if *value { 4 } else { 5 });
                },
                Value::Number(number) => {
                    output.bytes = output.bytes.saturating_add(number.to_string().len());
                },
                Value::String(text) => serde_json::to_writer(&mut output, text)?,
                Value::Array(values) => {
                    output.bytes = output.bytes.saturating_add(1);
                    frames.push(CountingFrame::Array {
                        remaining: values.iter(),
                        first: true,
                    });
                },
                Value::Object(values) => {
                    output.bytes = output.bytes.saturating_add(1);
                    frames.push(CountingFrame::Object {
                        remaining: values.iter(),
                        first: true,
                    });
                },
            }
        }

        loop {
            let Some(frame) = frames.last_mut() else {
                return Ok(output.bytes);
            };
            match frame {
                CountingFrame::Array { remaining, first } => {
                    if let Some(value) = remaining.next() {
                        if !*first {
                            output.bytes = output.bytes.saturating_add(1);
                        }
                        *first = false;
                        current = Some(value);
                        break;
                    }
                    output.bytes = output.bytes.saturating_add(1);
                    frames.pop();
                },
                CountingFrame::Object { remaining, first } => {
                    if let Some((key, value)) = remaining.next() {
                        if !*first {
                            output.bytes = output.bytes.saturating_add(1);
                        }
                        *first = false;
                        serde_json::to_writer(&mut output, key)?;
                        output.bytes = output.bytes.saturating_add(1);
                        current = Some(value);
                        break;
                    }
                    output.bytes = output.bytes.saturating_add(1);
                    frames.pop();
                },
            }
        }
    }
}

/// Infallible, fail-closed form used by admission boundaries. `Value`
/// serialization should not fail, but returning `usize::MAX` preserves the
/// limit contract if a future serializer behavior introduces an error.
pub fn exact_json_encoded_len(root: &Value) -> usize {
    json_encoded_len(root).unwrap_or(usize::MAX)
}

/// Serialize canonical JSON without recursive serde traversal.
pub fn canonical_json_bytes(root: &Value) -> Result<Vec<u8>, serde_json::Error> {
    canonical_json_bytes_with_capacity(root, 0)
}

/// Serialize canonical JSON into an exactly pre-sized buffer when the caller
/// already performed a counting pass. This avoids reallocating and briefly
/// retaining multiple backing buffers for large admitted values.
pub fn canonical_json_bytes_with_capacity(
    root: &Value,
    capacity: usize,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut output = Vec::with_capacity(capacity);
    write_canonical_json(root, &mut output)?;
    Ok(output)
}

/// Write canonical JSON iteratively into an arbitrary sink. This preserves the
/// exact byte contract of [`canonical_json_bytes`] while allowing hashes and
/// atomic file materialization to avoid a payload-sized byte vector.
pub fn write_canonical_json<W: std::io::Write + ?Sized>(
    root: &Value,
    output: &mut W,
) -> Result<(), serde_json::Error> {
    let mut frames = Vec::<CanonicalWriteFrame<'_>>::new();
    let mut current = Some(root);
    loop {
        if let Some(value) = current.take() {
            match value {
                Value::Null => write_json_bytes(output, b"null")?,
                Value::Bool(value) => {
                    if *value {
                        write_json_bytes(output, b"true")?;
                    } else {
                        write_json_bytes(output, b"false")?;
                    }
                },
                Value::Number(number) => {
                    write_json_bytes(output, number.to_string().as_bytes())?;
                },
                Value::String(text) => serde_json::to_writer(&mut *output, text)?,
                Value::Array(values) => {
                    write_json_bytes(output, b"[")?;
                    frames.push(CanonicalWriteFrame::Array {
                        remaining: values.iter(),
                        first: true,
                    });
                },
                Value::Object(values) => {
                    write_json_bytes(output, b"{")?;
                    let mut entries = values.iter().collect::<Vec<_>>();
                    entries.sort_by(|left, right| left.0.cmp(right.0));
                    frames.push(CanonicalWriteFrame::Object {
                        remaining: entries.into_iter(),
                        first: true,
                    });
                },
            }
        }

        loop {
            let Some(frame) = frames.last_mut() else {
                return Ok(());
            };
            match frame {
                CanonicalWriteFrame::Array { remaining, first } => {
                    if let Some(value) = remaining.next() {
                        if !*first {
                            write_json_bytes(output, b",")?;
                        }
                        *first = false;
                        current = Some(value);
                        break;
                    }
                    write_json_bytes(output, b"]")?;
                    frames.pop();
                },
                CanonicalWriteFrame::Object { remaining, first } => {
                    if let Some((key, value)) = remaining.next() {
                        if !*first {
                            write_json_bytes(output, b",")?;
                        }
                        *first = false;
                        serde_json::to_writer(&mut *output, key)?;
                        write_json_bytes(output, b":")?;
                        current = Some(value);
                        break;
                    }
                    write_json_bytes(output, b"}")?;
                    frames.pop();
                },
            }
        }
    }
}

/// Hash canonical JSON without materializing a payload-sized byte vector.
/// This is the digest counterpart of [`write_canonical_json`], including its
/// deterministic object-key ordering and heap-backed traversal state.
pub fn canonical_json_blake3_hex(root: &Value) -> Result<String, serde_json::Error> {
    struct HashWriter<'a>(&'a mut blake3::Hasher);

    impl std::io::Write for HashWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut hasher = blake3::Hasher::new();
    write_canonical_json(root, &mut HashWriter(&mut hasher))?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash the compact `Value` JSON wire without materializing an encoded
/// buffer. The writer cannot fail, while the `Result` preserves the same
/// future-proof serialization contract as [`write_json`].
pub fn json_blake3_hex(root: &Value) -> Result<String, serde_json::Error> {
    struct HashWriter<'a>(&'a mut blake3::Hasher);

    impl std::io::Write for HashWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut hasher = blake3::Hasher::new();
    write_json(root, &mut HashWriter(&mut hasher))?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Write compact JSON in the `Value`'s existing object iteration order while
/// retaining traversal state on the heap. This matches `serde_json::to_writer`
/// for a `Value` without recursive serializer frames.
pub fn write_json<W: std::io::Write + ?Sized>(
    root: &Value,
    output: &mut W,
) -> Result<(), serde_json::Error> {
    let mut frames = Vec::<JsonWriteFrame<'_>>::new();
    let mut current = Some(root);
    loop {
        if let Some(value) = current.take() {
            match value {
                Value::Null => write_json_bytes(output, b"null")?,
                Value::Bool(value) => {
                    if *value {
                        write_json_bytes(output, b"true")?;
                    } else {
                        write_json_bytes(output, b"false")?;
                    }
                },
                Value::Number(number) => write_json_bytes(output, number.to_string().as_bytes())?,
                Value::String(text) => serde_json::to_writer(&mut *output, text)?,
                Value::Array(values) => {
                    write_json_bytes(output, b"[")?;
                    frames.push(JsonWriteFrame::Array {
                        remaining: values.iter(),
                        first: true,
                    });
                },
                Value::Object(values) => {
                    write_json_bytes(output, b"{")?;
                    frames.push(JsonWriteFrame::Object {
                        remaining: values.iter(),
                        first: true,
                    });
                },
            }
        }

        loop {
            let Some(frame) = frames.last_mut() else {
                return Ok(());
            };
            match frame {
                JsonWriteFrame::Array { remaining, first } => {
                    if let Some(value) = remaining.next() {
                        if !*first {
                            write_json_bytes(output, b",")?;
                        }
                        *first = false;
                        current = Some(value);
                        break;
                    }
                    write_json_bytes(output, b"]")?;
                    frames.pop();
                },
                JsonWriteFrame::Object { remaining, first } => {
                    if let Some((key, value)) = remaining.next() {
                        if !*first {
                            write_json_bytes(output, b",")?;
                        }
                        *first = false;
                        serde_json::to_writer(&mut *output, key)?;
                        write_json_bytes(output, b":")?;
                        current = Some(value);
                        break;
                    }
                    write_json_bytes(output, b"}")?;
                    frames.pop();
                },
            }
        }
    }
}

enum DiscardFrame {
    Array(std::vec::IntoIter<Value>),
    Object(serde_json::map::IntoIter),
}

pub fn discard_json_iteratively(root: Value) {
    let mut frames = Vec::<DiscardFrame>::new();
    let mut current = Some(root);
    loop {
        if let Some(value) = current.take() {
            match value {
                Value::Array(values) => frames.push(DiscardFrame::Array(values.into_iter())),
                Value::Object(values) => frames.push(DiscardFrame::Object(values.into_iter())),
                _ => {},
            }
        }
        loop {
            let Some(frame) = frames.last_mut() else {
                return;
            };
            let next = match frame {
                DiscardFrame::Array(remaining) => remaining.next(),
                DiscardFrame::Object(remaining) => remaining.next().map(|(_, value)| value),
            };
            if let Some(next) = next {
                current = Some(next);
                break;
            }
            frames.pop();
        }
    }
}

enum OwnedFrame {
    Array {
        values: Vec<Value>,
        next_index: usize,
        depth: usize,
    },
    Object {
        remaining: serde_json::map::IntoIter,
        output: Map<String, Value>,
        active_key: Option<String>,
        depth: usize,
    },
}

enum BorrowedMappedFrame<'a> {
    Array {
        remaining: std::slice::Iter<'a, Value>,
        output: Vec<Value>,
        depth: usize,
    },
    Object {
        remaining: std::vec::IntoIter<(&'a String, &'a Value)>,
        output: Map<String, Value>,
        active_key: Option<String>,
        depth: usize,
    },
}

/// Canonically clone and map strings from a borrowed JSON tree without first
/// constructing a complete recursive clone. Only the retained-depth prefix is
/// copied; deeper containers become the shared fail-closed sentinel.
pub fn map_json_strings_borrowed_canonical<F, R>(
    root: &Value,
    mut map_string: F,
    mut replace_field: R,
) -> Value
where
    F: FnMut(&str) -> String,
    R: FnMut(&str, &Value) -> Option<Value>,
{
    let mut frames = Vec::<BorrowedMappedFrame<'_>>::new();
    let mut current = root;
    let mut current_depth = 0usize;
    let mut produced: Option<Value> = None;

    loop {
        if produced.is_none() {
            let is_container = matches!(current, Value::Array(_) | Value::Object(_));
            if current_depth >= MAX_RETAINED_JSON_DEPTH && is_container {
                produced = Some(Value::String(DEPTH_LIMIT_SENTINEL.to_string()));
            } else {
                match current {
                    Value::String(text) => produced = Some(Value::String(map_string(text))),
                    Value::Array(values) if !values.is_empty() => {
                        let mut remaining = values.iter();
                        current = remaining.next().expect("non-empty array");
                        current_depth = current_depth.saturating_add(1);
                        frames.push(BorrowedMappedFrame::Array {
                            remaining,
                            output: Vec::with_capacity(values.len()),
                            depth: current_depth,
                        });
                        continue;
                    },
                    Value::Object(values) if !values.is_empty() => {
                        let mut entries = values.iter().collect::<Vec<_>>();
                        entries.sort_by(|left, right| left.0.cmp(right.0));
                        let mut remaining = entries.into_iter();
                        let (key, child) = remaining.next().expect("non-empty object");
                        current_depth = current_depth.saturating_add(1);
                        if let Some(replacement) = replace_field(key, child) {
                            produced = Some(replacement);
                        } else {
                            current = child;
                        }
                        frames.push(BorrowedMappedFrame::Object {
                            remaining,
                            output: Map::new(),
                            active_key: Some(key.clone()),
                            depth: current_depth,
                        });
                        if produced.is_none() {
                            continue;
                        }
                    },
                    Value::Array(_) => produced = Some(Value::Array(Vec::new())),
                    Value::Object(_) => produced = Some(Value::Object(Map::new())),
                    scalar => produced = Some(scalar.clone()),
                }
            }
        }

        let value = produced.take().expect("a scalar or completed container");
        let Some(frame) = frames.last_mut() else {
            return value;
        };
        match frame {
            BorrowedMappedFrame::Array {
                remaining,
                output,
                depth,
            } => {
                output.push(value);
                if let Some(child) = remaining.next() {
                    current = child;
                    current_depth = *depth;
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Array(output));
                }
            },
            BorrowedMappedFrame::Object {
                remaining,
                output,
                active_key,
                depth,
            } => {
                output.insert(
                    active_key.take().expect("object child has an active key"),
                    value,
                );
                if let Some((key, child)) = remaining.next() {
                    current_depth = *depth;
                    if let Some(replacement) = replace_field(key, child) {
                        produced = Some(replacement);
                    } else {
                        current = child;
                    }
                    *active_key = Some(key.clone());
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Object(output));
                }
            },
        }
    }
}

/// Map strings and optionally replace exact object fields while consuming the
/// input iteratively. Containers below the retained-depth contract are drained
/// without recursive drop and replaced with a fail-closed sentinel.
pub fn map_json_strings_owned<F, R>(root: Value, mut map_string: F, mut replace_field: R) -> Value
where
    F: FnMut(String) -> String,
    R: FnMut(&str, &Value) -> Option<Value>,
{
    let mut frames = Vec::<OwnedFrame>::new();
    let mut current = root;
    let mut current_depth = 0usize;
    let mut produced: Option<Value> = None;

    loop {
        if produced.is_none() {
            let value = std::mem::replace(&mut current, Value::Null);
            let is_container = matches!(&value, Value::Array(_) | Value::Object(_));
            if current_depth >= MAX_RETAINED_JSON_DEPTH && is_container {
                discard_json_iteratively(value);
                produced = Some(Value::String(DEPTH_LIMIT_SENTINEL.to_string()));
            } else {
                match value {
                    Value::String(text) => produced = Some(Value::String(map_string(text))),
                    Value::Array(mut values) if !values.is_empty() => {
                        current = std::mem::take(&mut values[0]);
                        current_depth = current_depth.saturating_add(1);
                        frames.push(OwnedFrame::Array {
                            values,
                            next_index: 1,
                            depth: current_depth,
                        });
                        continue;
                    },
                    Value::Object(values) if !values.is_empty() => {
                        let mut remaining = values.into_iter();
                        let (key, child) = remaining.next().expect("non-empty object");
                        current_depth = current_depth.saturating_add(1);
                        if let Some(replacement) = replace_field(&key, &child) {
                            discard_json_iteratively(child);
                            produced = Some(replacement);
                        } else {
                            current = child;
                        }
                        frames.push(OwnedFrame::Object {
                            remaining,
                            output: Map::new(),
                            active_key: Some(key),
                            depth: current_depth,
                        });
                        if produced.is_some() {
                            // Attach the replacement through the common bubble path.
                        } else {
                            continue;
                        }
                    },
                    Value::Array(_) => produced = Some(Value::Array(Vec::new())),
                    Value::Object(_) => produced = Some(Value::Object(Map::new())),
                    scalar => produced = Some(scalar),
                }
            }
        }

        let value = produced.take().expect("a scalar or completed container");
        let Some(frame) = frames.last_mut() else {
            return value;
        };
        match frame {
            OwnedFrame::Array {
                values,
                next_index,
                depth,
            } => {
                let completed_index = next_index.saturating_sub(1);
                values[completed_index] = value;
                if *next_index < values.len() {
                    current = std::mem::take(&mut values[*next_index]);
                    *next_index = next_index.saturating_add(1);
                    current_depth = *depth;
                } else {
                    let output = std::mem::take(values);
                    frames.pop();
                    produced = Some(Value::Array(output));
                }
            },
            OwnedFrame::Object {
                remaining,
                output,
                active_key,
                depth,
            } => {
                output.insert(
                    active_key.take().expect("object child has an active key"),
                    value,
                );
                if let Some((key, child)) = remaining.next() {
                    current_depth = *depth;
                    if let Some(replacement) = replace_field(&key, &child) {
                        discard_json_iteratively(child);
                        produced = Some(replacement);
                    } else {
                        current = child;
                    }
                    *active_key = Some(key);
                } else {
                    let output = std::mem::take(output);
                    frames.pop();
                    produced = Some(Value::Object(output));
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deeply_nested_array(depth: usize) -> Value {
        let mut value = Value::String("secret".to_string());
        for _ in 0..depth {
            value = Value::Array(vec![value]);
        }
        value
    }

    #[test]
    fn iterative_metrics_and_serialization_handle_deep_values() {
        let value = deeply_nested_array(1_000);
        let metrics = inspect_json(&value);
        assert_eq!(metrics.max_depth, 1_000);
        assert_eq!(metrics.array_entries, 1_000);
        let bytes = canonical_json_bytes(&value).expect("canonical bytes");
        assert_eq!(
            json_encoded_len(&value).expect("encoded length"),
            bytes.len(),
            "the counting writer must exactly match compact JSON encoding"
        );
        assert!(bytes.starts_with(&vec![b'['; 1_000]));
        discard_json_iteratively(value);
    }

    #[test]
    fn bounded_metrics_stop_at_the_node_limit_for_a_wide_container() {
        let value = Value::Array(vec![Value::Null; 100_000]);
        assert_eq!(inspect_json_bounded(&value, 100), None);
        assert_eq!(
            inspect_json_bounded(&value, 100_001)
                .expect("root plus every child fits")
                .nodes,
            100_001
        );
    }

    #[test]
    fn encoded_nesting_preflight_ignores_strings_and_rejects_mismatched_or_deep_containers() {
        assert!(json_bytes_nesting_is_bounded(
            br#"{"text":"[{\\\"still text\\\"}]","value":[1]}"#,
            2,
        ));
        assert!(!json_bytes_nesting_is_bounded(br#"{"value":[1]}"#, 1));
        assert!(!json_bytes_nesting_is_bounded(br#"{"value":[1}"#, 8));
        assert!(!json_bytes_nesting_is_bounded(br#"{"value":]}"#, 8));
        assert!(json_bytes_depth_is_bounded(br#"{"value":]}"#, 8));
        assert!(!json_bytes_depth_is_bounded(&[b'['; 9], 8));
    }

    #[test]
    fn encoded_node_preflight_is_exact_and_does_not_count_string_contents_or_object_keys() {
        let exact = br#"{"text":"[null,{\"nested\":true}]","items":[null,false]}"#;
        // Root object + text string + items array + two array scalars.
        assert!(json_bytes_nodes_are_bounded(exact, 5));
        assert!(!json_bytes_nodes_are_bounded(exact, 4));

        let wide_string = format!(r#"{{"value":"{}"}}"#, "[null]".repeat(100_000));
        assert!(json_bytes_nodes_are_bounded(wide_string.as_bytes(), 2));
        assert!(!json_bytes_nodes_are_bounded(wide_string.as_bytes(), 1));
    }

    #[test]
    fn encoded_node_preflight_bounds_its_worklist_for_malformed_opening_delimiters() {
        let malformed = vec![b'{'; 100_000];
        assert!(!json_bytes_nodes_are_bounded(&malformed, usize::MAX));
    }

    #[test]
    fn streaming_encoded_admission_preserves_exact_limits_across_chunk_boundaries() {
        let value = br#"{"text":"escaped \" [null]","rows":[null,false]}"#;
        let mut admission = EncodedJsonStreamAdmission::new(3, 5);
        for byte in value {
            assert!(admission.feed(std::slice::from_ref(byte)));
        }
        assert!(admission.finish());
        assert_eq!(admission.admitted_nodes(), 5);

        let mut one_short = EncodedJsonStreamAdmission::new(3, 4);
        assert!(!one_short.feed(value));
        assert!(!one_short.finish());
    }

    #[test]
    fn owned_mapping_truncates_before_retained_values_become_too_deep() {
        let value = deeply_nested_array(1_000);
        let mapped = map_json_strings_owned(
            value,
            |text| text.replace("secret", "[REDACTED]"),
            |_, _| None,
        );
        let metrics = inspect_json(&mapped);
        assert!(metrics.max_depth <= MAX_RETAINED_JSON_DEPTH);
        assert!(canonical_json_bytes(&mapped)
            .expect("mapped bytes")
            .windows(DEPTH_LIMIT_SENTINEL.len())
            .any(|window| window == DEPTH_LIMIT_SENTINEL.as_bytes()));
    }

    #[test]
    fn owned_mapping_truncates_empty_containers_at_the_exact_depth_boundary() {
        for terminal in [Value::Array(Vec::new()), Value::Object(Map::new())] {
            let mut value = terminal;
            for _ in 0..MAX_RETAINED_JSON_DEPTH {
                value = Value::Array(vec![value]);
            }

            let mapped = map_json_strings_owned(value, std::convert::identity, |_, _| None);
            let metrics = inspect_json(&mapped);
            assert_eq!(metrics.max_depth, MAX_RETAINED_JSON_DEPTH);
            assert!(canonical_json_bytes(&mapped)
                .expect("mapped boundary bytes")
                .windows(DEPTH_LIMIT_SENTINEL.len())
                .any(|window| window == DEPTH_LIMIT_SENTINEL.as_bytes()));
        }
    }

    #[test]
    fn owned_mapping_moves_object_keys_without_payload_sized_key_clones() {
        let key = "provider-owned-key-".repeat(8_192);
        let key_ptr = key.as_ptr();
        let mut object = Map::new();
        object.insert(key, Value::Null);

        let mapped =
            map_json_strings_owned(Value::Object(object), std::convert::identity, |_, _| None);
        let retained_key = mapped
            .as_object()
            .and_then(|object| object.keys().next())
            .expect("mapped key");
        assert_eq!(retained_key.as_ptr(), key_ptr);
    }

    #[test]
    fn borrowed_canonical_mapping_matches_owned_mapping_and_bounds_retained_depth() {
        let fixture = serde_json::json!({
            "z": ["one", {"password": "secret"}],
            "a": "two",
        });
        let borrowed = map_json_strings_borrowed_canonical(
            &fixture,
            |text| text.to_ascii_uppercase(),
            |key, value| {
                (key == "password" && !value.is_null())
                    .then(|| Value::String("[REDACTED]".to_string()))
            },
        );
        let owned = map_json_strings_owned(
            canonicalize_json(&fixture),
            |text| text.to_ascii_uppercase(),
            |key, value| {
                (key == "password" && !value.is_null())
                    .then(|| Value::String("[REDACTED]".to_string()))
            },
        );
        assert_eq!(borrowed, owned);

        std::thread::Builder::new()
            .name("borrowed-map-small-stack".to_string())
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut deep = deeply_nested_array(4_096);
                let mapped = map_json_strings_borrowed_canonical(&deep, str::to_owned, |_, _| None);
                assert!(inspect_json(&mapped).max_depth <= MAX_RETAINED_JSON_DEPTH);
                assert!(canonical_json_bytes(&mapped)
                    .expect("bounded mapped JSON")
                    .windows(DEPTH_LIMIT_SENTINEL.len())
                    .any(|window| window == DEPTH_LIMIT_SENTINEL.as_bytes()));
                discard_json_iteratively(mapped);
                discard_json_iteratively(std::mem::replace(&mut deep, Value::Null));
            })
            .expect("spawn borrowed mapping worker")
            .join()
            .expect("borrowed mapping remains stack safe");
    }

    #[test]
    fn owned_canonicalization_reuses_the_tree_and_sorts_objects_stack_safely() {
        let value = serde_json::json!({"z": [{"b": 2, "a": 1}], "a": true});
        let expected = canonical_json_bytes(&value).expect("borrowed canonical bytes");
        assert_eq!(
            canonical_json_blake3_hex(&value).expect("streaming canonical digest"),
            blake3::hash(&expected).to_hex().to_string()
        );
        let owned = canonicalize_json_owned(value);
        assert_eq!(
            canonical_json_bytes(&owned).expect("owned canonical bytes"),
            expected
        );
    }

    #[test]
    fn owned_canonicalization_and_disposal_handle_adversarial_depth_on_a_small_stack() {
        std::thread::Builder::new()
            .name("owned-json-small-stack".to_string())
            .stack_size(512 * 1024)
            .spawn(|| {
                let canonical = canonicalize_json_owned(deeply_nested_array(10_000));
                let bytes = canonical_json_bytes(&canonical).expect("canonical bytes");
                assert_eq!(bytes.first(), Some(&b'['));
                discard_json_iteratively(canonical);
            })
            .expect("spawn owned JSON regression")
            .join()
            .expect("owned JSON regression completes");
    }

    #[test]
    fn iterative_clone_preserves_the_exact_json_value() {
        let value = serde_json::json!({
            "outer": [{"z": 3, "a": [true, null, "text"]}],
            "number": 42
        });
        let cloned = clone_json_iteratively(&value);
        assert_eq!(cloned, value);
        assert_eq!(
            serde_json::to_vec(&cloned).expect("cloned bytes"),
            serde_json::to_vec(&value).expect("source bytes")
        );
    }

    #[test]
    fn streaming_hash_matches_the_existing_compact_value_wire() {
        let value = serde_json::json!({
            "z": [1, true, null],
            "a": {"unicode": "नमस्ते", "escaped": "line\nnext"}
        });
        assert_eq!(
            json_blake3_hex(&value).unwrap(),
            blake3::hash(&serde_json::to_vec(&value).unwrap())
                .to_hex()
                .to_string()
        );
    }

    #[test]
    fn iterative_equality_handles_deep_values_on_a_small_stack() {
        std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let mut left = Value::String("same".to_string());
                let mut right = Value::String("same".to_string());
                for _ in 0..10_000 {
                    left = Value::Array(vec![left]);
                    right = Value::Array(vec![right]);
                }
                assert!(json_values_equal_iteratively(&left, &right));
                for value in [left, right] {
                    discard_json_iteratively(value);
                }
            })
            .expect("small-stack equality worker")
            .join()
            .expect("iterative equality must remain stack safe");
    }

    #[test]
    fn bounded_pretty_serialization_never_returns_an_oversized_buffer() {
        let small = serde_json::json!({"value": [1, 2, 3]});
        let expected = serde_json::to_vec_pretty(&small).expect("expected pretty bytes");
        assert_eq!(
            pretty_json_bytes_bounded(&small, expected.len())
                .expect("serialization")
                .expect("fits exactly"),
            expected
        );

        let large = serde_json::json!({"value": "x".repeat(2 * 1024 * 1024)});
        assert_eq!(
            pretty_json_bytes_bounded(&large, 8 * 1024).expect("bounded serialization"),
            None
        );
    }

    #[test]
    fn pretty_value_writer_uses_a_heap_stack_and_matches_serde_wire() {
        std::thread::Builder::new()
            .name("pretty-json-small-stack".to_string())
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut deep = deeply_nested_array(512);
                let mut measured = CountingWriter::default();
                write_pretty_json(&deep, &mut measured).expect("iterative pretty write");
                assert!(measured.bytes > 512);
                discard_json_iteratively(std::mem::replace(&mut deep, Value::Null));
            })
            .expect("spawn pretty JSON small-stack worker")
            .join()
            .expect("pretty JSON writer remains stack safe");

        let fixture = serde_json::json!({
            "escaped": "α\n🧭",
            "nested": [true, {"empty": [], "number": 42}],
        });
        let mut rendered = Vec::new();
        write_pretty_json(&fixture, &mut rendered).expect("iterative pretty fixture");
        assert_eq!(
            rendered,
            serde_json::to_vec_pretty(&fixture).expect("serde pretty fixture")
        );
        let mut compact = Vec::new();
        write_json(&fixture, &mut compact).expect("iterative compact fixture");
        assert_eq!(
            compact,
            serde_json::to_vec(&fixture).expect("serde compact fixture")
        );
    }

    #[test]
    fn compact_prefix_is_utf8_safe_and_rejects_unadmitted_trees() {
        let value = serde_json::json!({"text": "🙂".repeat(1_000)});
        let (prefix, truncated) = compact_json_prefix_bounded(&value, 101, 10, 8)
            .expect("prefix serialization")
            .expect("value admitted");
        assert!(truncated);
        assert!(prefix.len() <= 101);
        assert!(std::str::from_utf8(prefix.as_bytes()).is_ok());

        let deep = deeply_nested_array(MAX_RETAINED_JSON_DEPTH + 1);
        assert_eq!(
            compact_json_prefix_bounded(&deep, 128, 1_000, MAX_RETAINED_JSON_DEPTH,)
                .expect("deep admission result"),
            None
        );
        discard_json_iteratively(deep);
    }

    #[test]
    fn compact_prefix_serialization_keeps_deep_traversal_off_the_native_stack() {
        std::thread::Builder::new()
            .name("compact-json-prefix-small-stack".to_string())
            .stack_size(256 * 1024)
            .spawn(|| {
                let depth = 10_000;
                let mut deep = deeply_nested_array(depth);
                let (prefix, truncated) =
                    compact_json_prefix_bounded(&deep, 64, depth + 1, depth + 1)
                        .expect("prefix serialization")
                        .expect("deep fixture admitted");
                assert!(truncated);
                assert!(prefix.len() <= 64);
                discard_json_iteratively(std::mem::replace(&mut deep, Value::Null));
            })
            .expect("spawn compact-prefix small-stack worker")
            .join()
            .expect("compact prefix remains stack safe");
    }
}
