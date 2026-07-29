#[allow(dead_code)]
#[path = "../src/large_value.rs"]
mod large_value;

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use large_value::{
    LargeValueEncoding, LargeValueError, LargeValueStore, LargeValueStoreConfig, LargeValueType,
};
use uuid::{Uuid, Version};

fn config() -> LargeValueStoreConfig {
    LargeValueStoreConfig {
        preview_bytes: 4,
        max_chunk_size: 4,
        max_value_bytes: 64,
        max_total_bytes: 128,
        max_entries: 8,
        ttl: Duration::from_secs(600),
    }
}

fn token(preview: &large_value::LargeValuePreview) -> &str {
    preview
        .large_value_id
        .as_deref()
        .expect("truncated values must receive a token")
}

#[test]
fn unicode_and_binary_previews_are_bounded_and_explicitly_encoded() {
    let store = LargeValueStore::new(config());
    let text = store
        .retain_text("execution-1", "a你🙂z".to_owned())
        .expect("text should retain");
    assert_eq!(text.value, "a你");
    assert_eq!(text.loaded_bytes, 4);
    assert_eq!(text.loaded_chars, Some(2));
    assert_eq!(text.size_bytes, 9);
    assert_eq!(text.size_chars, Some(4));
    assert_eq!(text.value_type, LargeValueType::Text);
    assert_eq!(text.encoding, LargeValueEncoding::Utf8);
    assert!(text.large_value);
    assert!(text.truncated);

    let bytes = vec![0, 1, 2, 3, 4, 255];
    let binary = store
        .retain_binary("execution-1", bytes.clone())
        .expect("binary should retain");
    assert_eq!(binary.value, BASE64_STANDARD.encode(&bytes[..4]));
    assert_eq!(binary.loaded_bytes, 4);
    assert_eq!(binary.loaded_chars, None);
    assert_eq!(binary.size_chars, None);
    assert_eq!(binary.value_type, LargeValueType::Binary);
    assert_eq!(binary.encoding, LargeValueEncoding::Base64);

    let inline = store
        .retain_text("execution-1", "rust".to_owned())
        .expect("small text should remain inline");
    assert!(!inline.large_value);
    assert!(!inline.truncated);
    assert!(inline.large_value_id.is_none());
}

#[test]
fn tokens_are_unique_opaque_uuid_v4_values_and_owner_bound() {
    let store = LargeValueStore::new(config());
    let first = store
        .retain_text("execution-1", "first value".to_owned())
        .expect("first value should retain");
    let second = store
        .retain_text("execution-1", "second value".to_owned())
        .expect("second value should retain");
    assert_ne!(token(&first), token(&second));
    for value in [&first, &second] {
        let parsed = Uuid::parse_str(token(value)).expect("token must be a UUID");
        assert_eq!(parsed.get_version(), Some(Version::Random));
    }

    assert_eq!(
        store.read_chunk("execution-2", token(&first), 0, 1),
        Err(LargeValueError::OwnerMismatch)
    );
    assert_eq!(
        store.read_chunk("", token(&first), 0, 1),
        Err(LargeValueError::InvalidOwner)
    );
    assert_eq!(
        store.read_chunk("execution-1", "not-a-token", 0, 1),
        Err(LargeValueError::InvalidToken)
    );
}

#[test]
fn zero_ttl_expires_on_first_read_and_releases_capacity() {
    let mut limits = config();
    limits.ttl = Duration::ZERO;
    limits.max_entries = 1;
    limits.max_total_bytes = 16;
    let store = LargeValueStore::new(limits);
    let preview = store
        .retain_text("execution-1", "expired".to_owned())
        .expect("value should receive an immediately expiring token");

    assert_eq!(
        store.read_chunk("execution-1", token(&preview), 0, 1),
        Err(LargeValueError::Expired)
    );
    assert_eq!(store.stats().entries, 0);
    assert_eq!(store.stats().total_bytes, 0);
    assert_eq!(store.cleanup_expired(), 0);

    let replacement = store
        .retain_text("execution-1", "another".to_owned())
        .expect("expired capacity must be reusable");
    assert!(replacement.large_value_id.is_some());
}

#[test]
fn capacity_limits_reject_oversized_values_and_evict_oldest_entries() {
    let mut limits = config();
    limits.preview_bytes = 1;
    limits.max_value_bytes = 6;
    limits.max_total_bytes = 10;
    limits.max_entries = 2;
    let store = LargeValueStore::new(limits.clone());

    let too_large = store
        .retain_text("execution-1", "1234567".to_owned())
        .expect_err("single value limit must be enforced");
    assert_eq!(
        too_large,
        LargeValueError::CapacityExceeded {
            requested_bytes: 7,
            max_value_bytes: 6,
            max_total_bytes: 10,
            max_entries: 2,
        }
    );

    let first = store
        .retain_text("execution-1", "12345".to_owned())
        .expect("first value should retain");
    let second = store
        .retain_text("execution-1", "abcde".to_owned())
        .expect("second value should retain");
    let third = store
        .retain_text("execution-1", "vwxyz".to_owned())
        .expect("third value should evict the oldest");
    assert_eq!(store.stats().entries, 2);
    assert_eq!(store.stats().total_bytes, 10);
    assert_eq!(
        store.read_chunk("execution-1", token(&first), 0, 1),
        Err(LargeValueError::NotFound)
    );
    assert!(
        store
            .read_chunk("execution-1", token(&second), 0, 1)
            .is_ok()
    );
    assert!(store.read_chunk("execution-1", token(&third), 0, 1).is_ok());

    limits.max_entries = 0;
    let disabled = LargeValueStore::new(limits);
    assert!(matches!(
        disabled.retain_binary("execution-1", vec![1, 2]),
        Err(LargeValueError::CapacityExceeded { .. })
    ));
}

