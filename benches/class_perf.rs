use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================
// Inline types (avoid pulling full crate which requires native features)
// ============================================================

#[derive(Debug, Clone)]
struct ClassField {
    name: String,
    #[allow(dead_code)]
    type_hint: Option<String>,
    #[allow(dead_code)]
    default: Option<String>,
}

#[derive(Debug, Clone)]
struct ClassDef {
    fields: Vec<ClassField>,
    field_index: HashMap<String, usize>,
}

impl ClassDef {
    fn new(fields: Vec<ClassField>) -> Self {
        let field_index = fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        Self {
            fields,
            field_index,
        }
    }
}

// ============================================================
// TypedSlot (Phase B: native type storage)
// ============================================================

#[derive(Debug, Clone)]
enum TypedSlot {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Arc<str>),
    Json(Arc<Value>),
    Null,
}

impl TypedSlot {
    fn from_value(val: Value) -> Self {
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
            _ => TypedSlot::Json(Arc::new(val)),
        }
    }

    fn to_value(&self) -> Value {
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

    fn add(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => Some(TypedSlot::Int(a.wrapping_add(*b))),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(a + b)),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(*a as f64 + b)),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => Some(TypedSlot::Float(a + *b as f64)),
            _ => None,
        }
    }

    fn mul(&self, other: &TypedSlot) -> Option<TypedSlot> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => Some(TypedSlot::Int(a.wrapping_mul(*b))),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(a * b)),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => Some(TypedSlot::Float(*a as f64 * b)),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => Some(TypedSlot::Float(a * *b as f64)),
            _ => None,
        }
    }

    fn typed_eq(&self, other: &TypedSlot) -> bool {
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

    fn partial_cmp_numeric(&self, other: &TypedSlot) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (TypedSlot::Int(a), TypedSlot::Int(b)) => a.partial_cmp(b),
            (TypedSlot::Float(a), TypedSlot::Float(b)) => a.partial_cmp(b),
            (TypedSlot::Int(a), TypedSlot::Float(b)) => (*a as f64).partial_cmp(b),
            (TypedSlot::Float(a), TypedSlot::Int(b)) => a.partial_cmp(&(*b as f64)),
            _ => None,
        }
    }
}

// ============================================================
// Helper: build a class def with N fields
// ============================================================

fn make_class_def(n: usize) -> Arc<ClassDef> {
    let fields: Vec<ClassField> = (0..n)
        .map(|i| ClassField {
            name: format!("field_{}", i),
            type_hint: None,
            default: None,
        })
        .collect();
    Arc::new(ClassDef::new(fields))
}

// ============================================================
// Bench: Instance creation (Phase 1-3 format: Vec-based __fields__)
// ============================================================

fn create_instance_vec(_class_def: &ClassDef, values: &[Value]) -> Value {
    let fields_vec: Vec<Value> = values.to_vec();
    json!({
        "__class__": "BenchClass",
        "__fields__": fields_vec,
    })
}

/// Old format with __field_index__ embedded (pre Phase 4+5)
fn create_instance_vec_with_index(class_def: &ClassDef, values: &[Value]) -> Value {
    let fields_vec: Vec<Value> = values.to_vec();
    json!({
        "__class__": "BenchClass",
        "__fields__": fields_vec,
        "__field_index__": class_def.field_index,
    })
}

/// Old format: HashMap-based instance (pre Phase 1)
fn create_instance_hashmap(class_def: &ClassDef, values: &[Value]) -> Value {
    let mut instance = serde_json::Map::new();
    instance.insert("__class__".to_string(), json!("BenchClass"));
    for (i, field) in class_def.fields.iter().enumerate() {
        instance.insert(field.name.clone(), values[i].clone());
    }
    Value::Object(instance)
}

// ============================================================
// Bench: Field access
// ============================================================

/// Current (Phase 4+5): class registry lookup
fn access_field_registry(instance: &Value, field: &str, class_def: &ClassDef) -> Value {
    if let Value::Object(map) = instance {
        if let Some(fields_arr) = map.get("__fields__").and_then(|v| v.as_array()) {
            if let Some(&idx) = class_def.field_index.get(field) {
                return fields_arr.get(idx).cloned().unwrap_or(Value::Null);
            }
        }
    }
    Value::Null
}

