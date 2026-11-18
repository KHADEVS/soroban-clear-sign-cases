//! Descriptor documents (§1) and the validation of a function descriptor against a spec (§2, §3, §4.3).
use crate::spec::{text, type_name, Spec};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use stellar_xdr::ScSpecTypeDef;

/// Parses JSON, refusing duplicate keys in any object (§4.3).
pub fn parse_strict(text: &str) -> Result<Value, String> {
    struct Strict(Value);
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("JSON")
        }
        fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
            Ok(Value::Bool(v))
        }
        fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
            Ok(Value::from(v))
        }
        fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
            Ok(Value::from(v))
        }
        fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
            Ok(Value::from(v))
        }
        fn visit_str<E>(self, v: &str) -> Result<Value, E> {
            Ok(Value::String(v.to_owned()))
        }
        fn visit_string<E>(self, v: String) -> Result<Value, E> {
            Ok(Value::String(v))
        }
        fn visit_unit<E>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
            let mut out = Vec::new();
            while let Some(Strict(v)) = seq.next_element()? {
                out.push(v);
            }
            Ok(Value::Array(out))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut out = Map::new();
            while let Some(key) = map.next_key::<String>()? {
                let Strict(v) = map.next_value()?;
                if out.contains_key(&key) {
                    return Err(de::Error::custom(format!("duplicate key \"{key}\"")));
                }
                out.insert(key, v);
            }
            Ok(Value::Object(out))
        }
    }
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_any(V).map(Strict)
        }
    }
    let mut de = serde_json::Deserializer::from_str(text);
    let Strict(v) = Strict::deserialize(&mut de).map_err(|e| e.to_string())?;
    de.end().map_err(|e| e.to_string())?;
    Ok(v)
}

/// A descriptor document that passed the document-level rules of §1.
#[derive(Clone, Debug)]
pub struct Document {
    /// SHA-256 of the exact bytes, as §4.3 defines the document hash.
    pub hash: String,
    pub wasm_hashes: Vec<String>,
    /// Function descriptors, still unvalidated: each one is checked against the spec on use.
    pub functions: Map<String, Value>,
}

/// Why a document is not usable at all.
pub fn parse_document(bytes: &[u8]) -> Result<Document, String> {
    let hash = hex::encode(Sha256::digest(bytes));
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8".to_string())?;
    let v = parse_strict(text).map_err(|e| format!("invalid JSON: {e}"))?;
    let obj = v.as_object().ok_or("not a JSON object")?;
    match obj.get("version") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) && !n.is_f64() => {}
        Some(other) => return Err(format!("unknown version {other}: ignored")),
        None => return Err("no version".into()),
    }
    let hashes = obj
        .get("wasm_hashes")
        .and_then(Value::as_array)
        .ok_or("wasm_hashes missing or not an array")?;
    if hashes.is_empty() {
        return Err("wasm_hashes is empty".into());
    }
    let mut wasm_hashes = Vec::new();
    for h in hashes {
        let s = h.as_str().ok_or("a wasm hash is not a string")?;
        if s.len() != 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(format!(
                "wasm hash \"{s}\" is not 64 lowercase hex characters"
            ));
        }
        wasm_hashes.push(s.to_owned());
    }
    let functions = obj
        .get("functions")
        .and_then(Value::as_object)
        .ok_or("functions missing or not an object")?
        .clone();
    Ok(Document {
        hash,
        wasm_hashes,
        functions,
    })
}

/// One step of a path (§2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Name(String),
    Index(i64),
}

#[derive(Clone, Debug)]
pub enum Token {
    SelfToken,
    Path(Vec<Step>),
}

#[derive(Clone, Debug)]
pub enum Format {
    Raw,
    Amount(Token),
    Timestamp,
    Duration,
    Percent(u32),
    Bool(String, String),
    Enum(BTreeMap<i128, String>),
}

#[derive(Clone, Debug)]
pub struct Field {
    pub path: Vec<Step>,
    pub label: String,
    pub format: Format,
    /// Spec type at the end of the path.
    pub end_type: ScSpecTypeDef,
}

/// A function descriptor that passed §4.3 for one spec.
#[derive(Clone, Debug)]
pub struct FunctionDescriptor {
    pub intent: String,
    pub auth: bool,
    pub fields: Vec<Field>,
}

