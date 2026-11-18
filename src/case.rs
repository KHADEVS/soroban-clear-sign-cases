//! A case: inputs (a real transaction or a crafted invocation, the contracts it touches, token
//! metadata, descriptors) and what a wallet following the draft shows for them.
use crate::chain::{operations, parse_asset, ContractRecord};
use crate::descriptor::parse_document;
use crate::render::{network_id, AuthDisplay, Context, Display, Resolution, TokenMeta, MAINNET};
use crate::spec::Spec;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use stellar_xdr::{
    HostFunction, InvokeContractArgs, Limits, OperationBody, ReadXdr, SorobanAuthorizationEntry,
    TransactionEnvelope,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    pub title: String,
    pub draft_sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<InvokeContractArgs>,
    /// Standalone authorization entries (as signed through SEP-43 signAuthEntry).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auth: Vec<SorobanAuthorizationEntry>,
    /// Contract records that override real/contracts.json.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contracts: BTreeMap<String, ContractRecord>,
    /// Token metadata that overrides real/tokens.json.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, TokenMeta>,
    #[serde(default)]
    pub descriptors: Vec<String>,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Outcome>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Outcome {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents_ignored: Vec<String>,
    pub operations: Vec<OperationOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub standalone_auth: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OperationOutcome {
    pub invocation: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auth: Vec<Value>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let s = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

/// Loads the spec of a Wasm hash from specs/ (crafted) or real/specs/ (read from Mainnet).
pub fn load_spec(root: &Path, hash: &str) -> Result<Spec, String> {
    let crafted = root.join("specs").join(format!("{hash}.json"));
    let real = root.join("real/specs").join(format!("{hash}.json"));
    let path = if crafted.exists() { crafted } else { real };
    let v = read_json(&path)?;
    if let Some(entries) = v.get("entries") {
        let entries = serde_json::from_value(entries.clone())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(Spec { entries });
    }
    let b64 = v["contractspecv0"]
        .as_str()
        .ok_or(format!("{}: no contractspecv0", path.display()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    Spec::from_xdr(&bytes)
}

impl Case {
    pub fn context(&self, root: &Path) -> Result<(Context, Vec<String>), String> {
        let mut ctx = Context {
            network_id: network_id(MAINNET),
            ..Default::default()
        };
        let mut records: BTreeMap<String, ContractRecord> = BTreeMap::new();
        if let Value::Object(m) = read_json(&root.join("real/contracts.json"))? {
            for (id, v) in m {
                if let Ok(r) = serde_json::from_value::<ContractRecord>(v) {
                    records.insert(id, r);
                }
            }
        }
        records.extend(self.contracts.clone());
        for (id, r) in records {
            let res = match r {
                ContractRecord::Wasm { wasm_hash }
                | ContractRecord::ExternalRef {
                    wasm_hash: Some(wasm_hash),
                    ..
                } => Resolution::Wasm {
                    spec: load_spec(root, &wasm_hash),
                    hash: wasm_hash,
                },
                ContractRecord::StellarAsset { asset } => Resolution::StellarAsset {
                    asset: parse_asset(&asset).ok(),
                },
                _ => continue,
            };
            ctx.contracts.insert(id, res);
        }
        if let Value::Object(m) = read_json(&root.join("real/tokens.json"))? {
            for (id, v) in m {
                ctx.tokens.insert(
                    id,
                    TokenMeta {
                        decimals: v["decimals"].as_u64().map(|d| d as u32),
                        symbol: v["symbol"].as_str().map(str::to_owned),
                        source: "contract".into(),
                    },
                );
            }
        }
        ctx.tokens.extend(self.tokens.clone());
        let mut ignored = Vec::new();
        for d in &self.descriptors {
            let bytes = fs::read(root.join(d)).map_err(|e| format!("{d}: {e}"))?;
            match parse_document(&bytes) {
                Ok(doc) => ctx.documents.push(doc),
                Err(e) => ignored.push(format!("{d}: {e}")),
            }
        }
        Ok((ctx, ignored))
    }

    pub fn run(&self, root: &Path) -> Result<Outcome, String> {
        let (ctx, documents_ignored) = self.context(root)?;
        let mut ops = Vec::new();
        if let Some(t) = &self.transaction {
            let v = read_json(&root.join(t))?;
            let env = TransactionEnvelope::from_xdr_base64(
                v["envelope_xdr"].as_str().unwrap_or_default(),
                Limits::none(),
            )
            .map_err(|e| e.to_string())?;
            for op in operations(&env) {
                let OperationBody::InvokeHostFunction(ihf) = &op.body else {
                    continue;
                };
                let HostFunction::InvokeContract(call) = &ihf.host_function else {
                    continue;
                };
                let invocation = serde_json::to_value(ctx.render_call(call, false)).unwrap();
                let auth = ihf
                    .auth
                    .iter()
                    .map(|a| {
                        serde_json::to_value(ctx.render_auth(
                            a,
                            Some(call),
                            op.source_account.is_some(),
                        ))
                        .unwrap()
                    })
                    .collect();
                ops.push(OperationOutcome { invocation, auth });
            }
        }
        if let Some(call) = &self.invocation {
            ops.push(OperationOutcome {
                invocation: serde_json::to_value(ctx.render_call(call, false)).unwrap(),
                auth: vec![],
            });
        }
        let standalone_auth = self
            .auth
            .iter()
            .map(|a| serde_json::to_value(ctx.render_auth(a, None, false)).unwrap())
            .collect();
        Ok(Outcome {
            documents_ignored,
            operations: ops,
            standalone_auth,
        })
    }
}

/// A plain-text view of an outcome, in the style of the draft's examples.
pub fn to_text(o: &Outcome) -> String {
    let mut out = String::new();
    for d in &o.documents_ignored {
        out.push_str(&format!("Descriptor ignored: {d}\n\n"));
    }
    for (i, op) in o.operations.iter().enumerate() {
        if o.operations.len() > 1 {
            out.push_str(&format!("Operation {}\n", i + 1));
        }
        let d: Display = serde_json::from_value(op.invocation.clone()).unwrap();
        display_text(&d, "", &mut out);
        for (j, a) in op.auth.iter().enumerate() {
            let a: AuthDisplay = serde_json::from_value(a.clone()).unwrap();
            auth_text(&a, j, &mut out);
        }
    }
    for (j, a) in o.standalone_auth.iter().enumerate() {
        let a: AuthDisplay = serde_json::from_value(a.clone()).unwrap();
        auth_text(&a, j, &mut out);
    }
    out
}

fn auth_text(a: &AuthDisplay, j: usize, out: &mut String) {
    let who = match (
        &a.credentials.address,
        a.credentials.signature_expiration_ledger,
    ) {
        (Some(addr), Some(exp)) => format!(
            "{} {addr}, signature expires at ledger {exp}",
            a.credentials.kind
        ),
        _ => "the operation's source account".to_owned(),
    };
    out.push_str(&format!(
        "\nAuthorization entry {}: {who}{}\n",
        j + 1,
        if a.omittable {
            " (may be omitted: same call as above, no sub-invocation)"
        } else {
            ""
        }
    ));
    node_text(&a.tree, "  ", out);
}

fn node_text(n: &crate::render::Node, indent: &str, out: &mut String) {
    display_text(&n.display, indent, out);
    for s in &n.sub_invocations {
        node_text(s, &format!("{indent}  "), out);
    }
}

fn display_text(d: &Display, indent: &str, out: &mut String) {
    let line = |out: &mut String, k: &str, v: &str| out.push_str(&format!("{indent}{k:<18} {v}\n"));
    match &d.intent {
        Some(i) => out.push_str(&format!("{indent}{i}\n")),
        None => out.push_str(&format!(
            "{indent}[not described: {}]\n",
            d.not_described.clone().unwrap_or_default()
        )),
    }
    line(out, "Contract", &d.contract);
    line(out, "Function", &d.function);
    if d.described {
        for l in &d.fields {
            line(out, &l.label, &l.value);
        }
        if d.omitted > 0 {
            out.push_str(&format!(
                "{indent}+{} more parameter{} (raw view)\n",
                d.omitted,
                if d.omitted == 1 { "" } else { "s" }
            ));
        }
    } else {
        for l in &d.raw {
            line(out, &l.label, &l.value);
        }
    }
    for n in &d.notes {
        out.push_str(&format!("{indent}note: {n}\n"));
    }
}