#[test]
fn text_chunks_preserve_unicode_without_overlap_or_gaps() {
    let mut limits = config();
    limits.preview_bytes = 1;
    limits.max_chunk_size = 2;
    let store = LargeValueStore::new(limits);
    let source = "A你🙂BC界";
    let preview = store
        .retain_text("execution-1", source.to_owned())
        .expect("text should retain");

    let mut offset = 0;
    let mut rebuilt = String::new();
    loop {
        let chunk = store
            .read_chunk("execution-1", token(&preview), offset, u32::MAX)
            .expect("chunk should read");
        assert_eq!(chunk.offset, offset);
        assert_eq!(chunk.encoding, LargeValueEncoding::Utf8);
        assert_eq!(chunk.content_type, "text/plain");
        assert_eq!(chunk.display_mode, LargeValueType::Text);
        rebuilt.push_str(&chunk.value);
        assert!(chunk.next_offset > offset || chunk.eof);
        offset = chunk.next_offset;
        if chunk.eof {
            break;
        }
    }
    assert_eq!(rebuilt, source);
    assert_eq!(offset, source.chars().count() as u64);
}

#[test]
fn encoded_text_chunks_use_raw_byte_offsets() {
    let mut limits = config();
    limits.preview_bytes = 1;
    limits.max_chunk_size = 3;
    let store = LargeValueStore::new(limits);
    let source = "A你🙂BC界";
    let preview = store
        .retain_text("execution-1", source.to_owned())
        .expect("text should retain");

    let mut offset = 0;
    let mut rebuilt = Vec::new();
    loop {
        let chunk = store
            .read_encoded_chunk("execution-1", token(&preview), offset, u32::MAX)
            .expect("encoded chunk should read");
        let decoded = BASE64_STANDARD
            .decode(&chunk.value)
            .expect("encoded text chunk must be base64");
        assert_eq!(chunk.offset, offset);
        assert_eq!(chunk.next_offset, offset + decoded.len() as u64);
        assert_eq!(chunk.encoding, LargeValueEncoding::Base64);
        assert_eq!(chunk.content_type, "text/plain");
        assert_eq!(chunk.display_mode, LargeValueType::Text);
        rebuilt.extend(decoded);
        offset = chunk.next_offset;
        if chunk.eof {
            break;
        }
    }
    assert_eq!(rebuilt, source.as_bytes());
    assert_eq!(offset, source.len() as u64);
}

#[test]
fn binary_chunks_round_trip_and_range_validation_is_closed() {
    let mut limits = config();
    limits.preview_bytes = 1;
    limits.max_chunk_size = 3;
    let store = LargeValueStore::new(limits);
    let source = (0_u8..=12).collect::<Vec<_>>();
    let preview = store
        .retain_binary("execution-1", source.clone())
        .expect("binary should retain");

    let mut offset = 0;
    let mut rebuilt = Vec::new();
    loop {
        let chunk = store
            .read_chunk("execution-1", token(&preview), offset, u32::MAX)
            .expect("chunk should read");
        let decoded = BASE64_STANDARD
            .decode(&chunk.value)
            .expect("chunk must be base64");
        assert_eq!(chunk.offset, offset);
        assert_eq!(chunk.next_offset, offset + decoded.len() as u64);
        assert_eq!(chunk.encoding, LargeValueEncoding::Base64);
        assert_eq!(chunk.content_type, "application/octet-stream");
        rebuilt.extend(decoded);
        offset = chunk.next_offset;
        if chunk.eof {
            break;
        }
    }
    assert_eq!(rebuilt, source);
    assert_eq!(
        store.read_chunk("execution-1", token(&preview), 0, 0),
        Err(LargeValueError::InvalidLimit)
    );
    assert_eq!(
        store.read_chunk("execution-1", token(&preview), 14, 1),
        Err(LargeValueError::InvalidRange {
            offset: 14,
            length: 13,
        })
    );
    let eof = store
        .read_chunk("execution-1", token(&preview), 13, 1)
        .expect("offset at the end should be valid");
    assert!(eof.eof);
    assert!(eof.value.is_empty());
}

#[test]
fn explicit_token_and_owner_cleanup_leave_no_retained_bytes() {
    let mut limits = config();
    limits.preview_bytes = 1;
    let store = LargeValueStore::new(limits);
    let first = store
        .retain_text("execution-1", "first".to_owned())
        .expect("first should retain");
    let second = store
        .retain_binary("execution-1", vec![1, 2, 3, 4])
        .expect("second should retain");
    let other = store
        .retain_text("execution-2", "other".to_owned())
        .expect("other should retain");

    assert_eq!(
        store.remove_token("execution-2", token(&first)),
        Err(LargeValueError::OwnerMismatch)
    );
    store
        .remove_token("execution-1", token(&first))
        .expect("owner should remove token");
    assert_eq!(
        store.read_chunk("execution-1", token(&first), 0, 1),
        Err(LargeValueError::NotFound)
    );
    assert_eq!(store.remove_owner("execution-1"), 1);
    assert_eq!(
        store.read_chunk("execution-1", token(&second), 0, 1),
        Err(LargeValueError::NotFound)
    );
    assert_eq!(store.stats().entries, 1);
    assert_eq!(store.remove_owner("execution-2"), 1);
    assert_eq!(store.stats().entries, 0);
    assert_eq!(store.stats().total_bytes, 0);
    assert_eq!(
        store.remove_token("execution-2", token(&other)),
        Err(LargeValueError::NotFound)
    );
}
