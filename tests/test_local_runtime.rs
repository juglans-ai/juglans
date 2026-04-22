/// Integration test: LocalRuntime with DeepSeek provider
///
/// Run with:
///   DEEPSEEK_API_KEY=... cargo test --test test_local_runtime -- --nocapture
use juglans::services::local_runtime::{ChatOutput, ChatRequest, LocalRuntime};
use serde_json::json;

#[tokio::test]
async fn test_deepseek_chat() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        eprintln!("DEEPSEEK_API_KEY not set, skipping test");
        return;
    }

    let runtime = LocalRuntime::new();

    let req = ChatRequest {
        agent_config: json!({
            "model": "deepseek/deepseek-chat",
        }),
        messages: vec![json!({
            "role": "user",
            "parts": [{"type": "text", "content": "Say 'hello juglans' and nothing else."}],
        })],
        tools: None,
        token_sender: None,
        tool_handler: None,
    };

    let result = runtime.chat(req).await;
    match result {
        Ok(ChatOutput::Final { text, .. }) => {
            println!("DeepSeek response: {}", text);
            assert!(
                text.to_lowercase().contains("hello") || text.to_lowercase().contains("juglans"),
                "Expected 'hello juglans' in response, got: {}",
                text
            );
        }
        Ok(other) => panic!("Expected ChatOutput::Final, got: {:?}", other),
        Err(e) => panic!("Chat failed: {}", e),
    }
}
