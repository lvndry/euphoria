// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the weights of a PyTorch checkpoint.
//!
//! `torch.save` writes a ZIP archive of stored entries: one pickle naming the
//! tensors of the state dict and one raw little endian entry per storage. The
//! pickle is never executed. A restricted interpreter understands only the
//! few opcodes and the three globals such a state dict is made of, and
//! rejects everything else, so a hostile file can at worst produce an error.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::weights::WeightTensor;

const MAX_PICKLE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STACK_DEPTH: usize = 1_000_000;
const MAX_MEMO_ENTRIES: usize = 1_000_000;
const MAX_TENSORS: usize = 100_000;
const MAX_ARCHIVE_ENTRIES: usize = 1_000_000;
const MAX_TUPLE_NESTING: usize = 16;
const MAX_TENSOR_RANK: usize = 16;

const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4b50;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const END_RECORD_SIGNATURE: u32 = 0x0605_4b50;
const END_RECORD_LENGTH: usize = 22;
const LOCAL_HEADER_LENGTH: usize = 30;
const CENTRAL_HEADER_LENGTH: usize = 46;

#[derive(Debug, Clone, Copy)]
struct ArchiveEntry {
    start: usize,
    length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Global {
    OrderedDict,
    RebuildTensor,
    FloatStorage,
}

#[derive(Debug)]
struct StorageReference {
    key: String,
    element_count: usize,
}

#[derive(Debug)]
struct TensorReference {
    storage: Rc<StorageReference>,
    offset: i64,
    size: Vec<i64>,
    stride: Vec<i64>,
}

#[derive(Debug)]
struct TupleValue {
    items: Vec<Value>,
    nesting: usize,
}

#[derive(Debug, Clone)]
enum Value {
    Global(Global),
    Text(Rc<String>),
    Integer(i64),
    Boolean,
    None,
    Tuple(Rc<TupleValue>),
    Dictionary(Rc<RefCell<Vec<(Value, Value)>>>),
    Storage(Rc<StorageReference>),
    Tensor(Rc<TensorReference>),
}

impl Value {
    fn describe(&self) -> &'static str {
        match self {
            Value::Global(_) => "a global",
            Value::Text(_) => "a string",
            Value::Integer(_) => "an integer",
            Value::Boolean => "a boolean",
            Value::None => "None",
            Value::Tuple(_) => "a tuple",
            Value::Dictionary(_) => "a dictionary",
            Value::Storage(_) => "a storage",
            Value::Tensor(_) => "a tensor",
        }
    }
}

struct ByteReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("the pickle ends in the middle of an opcode")?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn unsigned_16(&mut self) -> Result<u16, String> {
        let raw: [u8; 2] = self.take(2)?.try_into().map_err(|_| "short read")?;
        Ok(u16::from_le_bytes(raw))
    }

    fn unsigned_32(&mut self) -> Result<u32, String> {
        let raw: [u8; 4] = self.take(4)?.try_into().map_err(|_| "short read")?;
        Ok(u32::from_le_bytes(raw))
    }

    fn signed_32(&mut self) -> Result<i32, String> {
        let raw: [u8; 4] = self.take(4)?.try_into().map_err(|_| "short read")?;
        Ok(i32::from_le_bytes(raw))
    }

    fn line(&mut self) -> Result<&'a [u8], String> {
        let rest = &self.bytes[self.position..];
        let length = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or("a GLOBAL opcode has no line ending")?;
        self.position += length + 1;
        Ok(&rest[..length])
    }
}

fn read_u16_at(bytes: &[u8], position: usize) -> Result<u16, String> {
    position
        .checked_add(2)
        .and_then(|end| bytes.get(position..end))
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| "the archive ends in the middle of a header".to_string())
}

fn read_u32_at(bytes: &[u8], position: usize) -> Result<u32, String> {
    position
        .checked_add(4)
        .and_then(|end| bytes.get(position..end))
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| "the archive ends in the middle of a header".to_string())
}

