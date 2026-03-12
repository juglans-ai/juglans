// src/core/type_checker.rs
//
// Type checker: performs static type analysis on WorkflowGraph
// - Checks completeness of class field type annotations
// - Infers method body expression types
// - Validates assignment type compatibility

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::graph::{ClassDef, WorkflowGraph};
use crate::core::types::JType;

/// Type error
#[derive(Debug, Clone)]
pub struct TypeError {
    pub class_name: String,
    pub field_or_method: String,
    pub message: String,
}

/// Type warning
#[derive(Debug, Clone)]
pub struct TypeWarning {
    pub class_name: String,
    pub field_or_method: String,
    pub message: String,
}

/// Type check result
#[derive(Debug, Clone, Default)]
pub struct TypeCheckResult {
    pub errors: Vec<TypeError>,
    pub warnings: Vec<TypeWarning>,
}

#[allow(dead_code)]
impl TypeCheckResult {
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Build mode: warnings are also treated as errors
    pub fn is_build_ready(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }
}

impl std::fmt::Display for TypeCheckResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for w in &self.warnings {
            writeln!(
                f,
                "  warning: class '{}' field '{}': {}",
                w.class_name, w.field_or_method, w.message
            )?;
        }
        for e in &self.errors {
            writeln!(
                f,
                "  error: class '{}' {}: {}",
                e.class_name, e.field_or_method, e.message
            )?;
        }
        Ok(())
    }
}

/// Type scope: variable types visible within a method body
struct TypeScope {
    /// Field types (class level)
    fields: HashMap<String, JType>,
    /// Local variable/parameter types (method level)
    locals: HashMap<String, JType>,
}

impl TypeScope {
    fn lookup(&self, name: &str) -> JType {
        if let Some(t) = self.locals.get(name) {
            return t.clone();
        }
        if let Some(t) = self.fields.get(name) {
            return t.clone();
        }
        JType::Any
    }
}

/// Type checker
#[derive(Default)]
pub struct TypeChecker {
    result: TypeCheckResult,
    /// Class definition registry (for resolving Class type references)
    class_registry: HashMap<String, Arc<ClassDef>>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            result: TypeCheckResult::default(),
            class_registry: HashMap::new(),
        }
    }

    /// Perform type checking on the entire WorkflowGraph
    pub fn check(mut self, graph: &WorkflowGraph) -> TypeCheckResult {
        self.class_registry = graph.classes.clone();

        for (name, class_def) in &graph.classes {
            self.check_class(name, class_def);
        }

        self.result
    }

    /// Check a single class definition
    fn check_class(&mut self, name: &str, def: &ClassDef) {
        let mut field_types = HashMap::new();

        // 1. Check field type annotations
        for field in &def.fields {
            let jtype = JType::from_hint(&field.type_hint);

            if jtype.is_any() && field.type_hint.is_none() {
                self.result.warnings.push(TypeWarning {
                    class_name: name.to_string(),
                    field_or_method: field.name.clone(),
                    message: format!(
                        "field '{}' has no type annotation (add ': int', ': str', etc.)",
                        field.name
                    ),
                });
            }

            // Verify Class type reference exists
            if let JType::Class(ref class_name) = jtype {
                if !self.class_registry.contains_key(class_name) {
                    self.result.errors.push(TypeError {
                        class_name: name.to_string(),
                        field_or_method: field.name.clone(),
                        message: format!("type '{}' is not defined (unknown class)", class_name),
                    });
                }
            }

            // Simple compatibility check between type annotation and default value
            if let (Some(default_expr), false) = (&field.default, jtype.is_any()) {
                if let Some(inferred) = infer_literal_type(default_expr) {
                    if !jtype.accepts(&inferred) {
                        self.result.errors.push(TypeError {
                            class_name: name.to_string(),
                            field_or_method: field.name.clone(),
                            message: format!(
                                "default value has type '{}' but field '{}' expects '{}'",
                                inferred, field.name, jtype
                            ),
                        });
                    }
                }
            }

            field_types.insert(field.name.clone(), jtype);
        }

        // 2. Check methods
        for (method_name, method_def) in &def.methods {
            let mut scope = TypeScope {
                fields: field_types.clone(),
                locals: HashMap::new(),
            };

            // Method params have no type annotations yet, marked as Any
            for param in &method_def.params {
                scope.locals.insert(param.clone(), JType::Any);
            }

            // Method body type checking (future: traverse each node in method_def.body)
            // Current phase: method body is a WorkflowGraph, traverse AssignCall nodes for assignment checks
            self.check_method_body(name, method_name, &method_def.body, &scope);
        }
    }

    /// Check method body (AssignCall assignment type compatibility)
    fn check_method_body(
        &mut self,
        class_name: &str,
        method_name: &str,
        body: &WorkflowGraph,
        scope: &TypeScope,
    ) {
        use crate::core::graph::NodeType;

        for idx in body.graph.node_indices() {
            let node = &body.graph[idx];
            if let NodeType::AssignCall { var, action } = &node.node_type {
                // var is the target field name being assigned to
                let target_type = scope.lookup(var);

                // Simple type inference for action (only handles literals and simple expressions)
                if let Some(value_type) = infer_literal_type(&action.name) {
                    if !target_type.is_any() && !target_type.accepts(&value_type) {
                        self.result.errors.push(TypeError {
                            class_name: class_name.to_string(),
                            field_or_method: format!(
                                "method '{}', assignment to '{}'",
                                method_name, var
                            ),
                            message: format!(
                                "cannot assign '{}' value to field '{}' of type '{}'",
                                value_type, var, target_type
                            ),
                        });
                    }
                }
            }
        }
    }
}