pub fn printable_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn path_of(v: &Value, what: &str) -> Result<Vec<Step>, String> {
    let arr = v.as_array().ok_or(format!("{what} is not an array"))?;
    if arr.is_empty() {
        return Err(format!("{what} is empty"));
    }
    let mut steps = Vec::new();
    for (i, s) in arr.iter().enumerate() {
        match s {
            Value::String(name) => steps.push(Step::Name(name.clone())),
            Value::Number(n) if i > 0 && n.as_i64().is_some() && !n.is_f64() => {
                steps.push(Step::Index(n.as_i64().unwrap()))
            }
            _ if i == 0 => return Err(format!("{what} does not start with an input name")),
            other => {
                return Err(format!(
                    "{what} has an element {other} that is neither a name nor an integer"
                ))
            }
        }
    }
    Ok(steps)
}

/// Follows a path through spec types (§4.3, "every path resolves against the spec").
pub fn resolve_type(
    spec: Option<&Spec>,
    inputs: &[(String, ScSpecTypeDef)],
    path: &[Step],
) -> Result<ScSpecTypeDef, String> {
    let Some(Step::Name(first)) = path.first() else {
        return Err("path does not start with an input name".into());
    };
    let mut t = inputs
        .iter()
        .find(|(n, _)| n == first)
        .map(|(_, t)| t.clone())
        .ok_or(format!("no input named \"{first}\""))?;
    for step in &path[1..] {
        t = match (step, &t) {
            (Step::Index(_), ScSpecTypeDef::Vec(v)) => (*v.element_type).clone(),
            (Step::Index(i), ScSpecTypeDef::Tuple(tp)) => {
                let len = tp.value_types.len() as i64;
                let idx = if *i < 0 { len + i } else { *i };
                if idx < 0 || idx >= len {
                    return Err(format!("index {i} is outside a tuple of {len}"));
                }
                tp.value_types[idx as usize].clone()
            }
            (Step::Name(n), ScSpecTypeDef::Udt(u)) => {
                let udt = text(u.name.as_slice());
                let st = spec
                    .and_then(|s| s.struct_named(&udt))
                    .ok_or(format!("\"{udt}\" is not a struct UDT of the spec"))?;
                st.fields
                    .iter()
                    .find(|f| text(f.name.as_slice()) == *n)
                    .map(|f| f.type_.clone())
                    .ok_or(format!("struct \"{udt}\" has no member \"{n}\""))?
            }
            (s, t) => return Err(format!("cannot step {s:?} into {}", type_name(t))),
        };
    }
    Ok(t)
}

fn accepts(format: &str, t: &ScSpecTypeDef) -> bool {
    use ScSpecTypeDef as T;
    match format {
        "amount" | "percent" => matches!(t, T::I128 | T::U128 | T::I64 | T::U64 | T::I32 | T::U32),
        "timestamp" => matches!(t, T::U64 | T::Timepoint),
        "duration" => matches!(t, T::U32 | T::U64 | T::Duration),
        "bool" => matches!(t, T::Bool),
        "enum" => matches!(t, T::U32 | T::I32 | T::U64 | T::I64),
        _ => false,
    }
}

fn only_keys(obj: &Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match obj.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown key \"{k}\" in {what}")),
        None => Ok(()),
    }
}