/// Old (Phase 1-3): __field_index__ in instance
fn access_field_embedded_index(instance: &Value, field: &str) -> Value {
    if let Value::Object(map) = instance {
        if let (Some(fields_arr), Some(index_map)) =
            (map.get("__fields__"), map.get("__field_index__"))
        {
            if let (Some(arr), Some(idx_obj)) = (fields_arr.as_array(), index_map.as_object()) {
                if let Some(idx_val) = idx_obj.get(field) {
                    if let Some(idx) = idx_val.as_u64() {
                        return arr.get(idx as usize).cloned().unwrap_or(Value::Null);
                    }
                }
            }
        }
    }
    Value::Null
}

/// Oldest (pre Phase 1): HashMap-based
fn access_field_hashmap(instance: &Value, field: &str) -> Value {
    if let Value::Object(map) = instance {
        return map.get(field).cloned().unwrap_or(Value::Null);
    }
    Value::Null
}

// ============================================================
// Bench: Method call simulation (bind fields + writeback)
// ============================================================

fn simulate_method_call_current(instance: &Value, class_def: &ClassDef) -> Value {
    // Read fields
    let map = instance.as_object().unwrap();
    let fields_arr = map.get("__fields__").unwrap().as_array().unwrap();
    let mut field_vals: Vec<Value> = Vec::with_capacity(class_def.fields.len());
    for (i, _field) in class_def.fields.iter().enumerate() {
        field_vals.push(fields_arr.get(i).cloned().unwrap_or(Value::Null));
    }

    // Simulate modifying field 0
    if !field_vals.is_empty() {
        field_vals[0] = json!("modified");
    }

    // Writeback
    json!({
        "__class__": "BenchClass",
        "__fields__": field_vals,
    })
}

fn simulate_method_call_old(instance: &Value, class_def: &ClassDef) -> Value {
    // Read fields
    let map = instance.as_object().unwrap();
    let fields_arr = map.get("__fields__").unwrap().as_array().unwrap();
    let mut field_vals: Vec<Value> = Vec::with_capacity(class_def.fields.len());
    for (i, _field) in class_def.fields.iter().enumerate() {
        field_vals.push(fields_arr.get(i).cloned().unwrap_or(Value::Null));
    }

    // Simulate modifying field 0
    if !field_vals.is_empty() {
        field_vals[0] = json!("modified");
    }

    // Writeback with __field_index__
    json!({
        "__class__": "BenchClass",
        "__fields__": field_vals,
        "__field_index__": class_def.field_index,
    })
}

fn simulate_method_call_hashmap(instance: &Value, class_def: &ClassDef) -> Value {
    let map = instance.as_object().unwrap();
    let mut new_map = serde_json::Map::new();
    new_map.insert("__class__".to_string(), json!("BenchClass"));
    for field in &class_def.fields {
        let val = map.get(&field.name).cloned().unwrap_or(Value::Null);
        new_map.insert(field.name.clone(), val);
    }
    // Modify field 0
    if let Some(f) = class_def.fields.first() {
        new_map.insert(f.name.clone(), json!("modified"));
    }
    Value::Object(new_map)
}

// ============================================================
// Rust native struct baseline
// ============================================================

#[derive(Clone)]
struct NativeStruct {
    field_0: String,
    field_1: i64,
    field_2: f64,
    field_3: String,
    field_4: bool,
}

// ============================================================
// Arena simulation (Phase 6: instances outside JSON tree)
// ============================================================

use std::sync::RwLock;

struct ArenaInstanceOld {
    #[allow(dead_code)]
    class_name: String,
    fields: Vec<Value>,
}

struct SimpleArenaOld {
    instances: RwLock<HashMap<u64, ArenaInstanceOld>>,
}

impl SimpleArenaOld {
    fn new() -> Self {
        Self {
            instances: RwLock::new(HashMap::new()),
        }
    }

