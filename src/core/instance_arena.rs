// src/core/instance_arena.rs
//
// InstanceArena: High-performance class instance storage, detached from JSON tree
// Phase B: TypedSlot — native type storage, eliminating serde_json::Value overhead
// Method calls achieve zero-lock field access via MethodScope + field_cache

use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use super::graph::ClassDef;

/// Native type storage slot, replacing serde_json::Value
/// Int/Float/Bool are Copy (~0ns clone), Str uses Arc<str> (~2ns clone)
#[derive(Debug, Clone)]
pub enum TypedSlot {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Arc<str>),
    /// Fallback: complex types or unannotated types
    Json(Arc<Value>),
    Null,
}

impl TypedSlot {
    /// Smart conversion from serde_json::Value (auto-infers native type)
    pub fn from_value(val: Value) -> Self {
        match &val {
            Value::Null => TypedSlot::Null,
            Value::Bool(b) => TypedSlot::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    TypedSlot::Int(i)
                } else if let Some(f) = n.as_f64() {
                    TypedSlot::Float(f)
                } else {
                    TypedSlot::Json(Arc::new(val))
                }
            }
            Value::String(s) => TypedSlot::Str(Arc::from(s.as_str())),
            // Array/Object → Json fallback
            _ => TypedSlot::Json(Arc::new(val)),
        }
    }

    // ============================================================
    // TypedSlot arithmetic / comparison / logical operations (zero-alloc fast path)
    // ============================================================

    /// Python-like truthiness
    pub fn is_truthy(&self) -> bool {
        match self {
            TypedSlot::Null => false,
            TypedSlot::Bool(b) => *b,
            TypedSlot::Int(n) => *n != 0,
            TypedSlot::Float(f) => *f != 0.0,
            TypedSlot::Str(s) => !s.is_empty(),
            TypedSlot::Json(v) => match v.as_ref() {
                Value::Null => false,
                Value::Bool(b) => *b,
                Value::Array(a) => !a.is_empty(),
                Value::Object(o) => !o.is_empty(),
                Value::String(s) => !s.is_empty(),
                Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
            },
        }
    }

    /// Convert to f64 (for arithmetic operations)
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            TypedSlot::Int(n) => Some(*n as f64),
            TypedSlot::Float(f) => Some(*f),
            TypedSlot::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            TypedSlot::Str(s) => s.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Addition (Int+Int→Int, Float mixed→Float, Str+Str→Str, Array+Array→Array)
    pub fn add(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => Some(TypedSlot::Int(a.wrapping_add(*b))),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(a + b)),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(*a as f64 + b)),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => Some(TypedSlot::Float(a + *b as f64)),
            (TypedSlot::Str(a), TypedSlot::Str(b)) => {
                let mut s = String::with_capacity(a.len() + b.len());
                s.push_str(a);
                s.push_str(b);
                Some(TypedSlot::Str(Arc::from(s.as_str())))
            }
            // Array + Array → Array concatenation (both stored as Json)
            (TypedSlot::Json(a), TypedSlot::Json(b)) => {
                if let (Value::Array(arr_a), Value::Array(arr_b)) = (a.as_ref(), b.as_ref()) {
                    let mut result = arr_a.clone();
                    result.extend(arr_b.iter().cloned());
                    Some(TypedSlot::Json(Arc::new(Value::Array(result))))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Subtraction
    pub fn sub(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => Some(TypedSlot::Int(a.wrapping_sub(*b))),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(a - b)),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(*a as f64 - b)),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => Some(TypedSlot::Float(a - *b as f64)),
            _ => None,
        }
    }

    /// Multiplication
    pub fn mul(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => Some(TypedSlot::Int(a.wrapping_mul(*b))),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(a * b)),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(*a as f64 * b)),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => Some(TypedSlot::Float(a * *b as f64)),
            _ => None,
        }
    }

    /// Division (returns None on divide-by-zero)
    pub fn div(&self, other: &TypedSlot) -> Option<TypedSlot> {
        let r = other.as_f64()?;
        if r == 0.0 {
            return None;
        }
        let l = self.as_f64()?;
        let result = l / r;
        // Two Ints dividing evenly return Int
        if matches!((self, other), (TypedSlot::Int(_), TypedSlot::Int(_)))
            && result.fract() == 0.0
            && result.abs() < (i64::MAX as f64)
        {
            Some(TypedSlot::Int(result as i64))
        } else {
            Some(TypedSlot::Float(result))
        }
    }

    /// Modulo
    pub fn modulo(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) if *b != 0 => Some(TypedSlot::Int(a % b)),
            _ => {
                let r = other.as_f64()?;
                if r == 0.0 {
                    return None;
                }
                Some(TypedSlot::Float(self.as_f64()? % r))
            }
        }
    }

    /// Negation
    pub fn neg(&self) -> Option<TypedSlot> {
        match self {
            TypedSlot::Int(n) => Some(TypedSlot::Int(-n)),
            TypedSlot::Float(f) => Some(TypedSlot::Float(-f)),
            _ => None,
        }
    }

    /// Equality comparison (cross-type compatible for Int/Float)
    pub fn typed_eq(&self, other: &TypedSlot) -> bool {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => a == b,
            (TypedSlot::Float(a), TypedSlot::Float(b)) => a == b,
            (TypedSlot::Int(a), TypedSlot::Float(b)) => (*a as f64) == *b,
            (TypedSlot::Float(a), TypedSlot::Int(b)) => *a == (*b as f64),
            (TypedSlot::Bool(a), TypedSlot::Bool(b)) => a == b,
            (TypedSlot::Str(a), TypedSlot::Str(b)) => a == b,
            (TypedSlot::Null, TypedSlot::Null) => true,
            _ => false,
        }
    }

    /// Numeric comparison (< > <= >=), returns None if incomparable
    pub fn partial_cmp_numeric(&self, other: &TypedSlot) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => a.partial_cmp(b),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => a.partial_cmp(b),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => (*a as f64).partial_cmp(b),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => a.partial_cmp(&(*b as f64)),
            (TypedSlot::Str(a), TypedSlot::Str(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// Convert to serde_json::Value (boundary materialization)
    pub fn to_value(&self) -> Value {
        match self {
            TypedSlot::Int(n) => Value::Number((*n).into()),
            TypedSlot::Float(f) => serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            TypedSlot::Bool(b) => Value::Bool(*b),
            TypedSlot::Str(s) => Value::String(s.to_string()),
            TypedSlot::Json(v) => (**v).clone(),
            TypedSlot::Null => Value::Null,
        }
    }
}

/// Unique instance identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstanceId(pub u64);

/// A single instance in the arena
#[derive(Debug, Clone)]
pub struct ArenaInstance {
    pub class_name: String,
    pub class_def: Arc<ClassDef>,
    /// Field values, stored in ClassDef.field_index order (native types)
    pub fields: Vec<TypedSlot>,
}

impl ArenaInstance {
    /// Materialize to serde_json::Value (for $output, serialization, etc.)
    #[allow(dead_code)]
    pub fn to_value(&self) -> Value {
        let fields_json: Vec<Value> = self.fields.iter().map(|s| s.to_value()).collect();
        json!({
            "__class__": self.class_name,
            "__fields__": fields_json,
        })
    }
}

/// Thread-safe instance arena
#[derive(Debug, Clone)]
pub struct InstanceArena {
    instances: Arc<RwLock<HashMap<InstanceId, ArenaInstance>>>,
    next_id: Arc<RwLock<u64>>,
    /// variable_name → InstanceId mapping
    name_map: Arc<RwLock<HashMap<String, InstanceId>>>,
}

impl Default for InstanceArena {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceArena {
    pub fn new() -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(0)),
            name_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Allocate a new instance, returns InstanceId
    /// fields still accepts Vec<Value> (boundary conversion), stored internally as Vec<TypedSlot>
    pub fn alloc(
        &self,
        name: String,
        class_name: String,
        class_def: Arc<ClassDef>,
        fields: Vec<Value>,
    ) -> InstanceId {
        let id = {
            let mut next = self.next_id.write();
            let id = InstanceId(*next);
            *next += 1;
            id
        };

        let typed_fields: Vec<TypedSlot> = fields.into_iter().map(TypedSlot::from_value).collect();

        let instance = ArenaInstance {
            class_name,
            class_def,
            fields: typed_fields,
        };

        self.instances.write().insert(id, instance);
        self.name_map.write().insert(name, id);
        id
    }

    /// Look up InstanceId by name
    pub fn lookup_by_name(&self, name: &str) -> Option<InstanceId> {
        self.name_map.read().get(name).copied()
    }

    /// Read a single field value (boundary-converted to Value)
    pub fn get_field(&self, id: InstanceId, field_idx: usize) -> Option<Value> {
        let guard = self.instances.read();
        let inst = guard.get(&id)?;
        inst.fields.get(field_idx).map(|s| s.to_value())
    }

    /// Snapshot all fields as TypedSlot Vec (for MethodScope field_cache)
    /// Single lock acquisition, then zero-lock access
    pub fn snapshot_fields(&self, id: InstanceId) -> Option<Vec<TypedSlot>> {
        let guard = self.instances.read();
        let inst = guard.get(&id)?;
        Some(inst.fields.clone())
    }

    /// Modify a single field in-place
    #[allow(dead_code)]
    pub fn set_field(&self, id: InstanceId, field_idx: usize, value: Value) {
        let mut guard = self.instances.write();
        if let Some(inst) = guard.get_mut(&id) {
            if field_idx < inst.fields.len() {
                inst.fields[field_idx] = TypedSlot::from_value(value);
            }
        }
    }

    /// Batch-write fields (single lock acquisition, Value → TypedSlot conversion)
    pub fn set_fields_batch(&self, id: InstanceId, updates: &[(usize, Value)]) {
        let mut guard = self.instances.write();
        if let Some(inst) = guard.get_mut(&id) {
            for (idx, val) in updates {
                if *idx < inst.fields.len() {
                    inst.fields[*idx] = TypedSlot::from_value(val.clone());
                }
            }
        }
    }

    /// Materialize instance to serde_json::Value
    pub fn materialize(&self, id: InstanceId) -> Option<Value> {
        let guard = self.instances.read();
        let inst = guard.get(&id)?;
        let fields_json: Vec<Value> = inst.fields.iter().map(|s| s.to_value()).collect();
        Some(json!({
            "__arena_ref__": id.0,
            "__class__": inst.class_name,
            "__fields__": fields_json,
        }))
    }

    /// Materialize instance, merging dirty fields
    pub fn materialize_with_dirty(
        &self,
        id: InstanceId,
        dirty: &HashMap<String, Value>,
    ) -> Option<Value> {
        let guard = self.instances.read();
        let inst = guard.get(&id)?;
        let mut fields_json: Vec<Value> = inst.fields.iter().map(|s| s.to_value()).collect();
        for (name, val) in dirty {
            if let Some(&idx) = inst.class_def.field_index.get(name.as_str()) {
                if idx < fields_json.len() {
                    fields_json[idx] = val.clone();
                }
            }
        }
        Some(json!({
            "__arena_ref__": id.0,
            "__class__": inst.class_name,
            "__fields__": fields_json,
        }))
    }

    /// Get the class name of an instance
    pub fn class_name(&self, id: InstanceId) -> Option<String> {
        let guard = self.instances.read();
        guard.get(&id).map(|inst| inst.class_name.clone())
    }

    /// Get the ClassDef of an instance
    pub fn class_def(&self, id: InstanceId) -> Option<Arc<ClassDef>> {
        let guard = self.instances.read();
        guard.get(&id).map(|inst| Arc::clone(&inst.class_def))
    }
}

/// Method execution scope: tracks current instance and field modifications
/// field_cache is snapshotted at scope entry, enabling zero-lock reads in method body
/// Phase C-2: field_cache syncs with dirty writes, ResolvedField direct indexing always correct
#[derive(Debug, Clone)]
pub struct MethodScope {
    pub instance_id: InstanceId,
    pub class_def: Arc<ClassDef>,
    #[allow(dead_code)]
    pub instance_path: String,
    /// Fields modified within method body (field_name → new_value)
    pub dirty: HashMap<String, Value>,
    /// Field snapshot at scope entry (zero-lock reads) — synced with dirty writes
    pub field_cache: Vec<TypedSlot>,
    /// Method parameter names (in FunctionDef.params order)
    pub method_params: Vec<String>,
    /// Method parameter TypedSlot values (in method_params order)
    pub param_values: Vec<TypedSlot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::{ClassDef, ClassField, FunctionDef};

    fn make_class_def(n: usize) -> Arc<ClassDef> {
        let fields: Vec<ClassField> = (0..n)
            .map(|i| ClassField {
                name: format!("field_{}", i),
                type_hint: None,
                default: None,
            })
            .collect();
        Arc::new(ClassDef::new(fields, HashMap::new()))
    }

    #[test]
    fn test_typed_slot_roundtrip() {
        // Int
        let slot = TypedSlot::from_value(json!(42));
        assert!(matches!(slot, TypedSlot::Int(42)));
        assert_eq!(slot.to_value(), json!(42));

        // Float
        let slot = TypedSlot::from_value(json!(3.14));
        assert!(matches!(slot, TypedSlot::Float(_)));
        assert_eq!(slot.to_value(), json!(3.14));

        // Bool
        let slot = TypedSlot::from_value(json!(true));
        assert!(matches!(slot, TypedSlot::Bool(true)));
        assert_eq!(slot.to_value(), json!(true));

        // Str
        let slot = TypedSlot::from_value(json!("hello"));
        assert!(matches!(slot, TypedSlot::Str(_)));
        assert_eq!(slot.to_value(), json!("hello"));

        // Null
        let slot = TypedSlot::from_value(Value::Null);
        assert!(matches!(slot, TypedSlot::Null));
        assert_eq!(slot.to_value(), Value::Null);

        // Array → Json fallback
        let slot = TypedSlot::from_value(json!([1, 2, 3]));
        assert!(matches!(slot, TypedSlot::Json(_)));
        assert_eq!(slot.to_value(), json!([1, 2, 3]));

        // Object → Json fallback
        let slot = TypedSlot::from_value(json!({"a": 1}));
        assert!(matches!(slot, TypedSlot::Json(_)));
        assert_eq!(slot.to_value(), json!({"a": 1}));
    }

    #[test]
    fn test_arena_alloc_and_lookup() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(3);
        let fields = vec![json!("a"), json!(1), json!(true)];

        let id = arena.alloc(
            "my_inst".to_string(),
            "MyClass".to_string(),
            class_def,
            fields,
        );

        assert_eq!(arena.lookup_by_name("my_inst"), Some(id));
        assert_eq!(arena.lookup_by_name("nonexistent"), None);
        assert_eq!(arena.class_name(id), Some("MyClass".to_string()));
    }

    #[test]
    fn test_arena_field_access() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(3);
        let fields = vec![json!("hello"), json!(42), json!(3.14)];

        let id = arena.alloc("inst".to_string(), "C".to_string(), class_def, fields);

        assert_eq!(arena.get_field(id, 0), Some(json!("hello")));
        assert_eq!(arena.get_field(id, 1), Some(json!(42)));
        assert_eq!(arena.get_field(id, 2), Some(json!(3.14)));
        assert_eq!(arena.get_field(id, 99), None);
    }

    #[test]
    fn test_arena_set_field() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(2);
        let fields = vec![json!("old"), json!(0)];

        let id = arena.alloc("inst".to_string(), "C".to_string(), class_def, fields);

        arena.set_field(id, 0, json!("new"));
        assert_eq!(arena.get_field(id, 0), Some(json!("new")));

        arena.set_field(id, 1, json!(99));
        assert_eq!(arena.get_field(id, 1), Some(json!(99)));
    }

    #[test]
    fn test_arena_materialize() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(2);
        let fields = vec![json!("val"), json!(1)];

        let id = arena.alloc("inst".to_string(), "C".to_string(), class_def, fields);

        let val = arena.materialize(id).unwrap();
        assert_eq!(val["__class__"], json!("C"));
        assert_eq!(val["__fields__"][0], json!("val"));
        assert_eq!(val["__fields__"][1], json!(1));
    }

    #[test]
    fn test_arena_materialize_with_dirty() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(2);
        let fields = vec![json!("original"), json!(0)];

        let id = arena.alloc(
            "inst".to_string(),
            "C".to_string(),
            Arc::clone(&class_def),
            fields,
        );

        let mut dirty = HashMap::new();
        dirty.insert("field_0".to_string(), json!("modified"));

        let val = arena.materialize_with_dirty(id, &dirty).unwrap();
        assert_eq!(val["__fields__"][0], json!("modified"));
        assert_eq!(val["__fields__"][1], json!(0));
    }

    #[test]
    fn test_arena_set_fields_batch() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(3);
        let fields = vec![json!(0), json!(0), json!(0)];

        let id = arena.alloc("inst".to_string(), "C".to_string(), class_def, fields);

        arena.set_fields_batch(id, &[(0, json!(10)), (2, json!(30))]);
        assert_eq!(arena.get_field(id, 0), Some(json!(10)));
        assert_eq!(arena.get_field(id, 1), Some(json!(0)));
        assert_eq!(arena.get_field(id, 2), Some(json!(30)));
    }

    #[test]
    fn test_arena_snapshot_fields() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(3);
        let fields = vec![json!("hello"), json!(42), json!(true)];

        let id = arena.alloc("inst".to_string(), "C".to_string(), class_def, fields);

        let snapshot = arena.snapshot_fields(id).unwrap();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].to_value(), json!("hello"));
        assert_eq!(snapshot[1].to_value(), json!(42));
        assert_eq!(snapshot[2].to_value(), json!(true));

        // Snapshot is independent — modifying arena doesn't affect it
        arena.set_field(id, 0, json!("changed"));
        assert_eq!(snapshot[0].to_value(), json!("hello")); // still original
    }

    #[test]
    fn test_field_cache_in_method_scope() {
        let arena = InstanceArena::new();
        let class_def = make_class_def(3);
        let fields = vec![json!("a"), json!(100), json!(true)];

        let id = arena.alloc(
            "inst".to_string(),
            "C".to_string(),
            Arc::clone(&class_def),
            fields,
        );

        // Snapshot for scope (single lock acquisition)
        let cache = arena.snapshot_fields(id).unwrap();

        let scope = MethodScope {
            instance_id: id,
            class_def,
            instance_path: "inst".to_string(),
            dirty: HashMap::new(),
            field_cache: cache,
            method_params: Vec::new(),
            param_values: Vec::new(),
        };

        // Zero-lock field reads from cache
        assert_eq!(scope.field_cache[0].to_value(), json!("a"));
        assert_eq!(scope.field_cache[1].to_value(), json!(100));
        assert_eq!(scope.field_cache[2].to_value(), json!(true));
    }
}
