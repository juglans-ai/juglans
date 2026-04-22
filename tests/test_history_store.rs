// Integration tests for the conversation-history storage layer.
// End-to-end chat() persistence requires a live LLM provider and is
// covered manually — here we verify the store + builtin tool surface.

#![cfg(not(target_arch = "wasm32"))]

use juglans::services::history::{
    ChatMessage, ConversationStore, JsonlStore, MemoryStore, SqliteStore,
};
use std::sync::Arc;

fn tmp_dir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("juglans-hist-it-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn jsonl_persists_across_reopens() {
    let dir = tmp_dir();
    {
        let s = JsonlStore::open(&dir).unwrap();
        s.append("t1", ChatMessage::new("user", "first")).await.unwrap();
        s.append("t1", ChatMessage::new("assistant", "hi")).await.unwrap();
    }
    // Re-open should see prior messages.
    let s2 = JsonlStore::open(&dir).unwrap();
    let msgs = s2.load("t1", 10).await.unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "first");
    assert_eq!(msgs[1].content, "hi");
}

#[tokio::test]
async fn sqlite_persists_across_reopens() {
    let dir = tmp_dir();
    let db = dir.join("h.db");
    {
        let s = SqliteStore::open(&db).unwrap();
        s.append("t1", ChatMessage::new("user", "first")).await.unwrap();
        s.append("t1", ChatMessage::new("assistant", "hi")).await.unwrap();
    }
    let s2 = SqliteStore::open(&db).unwrap();
    let msgs = s2.load("t1", 10).await.unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].content, "first");
    assert_eq!(msgs[1].content, "hi");
}

#[tokio::test]
async fn load_limit_keeps_tail() {
    let s = MemoryStore::new();
    for i in 0..10 {
        s.append("t", ChatMessage::new("user", &format!("m{}", i))).await.unwrap();
    }
    let tail = s.load("t", 3).await.unwrap();
    assert_eq!(tail.len(), 3);
    assert_eq!(tail[0].content, "m7");
    assert_eq!(tail[2].content, "m9");
}

#[tokio::test]
async fn replace_collapses_range_sqlite() {
    let dir = tmp_dir();
    let s = SqliteStore::open(&dir.join("h.db")).unwrap();
    for i in 0..5 {
        s.append("t", ChatMessage::new("user", &format!("m{}", i))).await.unwrap();
    }
    s.replace("t", 0, 3, ChatMessage::new("system", "[sum]")).await.unwrap();
    let all = s.load("t", 10).await.unwrap();
    assert_eq!(all.len(), 3);
    // Remaining messages include the two kept originals plus the summary;
    // order is not guaranteed across backends but the count + contents are.
    let contents: Vec<&str> = all.iter().map(|m| m.content.as_str()).collect();
    assert!(contents.contains(&"m3"));
    assert!(contents.contains(&"m4"));
    assert!(contents.contains(&"[sum]"));
}

#[tokio::test]
async fn isolated_chat_ids_never_cross() {
    let s: Arc<dyn ConversationStore> = Arc::new(MemoryStore::new());
    s.append("user_a", ChatMessage::new("user", "secret_a")).await.unwrap();
    s.append("user_b", ChatMessage::new("user", "secret_b")).await.unwrap();
    let a = s.load("user_a", 10).await.unwrap();
    let b = s.load("user_b", 10).await.unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].content, "secret_a");
    assert_eq!(b[0].content, "secret_b");
}

#[tokio::test]
async fn trim_drops_oldest() {
    let s = MemoryStore::new();
    for i in 0..5 {
        s.append("t", ChatMessage::new("user", &format!("m{}", i))).await.unwrap();
    }
    s.trim("t", 2).await.unwrap();
    let kept = s.load("t", 10).await.unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].content, "m3");
    assert_eq!(kept[1].content, "m4");
}