    fn alloc(&self, id: u64, class_name: String, fields: Vec<Value>) {
        let inst = ArenaInstanceOld { class_name, fields };
        self.instances.write().unwrap().insert(id, inst);
    }

    fn get_field(&self, id: u64, idx: usize) -> Value {
        let guard = self.instances.read().unwrap();
        guard
            .get(&id)
            .and_then(|inst| inst.fields.get(idx).cloned())
            .unwrap_or(Value::Null)
    }

    fn materialize(&self, id: u64) -> Value {
        let guard = self.instances.read().unwrap();
        if let Some(inst) = guard.get(&id) {
            json!({
                "__class__": inst.class_name,
                "__fields__": inst.fields,
            })
        } else {
            Value::Null
        }
    }
}

/// Arena method call simulation (Phase 6): push scope → lazy reads → dirty writes → flush → materialize
fn simulate_method_call_arena_old(arena: &SimpleArenaOld, id: u64, class_def: &ClassDef) -> Value {
    let mut dirty: HashMap<String, Value> = HashMap::new();

    let _f0 = arena.get_field(id, 0);
    let _f2 = arena.get_field(id, 2);

    dirty.insert("field_0".to_string(), json!("modified"));

    {
        let mut guard = arena.instances.write().unwrap();
        if let Some(inst) = guard.get_mut(&id) {
            for (name, val) in &dirty {
                if let Some(&idx) = class_def.field_index.get(name.as_str()) {
                    if idx < inst.fields.len() {
                        inst.fields[idx] = val.clone();
                    }
                }
            }
        }
    }

    arena.materialize(id)
}

// ============================================================
// Phase B: TypedSlot arena simulation
// ============================================================

struct ArenaInstanceTyped {
    #[allow(dead_code)]
    class_name: String,
    fields: Vec<TypedSlot>,
}

struct TypedArena {
    instances: RwLock<HashMap<u64, ArenaInstanceTyped>>,
}

impl TypedArena {
    fn new() -> Self {
        Self {
            instances: RwLock::new(HashMap::new()),
        }
    }

    fn alloc(&self, id: u64, class_name: String, fields: Vec<Value>) {
        let typed_fields: Vec<TypedSlot> = fields.into_iter().map(TypedSlot::from_value).collect();
        let inst = ArenaInstanceTyped {
            class_name,
            fields: typed_fields,
        };
        self.instances.write().unwrap().insert(id, inst);
    }

    /// Snapshot fields (single lock, returns owned Vec<TypedSlot>)
    fn snapshot_fields(&self, id: u64) -> Vec<TypedSlot> {
        let guard = self.instances.read().unwrap();
        guard
            .get(&id)
            .map(|inst| inst.fields.clone())
            .unwrap_or_default()
    }

    /// Get field via lock (for comparison)
    fn get_field_value(&self, id: u64, idx: usize) -> Value {
        let guard = self.instances.read().unwrap();
        guard
            .get(&id)
            .and_then(|inst| inst.fields.get(idx))
            .map(|s| s.to_value())
            .unwrap_or(Value::Null)
    }

    fn materialize(&self, id: u64) -> Value {
        let guard = self.instances.read().unwrap();
        if let Some(inst) = guard.get(&id) {
            let fields_json: Vec<Value> = inst.fields.iter().map(|s| s.to_value()).collect();
            json!({
                "__class__": inst.class_name,
                "__fields__": fields_json,
            })
        } else {
            Value::Null
        }
    }
}

/// Phase B method call: snapshot → zero-lock reads → dirty → flush → materialize
fn simulate_method_call_typed(arena: &TypedArena, id: u64, class_def: &ClassDef) -> Value {
    // Snapshot fields (single lock)
    let cache = arena.snapshot_fields(id);
    let mut dirty: HashMap<String, Value> = HashMap::new();

    // Zero-lock field reads from cache
    let _f0 = cache[0].to_value();
    let _f2 = cache[2].to_value();

    // Dirty write
    dirty.insert("field_0".to_string(), json!("modified"));

    // Flush dirty to arena
    {
        let mut guard = arena.instances.write().unwrap();
        if let Some(inst) = guard.get_mut(&id) {
            for (name, val) in &dirty {
                if let Some(&idx) = class_def.field_index.get(name.as_str()) {
                    if idx < inst.fields.len() {
                        inst.fields[idx] = TypedSlot::from_value(val.clone());
                    }
                }
            }
        }
    }

    arena.materialize(id)
}

