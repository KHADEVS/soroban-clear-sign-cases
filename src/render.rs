//! Rendering (§3, §4.4, §5, §6): what a wallet following the draft shows for an invocation.
use crate::descriptor::{
    validate_function, Document, Field, Format, FunctionDescriptor, Step, Token,
};
use crate::spec::{text, Spec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use stellar_xdr::{
    Asset, ContractIdPreimage, Hash, HashIdPreimage, HashIdPreimageContractId, InvokeContractArgs,
    Limits, ScAddress, ScSpecTypeDef, ScVal, SorobanAuthorizationEntry, SorobanAuthorizedFunction,
    SorobanAuthorizedInvocation, SorobanCredentials, WriteXdr,
};

/// How the wallet resolved a contract (§4.1, §5.3).
#[derive(Clone, Debug)]
pub enum Resolution {
    Wasm {
        hash: String,
        spec: Result<Spec, String>,
    },
    /// A Stellar Asset Contract read from a trusted source, with the asset if known.
    StellarAsset { asset: Option<Asset> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenMeta {
    pub decimals: Option<u32>,
    pub symbol: Option<String>,
    /// "curated" (a list the wallet trusts) or "contract" (read from the token itself).
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct Context {
    pub network_id: [u8; 32],
    pub contracts: BTreeMap<String, Resolution>,
    pub tokens: BTreeMap<String, TokenMeta>,
    pub documents: Vec<Document>,
}

pub fn network_id(passphrase: &str) -> [u8; 32] {
    Sha256::digest(passphrase.as_bytes()).into()
}

pub const MAINNET: &str = "Public Global Stellar Network ; September 2015";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Line {
    pub label: String,
    pub value: String,
}

/// One rendered invocation or authorization node.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Display {
    pub described: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    pub contract: String,
    pub function: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Line>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub omitted: usize,
    pub raw: Vec<Line>,
    /// Why the call is not described (§6 marker), when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_described: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

pub fn addr(a: &ScAddress) -> String {
    a.to_string()
}

/// Raw rendering (§3.0).
pub fn raw_value(v: &ScVal) -> String {
    match v {
        ScVal::Address(a) => addr(a),
        ScVal::U32(n) => n.to_string(),
        ScVal::I32(n) => n.to_string(),
        ScVal::U64(n) => n.to_string(),
        ScVal::I64(n) => n.to_string(),
        ScVal::U128(p) => p.to_string(),
        ScVal::I128(p) => p.to_string(),
        ScVal::Timepoint(t) => t.0.to_string(),
        ScVal::Duration(d) => d.0.to_string(),
        ScVal::Bool(b) => b.to_string(),
        ScVal::Symbol(s) => text(s.0.as_slice()),
        other => serde_json::to_string(other).unwrap_or_else(|_| "?".into()),
    }
}

/// The integer value of a formatted argument, as sign and magnitude.
fn int_of(v: &ScVal) -> Option<(bool, u128)> {
    let signed = |x: i128| (x < 0, x.unsigned_abs());
    Some(match v {
        ScVal::U32(n) => (false, u128::from(*n)),
        ScVal::I32(n) => signed(i128::from(*n)),
        ScVal::U64(n) => (false, u128::from(*n)),
        ScVal::I64(n) => signed(i128::from(*n)),
        ScVal::U128(p) => (false, (u128::from(p.hi) << 64) | u128::from(p.lo)),
        ScVal::I128(p) => signed(((i128::from(p.hi)) << 64) | i128::from(p.lo)),
        ScVal::Timepoint(t) => (false, u128::from(t.0)),
        ScVal::Duration(d) => (false, u128::from(d.0)),
        _ => return None,
    })
}

/// `mag / 10^decimals` in base 10, exact, trailing fractional zeros dropped.
pub fn scaled(neg: bool, mag: u128, decimals: u32) -> String {
    let digits = mag.to_string();
    let d = decimals as usize;
    let (int, frac) = if d == 0 {
        (digits, String::new())
    } else if digits.len() > d {
        (
            digits[..digits.len() - d].to_string(),
            digits[digits.len() - d..].to_string(),
        )
    } else {
        (
            "0".to_string(),
            format!("{}{}", "0".repeat(d - digits.len()), digits),
        )
    };
    let frac = frac.trim_end_matches('0');
    let body = if frac.is_empty() {
        int
    } else {
        format!("{int}.{frac}")
    };
    if neg && mag != 0 {
        format!("-{body}")
    } else {
        body
    }
}

/// RFC 3339 in UTC, or None past year 9999 (RFC 3339 has four-digit years).
pub fn rfc3339(secs: u64) -> Option<String> {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days as i128 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i128::from(m <= 2);
    if y > 9999 {
        return None;
    }
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    ))
}

pub fn duration(secs: u128) -> String {
    let (d, h, m, s) = (
        secs / 86_400,
        (secs % 86_400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );
    let parts: Vec<String> = [(d, "d"), (h, "h"), (m, "m"), (s, "s")]
        .iter()
        .filter(|(v, _)| *v > 0)
        .map(|(v, u)| format!("{v}{u}"))
        .collect();
    // "omitting leading units that are zero": the draft's examples drop inner zero units too ("1h 30m").
    if parts.is_empty() {
        "0s".into()
    } else {
        parts.join(" ")
    }
}

/// §3.1 symbol rule: printable, no control, bidi or format characters; truncated to 32 bytes.
pub fn clean_symbol(s: &str) -> Option<String> {
    let bad = |c: char| {
        c.is_control()
            || ('\u{202a}'..='\u{202e}').contains(&c)
            || ('\u{2066}'..='\u{2069}').contains(&c)
            || matches!(c, '\u{200b}'..='\u{200f}' | '\u{feff}')
    };
    if s.is_empty() || s.chars().any(bad) {
        return None;
    }
    if s.len() <= 32 {
        return Some(s.to_owned());
    }
    let mut end = 32;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}…", &s[..end]))
}

/// The contract ID a Stellar asset hashes to on a network (§5.3).
pub fn asset_contract_id(network_id: &[u8; 32], asset: &Asset) -> String {
    let preimage = HashIdPreimage::ContractId(HashIdPreimageContractId {
        network_id: Hash(*network_id),
        contract_id_preimage: ContractIdPreimage::Asset(asset.clone()),
    });
    let id: [u8; 32] = Sha256::digest(preimage.to_xdr(Limits::none()).unwrap()).into();
    ScAddress::Contract(stellar_xdr::ContractId(Hash(id))).to_string()
}

fn asset_code(asset: &Asset) -> String {
    match asset {
        Asset::Native => "XLM".into(),
        Asset::CreditAlphanum4(a) => a.asset_code.to_string(),
        Asset::CreditAlphanum12(a) => a.asset_code.to_string(),
    }
}

/// Built-in descriptors for Stellar Asset Contracts (§5.3), with their positional inputs.
pub fn sac_builtin(function: &str) -> Option<(FunctionDescriptor, Vec<(String, ScSpecTypeDef)>)> {
    use ScSpecTypeDef as T;
    let (inputs, json) = match function {
        "transfer" => (
            vec![
                ("from", T::Address),
                ("to", T::MuxedAddress),
                ("amount", T::I128),
            ],
            r#"{"intent":"Send tokens","auth":true,"fields":[{"path":["amount"],"label":"Amount","format":"amount","token":{"self":true}},{"path":["to"],"label":"Recipient"},{"path":["from"],"label":"From"}]}"#,
        ),
        "approve" => (
            vec![
                ("from", T::Address),
                ("spender", T::Address),
                ("amount", T::I128),
                ("live_until_ledger", T::U32),
            ],
            r#"{"intent":"Approve token spending","auth":true,"fields":[{"path":["spender"],"label":"Spender"},{"path":["amount"],"label":"Allowance","format":"amount","token":{"self":true}},{"path":["live_until_ledger"],"label":"Expires (ledger)"},{"path":["from"],"label":"Owner"}]}"#,
        ),
        "burn" => (
            vec![("from", T::Address), ("amount", T::I128)],
            r#"{"intent":"Burn tokens","auth":true,"fields":[{"path":["amount"],"label":"Amount","format":"amount","token":{"self":true}},{"path":["from"],"label":"From"}]}"#,
        ),
        _ => return None,
    };
    let inputs: Vec<(String, ScSpecTypeDef)> = inputs
        .into_iter()
        .map(|(n, t)| (n.to_string(), t))
        .collect();
    let fd = validate_function(
        function,
        &serde_json::from_str(json).unwrap(),
        None,
        &inputs,
    )
    .expect("built-in descriptor is valid");
    Some((fd, inputs))
}

enum Walk<'a> {
    Found(&'a ScVal),
    /// Stepped past the end of a vector: the field is omitted (§4.4).
    Beyond,
}

fn walk<'a>(
    args: &'a [ScVal],
    inputs: &[(String, ScSpecTypeDef)],
    path: &[Step],
) -> Result<Walk<'a>, String> {
    let Some(Step::Name(first)) = path.first() else {
        return Err("bad path".into());
    };
    let pos = inputs
        .iter()
        .position(|(n, _)| n == first)
        .ok_or("no such input")?;
    let mut v = &args[pos];
    for step in &path[1..] {
        v = match (step, v) {
            (Step::Index(i), ScVal::Vec(Some(vec))) => {
                let len = vec.len() as i64;
                let idx = if *i < 0 { len + i } else { *i };
                if idx >= len || idx < 0 {
                    // The draft says "an index beyond its length"; a negative index before the
                    // start is treated the same way here (see the case notes).
                    return Ok(Walk::Beyond);
                }
                &vec[idx as usize]
            }
            (Step::Name(n), ScVal::Map(Some(map))) => {
                &map.iter()
                    .find(|e| matches!(&e.key, ScVal::Symbol(s) if text(s.0.as_slice()) == *n))
                    .ok_or(format!("no map entry \"{n}\""))?
                    .val
            }
            (Step::Index(_), other) => {
                return Err(format!("expected SCV_VEC, found {}", kind(other)))
            }
            (Step::Name(_), other) => {
                return Err(format!("expected SCV_MAP, found {}", kind(other)))
            }
        };
    }
    Ok(Walk::Found(v))
}

