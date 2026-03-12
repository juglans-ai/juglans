// src/core/types.rs
//
// Juglans type system: supports static type checking and AOT compilation

use std::fmt;

/// Juglans type
#[derive(Debug, Clone, PartialEq)]
pub enum JType {
    /// Integer (i64)
    Int,
    /// Float (f64)
    Float,
    /// String
    Str,
    /// Boolean
    Bool,
    /// List with element type
    List(Box<JType>),
    /// Dict with key-value types
    Dict(Box<JType>, Box<JType>),
    /// Optional type (int? = int | null)
    Optional(Box<JType>),
    /// Custom class
    Class(String),
    /// Unannotated type (dynamic), backward compatible
    Any,
}

impl JType {
    /// Parse a type_hint string into JType
    /// Supports: int, float, str, bool, list[T], dict[K,V], T?, ClassName
    pub fn parse(hint: &str) -> Self {
        let hint = hint.trim();

        // Optional: "int?", "str?"
        if let Some(inner) = hint.strip_suffix('?') {
            return JType::Optional(Box::new(JType::parse(inner)));
        }

        // Generic: "list[int]", "dict[str, int]"
        if let Some(bracket_start) = hint.find('[') {
            let base = &hint[..bracket_start];
            let inner = hint
                .strip_suffix(']')
                .unwrap_or(hint)
                .get(bracket_start + 1..)
                .unwrap_or(""); // strip [ ]

            match base {
                "list" => {
                    return JType::List(Box::new(JType::parse(inner)));
                }
                "dict" => {
                    // dict[str, int] → split on first comma
                    if let Some(comma) = find_top_level_comma(inner) {
                        let key = &inner[..comma];
                        let val = &inner[comma + 1..];
                        return JType::Dict(
                            Box::new(JType::parse(key)),
                            Box::new(JType::parse(val)),
                        );
                    }
                    // fallback: dict with Any values
                    return JType::Dict(Box::new(JType::parse(inner)), Box::new(JType::Any));
                }
                _ => {} // unknown generic, fall through to Class
            }
        }

        // Primitive types
        match hint {
            "int" => JType::Int,
            "float" => JType::Float,
            "str" => JType::Str,
            "bool" => JType::Bool,
            "any" => JType::Any,
            "list" => JType::List(Box::new(JType::Any)),
            "dict" => JType::Dict(Box::new(JType::Str), Box::new(JType::Any)),
            _ => {
                // Capitalized → class name, otherwise Any
                if hint.chars().next().is_some_and(|c| c.is_uppercase()) {
                    JType::Class(hint.to_string())
                } else {
                    JType::Any
                }
            }
        }
    }

    /// Parse from Option<String> (None → Any)
    pub fn from_hint(hint: &Option<String>) -> Self {
        match hint {
            Some(s) => JType::parse(s),
            None => JType::Any,
        }
    }

    /// Whether this is Any (unannotated type)
    pub fn is_any(&self) -> bool {
        matches!(self, JType::Any)
    }

    /// Whether this is a numeric type
    #[allow(dead_code)]
    pub fn is_numeric(&self) -> bool {
        matches!(self, JType::Int | JType::Float)
    }

    /// Check if value_type can be assigned to self
    pub fn accepts(&self, value_type: &JType) -> bool {
        if self == value_type {
            return true;
        }
        // Any accepts everything
        if self.is_any() || value_type.is_any() {
            return true;
        }
        // Optional<T> accepts T and Null
        if let JType::Optional(inner) = self {
            return inner.accepts(value_type);
        }
        // Int accepts Float (numeric coercion)
        if matches!(self, JType::Float) && matches!(value_type, JType::Int) {
            return true;
        }
        // List covariance
        if let (JType::List(a), JType::List(b)) = (self, value_type) {
            return a.accepts(b);
        }
        false
    }