/// §4.3 for one function descriptor. `spec` is None for Stellar Asset Contracts, whose
/// built-in descriptors bind names to positions through `inputs`.
pub fn validate_function(
    name: &str,
    fd: &Value,
    spec: Option<&Spec>,
    inputs: &[(String, ScSpecTypeDef)],
) -> Result<FunctionDescriptor, String> {
    if let Some(s) = spec {
        let n = s.functions_named(name).len();
        if n != 1 {
            return Err(format!("the spec has {n} functions named \"{name}\""));
        }
    }
    let obj = fd
        .as_object()
        .ok_or("function descriptor is not an object")?;
    only_keys(
        obj,
        &["intent", "auth", "fields"],
        "the function descriptor",
    )?;
    let intent = obj
        .get("intent")
        .and_then(Value::as_str)
        .ok_or("intent missing or not a string")?
        .to_owned();
    if !printable_ascii(&intent) {
        return Err("intent is not printable ASCII".into());
    }
    let auth = match obj.get("auth") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("auth is not a boolean".into()),
    };
    let mut fields = Vec::new();
    let empty = Vec::new();
    let raw_fields = match obj.get("fields") {
        None => &empty,
        Some(Value::Array(a)) => a,
        Some(_) => return Err("fields is not an array".into()),
    };
    for (i, f) in raw_fields.iter().enumerate() {
        let what = format!("field {i}");
        let fo = f.as_object().ok_or(format!("{what} is not an object"))?;
        only_keys(
            fo,
            &[
                "path", "label", "format", "token", "decimals", "true", "false", "values",
            ],
            &what,
        )?;
        let path = path_of(
            fo.get("path").ok_or(format!("{what} has no path"))?,
            &format!("{what} path"),
        )?;
        let label = fo
            .get("label")
            .and_then(Value::as_str)
            .ok_or(format!("{what} label missing or not a string"))?
            .to_owned();
        if !printable_ascii(&label) {
            return Err(format!("{what} label is not printable ASCII"));
        }
        let end_type = resolve_type(spec, inputs, &path).map_err(|e| format!("{what}: {e}"))?;
        let format_name = match fo.get("format") {
            None => None,
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => return Err(format!("{what} format is not a string")),
        };
        for (key, needed) in [
            ("token", format_name == Some("amount")),
            ("decimals", format_name == Some("percent")),
            ("true", format_name == Some("bool")),
            ("false", format_name == Some("bool")),
            ("values", format_name == Some("enum")),
        ] {
            if fo.contains_key(key) != needed {
                return Err(format!(
                    "{what}: \"{key}\" must be present if and only if its format needs it"
                ));
            }
        }
        let format = match format_name {
            None => Format::Raw,
            Some(fmt) => {
                if !["amount", "timestamp", "duration", "percent", "bool", "enum"].contains(&fmt) {
                    return Err(format!("{what}: unknown format \"{fmt}\""));
                }
                if !accepts(fmt, &end_type) {
                    return Err(format!(
                        "{what}: format {fmt} does not accept {}",
                        type_name(&end_type)
                    ));
                }
                match fmt {
                    "amount" => {
                        let t = fo["token"]
                            .as_object()
                            .ok_or(format!("{what}: token is not an object"))?;
                        let token = match (t.get("self"), t.get("path"), t.len()) {
                            (Some(Value::Bool(true)), None, 1) => Token::SelfToken,
                            (None, Some(p), 1) => {
                                let p = path_of(p, &format!("{what} token.path"))?;
                                let tt = resolve_type(spec, inputs, &p).map_err(|e| format!("{what} token.path: {e}"))?;
                                if tt != ScSpecTypeDef::Address {
                                    return Err(format!("{what}: token.path ends at {}, not address", type_name(&tt)));
                                }
                                Token::Path(p)
                            }
                            _ => return Err(format!("{what}: token must be exactly {{\"self\": true}} or {{\"path\": [...]}}")),
                        };
                        Format::Amount(token)
                    }
                    "timestamp" => Format::Timestamp,
                    "duration" => Format::Duration,
                    "percent" => match &fo["decimals"] {
                        Value::Number(n) if !n.is_f64() && n.as_u64().is_some_and(|d| d <= 38) => {
                            Format::Percent(n.as_u64().unwrap() as u32)
                        }
                        _ => {
                            return Err(format!("{what}: decimals must be an integer from 0 to 38"))
                        }
                    },
                    "bool" => {
                        let t = fo["true"]
                            .as_str()
                            .filter(|s| printable_ascii(s))
                            .ok_or(format!("{what}: true is not printable ASCII text"))?;
                        let f = fo["false"]
                            .as_str()
                            .filter(|s| printable_ascii(s))
                            .ok_or(format!("{what}: false is not printable ASCII text"))?;
                        Format::Bool(t.to_owned(), f.to_owned())
                    }
                    _ => {
                        let vals = fo["values"]
                            .as_object()
                            .ok_or(format!("{what}: values is not an object"))?;
                        let mut out = BTreeMap::new();
                        for (k, v) in vals {
                            let key: i128 = k.parse().map_err(|_| {
                                format!("{what}: values key \"{k}\" is not a decimal integer")
                            })?;
                            let phrase = v.as_str().filter(|s| printable_ascii(s)).ok_or(
                                format!("{what}: values phrase is not printable ASCII text"),
                            )?;
                            out.insert(key, phrase.to_owned());
                        }
                        Format::Enum(out)
                    }
                }
            }
        };
        if fields.iter().any(|g: &Field| g.path == path) {
            return Err(format!("{what}: two fields have the same path"));
        }
        fields.push(Field {
            path,
            label,
            format,
            end_type,
        });
    }
    Ok(FunctionDescriptor {
        intent,
        auth,
        fields,
    })
}