// ============================================================
// Criterion benchmarks
// ============================================================

fn bench_instance_creation(c: &mut Criterion) {
    let class_def = make_class_def(5);
    let values: Vec<Value> = vec![
        json!("hello"),
        json!(42),
        json!(3.14),
        json!("world"),
        json!(true),
    ];

    let mut group = c.benchmark_group("instance_creation");

    group.bench_function("current_vec_no_index", |b| {
        b.iter(|| create_instance_vec(black_box(&class_def), black_box(&values)))
    });

    group.bench_function("old_vec_with_index", |b| {
        b.iter(|| create_instance_vec_with_index(black_box(&class_def), black_box(&values)))
    });

    group.bench_function("oldest_hashmap", |b| {
        b.iter(|| create_instance_hashmap(black_box(&class_def), black_box(&values)))
    });

    group.bench_function("arena_old_value", |b| {
        let arena = SimpleArenaOld::new();
        let mut next_id = 0u64;
        b.iter(|| {
            let id = next_id;
            next_id += 1;
            arena.alloc(id, "BenchClass".to_string(), values.clone());
            black_box(id);
        })
    });

    group.bench_function("arena_typed_slot", |b| {
        let arena = TypedArena::new();
        let mut next_id = 0u64;
        b.iter(|| {
            let id = next_id;
            next_id += 1;
            arena.alloc(id, "BenchClass".to_string(), values.clone());
            black_box(id);
        })
    });

    group.bench_function("rust_native_struct", |b| {
        b.iter(|| {
            black_box(NativeStruct {
                field_0: "hello".to_string(),
                field_1: 42,
                field_2: 3.14,
                field_3: "world".to_string(),
                field_4: true,
            })
        })
    });

    group.finish();
}

fn bench_field_access(c: &mut Criterion) {
    let class_def = make_class_def(5);
    let values: Vec<Value> = vec![
        json!("hello"),
        json!(42),
        json!(3.14),
        json!("world"),
        json!(true),
    ];

    let instance_current = create_instance_vec(&class_def, &values);
    let instance_old = create_instance_vec_with_index(&class_def, &values);
    let instance_hashmap = create_instance_hashmap(&class_def, &values);

    let native = NativeStruct {
        field_0: "hello".to_string(),
        field_1: 42,
        field_2: 3.14,
        field_3: "world".to_string(),
        field_4: true,
    };

    let mut group = c.benchmark_group("field_access");

    group.bench_function("current_registry_lookup", |b| {
        b.iter(|| {
            access_field_registry(
                black_box(&instance_current),
                black_box("field_2"),
                black_box(&class_def),
            )
        })
    });

    group.bench_function("old_embedded_index", |b| {
        b.iter(|| access_field_embedded_index(black_box(&instance_old), black_box("field_2")))
    });

    group.bench_function("oldest_hashmap", |b| {
        b.iter(|| access_field_hashmap(black_box(&instance_hashmap), black_box("field_2")))
    });

    // Phase 6: arena with RwLock (Value-based)
    let arena_old = SimpleArenaOld::new();
    arena_old.alloc(0, "BenchClass".to_string(), values.clone());

    group.bench_function("arena_old_rwlock", |b| {
        b.iter(|| black_box(arena_old.get_field(0, 2)))
    });

    // Phase B: TypedSlot arena with RwLock
    let arena_typed = TypedArena::new();
    arena_typed.alloc(0, "BenchClass".to_string(), values.clone());

    group.bench_function("arena_typed_rwlock", |b| {
        b.iter(|| black_box(arena_typed.get_field_value(0, 2)))
    });

    // Phase B: field_cache snapshot (ZERO lock — the key win)
    let cache = arena_typed.snapshot_fields(0);

    group.bench_function("typed_cache_int", |b| {
        b.iter(|| black_box(cache[1].to_value())) // field_1 = 42 (Int)
    });

    group.bench_function("typed_cache_float", |b| {
        b.iter(|| black_box(cache[2].to_value())) // field_2 = 3.14 (Float)
    });

    group.bench_function("typed_cache_str", |b| {
        b.iter(|| black_box(cache[0].to_value())) // field_0 = "hello" (Str)
    });

    group.bench_function("typed_cache_bool", |b| {
        b.iter(|| black_box(cache[4].to_value())) // field_4 = true (Bool)
    });

    // Raw TypedSlot clone (no Value conversion — future ExprEval path)
    group.bench_function("typed_cache_raw_int_clone", |b| {
        b.iter(|| black_box(cache[1].clone())) // Int is Copy-like
    });

    group.bench_function("typed_cache_raw_str_clone", |b| {
        b.iter(|| black_box(cache[0].clone())) // Arc<str> clone ~2ns
    });

    group.bench_function("rust_native_struct", |b| {
        b.iter(|| black_box(native.field_2))
    });

    // Access all 5 fields
    group.bench_function("current_all_5_fields", |b| {
        b.iter(|| {
            for i in 0..5 {
                let field = format!("field_{}", i);
                black_box(access_field_registry(&instance_current, &field, &class_def));
            }
        })
    });

    group.bench_function("old_all_5_fields", |b| {
        b.iter(|| {
            for i in 0..5 {
                let field = format!("field_{}", i);
                black_box(access_field_embedded_index(&instance_old, &field));
            }
        })
    });

    group.bench_function("typed_cache_all_5_fields", |b| {
        b.iter(|| {
            for i in 0..5 {
                black_box(cache[i].to_value());
            }
        })
    });

    group.finish();
}

