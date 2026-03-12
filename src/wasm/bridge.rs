// src/wasm/bridge.rs — JS callback bridge for tool execution
//
// All tool calls from the WASM executor are routed through a single JS function:
//   handler(name: string, params: object) -> Promise<result>

use anyhow::anyhow;
use js_sys::Promise;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// Call a JS tool handler: handler(name, params) → Promise<result>
pub async fn call_tool_handler(
    handler: &js_sys::Function,
    name: &str,
    params: &Value,
) -> Result<Value, anyhow::Error> {
    let this = JsValue::NULL;
    let js_name = JsValue::from_str(name);
    let js_params =
        serde_wasm_bindgen::to_value(params).map_err(|e| anyhow!("Serialize params: {}", e))?;

    let result = handler
        .call2(&this, &js_name, &js_params)
        .map_err(|e| anyhow!("JS call error: {:?}", e))?;

    // If the result is a Promise, await it
    let js_result = if result.is_instance_of::<Promise>() {
        let promise: Promise = result.unchecked_into();
        JsFuture::from(promise)
            .await
            .map_err(|e| anyhow!("JS promise rejected: {:?}", e))?
    } else {
        result
    };

    // Convert JS value back to serde_json::Value
    let value: Value = serde_wasm_bindgen::from_value(js_result).unwrap_or(Value::Null);
    Ok(value)
}
