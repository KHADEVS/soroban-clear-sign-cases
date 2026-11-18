//! The SEP-48 interface of a contract: the `contractspecv0` custom section of its Wasm.
use stellar_xdr::{
    Limited, Limits, ReadXdr, ScSpecEntry, ScSpecFunctionV0, ScSpecTypeDef, ScSpecUdtStructV0,
};

#[derive(Clone, Debug)]
pub struct Spec {
    pub entries: Vec<ScSpecEntry>,
}

/// Text of an XDR string or symbol, lossy for display.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Spec {
    /// Reads the spec from a Wasm module. A missing or undecodable section is an error.
    pub fn from_wasm(wasm: &[u8]) -> Result<Spec, String> {
        let section = custom_section(wasm, "contractspecv0")?.ok_or("no contractspecv0 section")?;
        Spec::from_xdr(section)
    }

    /// Reads a concatenation of `ScSpecEntry` values, as stored in the section.
    pub fn from_xdr(bytes: &[u8]) -> Result<Spec, String> {
        let mut reader = Limited::new(std::io::Cursor::new(bytes), Limits::none());
        let mut entries = Vec::new();
        for entry in ScSpecEntry::read_xdr_iter(&mut reader) {
            entries.push(entry.map_err(|e| format!("contractspecv0 does not decode: {e}"))?);
        }
        Ok(Spec { entries })
    }

    pub fn functions_named(&self, name: &str) -> Vec<&ScSpecFunctionV0> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                ScSpecEntry::FunctionV0(f) if text(f.name.0.as_slice()) == name => Some(f),
                _ => None,
            })
            .collect()
    }

    /// The struct UDT with this name, if exactly one is declared.
    pub fn struct_named(&self, name: &str) -> Option<&ScSpecUdtStructV0> {
        let mut found = self.entries.iter().filter_map(|e| match e {
            ScSpecEntry::UdtStructV0(s) if text(s.name.as_slice()) == name => Some(s),
            _ => None,
        });
        let first = found.next()?;
        found.next().is_none().then_some(first)
    }

    /// The inputs of a function, as (name, type) pairs in order.
    pub fn inputs(f: &ScSpecFunctionV0) -> Vec<(String, ScSpecTypeDef)> {
        f.inputs
            .iter()
            .map(|i| (text(i.name.as_slice()), i.type_.clone()))
            .collect()
    }
}

/// Short name of a spec type, in the SEP-51 snake_case form the draft uses.
pub fn type_name(t: &ScSpecTypeDef) -> String {
    match t {
        ScSpecTypeDef::Val => "val".into(),
        ScSpecTypeDef::Bool => "bool".into(),
        ScSpecTypeDef::Void => "void".into(),
        ScSpecTypeDef::Error => "error".into(),
        ScSpecTypeDef::U32 => "u32".into(),
        ScSpecTypeDef::I32 => "i32".into(),
        ScSpecTypeDef::U64 => "u64".into(),
        ScSpecTypeDef::I64 => "i64".into(),
        ScSpecTypeDef::Timepoint => "timepoint".into(),
        ScSpecTypeDef::Duration => "duration".into(),
        ScSpecTypeDef::U128 => "u128".into(),
        ScSpecTypeDef::I128 => "i128".into(),
        ScSpecTypeDef::U256 => "u256".into(),
        ScSpecTypeDef::I256 => "i256".into(),
        ScSpecTypeDef::Bytes => "bytes".into(),
        ScSpecTypeDef::String => "string".into(),
        ScSpecTypeDef::Symbol => "symbol".into(),
        ScSpecTypeDef::Address => "address".into(),
        ScSpecTypeDef::MuxedAddress => "muxed_address".into(),
        ScSpecTypeDef::Option(o) => format!("option<{}>", type_name(&o.value_type)),
        ScSpecTypeDef::Result(r) => format!("result<{}>", type_name(&r.ok_type)),
        ScSpecTypeDef::Vec(v) => format!("vec<{}>", type_name(&v.element_type)),
        ScSpecTypeDef::Map(m) => format!(
            "map<{},{}>",
            type_name(&m.key_type),
            type_name(&m.value_type)
        ),
        ScSpecTypeDef::Tuple(t) => {
            format!(
                "tuple<{}>",
                t.value_types
                    .iter()
                    .map(type_name)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        ScSpecTypeDef::BytesN(b) => format!("bytes_n<{}>", b.n),
        ScSpecTypeDef::Udt(u) => format!("udt:{}", text(u.name.as_slice())),
    }
}

/// Finds a custom section by name in a Wasm module.
pub fn custom_section<'a>(wasm: &'a [u8], name: &str) -> Result<Option<&'a [u8]>, String> {
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        return Err("not a Wasm module".into());
    }
    let mut pos = 8;
    while pos < wasm.len() {
        let id = wasm[pos];
        pos += 1;
        let (size, used) = leb128(&wasm[pos..])?;
        pos += used;
        let end = pos
            .checked_add(size as usize)
            .filter(|&e| e <= wasm.len())
            .ok_or("truncated section")?;
        if id == 0 {
            let (name_len, used) = leb128(&wasm[pos..end])?;
            let name_start = pos + used;
            let name_end = name_start
                .checked_add(name_len as usize)
                .filter(|&e| e <= end)
                .ok_or("bad section name")?;
            if &wasm[name_start..name_end] == name.as_bytes() {
                return Ok(Some(&wasm[name_end..end]));
            }
        }
        pos = end;
    }
    Ok(None)
}

fn leb128(bytes: &[u8]) -> Result<(u64, usize), String> {
    let mut value = 0u64;
    for (i, b) in bytes.iter().enumerate().take(10) {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err("bad LEB128".into())
}