fn kind(v: &ScVal) -> &'static str {
    match v {
        ScVal::Vec(_) => "SCV_VEC",
        ScVal::Map(_) => "SCV_MAP",
        ScVal::Address(_) => "SCV_ADDRESS",
        ScVal::I128(_) => "SCV_I128",
        ScVal::U128(_) => "SCV_U128",
        ScVal::U64(_) => "SCV_U64",
        ScVal::I64(_) => "SCV_I64",
        ScVal::U32(_) => "SCV_U32",
        ScVal::I32(_) => "SCV_I32",
        ScVal::Bool(_) => "SCV_BOOL",
        ScVal::Timepoint(_) => "SCV_TIMEPOINT",
        ScVal::Duration(_) => "SCV_DURATION",
        _ => "another SCVal",
    }
}

fn end_ok(t: &ScSpecTypeDef, v: &ScVal) -> bool {
    use ScSpecTypeDef as T;
    matches!(
        (t, v),
        (T::U32, ScVal::U32(_))
            | (T::I32, ScVal::I32(_))
            | (T::U64, ScVal::U64(_))
            | (T::I64, ScVal::I64(_))
            | (T::U128, ScVal::U128(_))
            | (T::I128, ScVal::I128(_))
            | (T::Timepoint, ScVal::Timepoint(_))
            | (T::Duration, ScVal::Duration(_))
            | (T::Bool, ScVal::Bool(_))
    )
}

