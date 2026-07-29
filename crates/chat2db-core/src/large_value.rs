use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_PREVIEW_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_CHUNK_SIZE: u32 = 256 * 1024;
const DEFAULT_MAX_VALUE_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
const DEFAULT_MAX_ENTRIES: usize = 512;
const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
const SCOPED_TOKEN_SEPARATOR: char = '.';

pub(crate) fn new_owner_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub(crate) fn scope_preview(owner_id: &str, mut preview: LargeValuePreview) -> LargeValuePreview {
    if let Some(token) = preview.large_value_id.take() {
        preview.large_value_id = Some(format!("{owner_id}{SCOPED_TOKEN_SEPARATOR}{token}"));
    }
    preview
}

pub(crate) fn scoped_token(value: &str) -> Result<(&str, &str), LargeValueError> {
    let (owner_id, token) = value
        .split_once(SCOPED_TOKEN_SEPARATOR)
        .ok_or(LargeValueError::InvalidToken)?;
    Uuid::parse_str(owner_id).map_err(|_| LargeValueError::InvalidToken)?;
    parse_token(token)?;
    Ok((owner_id, token))
}

/// Resource limits for retained Console large values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargeValueStoreConfig {
    /// Maximum raw bytes included in an inline preview.
    pub preview_bytes: usize,
    /// Maximum character count for text chunks and byte count for binary chunks.
    pub max_chunk_size: u32,
    /// Maximum raw size accepted for one retained value.
    pub max_value_bytes: u64,
    /// Maximum raw size retained across all values.
    pub max_total_bytes: u64,
    /// Maximum number of retained values.
    pub max_entries: usize,
    /// Fixed lifetime of a token. Reads do not extend it.
    pub ttl: Duration,
}

impl Default for LargeValueStoreConfig {
    fn default() -> Self {
        Self {
            preview_bytes: DEFAULT_PREVIEW_BYTES,
            max_chunk_size: DEFAULT_MAX_CHUNK_SIZE,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
            ttl: DEFAULT_TTL,
        }
    }
}

/// The portable type of a retained large value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LargeValueType {
    Text,
    Binary,
}

/// Encoding used by a preview or chunk payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LargeValueEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "base64")]
    Base64,
}

/// Bounded cell value returned with a Console result row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValuePreview {
    pub value: String,
    pub large_value: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_value_id: Option<String>,
    pub value_type: LargeValueType,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_chars: Option<u64>,
    pub loaded_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaded_chars: Option<u64>,
    pub truncated: bool,
    pub encoding: LargeValueEncoding,
}

/// One bounded read from a retained value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeValueChunk {
    pub value: String,
    /// Character offset for text and byte offset for binary.
    pub offset: u64,
    /// Character offset for text and byte offset for binary.
    pub next_offset: u64,
    pub eof: bool,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_chars: Option<u64>,
    pub encoding: LargeValueEncoding,
    pub content_type: String,
    pub display_mode: LargeValueType,
}

/// Current retained-value resource usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LargeValueStoreStats {
    pub entries: usize,
    pub total_bytes: u64,
}

/// A closed error contract for token and chunk operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LargeValueError {
    #[error("large value owner id is required")]
    InvalidOwner,
    #[error("large value token is malformed")]
    InvalidToken,
    #[error("large value token was not found")]
    NotFound,
    #[error("large value token has expired")]
    Expired,
    #[error("large value token belongs to another owner")]
    OwnerMismatch,
    #[error("large value chunk limit must be greater than zero")]
    InvalidLimit,
    #[error("large value offset {offset} exceeds length {length}")]
    InvalidRange { offset: u64, length: u64 },
    #[error(
        "large value capacity exceeded: requested {requested_bytes} bytes, value limit {max_value_bytes}, total limit {max_total_bytes}, entry limit {max_entries}"
    )]
    CapacityExceeded {
        requested_bytes: u64,
        max_value_bytes: u64,
        max_total_bytes: u64,
        max_entries: usize,
    },
}