    /// Return the corresponding Rust type name
    #[allow(dead_code)]
    pub fn rust_type(&self) -> String {
        match self {
            JType::Int => "i64".to_string(),
            JType::Float => "f64".to_string(),
            JType::Str => "String".to_string(),
            JType::Bool => "bool".to_string(),
            JType::List(inner) => format!("Vec<{}>", inner.rust_type()),
            JType::Dict(k, v) => format!("HashMap<{}, {}>", k.rust_type(), v.rust_type()),
            JType::Optional(inner) => format!("Option<{}>", inner.rust_type()),
            JType::Class(name) => name.clone(),
            JType::Any => "Value".to_string(),
        }
    }
}

impl fmt::Display for JType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JType::Int => write!(f, "int"),
            JType::Float => write!(f, "float"),
            JType::Str => write!(f, "str"),
            JType::Bool => write!(f, "bool"),
            JType::List(inner) => write!(f, "list[{}]", inner),
            JType::Dict(k, v) => write!(f, "dict[{}, {}]", k, v),
            JType::Optional(inner) => write!(f, "{}?", inner),
            JType::Class(name) => write!(f, "{}", name),
            JType::Any => write!(f, "any"),
        }
    }
}

/// Find the first top-level comma (not inside brackets)
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_primitives() {
        assert_eq!(JType::parse("int"), JType::Int);
        assert_eq!(JType::parse("float"), JType::Float);
        assert_eq!(JType::parse("str"), JType::Str);
        assert_eq!(JType::parse("bool"), JType::Bool);
        assert_eq!(JType::parse("any"), JType::Any);
    }

    #[test]
    fn test_parse_generics() {
        assert_eq!(JType::parse("list[int]"), JType::List(Box::new(JType::Int)));
        assert_eq!(
            JType::parse("dict[str, int]"),
            JType::Dict(Box::new(JType::Str), Box::new(JType::Int))
        );
        assert_eq!(
            JType::parse("list[list[str]]"),
            JType::List(Box::new(JType::List(Box::new(JType::Str))))
        );
    }

    #[test]
    fn test_parse_optional() {
        assert_eq!(JType::parse("int?"), JType::Optional(Box::new(JType::Int)));
        assert_eq!(JType::parse("str?"), JType::Optional(Box::new(JType::Str)));
    }

    #[test]
    fn test_parse_class() {
        assert_eq!(JType::parse("Counter"), JType::Class("Counter".to_string()));
        assert_eq!(
            JType::parse("UserRequest"),
            JType::Class("UserRequest".to_string())
        );
    }

    #[test]
    fn test_from_hint() {
        assert_eq!(JType::from_hint(&Some("int".to_string())), JType::Int);
        assert_eq!(JType::from_hint(&None), JType::Any);
    }

    #[test]
    fn test_accepts() {
        assert!(JType::Int.accepts(&JType::Int));
        assert!(JType::Float.accepts(&JType::Int)); // numeric coercion
        assert!(!JType::Int.accepts(&JType::Str));
        assert!(JType::Any.accepts(&JType::Int));
        assert!(JType::Int.accepts(&JType::Any));
        assert!(JType::Optional(Box::new(JType::Int)).accepts(&JType::Int));
    }

    #[test]
    fn test_rust_type() {
        assert_eq!(JType::Int.rust_type(), "i64");
        assert_eq!(JType::Str.rust_type(), "String");
        assert_eq!(JType::List(Box::new(JType::Int)).rust_type(), "Vec<i64>");
        assert_eq!(
            JType::Optional(Box::new(JType::Str)).rust_type(),
            "Option<String>"
        );
    }

    #[test]
    fn test_display() {
        assert_eq!(JType::Int.to_string(), "int");
        assert_eq!(JType::List(Box::new(JType::Str)).to_string(), "list[str]");
        assert_eq!(JType::Optional(Box::new(JType::Int)).to_string(), "int?");
        assert_eq!(
            JType::Dict(Box::new(JType::Str), Box::new(JType::Int)).to_string(),
            "dict[str, int]"
        );
    }
}