/// §4.4. Returns, per field, the value it shows (None when omitted).
fn argument_check<'a>(
    fd: &FunctionDescriptor,
    inputs: &[(String, ScSpecTypeDef)],
    args: &'a [ScVal],
) -> Result<Vec<Option<&'a ScVal>>, String> {
    if args.len() != inputs.len() {
        return Err(format!(
            "{} arguments for {} inputs",
            args.len(),
            inputs.len()
        ));
    }
    let mut out = Vec::new();
    for f in &fd.fields {
        let v = match walk(args, inputs, &f.path)? {
            Walk::Beyond => {
                out.push(None);
                continue;
            }
            Walk::Found(v) => v,
        };
        if !matches!(f.format, Format::Raw) && !end_ok(&f.end_type, v) {
            return Err(format!(
                "field \"{}\" holds {}, not the spec type",
                f.label,
                kind(v)
            ));
        }
        if let Format::Amount(Token::Path(p)) = &f.format {
            match walk(args, inputs, p)? {
                Walk::Found(ScVal::Address(ScAddress::Contract(_))) => {}
                Walk::Found(other) => {
                    return Err(format!(
                        "token path of \"{}\" ends at {}, not a contract address",
                        f.label,
                        kind(other)
                    ))
                }
                Walk::Beyond => {
                    return Err(format!(
                        "token path of \"{}\" steps past the end of a vector",
                        f.label
                    ))
                }
            }
        }
        out.push(Some(v));
    }
    Ok(out)
}