/// In-memory large-value retention for native Console results.
///
/// Tokens are random UUID v4 values, have a fixed TTL, and are scoped to an
/// execution owner. When capacity is needed, the oldest retained value is
/// evicted first.
#[derive(Debug)]
pub struct LargeValueStore {
    config: LargeValueStoreConfig,
    state: Mutex<StoreState>,
}

impl LargeValueStore {
    /// Creates an empty store with explicit retention limits.
    #[must_use]
    pub fn new(config: LargeValueStoreConfig) -> Self {
        Self {
            config,
            state: Mutex::new(StoreState::default()),
        }
    }

    /// Builds a bounded UTF-8 preview and retains the full value when needed.
    ///
    /// # Errors
    ///
    /// Returns [`LargeValueError::InvalidOwner`] for an empty owner, or
    /// [`LargeValueError::CapacityExceeded`] when the full value cannot fit.
    pub fn retain_text(
        &self,
        owner_id: &str,
        value: String,
    ) -> Result<LargeValuePreview, LargeValueError> {
        self.retain(owner_id, StoredValue::Text(value))
    }

    /// Builds a bounded base64 preview and retains the full value when needed.
    ///
    /// # Errors
    ///
    /// Returns [`LargeValueError::InvalidOwner`] for an empty owner, or
    /// [`LargeValueError::CapacityExceeded`] when the full value cannot fit.
    pub fn retain_binary(
        &self,
        owner_id: &str,
        value: Vec<u8>,
    ) -> Result<LargeValuePreview, LargeValueError> {
        self.retain(owner_id, StoredValue::Binary(value))
    }

    /// Reads a chunk after validating token syntax, expiry, and ownership.
    ///
    /// # Errors
    ///
    /// Returns a closed [`LargeValueError`] variant for invalid owners, tokens,
    /// ranges, limits, expiry, missing entries, or owner mismatches.
    pub fn read_chunk(
        &self,
        owner_id: &str,
        token: &str,
        offset: u64,
        limit: u32,
    ) -> Result<LargeValueChunk, LargeValueError> {
        self.read_chunk_inner(owner_id, token, offset, limit, false)
    }

    /// Reads a base64 chunk whose offsets and limit are measured in raw bytes.
    ///
    /// This matches the Community large-cell transfer contract for encoded
    /// text and binary values.
    ///
    /// # Errors
    ///
    /// Returns a closed [`LargeValueError`] variant for invalid owners, tokens,
    /// ranges, limits, expiry, missing entries, or owner mismatches.
    pub fn read_encoded_chunk(
        &self,
        owner_id: &str,
        token: &str,
        offset: u64,
        limit: u32,
    ) -> Result<LargeValueChunk, LargeValueError> {
        self.read_chunk_inner(owner_id, token, offset, limit, true)
    }

    fn read_chunk_inner(
        &self,
        owner_id: &str,
        token: &str,
        offset: u64,
        limit: u32,
        encoded: bool,
    ) -> Result<LargeValueChunk, LargeValueError> {
        validate_owner(owner_id)?;
        if limit == 0 || self.config.max_chunk_size == 0 {
            return Err(LargeValueError::InvalidLimit);
        }
        let token = parse_token(token)?;
        let now = Instant::now();
        let mut state = self.lock_state();
        if state
            .entries
            .get(&token)
            .is_some_and(|entry| now >= entry.expires_at)
        {
            state.remove(&token);
            return Err(LargeValueError::Expired);
        }
        state.cleanup_expired(now);
        let entry = state.entries.get(&token).ok_or(LargeValueError::NotFound)?;
        if entry.owner_id != owner_id {
            return Err(LargeValueError::OwnerMismatch);
        }
        let limit = limit.min(self.config.max_chunk_size);
        if encoded {
            entry
                .value
                .encoded_chunk(offset, limit, entry.size_bytes, entry.size_chars)
        } else {
            entry
                .value
                .chunk(offset, limit, entry.size_bytes, entry.size_chars)
        }
    }

