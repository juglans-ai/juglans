use serde_json::Value;

/// Lightweight wrapper around `serde_json::Value` for ergonomic chainable access.
///
/// Key design: **never panics**. Missing paths return `JValue(Value::Null)`.
///
/// ```
/// use serde_json::json;
/// use juglans::core::jvalue::JValue;
///
/// let v = JValue::from(json!({"user": {"name": "Alice", "age": 30}}));
/// assert_eq!(v.path("user.name").str(), Some("Alice"));
/// assert_eq!(v.path("user.age").i64(), Some(30));
/// assert_eq!(v.path("user.missing").str(), None);
/// assert_eq!(v.path("user.missing").str_or("default"), "default");
/// ```
pub struct JValue(pub Value);

impl JValue {
    /// Access a nested value via dot-notation path (e.g. `"a.b.c"`).
    pub fn path(&self, dotted: &str) -> JValue {
        let mut current = &self.0;
        for part in dotted.split('.') {
            match current.get(part) {
                Some(v) => current = v,
                None => return JValue(Value::Null),
            }
        }
        JValue(current.clone())
    }

    /// Access a single key in an object.
    pub fn get(&self, key: &str) -> JValue {
        match self.0.get(key) {
            Some(v) => JValue(v.clone()),
            None => JValue(Value::Null),
        }
    }

    /// Access an array element by index.
    pub fn idx(&self, i: usize) -> JValue {
        match self.0.get(i) {
            Some(v) => JValue(v.clone()),
            None => JValue(Value::Null),
        }
    }

    /// Extract as `&str` (borrowed from inner Value).
    pub fn str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// Extract as `&str` with a default fallback.
    pub fn str_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.0.as_str().unwrap_or(default)
    }

    /// Extract as owned `String`.
    pub fn string(&self) -> Option<String> {
        self.0.as_str().map(|s| s.to_string())
    }

    /// Extract as `i64`.
    pub fn i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    /// Extract as `f64`.
    pub fn f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    /// Extract as `bool`.
    pub fn bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    /// Extract as array slice.
    pub fn array(&self) -> Option<&Vec<Value>> {
        self.0.as_array()
    }

    /// Check if the inner value is null (including missing paths).
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// Consume and return the inner `Value`.
    pub fn into_inner(self) -> Value {
        self.0
    }

    /// Borrow the inner `Value`.
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

impl From<Value> for JValue {
    fn from(v: Value) -> Self {
        JValue(v)
    }
}

impl From<JValue> for Value {
    fn from(j: JValue) -> Self {
        j.0
    }
}

impl From<Option<Value>> for JValue {
    fn from(opt: Option<Value>) -> Self {
        JValue(opt.unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_path_nested() {
        let v = JValue::from(json!({"a": {"b": {"c": 42}}}));
        assert_eq!(v.path("a.b.c").i64(), Some(42));
    }

    #[test]
    fn test_path_missing_returns_null() {
        let v = JValue::from(json!({"a": 1}));
        assert!(v.path("x.y.z").is_null());
    }

    #[test]
    fn test_str_and_str_or() {
        let v = JValue::from(json!({"name": "Alice"}));
        assert_eq!(v.get("name").str(), Some("Alice"));
        assert_eq!(v.get("name").str_or("?"), "Alice");
        assert_eq!(v.get("missing").str(), None);
        assert_eq!(v.get("missing").str_or("default"), "default");
    }

    #[test]
    fn test_string_owned() {
        let v = JValue::from(json!({"k": "hello"}));
        let s: Option<String> = v.get("k").string();
        assert_eq!(s, Some("hello".to_string()));
    }

    #[test]
    fn test_numeric() {
        let v = JValue::from(json!({"i": 10, "f": 3.14}));
        assert_eq!(v.get("i").i64(), Some(10));
        assert_eq!(v.get("f").f64(), Some(3.14));
    }

    #[test]
    fn test_bool() {
        let v = JValue::from(json!({"flag": true}));
        assert_eq!(v.get("flag").bool(), Some(true));
        assert_eq!(v.get("missing").bool(), None);
    }

    #[test]
    fn test_array_and_idx() {
        let v = JValue::from(json!({"items": [10, 20, 30]}));
        assert_eq!(v.get("items").array().map(|a| a.len()), Some(3));
        assert_eq!(v.get("items").idx(1).i64(), Some(20));
        assert!(v.get("items").idx(99).is_null());
    }

    #[test]
    fn test_from_none() {
        let v = JValue::from(None::<Value>);
        assert!(v.is_null());
        assert_eq!(v.str_or("fallback"), "fallback");
    }

    #[test]
    fn test_into_inner() {
        let original = json!({"x": 1});
        let j = JValue::from(original.clone());
        assert_eq!(j.into_inner(), original);
    }
}