/// Infer type from a literal string
fn infer_literal_type(expr: &str) -> Option<JType> {
    let expr = expr.trim();

    // String literal
    if (expr.starts_with('"') && expr.ends_with('"'))
        || (expr.starts_with('\'') && expr.ends_with('\''))
    {
        return Some(JType::Str);
    }

    // Boolean
    if expr == "true" || expr == "false" {
        return Some(JType::Bool);
    }

    // null
    if expr == "null" || expr == "none" || expr == "None" {
        return Some(JType::Optional(Box::new(JType::Any)));
    }

    // Integer
    if expr.parse::<i64>().is_ok() {
        return Some(JType::Int);
    }

    // Float
    if expr.parse::<f64>().is_ok() {
        return Some(JType::Float);
    }

    // List literal
    if expr.starts_with('[') && expr.ends_with(']') {
        return Some(JType::List(Box::new(JType::Any)));
    }

    // Object/dict literal
    if expr.starts_with('{') && expr.ends_with('}') {
        return Some(JType::Dict(Box::new(JType::Str), Box::new(JType::Any)));
    }

    // Complex expression → cannot infer
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::{ClassDef, ClassField, FunctionDef};

    fn make_field(name: &str, type_hint: Option<&str>, default: Option<&str>) -> ClassField {
        ClassField {
            name: name.to_string(),
            type_hint: type_hint.map(|s| s.to_string()),
            default: default.map(|s| s.to_string()),
        }
    }

    fn make_class(fields: Vec<ClassField>) -> Arc<ClassDef> {
        Arc::new(ClassDef::new(fields, HashMap::new()))
    }

    #[test]
    fn test_no_warnings_for_typed_fields() {
        let class_def = make_class(vec![
            make_field("count", Some("int"), Some("0")),
            make_field("name", Some("str"), Some("\"default\"")),
        ]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("Counter".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(!result.has_warnings());
        assert!(!result.has_errors());
    }

    #[test]
    fn test_warning_for_untyped_field() {
        let class_def = make_class(vec![
            make_field("count", None, Some("0")),
            make_field("name", Some("str"), None),
        ]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("Counter".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(result.has_warnings());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message.contains("count"));
        assert!(result.warnings[0].message.contains("no type annotation"));
    }

    #[test]
    fn test_error_for_type_mismatch_default() {
        let class_def = make_class(vec![make_field("count", Some("int"), Some(r#""hello""#))]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("Bad".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(result.has_errors(), "Expected errors, got: {:?}", result);
        assert!(
            result.errors[0].message.contains("expects 'int'"),
            "Got: {}",
            result.errors[0].message
        );
    }

    #[test]
    fn test_error_for_unknown_class_type() {
        let class_def = make_class(vec![make_field("item", Some("NonExistent"), None)]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("Holder".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(result.has_errors());
        assert!(result.errors[0].message.contains("not defined"));
    }

    #[test]
    fn test_build_ready() {
        let class_def = make_class(vec![make_field("x", Some("int"), Some("0"))]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("OK".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(result.is_build_ready());
    }

    #[test]
    fn test_not_build_ready_with_warnings() {
        let class_def = make_class(vec![make_field("x", None, Some("0"))]);

        let mut graph = WorkflowGraph::empty();
        graph.classes.insert("Untyped".to_string(), class_def);

        let result = TypeChecker::new().check(&graph);
        assert!(!result.is_build_ready());
    }

    #[test]
    fn test_infer_literal_type() {
        assert_eq!(infer_literal_type("42"), Some(JType::Int));
        assert_eq!(infer_literal_type("3.14"), Some(JType::Float));
        assert_eq!(infer_literal_type("\"hello\""), Some(JType::Str));
        assert_eq!(infer_literal_type("true"), Some(JType::Bool));
        assert_eq!(infer_literal_type("$self.count + 1"), None);
    }
}