fn bench_method_call(c: &mut Criterion) {
    let class_def = make_class_def(5);
    let values: Vec<Value> = vec![
        json!("hello"),
        json!(42),
        json!(3.14),
        json!("world"),
        json!(true),
    ];

    let instance_current = create_instance_vec(&class_def, &values);
    let instance_old = create_instance_vec_with_index(&class_def, &values);
    let instance_hashmap = create_instance_hashmap(&class_def, &values);

    let mut group = c.benchmark_group("method_call_sim");

    group.bench_function("current_no_index", |b| {
        b.iter(|| simulate_method_call_current(black_box(&instance_current), black_box(&class_def)))
    });

    group.bench_function("old_with_index", |b| {
        b.iter(|| simulate_method_call_old(black_box(&instance_old), black_box(&class_def)))
    });

    group.bench_function("oldest_hashmap", |b| {
        b.iter(|| simulate_method_call_hashmap(black_box(&instance_hashmap), black_box(&class_def)))
    });

    // Phase 6: arena method call (Value-based)
    let arena_old = SimpleArenaOld::new();
    arena_old.alloc(0, "BenchClass".to_string(), values.clone());

    group.bench_function("arena_old_method_call", |b| {
        b.iter(|| {
            simulate_method_call_arena_old(
                black_box(&arena_old),
                black_box(0),
                black_box(&class_def),
            )
        })
    });

    // Phase B: TypedSlot arena method call
    let arena_typed = TypedArena::new();
    arena_typed.alloc(0, "BenchClass".to_string(), values.clone());

    group.bench_function("arena_typed_method_call", |b| {
        b.iter(|| {
            simulate_method_call_typed(black_box(&arena_typed), black_box(0), black_box(&class_def))
        })
    });

    group.finish();
}