    /// Removes one retained value after validating its owner.
    ///
    /// # Errors
    ///
    /// Returns a closed [`LargeValueError`] variant for invalid owners, tokens,
    /// expiry, missing entries, or owner mismatches.
    pub fn remove_token(&self, owner_id: &str, token: &str) -> Result<(), LargeValueError> {
        validate_owner(owner_id)?;
        let token = parse_token(token)?;
        let now = Instant::now();
        let mut state = self.lock_state();
        let entry = state.entries.get(&token).ok_or(LargeValueError::NotFound)?;
        if now >= entry.expires_at {
            state.remove(&token);
            return Err(LargeValueError::Expired);
        }
        if entry.owner_id != owner_id {
            return Err(LargeValueError::OwnerMismatch);
        }
        state.remove(&token);
        Ok(())
    }

    /// Removes every retained value for an execution or result owner.
    #[must_use]
    pub fn remove_owner(&self, owner_id: &str) -> usize {
        let mut state = self.lock_state();
        state.cleanup_expired(Instant::now());
        let tokens = state
            .entries
            .iter()
            .filter_map(|(token, entry)| (entry.owner_id == owner_id).then_some(*token))
            .collect::<Vec<_>>();
        let removed = tokens.len();
        for token in tokens {
            state.remove(&token);
        }
        removed
    }

    /// Deletes expired entries and returns the number released.
    #[must_use]
    pub fn cleanup_expired(&self) -> usize {
        self.lock_state().cleanup_expired(Instant::now())
    }

    /// Returns usage after removing expired entries.
    #[must_use]
    pub fn stats(&self) -> LargeValueStoreStats {
        let mut state = self.lock_state();
        state.cleanup_expired(Instant::now());
        LargeValueStoreStats {
            entries: state.entries.len(),
            total_bytes: state.total_bytes,
        }
    }

    fn retain(
        &self,
        owner_id: &str,
        value: StoredValue,
    ) -> Result<LargeValuePreview, LargeValueError> {
        validate_owner(owner_id)?;
        let preview = value.preview(self.config.preview_bytes);
        if !preview.truncated {
            return Ok(preview.into_public(None));
        }
        let size_bytes = value.size_bytes();
        if size_bytes > self.config.max_value_bytes
            || size_bytes > self.config.max_total_bytes
            || self.config.max_entries == 0
        {
            return Err(self.capacity_error(size_bytes));
        }

        let now = Instant::now();
        let mut state = self.lock_state();
        state.cleanup_expired(now);
        while state.entries.len() >= self.config.max_entries
            || state.total_bytes.saturating_add(size_bytes) > self.config.max_total_bytes
        {
            if !state.evict_oldest() {
                break;
            }
        }
        if state.entries.len() >= self.config.max_entries
            || state.total_bytes.saturating_add(size_bytes) > self.config.max_total_bytes
        {
            return Err(self.capacity_error(size_bytes));
        }

        let token = loop {
            let candidate = Uuid::new_v4();
            if !state.entries.contains_key(&candidate) {
                break candidate;
            }
        };
        let size_chars = value.size_chars();
        let expires_at = now.checked_add(self.config.ttl).unwrap_or(now);
        state.total_bytes = state.total_bytes.saturating_add(size_bytes);
        state.order.push_back(token);
        state.entries.insert(
            token,
            StoredEntry {
                owner_id: owner_id.to_owned(),
                value,
                size_bytes,
                size_chars,
                expires_at,
            },
        );
        Ok(preview.into_public(Some(token.simple().to_string())))
    }