impl Context {
    fn token_meta(&self, token: &str) -> (Option<u32>, Option<String>, Option<&'static str>) {
        if let Some(Resolution::StellarAsset { asset: Some(a) }) = self.contracts.get(token) {
            if asset_contract_id(&self.network_id, a) == token {
                return (Some(7), Some(asset_code(a)), None);
            }
        }
        match self.tokens.get(token) {
            Some(m) => {
                let unverified = (m.source == "contract").then_some("unverified");
                (
                    m.decimals.filter(|d| *d <= 38),
                    m.symbol.as_deref().and_then(clean_symbol),
                    unverified,
                )
            }
            None => (None, None, None),
        }
    }

    fn format_field(
        &self,
        f: &Field,
        v: &ScVal,
        args: &[ScVal],
        inputs: &[(String, ScSpecTypeDef)],
        contract: &str,
        notes: &mut Vec<String>,
    ) -> String {
        match &f.format {
            Format::Raw => raw_value(v),
            Format::Amount(token) => {
                let token_id = match token {
                    Token::SelfToken => contract.to_owned(),
                    Token::Path(p) => match walk(args, inputs, p) {
                        Ok(Walk::Found(ScVal::Address(a))) => addr(a),
                        _ => unreachable!("checked in §4.4"),
                    },
                };
                let (neg, mag) = int_of(v).expect("checked in §4.4");
                let (decimals, symbol, flag) = self.token_meta(&token_id);
                let Some(decimals) = decimals else {
                    notes.push(format!("{}: decimals of {token_id} unresolved", f.label));
                    return format!("{}{} (raw units)", if neg { "-" } else { "" }, mag);
                };
                let unit = symbol.unwrap_or_else(|| token_id.clone());
                let mut s = format!("{} {unit}", scaled(neg, mag, decimals));
                if let Some(flag) = flag {
                    s.push_str(&format!(" ({flag})"));
                }
                s
            }
            Format::Timestamp => {
                let (_, secs) = int_of(v).unwrap();
                match u64::try_from(secs).ok().and_then(rfc3339) {
                    Some(t) => t,
                    None => {
                        notes.push(format!(
                            "{}: {secs} has no RFC 3339 form (year past 9999)",
                            f.label
                        ));
                        format!("{secs} (not a date)")
                    }
                }
            }
            Format::Duration => duration(int_of(v).unwrap().1),
            Format::Percent(d) => {
                let (neg, mag) = int_of(v).unwrap();
                // value / 10^d * 100 = value / 10^(d-2)
                if *d >= 2 {
                    format!("{}%", scaled(neg, mag, d - 2))
                } else {
                    format!("{}%", scaled(neg, mag * 10u128.pow(2 - d), 0))
                }
            }
            Format::Bool(t, fl) => match v {
                ScVal::Bool(true) => t.clone(),
                _ => fl.clone(),
            },
            Format::Enum(values) => {
                let (neg, mag) = int_of(v).unwrap();
                let n = if neg { -(mag as i128) } else { mag as i128 };
                values.get(&n).cloned().unwrap_or_else(|| n.to_string())
            }
        }
    }