fn bench_instance_clone(c: &mut Criterion) {
    let class_def = make_class_def(5);
    let values: Vec<Value> = vec![
        json!("hello"),
        json!(42),
        json!(3.14),
        json!("world"),
        json!(true),
    ];

    let instance_current = create_instance_vec(&class_def, &values);
    let instance_old = create_instance_vec_with_index(&class_def, &values);
    let instance_hashmap = create_instance_hashmap(&class_def, &values);

    let native = NativeStruct {
        field_0: "hello".to_string(),
        field_1: 42,
        field_2: 3.14,
        field_3: "world".to_string(),
        field_4: true,
    };

    // TypedSlot Vec clone (simulates field_cache snapshot)
    let typed_fields: Vec<TypedSlot> = values.iter().cloned().map(TypedSlot::from_value).collect();

    let mut group = c.benchmark_group("instance_clone");

    group.bench_function("current_no_index", |b| {
        b.iter(|| black_box(instance_current.clone()))
    });

    group.bench_function("old_with_index", |b| {
        b.iter(|| black_box(instance_old.clone()))
    });

    group.bench_function("oldest_hashmap", |b| {
        b.iter(|| black_box(instance_hashmap.clone()))
    });

    group.bench_function("typed_slot_vec_clone", |b| {
        b.iter(|| black_box(typed_fields.clone()))
    });

    group.bench_function("rust_native_struct", |b| {
        b.iter(|| black_box(native.clone()))
    });

    group.finish();
}

fn bench_typed_slot_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("typed_slot_conversion");

    // from_value benchmarks
    group.bench_function("from_value_int", |b| {
        b.iter(|| black_box(TypedSlot::from_value(json!(42))))
    });

    group.bench_function("from_value_float", |b| {
        b.iter(|| black_box(TypedSlot::from_value(json!(3.14))))
    });

    group.bench_function("from_value_str", |b| {
        b.iter(|| black_box(TypedSlot::from_value(json!("hello world"))))
    });

    group.bench_function("from_value_bool", |b| {
        b.iter(|| black_box(TypedSlot::from_value(json!(true))))
    });

    group.bench_function("from_value_array", |b| {
        b.iter(|| black_box(TypedSlot::from_value(json!([1, 2, 3]))))
    });

    // to_value benchmarks
    let slot_int = TypedSlot::Int(42);
    let slot_float = TypedSlot::Float(3.14);
    let slot_str = TypedSlot::Str(Arc::from("hello world"));
    let slot_bool = TypedSlot::Bool(true);

    group.bench_function("to_value_int", |b| {
        b.iter(|| black_box(slot_int.to_value()))
    });

    group.bench_function("to_value_float", |b| {
        b.iter(|| black_box(slot_float.to_value()))
    });

    group.bench_function("to_value_str", |b| {
        b.iter(|| black_box(slot_str.to_value()))
    });

    group.bench_function("to_value_bool", |b| {
        b.iter(|| black_box(slot_bool.to_value()))
    });

    group.finish();
}

// ============================================================
// Phase B+3: TypedSlot 算术 vs Value 算术
// ============================================================