    fn capacity_error(&self, requested_bytes: u64) -> LargeValueError {
        LargeValueError::CapacityExceeded {
            requested_bytes,
            max_value_bytes: self.config.max_value_bytes,
            max_total_bytes: self.config.max_total_bytes,
            max_entries: self.config.max_entries,
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, StoreState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for LargeValueStore {
    fn default() -> Self {
        Self::new(LargeValueStoreConfig::default())
    }
}

#[derive(Debug, Default)]
struct StoreState {
    entries: HashMap<Uuid, StoredEntry>,
    order: VecDeque<Uuid>,
    total_bytes: u64,
}

impl StoreState {
    fn remove(&mut self, token: &Uuid) -> bool {
        let Some(entry) = self.entries.remove(token) else {
            return false;
        };
        self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
        self.order.retain(|queued| queued != token);
        true
    }

    fn evict_oldest(&mut self) -> bool {
        while let Some(token) = self.order.pop_front() {
            if let Some(entry) = self.entries.remove(&token) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
                return true;
            }
        }
        false
    }

    fn cleanup_expired(&mut self, now: Instant) -> usize {
        let expired = self
            .entries
            .iter()
            .filter_map(|(token, entry)| (now >= entry.expires_at).then_some(*token))
            .collect::<Vec<_>>();
        let removed = expired.len();
        for token in expired {
            if let Some(entry) = self.entries.remove(&token) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.size_bytes);
            }
        }
        self.order.retain(|token| self.entries.contains_key(token));
        removed
    }
}

#[derive(Debug)]
struct StoredEntry {
    owner_id: String,
    value: StoredValue,
    size_bytes: u64,
    size_chars: Option<u64>,
    expires_at: Instant,
}

#[derive(Debug)]
enum StoredValue {
    Text(String),
    Binary(Vec<u8>),
}

impl StoredValue {
    fn size_bytes(&self) -> u64 {
        match self {
            Self::Text(value) => value.len() as u64,
            Self::Binary(value) => value.len() as u64,
        }
    }

    fn size_chars(&self) -> Option<u64> {
        match self {
            Self::Text(value) => Some(value.chars().count() as u64),
            Self::Binary(_) => None,
        }
    }

    fn preview(&self, max_bytes: usize) -> PreviewParts {
        match self {
            Self::Text(value) => {
                let mut end = value.len().min(max_bytes);
                while end > 0 && !value.is_char_boundary(end) {
                    end -= 1;
                }
                let preview = &value[..end];
                PreviewParts {
                    value: preview.to_owned(),
                    value_type: LargeValueType::Text,
                    size_bytes: value.len() as u64,
                    size_chars: Some(value.chars().count() as u64),
                    loaded_bytes: end as u64,
                    loaded_chars: Some(preview.chars().count() as u64),
                    truncated: end < value.len(),
                    encoding: LargeValueEncoding::Utf8,
                }
            }
            Self::Binary(value) => {
                let included = value.len().min(max_bytes);
                PreviewParts {
                    value: BASE64_STANDARD.encode(&value[..included]),
                    value_type: LargeValueType::Binary,
                    size_bytes: value.len() as u64,
                    size_chars: None,
                    loaded_bytes: included as u64,
                    loaded_chars: None,
                    truncated: included < value.len(),
                    encoding: LargeValueEncoding::Base64,
                }
            }
        }
    }

    fn chunk(
        &self,
        offset: u64,
        limit: u32,
        size_bytes: u64,
        size_chars: Option<u64>,
    ) -> Result<LargeValueChunk, LargeValueError> {
        match self {
            Self::Text(value) => text_chunk(value, offset, limit, size_bytes, size_chars),
            Self::Binary(value) => binary_chunk(value, offset, limit, size_bytes),
        }
    }

    fn encoded_chunk(
        &self,
        offset: u64,
        limit: u32,
        size_bytes: u64,
        size_chars: Option<u64>,
    ) -> Result<LargeValueChunk, LargeValueError> {
        match self {
            Self::Text(value) => byte_chunk(
                value.as_bytes(),
                offset,
                limit,
                size_bytes,
                size_chars,
                LargeValueType::Text,
                "text/plain",
            ),
            Self::Binary(value) => binary_chunk(value, offset, limit, size_bytes),
        }
    }
}