fn read_archive_entries(bytes: &[u8]) -> Result<Vec<(String, ArchiveEntry)>, String> {
    if bytes.len() < END_RECORD_LENGTH {
        return Err("the file is too short to be a ZIP archive".to_string());
    }
    let search_start = bytes
        .len()
        .saturating_sub(END_RECORD_LENGTH + usize::from(u16::MAX));
    let end_record = (search_start..=bytes.len() - END_RECORD_LENGTH)
        .rev()
        .find(|position| read_u32_at(bytes, *position) == Ok(END_RECORD_SIGNATURE))
        .ok_or("the file has no ZIP end of central directory record")?;

    let entry_count = read_u16_at(bytes, end_record + 10)?;
    let directory_size = read_u32_at(bytes, end_record + 12)?;
    let directory_offset = read_u32_at(bytes, end_record + 16)?;
    if entry_count == u16::MAX || directory_size == u32::MAX || directory_offset == u32::MAX {
        return Err("ZIP64 archives are not supported".to_string());
    }
    let entry_count = usize::from(entry_count);
    if entry_count > MAX_ARCHIVE_ENTRIES {
        return Err("the archive holds too many entries".to_string());
    }

    let mut position =
        usize::try_from(directory_offset).map_err(|_| "the central directory is out of range")?;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        if read_u32_at(bytes, position)? != CENTRAL_HEADER_SIGNATURE {
            return Err("a central directory entry has a bad signature".to_string());
        }
        let flags = read_u16_at(bytes, position + 8)?;
        let method = read_u16_at(bytes, position + 10)?;
        let compressed_size = read_u32_at(bytes, position + 20)?;
        let uncompressed_size = read_u32_at(bytes, position + 24)?;
        let name_length = usize::from(read_u16_at(bytes, position + 28)?);
        let extra_length = usize::from(read_u16_at(bytes, position + 30)?);
        let comment_length = usize::from(read_u16_at(bytes, position + 32)?);
        let local_offset = read_u32_at(bytes, position + 42)?;
        let name_start = position + CENTRAL_HEADER_LENGTH;
        let name_bytes = name_start
            .checked_add(name_length)
            .and_then(|end| bytes.get(name_start..end))
            .ok_or("the archive ends in the middle of an entry name")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        position = name_start + name_length + extra_length + comment_length;

        if name.ends_with('/') {
            continue;
        }
        if compressed_size == u32::MAX || uncompressed_size == u32::MAX || local_offset == u32::MAX
        {
            return Err(format!("entry {name} needs ZIP64, which is not supported"));
        }
        if flags & 1 != 0 {
            return Err(format!("entry {name} is encrypted"));
        }
        if method != 0 {
            return Err(format!(
                "entry {name} is compressed (method {method}); PyTorch stores its entries uncompressed"
            ));
        }
        if compressed_size != uncompressed_size {
            return Err(format!("entry {name} has inconsistent sizes"));
        }
        let local_start =
            usize::try_from(local_offset).map_err(|_| format!("entry {name} is out of range"))?;
        if read_u32_at(bytes, local_start)? != LOCAL_HEADER_SIGNATURE {
            return Err(format!("entry {name} has a bad local header"));
        }
        let local_name_length = usize::from(read_u16_at(bytes, local_start + 26)?);
        let local_extra_length = usize::from(read_u16_at(bytes, local_start + 28)?);
        let data_start = local_start + LOCAL_HEADER_LENGTH + local_name_length + local_extra_length;
        let length = usize::try_from(uncompressed_size)
            .map_err(|_| format!("entry {name} is out of range"))?;
        data_start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| format!("entry {name} runs past the end of the file"))?;
        entries.push((
            name,
            ArchiveEntry {
                start: data_start,
                length,
            },
        ));
    }
    Ok(entries)
}

fn allowed_global(module: &[u8], name: &[u8]) -> Result<Global, String> {
    match (module, name) {
        (b"collections", b"OrderedDict") => Ok(Global::OrderedDict),
        (b"torch._utils", b"_rebuild_tensor_v2") => Ok(Global::RebuildTensor),
        (b"torch", b"FloatStorage") => Ok(Global::FloatStorage),
        (b"torch", other) if other.ends_with(b"Storage") => Err(format!(
            "unsupported storage type torch.{}; only float32 (torch.FloatStorage) is supported",
            String::from_utf8_lossy(other)
        )),
        _ => Err(format!(
            "the pickle uses the disallowed global {} {}",
            String::from_utf8_lossy(module),
            String::from_utf8_lossy(name)
        )),
    }
}