fn bench_typed_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("typed_arithmetic");

    // TypedSlot native arithmetic
    let slot_a = TypedSlot::Int(42);
    let slot_b = TypedSlot::Int(1);
    let slot_fa = TypedSlot::Float(3.14);
    let slot_fb = TypedSlot::Float(2.0);

    group.bench_function("typed_int_add", |b| {
        b.iter(|| black_box(slot_a.add(&slot_b)))
    });

    group.bench_function("typed_float_add", |b| {
        b.iter(|| black_box(slot_fa.add(&slot_fb)))
    });

    group.bench_function("typed_int_mul", |b| {
        b.iter(|| black_box(slot_a.mul(&slot_b)))
    });

    group.bench_function("typed_int_cmp", |b| {
        b.iter(|| black_box(slot_a.partial_cmp_numeric(&slot_b)))
    });

    group.bench_function("typed_int_eq", |b| {
        b.iter(|| black_box(slot_a.typed_eq(&slot_b)))
    });

    // Value-based arithmetic (current eval_expr path)
    let val_a = json!(42);
    let val_b = json!(1);
    let val_fa = json!(3.14);
    let val_fb = json!(2.0);

    group.bench_function("value_int_add", |b| {
        b.iter(|| {
            // Simulate: value_to_f64 + arithmetic + json_number
            let a = black_box(&val_a).as_f64().unwrap();
            let b_val = black_box(&val_b).as_f64().unwrap();
            let result = a + b_val;
            if result.fract() == 0.0 && result.abs() < (i64::MAX as f64) {
                black_box(json!(result as i64))
            } else {
                black_box(json!(result))
            }
        })
    });

    group.bench_function("value_float_add", |b| {
        b.iter(|| {
            let a = black_box(&val_fa).as_f64().unwrap();
            let b_val = black_box(&val_fb).as_f64().unwrap();
            black_box(json!(a + b_val))
        })
    });

    // End-to-end: field_cache read + arithmetic + result
    let cache: Vec<TypedSlot> = vec![
        TypedSlot::Str(Arc::from("hello")),
        TypedSlot::Int(42),
        TypedSlot::Float(3.14),
        TypedSlot::Str(Arc::from("world")),
        TypedSlot::Bool(true),
    ];

    group.bench_function("e2e_typed_cache_add", |b| {
        b.iter(|| {
            // $self.field_1 + 1 — TypedSlot fast path
            let field = black_box(&cache[1]);
            let one = TypedSlot::Int(1);
            let result = field.add(&one).unwrap();
            black_box(result.to_value()) // boundary materialization
        })
    });

    group.bench_function("e2e_value_cache_add", |b| {
        b.iter(|| {
            // $self.field_1 + 1 — Value path (current)
            let field = black_box(&cache[1]).to_value();
            let a = field.as_f64().unwrap();
            let result = a + 1.0;
            if result.fract() == 0.0 && result.abs() < (i64::MAX as f64) {
                black_box(json!(result as i64))
            } else {
                black_box(json!(result))
            }
        })
    });

    // Multi-step: ($self.field_1 + $self.field_2) * 2
    group.bench_function("e2e_typed_multi_step", |b| {
        b.iter(|| {
            let f1 = black_box(&cache[1]);
            let f2 = black_box(&cache[2]);
            let sum = f1.add(f2).unwrap();
            let two = TypedSlot::Int(2);
            let result = sum.mul(&two).unwrap();
            black_box(result.to_value())
        })
    });

    group.bench_function("e2e_value_multi_step", |b| {
        b.iter(|| {
            let f1 = black_box(&cache[1]).to_value();
            let f2 = black_box(&cache[2]).to_value();
            let a = f1.as_f64().unwrap();
            let b_val = f2.as_f64().unwrap();
            let sum = a + b_val;
            let result = sum * 2.0;
            if result.fract() == 0.0 && result.abs() < (i64::MAX as f64) {
                black_box(json!(result as i64))
            } else {
                black_box(json!(result))
            }
        })
    });

    group.finish();
}

// ============================================================
// Phase C-2: 方法体表达式求值 — ResolvedField 直接索引 vs HashMap 查找
// ============================================================