    /// Picks the descriptor and inputs that apply to a call, or says why none does.
    fn descriptor_for(
        &self,
        contract: &str,
        function: &str,
    ) -> Result<(FunctionDescriptor, Vec<(String, ScSpecTypeDef)>), String> {
        match self.contracts.get(contract) {
            None => Err("contract not resolved".into()),
            Some(Resolution::StellarAsset { asset }) => {
                let verified = asset
                    .as_ref()
                    .is_some_and(|a| asset_contract_id(&self.network_id, a) == contract);
                if !verified {
                    return Err("Stellar Asset Contract whose asset does not hash to its ID".into());
                }
                sac_builtin(function).ok_or(format!(
                    "Stellar Asset Contract function \"{function}\" has no built-in rendering"
                ))
            }
            Some(Resolution::Wasm { hash, spec }) => {
                let spec = spec.as_ref().map_err(|e| format!("spec unreadable: {e}"))?;
                let doc = self
                    .documents
                    .iter()
                    .find(|d| d.wasm_hashes.contains(hash))
                    .ok_or("no descriptor for this Wasm")?;
                let fd = doc
                    .functions
                    .get(function)
                    .ok_or(format!("no descriptor for function \"{function}\""))?;
                let f = spec.functions_named(function);
                let inputs = f.first().map(|f| Spec::inputs(f)).unwrap_or_default();
                let valid = validate_function(function, fd, Some(spec), &inputs)
                    .map_err(|e| format!("descriptor invalid: {e}"))?;
                Ok((valid, inputs))
            }
        }
    }

    fn spec_names(&self, contract: &str, function: &str) -> Option<Vec<String>> {
        match self.contracts.get(contract) {
            Some(Resolution::Wasm { spec: Ok(spec), .. }) => spec
                .functions_named(function)
                .first()
                .map(|f| Spec::inputs(f).into_iter().map(|(n, _)| n).collect()),
            _ => None,
        }
    }

    fn raw_lines(&self, args: &[ScVal], names: Option<Vec<String>>) -> Vec<Line> {
        args.iter()
            .enumerate()
            .map(|(i, v)| Line {
                label: names
                    .as_ref()
                    .and_then(|n| n.get(i).cloned())
                    .unwrap_or_else(|| format!("arg {i}")),
                value: raw_value(v),
            })
            .collect()
    }

    /// Renders one call. `auth_node` applies §5.2: an `auth: true` assertion is required for
    /// Wasm contracts, and the raw view carries no spec labels.
    pub fn render_call(&self, call: &InvokeContractArgs, auth_node: bool) -> Display {
        let contract = addr(&call.contract_address);
        let function = text(call.function_name.0.as_slice());
        let args: Vec<ScVal> = call.args.to_vec();
        let names = if auth_node {
            None
        } else {
            self.spec_names(&contract, &function)
        };
        let raw = self.raw_lines(&args, names);
        let mut d = Display {
            described: false,
            intent: None,
            contract: contract.clone(),
            function: function.clone(),
            fields: vec![],
            omitted: 0,
            raw,
            not_described: None,
            notes: vec![],
        };
        let (fd, inputs) = match self.descriptor_for(&contract, &function) {
            Ok(x) => x,
            Err(why) => {
                d.not_described = Some(why);
                return d;
            }
        };
        if auth_node && !fd.auth {
            d.not_described = Some("descriptor does not set auth: true".into());
            return d;
        }
        let values = match argument_check(&fd, &inputs, &args) {
            Ok(v) => v,
            Err(why) => {
                d.not_described = Some(format!("argument check failed: {why}"));
                return d;
            }
        };
        let mut notes = Vec::new();
        let shown = values.iter().filter(|v| v.is_some()).count();
        for (f, v) in fd.fields.iter().zip(&values) {
            if let Some(v) = v {
                let value = self.format_field(f, v, &args, &inputs, &contract, &mut notes);
                d.fields.push(Line {
                    label: f.label.clone(),
                    value,
                });
            }
        }
        // §5.1's omission count, as read here: a parameter is listed when some field's path
        // starts at it (a token path alone does not list it), and each field omitted under
        // §4.4 counts too.
        let listed_inputs: std::collections::BTreeSet<&str> = fd
            .fields
            .iter()
            .filter_map(|f| match f.path.first() {
                Some(Step::Name(n)) => Some(n.as_str()),
                _ => None,
            })
            .collect();
        let unlisted = inputs
            .iter()
            .filter(|(n, _)| !listed_inputs.contains(n.as_str()))
            .count();
        d.omitted = unlisted + (values.len() - shown);
        d.described = true;
        d.intent = Some(fd.intent);
        d.notes = notes;
        d
    }
}