fn make_tuple(items: Vec<Value>) -> Result<Value, String> {
    let nesting = items
        .iter()
        .map(|item| match item {
            Value::Tuple(tuple) => tuple.nesting,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
        + 1;
    if nesting > MAX_TUPLE_NESTING {
        return Err("the pickle nests tuples too deeply".to_string());
    }
    Ok(Value::Tuple(Rc::new(TupleValue { items, nesting })))
}

fn integer_argument(value: &Value, what: &str) -> Result<i64, String> {
    match value {
        Value::Integer(integer) => Ok(*integer),
        other => Err(format!("{what} is {}, not an integer", other.describe())),
    }
}

fn integer_tuple(value: &Value, what: &str) -> Result<Vec<i64>, String> {
    match value {
        Value::Tuple(tuple) if tuple.items.len() <= MAX_TENSOR_RANK => tuple
            .items
            .iter()
            .map(|item| integer_argument(item, what))
            .collect(),
        Value::Tuple(_) => Err(format!("{what} has too many dimensions")),
        other => Err(format!("{what} is {}, not a tuple", other.describe())),
    }
}

fn load_storage(arguments: &[Value]) -> Result<Value, String> {
    let [
        Value::Text(tag),
        Value::Global(Global::FloatStorage),
        Value::Text(key),
        Value::Text(_),
        count,
    ] = arguments
    else {
        return Err(
            "a persistent id is not ('storage', FloatStorage, key, location, count)".to_string(),
        );
    };
    if tag.as_str() != "storage" {
        return Err(format!("a persistent id has the unknown tag {tag}"));
    }
    let element_count = usize::try_from(integer_argument(count, "a storage size")?)
        .map_err(|_| "a storage has a negative size")?;
    Ok(Value::Storage(Rc::new(StorageReference {
        key: key.as_str().to_owned(),
        element_count,
    })))
}

fn rebuild_tensor(arguments: &[Value]) -> Result<Value, String> {
    let [
        Value::Storage(storage),
        offset,
        size,
        stride,
        Value::Boolean,
        Value::Dictionary(_),
    ] = arguments
    else {
        return Err("a tensor is not _rebuild_tensor_v2(storage, offset, size, stride, requires_grad, hooks)".to_string());
    };
    Ok(Value::Tensor(Rc::new(TensorReference {
        storage: Rc::clone(storage),
        offset: integer_argument(offset, "a storage offset")?,
        size: integer_tuple(size, "a tensor size")?,
        stride: integer_tuple(stride, "a tensor stride")?,
    })))
}

fn pop_to_mark(stack: &mut Vec<Value>, marks: &mut Vec<usize>) -> Result<Vec<Value>, String> {
    let mark = marks
        .pop()
        .ok_or("the pickle closes a MARK it never opened")?;
    if mark > stack.len() {
        return Err("the pickle stack is inconsistent with its marks".to_string());
    }
    Ok(stack.split_off(mark))
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, String> {
    stack
        .pop()
        .ok_or_else(|| "the pickle pops an empty stack".to_string())
}

fn push(stack: &mut Vec<Value>, value: Value) -> Result<(), String> {
    if stack.len() >= MAX_STACK_DEPTH {
        return Err("the pickle stack grows too deep".to_string());
    }
    stack.push(value);
    Ok(())
}

fn memoize(memo: &mut HashMap<u32, Value>, index: u32, value: Value) -> Result<(), String> {
    if memo.len() >= MAX_MEMO_ENTRIES && !memo.contains_key(&index) {
        return Err("the pickle memo grows too large".to_string());
    }
    memo.insert(index, value);
    Ok(())
}

fn text_value(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "a pickle string is not UTF-8")?;
    Ok(Value::Text(Rc::new(text.to_owned())))
}

fn set_items(dictionary: &Value, items: Vec<Value>) -> Result<(), String> {
    let Value::Dictionary(entries) = dictionary else {
        return Err(format!(
            "SETITEMS targets {}, not a dictionary",
            dictionary.describe()
        ));
    };
    let mut entries = entries.borrow_mut();
    let mut items = items.into_iter();
    while let (Some(key), Some(value)) = (items.next(), items.next()) {
        if entries.len() >= MAX_TENSORS {
            return Err("the state dict holds too many entries".to_string());
        }
        entries.push((key, value));
    }
    Ok(())
}

fn run_pickle(pickle: &[u8]) -> Result<Vec<(String, Rc<TensorReference>)>, String> {
    let mut reader = ByteReader {
        bytes: pickle,
        position: 0,
    };
    let mut stack: Vec<Value> = Vec::new();
    let mut marks: Vec<usize> = Vec::new();
    let mut memo: HashMap<u32, Value> = HashMap::new();

    loop {
        let opcode = reader.byte()?;
        match opcode {
            0x80 => {
                let version = reader.byte()?;
                if version != 2 && version != 3 {
                    return Err(format!("the pickle protocol {version} is not supported"));
                }
            }
            b'c' => {
                let module = reader.line()?;
                let name = reader.line()?;
                push(&mut stack, Value::Global(allowed_global(module, name)?))?;
            }
            b'q' => {
                let index = u32::from(reader.byte()?);
                let top = stack.last().ok_or("BINPUT on an empty stack")?.clone();
                memoize(&mut memo, index, top)?;
            }
            b'r' => {
                let index = reader.unsigned_32()?;
                let top = stack.last().ok_or("LONG_BINPUT on an empty stack")?.clone();
                memoize(&mut memo, index, top)?;
            }
            b'h' | b'j' => {
                let index = if opcode == b'h' {
                    u32::from(reader.byte()?)
                } else {
                    reader.unsigned_32()?
                };
                let value = memo
                    .get(&index)
                    .ok_or_else(|| format!("the pickle reads the unset memo entry {index}"))?
                    .clone();
                push(&mut stack, value)?;
            }
            b')' => push(&mut stack, make_tuple(Vec::new())?)?,
            b'(' => {
                if marks.len() >= MAX_STACK_DEPTH {
                    return Err("the pickle opens too many marks".to_string());
                }
                marks.push(stack.len());
            }
            b't' => {
                let items = pop_to_mark(&mut stack, &mut marks)?;
                push(&mut stack, make_tuple(items)?)?;
            }
            0x85..=0x87 => {
                let length = usize::from(opcode - 0x84);
                if stack.len() < length {
                    return Err("a tuple opcode pops an empty stack".to_string());
                }
                let items = stack.split_off(stack.len() - length);
                push(&mut stack, make_tuple(items)?)?;
            }
            b'X' => {
                let length = usize::try_from(reader.unsigned_32()?)
                    .map_err(|_| "a pickle string is too long")?;
                push(&mut stack, text_value(reader.take(length)?)?)?;
            }
            0x8c => {
                let length = usize::from(reader.byte()?);
                push(&mut stack, text_value(reader.take(length)?)?)?;
            }
            b'J' => {
                let integer = i64::from(reader.signed_32()?);
                push(&mut stack, Value::Integer(integer))?;
            }
            b'K' => {
                let integer = i64::from(reader.byte()?);
                push(&mut stack, Value::Integer(integer))?;
            }
            b'M' => {
                let integer = i64::from(reader.unsigned_16()?);
                push(&mut stack, Value::Integer(integer))?;
            }
            0x8a => {
                let length = usize::from(reader.byte()?);
                if length > 8 {
                    return Err("a pickle integer is wider than 64 bits".to_string());
                }
                let raw = reader.take(length)?;
                let negative = raw.last().is_some_and(|byte| byte & 0x80 != 0);
                let mut buffer = if negative { [0xff; 8] } else { [0; 8] };
                buffer[..length].copy_from_slice(raw);
                push(&mut stack, Value::Integer(i64::from_le_bytes(buffer)))?;
            }
            0x88 => push(&mut stack, Value::Boolean)?,
            0x89 => push(&mut stack, Value::Boolean)?,
            b'N' => push(&mut stack, Value::None)?,
            b'}' => push(
                &mut stack,
                Value::Dictionary(Rc::new(RefCell::new(Vec::new()))),
            )?,
            b'R' => {
                let arguments = pop(&mut stack)?;
                let callable = pop(&mut stack)?;
                let (Value::Global(global), Value::Tuple(arguments)) = (&callable, &arguments)
                else {
                    return Err(format!(
                        "REDUCE calls {} with {}",
                        callable.describe(),
                        arguments.describe()
                    ));
                };
                let result = match global {
                    Global::OrderedDict if arguments.items.is_empty() => {
                        Value::Dictionary(Rc::new(RefCell::new(Vec::new())))
                    }
                    Global::RebuildTensor => rebuild_tensor(&arguments.items)?,
                    _ => return Err("REDUCE calls something that is not allowed".to_string()),
                };
                push(&mut stack, result)?;
            }
            b'Q' => {
                let identifier = pop(&mut stack)?;
                let Value::Tuple(arguments) = &identifier else {
                    return Err("a persistent id is not a tuple".to_string());
                };
                push(&mut stack, load_storage(&arguments.items)?)?;
            }
            b'u' => {
                let items = pop_to_mark(&mut stack, &mut marks)?;
                if items.len() % 2 != 0 {
                    return Err("SETITEMS has a key without a value".to_string());
                }
                let dictionary = stack.last().ok_or("SETITEMS on an empty stack")?;
                set_items(dictionary, items)?;
            }
            b's' => {
                let value = pop(&mut stack)?;
                let key = pop(&mut stack)?;
                let dictionary = stack.last().ok_or("SETITEM on an empty stack")?;
                set_items(dictionary, vec![key, value])?;
            }
            b'b' => {
                let state = pop(&mut stack)?;
                let target = stack.last().ok_or("BUILD on an empty stack")?;
                if !matches!(
                    (target, &state),
                    (Value::Dictionary(_), Value::Dictionary(_))
                ) {
                    return Err(
                        "BUILD is only allowed to set attributes of a dictionary".to_string()
                    );
                }
            }
            b'.' => {
                let result = pop(&mut stack)?;
                return collect_state_dict(&result);
            }
            other => {
                return Err(format!(
                    "the pickle uses the unsupported opcode {:?} (0x{other:02x})",
                    char::from(other)
                ));
            }
        }
    }
}

fn collect_state_dict(result: &Value) -> Result<Vec<(String, Rc<TensorReference>)>, String> {
    let Value::Dictionary(entries) = result else {
        return Err(format!(
            "the pickle holds {}, not a state dict",
            result.describe()
        ));
    };
    let entries = entries.borrow();
    if entries.len() > MAX_TENSORS {
        return Err("the state dict holds too many tensors".to_string());
    }
    let mut tensors = Vec::with_capacity(entries.len());
    for (key, value) in entries.iter() {
        let Value::Text(name) = key else {
            return Err(format!(
                "a state dict key is {}, not a string",
                key.describe()
            ));
        };
        let Value::Tensor(tensor) = value else {
            return Err(format!(
                "state dict entry {name} is {}, not a tensor",
                value.describe()
            ));
        };
        tensors.push((name.as_str().to_owned(), Rc::clone(tensor)));
    }
    Ok(tensors)
}

fn contiguous_element_count(name: &str, tensor: &TensorReference) -> Result<usize, String> {
    if tensor.size.len() != tensor.stride.len() {
        return Err(format!(
            "tensor {name} has a size and stride of different ranks"
        ));
    }
    let mut element_count = 1usize;
    let mut expected_stride = 1usize;
    for (dimension, stride) in tensor.size.iter().zip(&tensor.stride).rev() {
        let dimension = usize::try_from(*dimension)
            .map_err(|_| format!("tensor {name} has a negative dimension"))?;
        if dimension != 1 && usize::try_from(*stride) != Ok(expected_stride) {
            return Err(format!(
                "tensor {name} is not contiguous (stride {stride}, expected {expected_stride})"
            ));
        }
        element_count = element_count
            .checked_mul(dimension)
            .ok_or_else(|| format!("tensor {name} has an impossible shape"))?;
        expected_stride = expected_stride.saturating_mul(dimension);
    }
    Ok(element_count)
}

/// Reads every tensor of a `torch.save` checkpoint held in memory, in the
/// order of its state dict. The pickle is interpreted, never executed, and
/// only contiguous float32 tensors are accepted.
pub(crate) fn read_torch_checkpoint(bytes: &[u8]) -> Result<Vec<WeightTensor>, String> {
    let entries = read_archive_entries(bytes)?;
    let mut pickle_entries = entries
        .iter()
        .filter(|(name, _)| name.ends_with("/data.pkl"));
    let (pickle_name, pickle_entry) = pickle_entries
        .next()
        .ok_or("the archive has no data.pkl entry")?;
    if pickle_entries.next().is_some() {
        return Err("the archive holds more than one data.pkl entry".to_string());
    }
    if pickle_entry.length > MAX_PICKLE_BYTES {
        return Err("the pickle is larger than the allowed 16 MiB".to_string());
    }
    let root = &pickle_name[..pickle_name.len() - "data.pkl".len()];
    let storages: HashMap<&str, ArchiveEntry> = entries
        .iter()
        .filter_map(|(name, entry)| {
            name.strip_prefix(root)
                .and_then(|rest| rest.strip_prefix("data/"))
                .map(|key| (key, *entry))
        })
        .collect();

    let pickle = &bytes[pickle_entry.start..pickle_entry.start + pickle_entry.length];
    let state_dict = run_pickle(pickle)?;

    let mut tensors = Vec::with_capacity(state_dict.len());
    for (name, tensor) in state_dict {
        let element_count = contiguous_element_count(&name, &tensor)?;
        let storage = &tensor.storage;
        let entry = storages
            .get(storage.key.as_str())
            .ok_or_else(|| format!("tensor {name} uses the missing storage {}", storage.key))?;
        let storage_bytes = storage
            .element_count
            .checked_mul(4)
            .filter(|needed| *needed <= entry.length)
            .ok_or_else(|| {
                format!(
                    "the storage {} is shorter than its declared size",
                    storage.key
                )
            })?;
        let offset = usize::try_from(tensor.offset)
            .map_err(|_| format!("tensor {name} has a negative storage offset"))?;
        let end = offset
            .checked_add(element_count)
            .filter(|end| *end <= storage.element_count)
            .ok_or_else(|| format!("tensor {name} runs past the end of its storage"))?;
        let storage_start = entry.start;
        let raw = &bytes[storage_start..storage_start + storage_bytes][offset * 4..end * 4];
        let values: Vec<f32> = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|quad| f32::from_le_bytes(*quad))
            .collect();
        tensors.push(WeightTensor {
            name,
            shape: tensor.size.clone(),
            values,
        });
    }
    Ok(tensors)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEntry {
        name: String,
        data: Vec<u8>,
        method: u16,
        local_extra: usize,
    }

    fn entry(name: &str, data: &[u8]) -> TestEntry {
        TestEntry {
            name: name.to_string(),
            data: data.to_vec(),
            method: 0,
            local_extra: 0,
        }
    }

    fn build_zip(entries: &[TestEntry]) -> Vec<u8> {
        let mut archive = Vec::new();
        let mut directory = Vec::new();
        for entry in entries {
            let local_offset = archive.len() as u32;
            let name = entry.name.as_bytes();
            archive.extend_from_slice(&LOCAL_HEADER_SIGNATURE.to_le_bytes());
            archive.extend_from_slice(&[20, 0, 0, 0]);
            archive.extend_from_slice(&entry.method.to_le_bytes());
            archive.extend_from_slice(&[0; 8]);
            archive.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            archive.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
            archive.extend_from_slice(&(entry.local_extra as u16).to_le_bytes());
            archive.extend_from_slice(name);
            archive.extend(std::iter::repeat_n(0xAB, entry.local_extra));
            archive.extend_from_slice(&entry.data);

            directory.extend_from_slice(&CENTRAL_HEADER_SIGNATURE.to_le_bytes());
            directory.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
            directory.extend_from_slice(&entry.method.to_le_bytes());
            directory.extend_from_slice(&[0; 8]);
            directory.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(entry.data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&[0; 12]);
            directory.extend_from_slice(&local_offset.to_le_bytes());
            directory.extend_from_slice(name);
        }
        let directory_offset = archive.len() as u32;
        archive.extend_from_slice(&directory);
        archive.extend_from_slice(&END_RECORD_SIGNATURE.to_le_bytes());
        archive.extend_from_slice(&[0; 4]);
        archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        archive.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        archive.extend_from_slice(&directory_offset.to_le_bytes());
        archive.extend_from_slice(&[0; 2]);
        archive
    }

    fn push_text(pickle: &mut Vec<u8>, text: &str) {
        pickle.push(b'X');
        pickle.extend_from_slice(&(text.len() as u32).to_le_bytes());
        pickle.extend_from_slice(text.as_bytes());
    }

    fn push_global(pickle: &mut Vec<u8>, module: &str, name: &str) {
        pickle.push(b'c');
        pickle.extend_from_slice(module.as_bytes());
        pickle.push(b'\n');
        pickle.extend_from_slice(name.as_bytes());
        pickle.push(b'\n');
    }

    fn push_tuple_of_integers(pickle: &mut Vec<u8>, values: &[u8]) {
        pickle.push(b'(');
        for value in values {
            pickle.extend_from_slice(&[b'K', *value]);
        }
        pickle.push(b't');
    }

    struct TestTensor<'a> {
        name: &'a str,
        key: &'a str,
        element_count: u8,
        offset: u8,
        size: &'a [u8],
        stride: &'a [u8],
    }

    fn build_pickle(tensors: &[TestTensor<'_>]) -> Vec<u8> {
        let mut pickle = vec![0x80, 2];
        push_global(&mut pickle, "collections", "OrderedDict");
        pickle.extend_from_slice(b")R(");
        for tensor in tensors {
            push_text(&mut pickle, tensor.name);
            push_global(&mut pickle, "torch._utils", "_rebuild_tensor_v2");
            pickle.push(b'(');
            pickle.push(b'(');
            push_text(&mut pickle, "storage");
            push_global(&mut pickle, "torch", "FloatStorage");
            push_text(&mut pickle, tensor.key);
            push_text(&mut pickle, "cpu");
            pickle.extend_from_slice(&[b'K', tensor.element_count]);
            pickle.extend_from_slice(b"tQ");
            pickle.extend_from_slice(&[b'K', tensor.offset]);
            push_tuple_of_integers(&mut pickle, tensor.size);
            push_tuple_of_integers(&mut pickle, tensor.stride);
            pickle.push(0x89);
            push_global(&mut pickle, "collections", "OrderedDict");
            pickle.extend_from_slice(b")Rt");
            pickle.push(b'R');
        }
        pickle.extend_from_slice(b"u.");
        pickle
    }

    fn storage_bytes(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn archive_with(pickle: &[u8], storage: &[u8]) -> Vec<u8> {
        build_zip(&[
            entry("model/data.pkl", pickle),
            entry("model/data/0", storage),
        ])
    }

    fn simple_tensor<'a>(
        size: &'a [u8],
        stride: &'a [u8],
        offset: u8,
        count: u8,
    ) -> TestTensor<'a> {
        TestTensor {
            name: "w",
            key: "0",
            element_count: count,
            offset,
            size,
            stride,
        }
    }

    #[test]
    fn reads_the_tiny_fixture() {
        let tensors = read_torch_checkpoint(include_bytes!("fixtures/tiny_checkpoint.pt"))
            .expect("the fixture should load");
        let names: Vec<&str> = tensors.iter().map(|tensor| tensor.name.as_str()).collect();
        assert_eq!(names, ["a", "b", "nested.c", "scalar"]);
        assert_eq!(tensors[0].shape, [3, 4]);
        let expected: Vec<f32> = (0..12).map(|index| index as f32 * 0.5).collect();
        assert_eq!(tensors[0].values, expected);
        assert_eq!(tensors[1].shape, [5]);
        assert_eq!(tensors[1].values, [1.5, -2.0, 3.25, 0.0, 7.0]);
        assert_eq!(tensors[2].shape, [2, 3, 2]);
        for (index, value) in tensors[2].values.iter().enumerate() {
            let expected = -1.0 + 2.0 * index as f32 / 11.0;
            assert!((value - expected).abs() < 1e-6, "{index}: {value}");
        }
        assert_eq!(tensors[2].values.len(), 12);
        assert!(tensors[3].shape.is_empty());
        assert_eq!(tensors[3].values, [0.75]);
    }

    #[test]
    fn shares_one_storage_between_tensors() {
        let values: Vec<f32> = (0..6).map(|index| index as f32).collect();
        let mut first = simple_tensor(&[2], &[1], 0, 6);
        first.name = "first";
        let mut second = simple_tensor(&[2, 2], &[2, 1], 2, 6);
        second.name = "second";
        let pickle = build_pickle(&[first, second]);
        let tensors = read_torch_checkpoint(&archive_with(&pickle, &storage_bytes(&values)))
            .expect("shared storage should load");
        assert_eq!(tensors[0].values, [0.0, 1.0]);
        assert_eq!(tensors[1].shape, [2, 2]);
        assert_eq!(tensors[1].values, [2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn ignores_the_stride_of_unit_dimensions() {
        let pickle = build_pickle(&[simple_tensor(&[1, 3], &[99, 1], 0, 3)]);
        let tensors =
            read_torch_checkpoint(&archive_with(&pickle, &storage_bytes(&[1.0, 2.0, 3.0])))
                .expect("unit dimension strides do not matter");
        assert_eq!(tensors[0].values, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn rejects_non_contiguous_strides() {
        let pickle = build_pickle(&[simple_tensor(&[2, 2], &[1, 2], 0, 4)]);
        let error = read_torch_checkpoint(&archive_with(&pickle, &storage_bytes(&[0.0; 4])))
            .expect_err("transposed strides are not contiguous");
        assert!(error.contains("not contiguous"), "{error}");
    }

    #[test]
    fn rejects_a_disallowed_global_without_running_it() {
        let marker = std::env::temp_dir().join("euphoria_torch_checkpoint_must_not_exist");
        let _ = std::fs::remove_file(&marker);
        let mut pickle = vec![0x80, 2];
        push_global(&mut pickle, "os", "system");
        push_text(&mut pickle, &format!("touch {}", marker.display()));
        pickle.extend_from_slice(b"\x85R.");
        let error = read_torch_checkpoint(&archive_with(&pickle, &[]))
            .expect_err("os.system is not allowed");
        assert!(error.contains("os system"), "{error}");
        assert!(!marker.exists());
    }

    #[test]
    fn rejects_other_storage_types() {
        let mut pickle = vec![0x80, 2];
        push_global(&mut pickle, "torch", "HalfStorage");
        pickle.push(b'.');
        let error = read_torch_checkpoint(&archive_with(&pickle, &[]))
            .expect_err("half storage is not supported");
        assert!(error.contains("HalfStorage"), "{error}");
    }

    #[test]
    fn rejects_an_unsupported_opcode() {
        let pickle = [0x80, 2, b'I', b'1', b'\n', b'.'];
        let error =
            read_torch_checkpoint(&archive_with(&pickle, &[])).expect_err("INT is not supported");
        assert!(error.contains("unsupported opcode"), "{error}");
    }

    #[test]
    fn rejects_a_truncated_archive() {
        let pickle = build_pickle(&[simple_tensor(&[2], &[1], 0, 2)]);
        let archive = archive_with(&pickle, &storage_bytes(&[1.0, 2.0]));
        for length in [0, 10, archive.len() / 2, archive.len() - 1] {
            assert!(
                read_torch_checkpoint(&archive[..length]).is_err(),
                "{length}"
            );
        }
    }

    #[test]
    fn rejects_a_compressed_entry() {
        let pickle = build_pickle(&[simple_tensor(&[2], &[1], 0, 2)]);
        let mut pickle_entry = entry("model/data.pkl", &pickle);
        pickle_entry.method = 8;
        let archive = build_zip(&[
            pickle_entry,
            entry("model/data/0", &storage_bytes(&[1.0, 2.0])),
        ]);
        let error = read_torch_checkpoint(&archive).expect_err("deflate is not supported");
        assert!(error.contains("compressed"), "{error}");
    }

    #[test]
    fn rejects_a_storage_shorter_than_declared() {
        let pickle = build_pickle(&[simple_tensor(&[4], &[1], 0, 4)]);
        let error = read_torch_checkpoint(&archive_with(&pickle, &storage_bytes(&[1.0, 2.0])))
            .expect_err("the storage entry is too short");
        assert!(error.contains("shorter"), "{error}");
    }

    #[test]
    fn rejects_a_missing_pickle() {
        let archive = build_zip(&[entry("model/data/0", &storage_bytes(&[1.0]))]);
        let error = read_torch_checkpoint(&archive).expect_err("data.pkl is missing");
        assert!(error.contains("data.pkl"), "{error}");
    }

    #[test]
    fn rejects_a_missing_storage() {
        let pickle = build_pickle(&[simple_tensor(&[1], &[1], 0, 1)]);
        let archive = build_zip(&[entry("model/data.pkl", &pickle)]);
        let error = read_torch_checkpoint(&archive).expect_err("storage 0 is missing");
        assert!(error.contains("missing storage"), "{error}");
    }

    #[test]
    fn rejects_an_offset_past_the_storage() {
        let pickle = build_pickle(&[simple_tensor(&[3], &[1], 2, 4)]);
        let error = read_torch_checkpoint(&archive_with(&pickle, &storage_bytes(&[0.0; 4])))
            .expect_err("offset 2 plus 3 elements exceeds 4");
        assert!(error.contains("past the end"), "{error}");
    }

    #[test]
    fn reads_entries_padded_by_the_local_extra_field() {
        let values = [1.0f32, 2.0, 3.0];
        let pickle = build_pickle(&[simple_tensor(&[3], &[1], 0, 3)]);
        let mut pickle_entry = entry("model/data.pkl", &pickle);
        pickle_entry.local_extra = 13;
        let mut storage_entry = entry("model/data/0", &storage_bytes(&values));
        storage_entry.local_extra = 29;
        let archive = build_zip(&[pickle_entry, storage_entry]);
        let tensors = read_torch_checkpoint(&archive).expect("padding must be skipped");
        assert_eq!(tensors[0].values, values);
    }

    #[test]
    fn real_checkpoint_loads_when_provided() {
        let Ok(path) = std::env::var("STEMS_REAL_CHECKPOINT") else {
            eprintln!("STEMS_REAL_CHECKPOINT is not set, skipping the real checkpoint test");
            return;
        };
        let started = std::time::Instant::now();
        let bytes = std::fs::read(&path).expect("the checkpoint should be readable");
        let tensors = read_torch_checkpoint(&bytes).expect("the checkpoint should load");
        assert_eq!(tensors.len(), 510);
        assert!(!tensors[0].name.is_empty());
        assert!(
            tensors
                .iter()
                .all(|tensor| tensor.values.iter().all(|value| value.is_finite()))
        );
        let total: usize = tensors.iter().map(|tensor| tensor.values.len()).sum();
        eprintln!(
            "{} tensors, {total} elements, first {}, last {}, {:?}",
            tensors.len(),
            tensors[0].name,
            tensors[tensors.len() - 1].name,
            started.elapsed()
        );
    }
}