fn bench_c2_method_eval(c: &mut Criterion) {
    let class_def = make_class_def(5);
    let mut group = c.benchmark_group("c2_method_eval");

    // 模拟 field_cache (方法 scope 入口快照)
    let field_cache: Vec<TypedSlot> = vec![
        TypedSlot::Str(Arc::from("hello")),
        TypedSlot::Int(42), // field_1 = value
        TypedSlot::Float(3.14),
        TypedSlot::Str(Arc::from("world")),
        TypedSlot::Bool(true),
    ];

    // 模拟方法参数
    let param_values: Vec<TypedSlot> = vec![TypedSlot::Int(1)]; // n = 1

    // Rust 原生 baseline
    let native = NativeStruct {
        field_0: "hello".to_string(),
        field_1: 42,
        field_2: 3.14,
        field_3: "world".to_string(),
        field_4: true,
    };
    let param_n: i64 = 1;

    // --- 单字段读取 ---

    // C-2: ResolvedField 直接索引（field_cache[1]）
    group.bench_function("field_read_resolved_idx", |b| {
        let idx = 1usize; // 预解析的索引
        b.iter(|| black_box(field_cache[black_box(idx)].clone()))
    });

    // C-1: HashMap lookup → field_cache[idx]
    group.bench_function("field_read_hashmap_lookup", |b| {
        b.iter(|| {
            let idx = class_def.field_index.get(black_box("field_1")).unwrap();
            black_box(field_cache[*idx].clone())
        })
    });

    // Rust 原生
    group.bench_function("field_read_rust_native", |b| {
        b.iter(|| black_box(native.field_1))
    });

    // --- value = $self.value + n（核心方法体表达式）---

    // C-2: ResolvedField(1) + ResolvedParam(0)（直接索引 + TypedSlot 算术）
    group.bench_function("add_resolved_field_param", |b| {
        let field_idx = 1usize;
        let param_idx = 0usize;
        b.iter(|| {
            let f = &field_cache[black_box(field_idx)];
            let p = &param_values[black_box(param_idx)];
            black_box(f.add(p).unwrap())
        })
    });

    // C-1: HashMap lookup + TypedSlot 算术
    group.bench_function("add_hashmap_lookup", |b| {
        b.iter(|| {
            let idx = *class_def.field_index.get(black_box("field_1")).unwrap();
            let f = &field_cache[idx];
            let p = &param_values[0];
            black_box(f.add(p).unwrap())
        })
    });

    // Rust 原生
    group.bench_function("add_rust_native", |b| {
        b.iter(|| black_box(black_box(native.field_1) + black_box(param_n)))
    });

    // --- ok = $self.value != 0（比较表达式）---

    // C-2
    group.bench_function("cmp_ne_resolved", |b| {
        let field_idx = 1usize;
        let zero = TypedSlot::Int(0);
        b.iter(|| {
            let f = &field_cache[black_box(field_idx)];
            black_box(!f.typed_eq(&zero))
        })
    });

    // C-1
    group.bench_function("cmp_ne_hashmap", |b| {
        let zero = TypedSlot::Int(0);
        b.iter(|| {
            let idx = *class_def.field_index.get(black_box("field_1")).unwrap();
            let f = &field_cache[idx];
            black_box(!f.typed_eq(&zero))
        })
    });

    // Rust 原生
    group.bench_function("cmp_ne_rust_native", |b| {
        b.iter(|| black_box(black_box(native.field_1) != 0))
    });

    // --- 多步：result = ($self.field_1 + n) * 2 ---

    // C-2
    group.bench_function("multi_step_resolved", |b| {
        let f_idx = 1usize;
        let p_idx = 0usize;
        let two = TypedSlot::Int(2);
        b.iter(|| {
            let f = &field_cache[black_box(f_idx)];
            let p = &param_values[black_box(p_idx)];
            let sum = f.add(p).unwrap();
            black_box(sum.mul(&two).unwrap())
        })
    });

    // C-1
    group.bench_function("multi_step_hashmap", |b| {
        let two = TypedSlot::Int(2);
        b.iter(|| {
            let idx = *class_def.field_index.get(black_box("field_1")).unwrap();
            let f = &field_cache[idx];
            let p = &param_values[0];
            let sum = f.add(p).unwrap();
            black_box(sum.mul(&two).unwrap())
        })
    });

    // Rust 原生
    group.bench_function("multi_step_rust_native", |b| {
        b.iter(|| black_box((black_box(native.field_1) + black_box(param_n)) * 2))
    });

    // --- 含边界物化（to_value）：完整方法体路径 ---

    // C-2: ResolvedField → add → to_value（方法体完整路径）
    group.bench_function("e2e_resolved_add_materialize", |b| {
        let f_idx = 1usize;
        let p_idx = 0usize;
        b.iter(|| {
            let f = &field_cache[black_box(f_idx)];
            let p = &param_values[black_box(p_idx)];
            let result = f.add(p).unwrap();
            black_box(result.to_value())
        })
    });

    // C-1: HashMap → add → to_value
    group.bench_function("e2e_hashmap_add_materialize", |b| {
        b.iter(|| {
            let idx = *class_def.field_index.get(black_box("field_1")).unwrap();
            let f = &field_cache[idx];
            let p = &param_values[0];
            let result = f.add(p).unwrap();
            black_box(result.to_value())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_instance_creation,
    bench_field_access,
    bench_method_call,
    bench_instance_clone,
    bench_typed_slot_conversion,
    bench_typed_arithmetic,
    bench_c2_method_eval,
);
criterion_main!(benches);
