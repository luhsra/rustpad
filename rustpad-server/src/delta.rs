//! A serde-compatible Rust implementation of `quill-delta` 5.1.0.
//!
//! Quill indexes strings in JavaScript UTF-16 code units. Rust strings cannot
//! represent an isolated UTF-16 surrogate, so operations which would split a
//! surrogate pair return [`DeltaError::InvalidUtf16Boundary`].

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

pub type Attributes = Map<String, Value>;
pub type Embed = Map<String, Value>;

/// Embed formats registered by the standard full Quill build used by Rustpad.
pub const DEFAULT_QUILL_EMBEDS: &[&str] = &["formula", "image", "video"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InsertValue {
    String(String),
    Embed(Embed),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RetainValue {
    Length(usize),
    Embed(Embed),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Op {
    Insert {
        insert: InsertValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        attributes: Option<Attributes>,
    },
    Delete {
        delete: usize,
    },
    Retain {
        retain: RetainValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        attributes: Option<Attributes>,
    },
}

impl<'de> Deserialize<'de> for Op {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_op(value).map_err(serde::de::Error::custom)
    }
}

fn parse_op(value: Value) -> Result<Op, DeltaError> {
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            return Err(DeltaError::MalformedOperation(
                "operation must be an object",
            ));
        }
    };
    let had_attributes = object.contains_key("attributes");
    let attributes = match object.remove("attributes") {
        Some(Value::Object(attributes)) if !attributes.is_empty() => Some(attributes),
        Some(Value::Object(_)) => None,
        Some(_) => {
            return Err(DeltaError::MalformedOperation(
                "attributes must be a JSON object",
            ));
        }
        None => None,
    };

    let action_count = ["insert", "delete", "retain"]
        .iter()
        .filter(|key| object.contains_key(**key))
        .count();
    if action_count != 1 {
        return Err(DeltaError::MalformedOperation(
            "operation must contain exactly one of insert, delete, or retain",
        ));
    }

    if let Some(insert) = object.remove("insert") {
        if !object.is_empty() {
            return Err(DeltaError::MalformedOperation("unknown operation field"));
        }
        let insert = match insert {
            Value::String(text) if !text.is_empty() => InsertValue::String(text),
            Value::String(_) => {
                return Err(DeltaError::MalformedOperation(
                    "insert strings must not be empty",
                ));
            }
            Value::Object(embed) => InsertValue::Embed(embed),
            _ => {
                return Err(DeltaError::MalformedOperation(
                    "insert must be a string or object",
                ));
            }
        };
        return Ok(Op::Insert { insert, attributes });
    }

    if let Some(delete) = object.remove("delete") {
        if had_attributes || !object.is_empty() {
            return Err(DeltaError::MalformedOperation(
                "delete cannot have attributes or unknown fields",
            ));
        }
        let delete = positive_usize(&delete, "delete")?;
        return Ok(Op::Delete { delete });
    }

    let retain = object
        .remove("retain")
        .ok_or(DeltaError::MalformedOperation("missing operation action"))?;
    if !object.is_empty() {
        return Err(DeltaError::MalformedOperation("unknown operation field"));
    }
    let retain = match retain {
        Value::Object(embed) => RetainValue::Embed(embed),
        value => RetainValue::Length(positive_usize(&value, "retain")?),
    };
    Ok(Op::Retain { retain, attributes })
}

fn positive_usize(value: &Value, name: &'static str) -> Result<usize, DeltaError> {
    let number = value
        .as_u64()
        .ok_or(DeltaError::MalformedOperation(match name {
            "delete" => "delete must be a positive integer",
            _ => "retain must be a positive integer or object",
        }))?;
    if number == 0 || number > usize::MAX as u64 {
        return Err(DeltaError::MalformedOperation(match name {
            "delete" => "delete must be a positive integer",
            _ => "retain must be a positive integer or object",
        }));
    }
    Ok(number as usize)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaError {
    MalformedOperation(&'static str),
    InvalidUtf16Boundary {
        index: usize,
    },
    LengthOverflow,
    LengthConflict {
        required: usize,
        available: usize,
    },
    ExpectedDocument,
    UnsupportedEmbedRetain {
        embed_type: Option<String>,
    },
    EmbedTypeMismatch {
        left: Option<String>,
        right: Option<String>,
    },
    CarriageReturn,
    UnsupportedEmbed {
        embed_type: Option<String>,
    },
}

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedOperation(message) => write!(f, "malformed delta operation: {message}"),
            Self::InvalidUtf16Boundary { index } => write!(
                f,
                "UTF-16 index {index} splits a surrogate pair, which Rust strings cannot represent"
            ),
            Self::LengthOverflow => write!(f, "delta length overflow"),
            Self::LengthConflict {
                required,
                available,
            } => write!(
                f,
                "delta consumes {required} UTF-16 code units but only {available} are available"
            ),
            Self::ExpectedDocument => {
                write!(f, "expected a document delta containing only inserts")
            }
            Self::UnsupportedEmbedRetain { embed_type } => match embed_type {
                Some(kind) => write!(
                    f,
                    "embed retain for type {kind:?} requires a registered handler, which is unsupported server-side"
                ),
                None => write!(
                    f,
                    "embed retain requires a registered handler, which is unsupported server-side"
                ),
            },
            Self::EmbedTypeMismatch { left, right } => {
                write!(f, "embed types do not match: {left:?} != {right:?}")
            }
            Self::CarriageReturn => {
                write!(f, "Quill insert strings cannot contain carriage returns")
            }
            Self::UnsupportedEmbed { embed_type } => {
                write!(f, "unsupported Quill embed type: {embed_type:?}")
            }
        }
    }
}