/// How an authorization entry's credentials are shown (§5.2).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Credentials {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_expiration_ledger: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub display: Display,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_invocations: Vec<Node>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuthDisplay {
    pub credentials: Credentials,
    /// The entry may be left out under §5.2's omission rule.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub omittable: bool,
    pub tree: Node,
}

impl Context {
    fn render_node(&self, inv: &SorobanAuthorizedInvocation, legacy: bool) -> Node {
        let display = match &inv.function {
            SorobanAuthorizedFunction::ContractFn(call) if !legacy => self.render_call(call, true),
            SorobanAuthorizedFunction::ContractFn(call) => {
                let mut d = self.render_call(call, true);
                d = Display {
                    described: false,
                    intent: None,
                    fields: vec![],
                    omitted: 0,
                    not_described: Some("legacy SOROBAN_CREDENTIALS_ADDRESS: out of scope".into()),
                    notes: vec![],
                    ..d
                };
                d
            }
            other => Display {
                described: false,
                intent: None,
                contract: String::new(),
                function: "create contract".into(),
                fields: vec![],
                omitted: 0,
                raw: vec![Line {
                    label: "arg 0".into(),
                    value: serde_json::to_string(other).unwrap_or_default(),
                }],
                not_described: Some("contract creation".into()),
                notes: vec![],
            },
        };
        Node {
            display,
            sub_invocations: inv
                .sub_invocations
                .iter()
                .map(|s| self.render_node(s, legacy))
                .collect(),
        }
    }

    /// §5.2 for one entry. `op_call` and `op_has_source` describe the enclosing operation, when known.
    pub fn render_auth(
        &self,
        entry: &SorobanAuthorizationEntry,
        op_call: Option<&InvokeContractArgs>,
        op_has_source: bool,
    ) -> AuthDisplay {
        let (credentials, legacy) = match &entry.credentials {
            SorobanCredentials::SourceAccount => (
                Credentials {
                    kind: "source_account".into(),
                    address: None,
                    signature_expiration_ledger: None,
                },
                false,
            ),
            SorobanCredentials::Address(c) => (
                Credentials {
                    kind: "address (legacy)".into(),
                    address: Some(addr(&c.address)),
                    signature_expiration_ledger: Some(c.signature_expiration_ledger),
                },
                true,
            ),
            SorobanCredentials::AddressV2(c) => (
                Credentials {
                    kind: "address_v2".into(),
                    address: Some(addr(&c.address)),
                    signature_expiration_ledger: Some(c.signature_expiration_ledger),
                },
                false,
            ),
            SorobanCredentials::AddressWithDelegates(c) => (
                Credentials {
                    kind: "address_with_delegates".into(),
                    address: Some(addr(&c.address_credentials.address)),
                    signature_expiration_ledger: Some(
                        c.address_credentials.signature_expiration_ledger,
                    ),
                },
                false,
            ),
        };
        let omittable = matches!(entry.credentials, SorobanCredentials::SourceAccount)
            && !op_has_source
            && entry.root_invocation.sub_invocations.is_empty()
            && matches!((&entry.root_invocation.function, op_call), (SorobanAuthorizedFunction::ContractFn(a), Some(b)) if a == b);
        AuthDisplay {
            credentials,
            omittable,
            tree: self.render_node(&entry.root_invocation, legacy),
        }
    }
}