struct PreviewParts {
    value: String,
    value_type: LargeValueType,
    size_bytes: u64,
    size_chars: Option<u64>,
    loaded_bytes: u64,
    loaded_chars: Option<u64>,
    truncated: bool,
    encoding: LargeValueEncoding,
}

impl PreviewParts {
    fn into_public(self, token: Option<String>) -> LargeValuePreview {
        LargeValuePreview {
            value: self.value,
            large_value: self.truncated,
            large_value_id: token,
            value_type: self.value_type,
            size_bytes: self.size_bytes,
            size_chars: self.size_chars,
            loaded_bytes: self.loaded_bytes,
            loaded_chars: self.loaded_chars,
            truncated: self.truncated,
            encoding: self.encoding,
        }
    }
}

fn text_chunk(
    value: &str,
    offset: u64,
    limit: u32,
    size_bytes: u64,
    size_chars: Option<u64>,
) -> Result<LargeValueChunk, LargeValueError> {
    let length = size_chars.unwrap_or_else(|| value.chars().count() as u64);
    if offset > length {
        return Err(LargeValueError::InvalidRange { offset, length });
    }

    let offset_index =
        usize::try_from(offset).map_err(|_| LargeValueError::InvalidRange { offset, length })?;
    let start = if offset == length {
        value.len()
    } else {
        value
            .char_indices()
            .nth(offset_index)
            .map(|(index, _)| index)
            .ok_or(LargeValueError::InvalidRange { offset, length })?
    };

    let tail = &value[start..];
    let mut included = 0_u64;
    let mut end = tail.len();
    for (index, _) in tail.char_indices() {
        if included == u64::from(limit) {
            end = index;
            break;
        }
        included += 1;
    }
    let next_offset = offset + included;
    Ok(LargeValueChunk {
        value: tail[..end].to_owned(),
        offset,
        next_offset,
        eof: next_offset == length,
        size_bytes,
        size_chars: Some(length),
        encoding: LargeValueEncoding::Utf8,
        content_type: "text/plain".to_owned(),
        display_mode: LargeValueType::Text,
    })
}

fn binary_chunk(
    value: &[u8],
    offset: u64,
    limit: u32,
    size_bytes: u64,
) -> Result<LargeValueChunk, LargeValueError> {
    byte_chunk(
        value,
        offset,
        limit,
        size_bytes,
        None,
        LargeValueType::Binary,
        "application/octet-stream",
    )
}

fn byte_chunk(
    value: &[u8],
    offset: u64,
    limit: u32,
    size_bytes: u64,
    size_chars: Option<u64>,
    display_mode: LargeValueType,
    content_type: &str,
) -> Result<LargeValueChunk, LargeValueError> {
    let length = value.len() as u64;
    if offset > length {
        return Err(LargeValueError::InvalidRange { offset, length });
    }
    let start =
        usize::try_from(offset).map_err(|_| LargeValueError::InvalidRange { offset, length })?;
    let end = start.saturating_add(limit as usize).min(value.len());
    Ok(LargeValueChunk {
        value: BASE64_STANDARD.encode(&value[start..end]),
        offset,
        next_offset: end as u64,
        eof: end == value.len(),
        size_bytes,
        size_chars,
        encoding: LargeValueEncoding::Base64,
        content_type: content_type.to_owned(),
        display_mode,
    })
}

fn validate_owner(owner_id: &str) -> Result<(), LargeValueError> {
    if owner_id.trim().is_empty() {
        Err(LargeValueError::InvalidOwner)
    } else {
        Ok(())
    }
}

fn parse_token(token: &str) -> Result<Uuid, LargeValueError> {
    Uuid::parse_str(token).map_err(|_| LargeValueError::InvalidToken)
}