impl Error for DeltaError {}

impl InsertValue {
    pub fn object(value: Value) -> Result<Self, DeltaError> {
        match value {
            Value::Object(object) => Ok(Self::Embed(object)),
            _ => Err(DeltaError::MalformedOperation(
                "embed must be a JSON object",
            )),
        }
    }
}

impl From<String> for InsertValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for InsertValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<Embed> for InsertValue {
    fn from(value: Embed) -> Self {
        Self::Embed(value)
    }
}

impl From<usize> for RetainValue {
    fn from(value: usize) -> Self {
        Self::Length(value)
    }
}

impl From<Embed> for RetainValue {
    fn from(value: Embed) -> Self {
        Self::Embed(value)
    }
}

impl Op {
    pub fn insert(value: impl Into<InsertValue>, attributes: Option<Attributes>) -> Self {
        Self::Insert {
            insert: value.into(),
            attributes: nonempty_attributes(attributes),
        }
    }

    pub fn delete(length: usize) -> Self {
        Self::Delete { delete: length }
    }

    pub fn retain(value: impl Into<RetainValue>, attributes: Option<Attributes>) -> Self {
        Self::Retain {
            retain: value.into(),
            attributes: nonempty_attributes(attributes),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Insert {
                insert: InsertValue::String(text),
                ..
            } => utf16_len(text),
            Self::Insert { .. }
            | Self::Retain {
                retain: RetainValue::Embed(_),
                ..
            } => 1,
            Self::Delete { delete } => *delete,
            Self::Retain {
                retain: RetainValue::Length(length),
                ..
            } => *length,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn attributes(&self) -> Option<&Attributes> {
        match self {
            Self::Insert { attributes, .. } | Self::Retain { attributes, .. } => {
                attributes.as_ref()
            }
            Self::Delete { .. } => None,
        }
    }

    fn kind(&self) -> OpKind {
        match self {
            Self::Insert { .. } => OpKind::Insert,
            Self::Delete { .. } => OpKind::Delete,
            Self::Retain { .. } => OpKind::Retain,
        }
    }

    fn validate(&self) -> Result<(), DeltaError> {
        match self {
            Self::Insert {
                insert: InsertValue::String(text),
                ..
            } if text.is_empty() => Err(DeltaError::MalformedOperation(
                "insert strings must not be empty",
            )),
            Self::Delete { delete: 0 } => Err(DeltaError::MalformedOperation(
                "delete must be a positive integer",
            )),
            Self::Retain {
                retain: RetainValue::Length(0),
                ..
            } => Err(DeltaError::MalformedOperation(
                "retain must be a positive integer or object",
            )),
            _ => Ok(()),
        }
    }
}

impl Delta {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_ops(ops: Vec<Op>) -> Result<Self, DeltaError> {
        let mut delta = Self::new();
        for op in ops {
            delta.push(op)?;
        }
        Ok(delta)
    }

    pub fn insert(
        &mut self,
        value: impl Into<InsertValue>,
        attributes: Option<Attributes>,
    ) -> Result<&mut Self, DeltaError> {
        let value = value.into();
        if matches!(&value, InsertValue::String(text) if text.is_empty()) {
            return Ok(self);
        }
        self.push(Op::insert(value, attributes))
    }

    pub fn delete(&mut self, length: usize) -> Result<&mut Self, DeltaError> {
        if length == 0 {
            return Ok(self);
        }
        self.push(Op::delete(length))
    }

    pub fn retain(
        &mut self,
        value: impl Into<RetainValue>,
        attributes: Option<Attributes>,
    ) -> Result<&mut Self, DeltaError> {
        let value = value.into();
        if matches!(value, RetainValue::Length(0)) {
            return Ok(self);
        }
        self.push(Op::retain(value, attributes))
    }

    pub fn push(&mut self, mut new_op: Op) -> Result<&mut Self, DeltaError> {
        new_op.validate()?;
        normalize_attributes(&mut new_op);

        if let (Some(Op::Delete { delete: previous }), Op::Delete { delete }) =
            (self.ops.last_mut(), &new_op)
        {
            *previous = previous
                .checked_add(*delete)
                .ok_or(DeltaError::LengthOverflow)?;
            return Ok(self);
        }

        let mut index = self.ops.len();
        if matches!(self.ops.last(), Some(Op::Delete { .. })) && matches!(new_op, Op::Insert { .. })
        {
            index -= 1;
        }

        if index > 0 && merge_ops(&mut self.ops[index - 1], &new_op)? {
            return Ok(self);
        }
        self.ops.insert(index, new_op);
        Ok(self)
    }

    pub fn chop(&mut self) -> &mut Self {
        if matches!(
            self.ops.last(),
            Some(Op::Retain {
                retain: RetainValue::Length(_),
                attributes: None
            })
        ) {
            self.ops.pop();
        }
        self
    }

    pub fn concat(&self, other: &Self) -> Result<Self, DeltaError> {
        self.validate()?;
        other.validate()?;
        let mut result = self.clone();
        if let Some((first, rest)) = other.ops.split_first() {
            result.push(first.clone())?;
            result.ops.extend_from_slice(rest);
        }
        Ok(result)
    }

    pub fn slice(&self, start: usize, end: usize) -> Result<Self, DeltaError> {
        self.validate()?;
        if end <= start {
            return Ok(Self::new());
        }
        let mut iterator = OpIterator::new(&self.ops);
        let mut index = 0usize;
        let mut result = Self::new();
        while index < end && iterator.has_next() {
            let amount = if index < start {
                start - index
            } else {
                end - index
            };
            let op = iterator.next(amount)?;
            let length = op.len();
            if index >= start {
                result.ops.push(op);
            }
            index = index
                .checked_add(length)
                .ok_or(DeltaError::LengthOverflow)?;
        }
        Ok(result)
    }

    pub fn compose(&self, other: &Self) -> Result<Self, DeltaError> {
        self.validate()?;
        other.validate()?;
        let mut left = OpIterator::new(&self.ops);
        let mut right = OpIterator::new(&other.ops);
        let mut result = Self::new();

        while left.has_next() || right.has_next() {
            if right.peek_kind() == OpKind::Insert {
                result.push(right.next_all()?)?;
                continue;
            }
            if left.peek_kind() == OpKind::Delete {
                result.push(left.next_all()?)?;
                continue;
            }
            let length = left
                .peek_len()
                .unwrap_or(usize::MAX)
                .min(right.peek_len().unwrap_or(usize::MAX));
            let left_op = left.next_virtual(length)?;
            let right_op = right.next_virtual(length)?;

            match right_op {
                Op::Retain {
                    retain: right_retain,
                    attributes: right_attributes,
                } => {
                    let keep_null = matches!(
                        left_op,
                        Op::Retain {
                            retain: RetainValue::Length(_),
                            ..
                        }
                    );
                    let left_attributes = left_op.attributes();
                    let attributes =
                        attributes_compose(left_attributes, right_attributes.as_ref(), keep_null);
                    let composed = match left_op {
                        Op::Insert { insert, .. } => match right_retain {
                            RetainValue::Length(_) => Op::Insert { insert, attributes },
                            RetainValue::Embed(right_embed) => {
                                return Err(embed_handler_error_for_pair(
                                    insert_embed_ref(&insert),
                                    &right_embed,
                                ));
                            }
                        },
                        Op::Retain {
                            retain: left_retain,
                            ..
                        } => match (left_retain, right_retain) {
                            (RetainValue::Length(_), retain) => Op::Retain { retain, attributes },
                            (retain @ RetainValue::Embed(_), RetainValue::Length(_)) => {
                                Op::Retain { retain, attributes }
                            }
                            (RetainValue::Embed(left_embed), RetainValue::Embed(right_embed)) => {
                                return Err(embed_handler_error_for_pair(
                                    Some(&left_embed),
                                    &right_embed,
                                ));
                            }
                        },
                        Op::Delete { .. } => unreachable!("delete handled before iterator split"),
                    };
                    result.push(composed)?;
                }
                Op::Delete { delete } => {
                    if matches!(left_op, Op::Retain { .. }) {
                        result.push(Op::Delete { delete })?;
                    }
                }
                Op::Insert { .. } => unreachable!("insert handled before iterator split"),
            }
        }
        result.chop();
        Ok(result)
    }

    pub fn transform(&self, other: &Self, priority: bool) -> Result<Self, DeltaError> {
        self.validate()?;
        other.validate()?;
        let mut left = OpIterator::new(&self.ops);
        let mut right = OpIterator::new(&other.ops);
        let mut result = Self::new();

        while left.has_next() || right.has_next() {
            if left.peek_kind() == OpKind::Insert
                && (priority || right.peek_kind() != OpKind::Insert)
            {
                result.retain(left.next_all()?.len(), None)?;
            } else if right.peek_kind() == OpKind::Insert {
                result.push(right.next_all()?)?;
            } else {
                let length = left
                    .peek_len()
                    .unwrap_or(usize::MAX)
                    .min(right.peek_len().unwrap_or(usize::MAX));
                let left_op = left.next_virtual(length)?;
                let right_op = right.next_virtual(length)?;
                if matches!(left_op, Op::Delete { .. }) {
                    continue;
                }
                if matches!(right_op, Op::Delete { .. }) {
                    result.push(right_op)?;
                    continue;
                }

                let left_attributes = left_op.attributes();
                let right_attributes = right_op.attributes();
                let attributes = attributes_transform(left_attributes, right_attributes, priority);
                let retain = match (&left_op, &right_op) {
                    (
                        Op::Retain {
                            retain: RetainValue::Embed(left_embed),
                            ..
                        },
                        Op::Retain {
                            retain: RetainValue::Embed(right_embed),
                            ..
                        },
                    ) if embed_type(left_embed) == embed_type(right_embed) => {
                        return Err(embed_handler_error_for_pair(Some(left_embed), right_embed));
                    }
                    (
                        _,
                        Op::Retain {
                            retain: RetainValue::Embed(embed),
                            ..
                        },
                    ) => RetainValue::Embed(embed.clone()),
                    _ => RetainValue::Length(length),
                };
                result.retain(retain, attributes)?;
            }
        }
        result.chop();
        Ok(result)
    }

    pub fn transform_position(
        &self,
        mut index: usize,
        priority: bool,
    ) -> Result<usize, DeltaError> {
        self.validate()?;
        let mut iterator = OpIterator::new(&self.ops);
        let mut offset = 0usize;
        while iterator.has_next() && offset <= index {
            let length = iterator
                .peek_len()
                .expect("has_next guarantees an operation");
            let kind = iterator.peek_kind();
            iterator.next_all()?;
            match kind {
                OpKind::Delete => {
                    index -= length.min(index - offset);
                    continue;
                }
                OpKind::Insert if offset < index || !priority => {
                    index = index
                        .checked_add(length)
                        .ok_or(DeltaError::LengthOverflow)?;
                }
                _ => {}
            }
            offset = offset
                .checked_add(length)
                .ok_or(DeltaError::LengthOverflow)?;
        }
        Ok(index)
    }

    pub fn invert(&self, base: &Self) -> Result<Self, DeltaError> {
        self.validate()?;
        base.validate_document()?;
        self.ensure_consumes_at_most(base.length()?)?;
        let mut result = Self::new();
        let mut base_index = 0usize;

        for op in &self.ops {
            match op {
                Op::Insert { .. } => {
                    result.delete(op.len())?;
                }
                Op::Retain {
                    retain: RetainValue::Length(length),
                    attributes: None,
                } => {
                    result.retain(*length, None)?;
                    base_index += length;
                }
                Op::Delete { delete } => {
                    let slice = base.slice(base_index, base_index + delete)?;
                    for base_op in slice.ops {
                        result.push(base_op)?;
                    }
                    base_index += delete;
                }
                Op::Retain {
                    retain: RetainValue::Length(length),
                    attributes: Some(attributes),
                } => {
                    let slice = base.slice(base_index, base_index + length)?;
                    for base_op in slice.ops {
                        result.retain(
                            base_op.len(),
                            attributes_invert(attributes, base_op.attributes()),
                        )?;
                    }
                    base_index += length;
                }
                Op::Retain {
                    retain: RetainValue::Embed(embed),
                    ..
                } => {
                    return Err(DeltaError::UnsupportedEmbedRetain {
                        embed_type: embed_type(embed).map(str::to_owned),
                    });
                }
            }
        }
        result.chop();
        Ok(result)
    }

    pub fn apply_to_document(&self, document: &Self) -> Result<Self, DeltaError> {
        document.validate_document()?;
        self.validate()?;
        self.ensure_consumes_at_most(document.length()?)?;
        let result = document.compose(self)?;
        result.validate_document()?;
        Ok(result)
    }

    pub fn to_plain_text(&self) -> Result<String, DeltaError> {
        self.validate_document()?;
        let mut text = String::new();
        for op in &self.ops {
            if let Op::Insert {
                insert: InsertValue::String(value),
                ..
            } = op
            {
                text.push_str(value);
            }
        }
        Ok(text)
    }

    pub fn change_length(&self) -> Result<isize, DeltaError> {
        self.validate()?;
        let mut length = 0i128;
        for op in &self.ops {
            match op {
                Op::Insert { .. } => length += op.len() as i128,
                Op::Delete { delete } => length -= *delete as i128,
                Op::Retain { .. } => {}
            }
        }
        isize::try_from(length).map_err(|_| DeltaError::LengthOverflow)
    }

    pub fn length(&self) -> Result<usize, DeltaError> {
        self.validate()?;
        self.ops.iter().try_fold(0usize, |length, op| {
            length
                .checked_add(op.len())
                .ok_or(DeltaError::LengthOverflow)
        })
    }

    pub fn validate(&self) -> Result<(), DeltaError> {
        for op in &self.ops {
            op.validate()?;
        }
        self.length_without_validation().map(|_| ())
    }

    pub fn is_document(&self) -> bool {
        self.ops.iter().all(|op| matches!(op, Op::Insert { .. }))
    }

    /// Validate values that Quill can apply without normalization or custom blots.
    pub fn validate_quill(&self, supported_embeds: &[&str]) -> Result<(), DeltaError> {
        self.validate()?;
        for op in &self.ops {
            match op {
                Op::Insert {
                    insert: InsertValue::String(text),
                    ..
                } if text.contains('\r') => return Err(DeltaError::CarriageReturn),
                Op::Insert {
                    insert: InsertValue::Embed(embed),
                    ..
                }
                | Op::Retain {
                    retain: RetainValue::Embed(embed),
                    ..
                } => {
                    let embed_type = (embed.len() == 1).then(|| embed.keys().next()).flatten();
                    if !embed_type.is_some_and(|kind| supported_embeds.contains(&kind.as_str())) {
                        return Err(DeltaError::UnsupportedEmbed {
                            embed_type: embed_type.cloned(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Returns whether this document has Quill's required terminal newline.
    pub fn ends_with_newline(&self) -> bool {
        matches!(
            self.ops.last(),
            Some(Op::Insert {
                insert: InsertValue::String(text),
                ..
            }) if text.ends_with('\n')
        )
    }

    fn validate_document(&self) -> Result<(), DeltaError> {
        self.validate()?;
        if self.is_document() {
            Ok(())
        } else {
            Err(DeltaError::ExpectedDocument)
        }
    }

    fn length_without_validation(&self) -> Result<usize, DeltaError> {
        self.ops.iter().try_fold(0usize, |length, op| {
            length
                .checked_add(op.len())
                .ok_or(DeltaError::LengthOverflow)
        })
    }

    fn ensure_consumes_at_most(&self, available: usize) -> Result<(), DeltaError> {
        let required = self.ops.iter().try_fold(0usize, |total, op| match op {
            Op::Insert { .. } => Ok(total),
            Op::Delete { .. } | Op::Retain { .. } => total
                .checked_add(op.len())
                .ok_or(DeltaError::LengthOverflow),
        })?;
        if required > available {
            Err(DeltaError::LengthConflict {
                required,
                available,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    Insert,
    Delete,
    Retain,
}

struct OpIterator<'a> {
    ops: &'a [Op],
    index: usize,
    offset: usize,
}

impl<'a> OpIterator<'a> {
    fn new(ops: &'a [Op]) -> Self {
        Self {
            ops,
            index: 0,
            offset: 0,
        }
    }

    fn has_next(&self) -> bool {
        self.index < self.ops.len()
    }

    fn peek_len(&self) -> Option<usize> {
        self.ops.get(self.index).map(|op| op.len() - self.offset)
    }

    fn peek_kind(&self) -> OpKind {
        self.ops
            .get(self.index)
            .map(Op::kind)
            .unwrap_or(OpKind::Retain)
    }

    fn next_all(&mut self) -> Result<Op, DeltaError> {
        let length = self
            .peek_len()
            .ok_or(DeltaError::MalformedOperation("iterator is exhausted"))?;
        self.next(length)
    }

    fn next_virtual(&mut self, length: usize) -> Result<Op, DeltaError> {
        if self.has_next() {
            self.next(length)
        } else {
            Ok(Op::retain(length, None))
        }
    }

    fn next(&mut self, requested: usize) -> Result<Op, DeltaError> {
        let op = self
            .ops
            .get(self.index)
            .ok_or(DeltaError::MalformedOperation("iterator is exhausted"))?;
        let remaining = op.len() - self.offset;
        let length = requested.min(remaining);
        let offset = self.offset;
        if length == remaining {
            self.index += 1;
            self.offset = 0;
        } else {
            self.offset += length;
        }

        match op {
            Op::Delete { .. } => Ok(Op::Delete { delete: length }),
            Op::Retain {
                retain: RetainValue::Length(_),
                attributes,
            } => Ok(Op::Retain {
                retain: RetainValue::Length(length),
                attributes: attributes.clone(),
            }),
            Op::Retain {
                retain: RetainValue::Embed(embed),
                attributes,
            } => Ok(Op::Retain {
                retain: RetainValue::Embed(embed.clone()),
                attributes: attributes.clone(),
            }),
            Op::Insert {
                insert: InsertValue::String(text),
                attributes,
            } => Ok(Op::Insert {
                insert: InsertValue::String(utf16_slice(text, offset, offset + length)?),
                attributes: attributes.clone(),
            }),
            Op::Insert {
                insert: InsertValue::Embed(embed),
                attributes,
            } => Ok(Op::Insert {
                insert: InsertValue::Embed(embed.clone()),
                attributes: attributes.clone(),
            }),
        }
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn utf16_slice(text: &str, start: usize, end: usize) -> Result<String, DeltaError> {
    let units: Vec<u16> = text.encode_utf16().collect();
    if start > units.len() || end > units.len() || start > end {
        return Err(DeltaError::LengthConflict {
            required: end,
            available: units.len(),
        });
    }
    String::from_utf16(&units[start..end]).map_err(|_| DeltaError::InvalidUtf16Boundary {
        index: if start > 0 && is_low_surrogate(units[start]) {
            start
        } else {
            end
        },
    })
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

fn nonempty_attributes(attributes: Option<Attributes>) -> Option<Attributes> {
    attributes.filter(|attributes| !attributes.is_empty())
}

fn normalize_attributes(op: &mut Op) {
    match op {
        Op::Insert { attributes, .. } | Op::Retain { attributes, .. } => {
            if attributes.as_ref().is_some_and(Map::is_empty) {
                *attributes = None;
            }
        }
        Op::Delete { .. } => {}
    }
}

fn merge_ops(previous: &mut Op, next: &Op) -> Result<bool, DeltaError> {
    match (previous, next) {
        (
            Op::Insert {
                insert: InsertValue::String(left),
                attributes: left_attributes,
            },
            Op::Insert {
                insert: InsertValue::String(right),
                attributes: right_attributes,
            },
        ) if left_attributes == right_attributes => {
            left.push_str(right);
            Ok(true)
        }
        (
            Op::Retain {
                retain: RetainValue::Length(left),
                attributes: left_attributes,
            },
            Op::Retain {
                retain: RetainValue::Length(right),
                attributes: right_attributes,
            },
        ) if left_attributes == right_attributes => {
            *left = left.checked_add(*right).ok_or(DeltaError::LengthOverflow)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn attributes_compose(
    left: Option<&Attributes>,
    right: Option<&Attributes>,
    keep_null: bool,
) -> Option<Attributes> {
    let mut result = right.cloned().unwrap_or_default();
    if !keep_null {
        result.retain(|_, value| !value.is_null());
    }
    if let Some(left) = left {
        for (key, value) in left {
            if !right.is_some_and(|right| right.contains_key(key)) {
                result.insert(key.clone(), value.clone());
            }
        }
    }
    (!result.is_empty()).then_some(result)
}

fn attributes_transform(
    left: Option<&Attributes>,
    right: Option<&Attributes>,
    priority: bool,
) -> Option<Attributes> {
    let right = right?;
    if !priority || left.is_none() {
        return Some(right.clone()).filter(|attributes| !attributes.is_empty());
    }
    let left = left.expect("checked above");
    let result = right
        .iter()
        .filter(|(key, _)| !left.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Attributes>();
    (!result.is_empty()).then_some(result)
}

fn attributes_invert(attributes: &Attributes, base: Option<&Attributes>) -> Option<Attributes> {
    let empty = Attributes::new();
    let base = base.unwrap_or(&empty);
    let mut result = Attributes::new();
    for (key, base_value) in base {
        if attributes.get(key).is_some_and(|value| value != base_value) {
            result.insert(key.clone(), base_value.clone());
        }
    }
    for key in attributes.keys() {
        if !base.contains_key(key) {
            result.insert(key.clone(), Value::Null);
        }
    }
    (!result.is_empty()).then_some(result)
}

fn insert_embed_ref(insert: &InsertValue) -> Option<&Embed> {
    match insert {
        InsertValue::Embed(embed) => Some(embed),
        InsertValue::String(_) => None,
    }
}

fn embed_type(embed: &Embed) -> Option<&str> {
    embed.keys().next().map(String::as_str)
}

fn embed_handler_error_for_pair(left: Option<&Embed>, right: &Embed) -> DeltaError {
    let left_type = left.and_then(embed_type).map(str::to_owned);
    let right_type = embed_type(right).map(str::to_owned);
    if left_type != right_type {
        DeltaError::EmbedTypeMismatch {
            left: left_type,
            right: right_type,
        }
    } else {
        DeltaError::UnsupportedEmbedRetain {
            embed_type: left_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn delta(value: Value) -> Delta {
        serde_json::from_value(value).unwrap()
    }

    fn attrs(value: Value) -> Attributes {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn serialization_matches_quill_schema_and_rejects_malformed_ops() {
        let value = json!({"ops": [
            {"insert": "A", "attributes": {"bold": true, "meta": [1, {"x": null}]}},
            {"insert": {"image": {"url": "x"}}},
            {"delete": 2},
            {"retain": 3, "attributes": {"color": "red"}},
            {"retain": {"table": {"rows": 2}}}
        ]});
        let parsed: Delta = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);

        for invalid in [
            json!({"ops":[{}]}),
            json!({"ops":[{"insert":"","retain":1}]}),
            json!({"ops":[{"delete":0}]}),
            json!({"ops":[{"retain":-1}]}),
            json!({"ops":[{"insert":false}]}),
            json!({"ops":[{"delete":1,"attributes":{}}]}),
            json!({"ops":[{"retain":1,"extra":true}]}),
        ] {
            assert!(serde_json::from_value::<Delta>(invalid).is_err());
        }
    }

    #[test]
    fn uses_utf16_lengths_and_slices_without_panicking() {
        let document = delta(json!({"ops":[{"insert":"a😀b"}]}));
        assert_eq!(document.length().unwrap(), 4);
        assert_eq!(
            document.slice(1, 3).unwrap(),
            delta(json!({"ops":[{"insert":"😀"}]}))
        );
        assert!(matches!(
            document.slice(1, 2),
            Err(DeltaError::InvalidUtf16Boundary { .. })
        ));
    }

    #[test]
    fn push_normalizes_order_merges_and_chop() {
        let mut value = Delta::new();
        value.delete(1).unwrap();
        value.insert("a", None).unwrap();
        value.insert("b", None).unwrap();
        value.delete(2).unwrap();
        value.retain(4, None).unwrap();
        assert_eq!(
            value,
            delta(json!({"ops":[{"insert":"ab"},{"delete":3},{"retain":4}]}))
        );
        value.chop();
        assert_eq!(value, delta(json!({"ops":[{"insert":"ab"},{"delete":3}]})));
    }

    #[test]
    fn concat_and_slice_preserve_canonical_boundaries() {
        let left = delta(json!({"ops":[{"insert":"ab"}]}));
        let right = delta(json!({"ops":[{"insert":"cd"},{"delete":1}]}));
        assert_eq!(
            left.concat(&right).unwrap(),
            delta(json!({"ops":[{"insert":"abcd"},{"delete":1}]}))
        );
        assert_eq!(
            right.slice(1, 3).unwrap(),
            delta(json!({"ops":[{"insert":"d"},{"delete":1}]}))
        );
    }

    #[test]
    fn compose_text_and_attributes() {
        let first = delta(json!({"ops":[
            {"insert":"A","attributes":{"bold":true,"color":"red"}},
            {"insert":"BC"}
        ]}));
        let second = delta(json!({"ops":[
            {"retain":1,"attributes":{"bold":null,"italic":true}},
            {"delete":1},
            {"insert":"x"}
        ]}));
        assert_eq!(
            first.compose(&second).unwrap(),
            delta(json!({"ops":[
                {"insert":"A","attributes":{"color":"red","italic":true}},
                {"insert":"xC"}
            ]}))
        );
    }

    #[test]
    fn concurrent_transforms_converge_with_attributes() {
        let document = delta(json!({"ops":[{"insert":"ab"}]}));
        let a = delta(json!({"ops":[
            {"retain":1,"attributes":{"bold":true}},
            {"insert":"A"}
        ]}));
        let b = delta(json!({"ops":[
            {"retain":1,"attributes":{"bold":false,"color":"blue"}},
            {"insert":"B"}
        ]}));
        let b_after_a = a.transform(&b, true).unwrap();
        let a_after_b = b.transform(&a, false).unwrap();
        let left = b_after_a
            .apply_to_document(&a.apply_to_document(&document).unwrap())
            .unwrap();
        let right = a_after_b
            .apply_to_document(&b.apply_to_document(&document).unwrap())
            .unwrap();
        assert_eq!(left, right);
        assert_eq!(
            left,
            delta(json!({"ops":[
                {"insert":"a","attributes":{"bold":true,"color":"blue"}},
                {"insert":"ABb"}
            ]}))
        );
    }

    #[test]
    fn transforms_positions_in_utf16_units() {
        let change = delta(json!({"ops":[{"retain":1},{"insert":"😀"},{"delete":2}]}));
        assert_eq!(change.transform_position(1, false).unwrap(), 3);
        assert_eq!(change.transform_position(1, true).unwrap(), 1);
        assert_eq!(change.transform_position(4, false).unwrap(), 4);
    }

    #[test]
    fn invert_restores_document_and_attributes() {
        let base = delta(json!({"ops":[
            {"insert":"a","attributes":{"bold":true}},
            {"insert":"bc"}
        ]}));
        let change = delta(json!({"ops":[
            {"retain":1,"attributes":{"bold":null,"color":"red"}},
            {"delete":1},
            {"insert":"X"}
        ]}));
        let changed = change.apply_to_document(&base).unwrap();
        let inverse = change.invert(&base).unwrap();
        assert_eq!(inverse.apply_to_document(&changed).unwrap(), base);
    }

    #[test]
    fn embeds_are_atomic_where_handlers_are_not_needed() {
        let base = delta(json!({"ops":[
            {"insert":"a"},
            {"insert":{"image":"url"},"attributes":{"alt":"x"}},
            {"insert":"b"}
        ]}));
        let delete_embed = delta(json!({"ops":[{"retain":1},{"delete":1}]}));
        let changed = delete_embed.apply_to_document(&base).unwrap();
        assert_eq!(changed.to_plain_text().unwrap(), "ab");
        let inverse = delete_embed.invert(&base).unwrap();
        assert_eq!(inverse.apply_to_document(&changed).unwrap(), base);

        let retain_embed = delta(json!({"ops":[{"retain":1},{"retain":{"image":{"crop":1}}}]}));
        assert!(matches!(
            retain_embed.apply_to_document(&base),
            Err(DeltaError::UnsupportedEmbedRetain { .. })
        ));
        assert!(matches!(
            retain_embed.invert(&base),
            Err(DeltaError::UnsupportedEmbedRetain { .. })
        ));
    }

    #[test]
    fn applying_changes_checks_documents_and_length_conflicts() {
        let document = delta(json!({"ops":[{"insert":"abc"}]}));
        let change = delta(json!({"ops":[{"retain":1},{"delete":1},{"insert":"X"}]}));
        assert_eq!(
            change.apply_to_document(&document).unwrap(),
            delta(json!({"ops":[{"insert":"aXc"}]}))
        );
        assert_eq!(change.change_length().unwrap(), 0);
        assert_eq!(change.length().unwrap(), 3);

        let too_long = delta(json!({"ops":[{"retain":4}]}));
        assert!(matches!(
            too_long.apply_to_document(&document),
            Err(DeltaError::LengthConflict {
                required: 4,
                available: 3
            })
        ));
        assert!(Delta::new().apply_to_document(&change).is_err());
    }

    #[test]
    fn validates_quill_specific_values() {
        let valid = delta(json!({"ops":[
            {"insert":{"image":"https://example.com/image.png"}},
            {"insert":"\n"}
        ]}));
        assert!(valid.validate_quill(DEFAULT_QUILL_EMBEDS).is_ok());
        assert!(valid.ends_with_newline());

        let carriage_return = delta(json!({"ops":[{"insert":"a\r\n"}]}));
        assert_eq!(
            carriage_return.validate_quill(DEFAULT_QUILL_EMBEDS),
            Err(DeltaError::CarriageReturn)
        );
        let unknown = delta(json!({"ops":[{"insert":{"custom":true}}]}));
        assert!(matches!(
            unknown.validate_quill(DEFAULT_QUILL_EMBEDS),
            Err(DeltaError::UnsupportedEmbed { .. })
        ));
    }

    #[test]
    fn arbitrary_attribute_json_is_compared_deeply() {
        let nested = attrs(json!({"data":{"list":[1,true,null]}}));
        let mut value = Delta::new();
        value.insert("a", Some(nested.clone())).unwrap();
        value.insert("b", Some(nested)).unwrap();
        assert_eq!(value.ops.len(), 1);
    }
}
