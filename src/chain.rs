//! Reading real Mainnet data: transactions from Horizon, contract instances and code from Soroban RPC.
//! Used by the `fetch` and `survey` tools only; the cases themselves are checked offline.
use crate::net::{get_json, rpc, HORIZON};
use crate::render::asset_contract_id;
use crate::spec::{custom_section, text};
use serde::{Deserialize, Serialize};
use serde_json::json;
use stellar_xdr::{
    AccountId, AlphaNum12, AlphaNum4, Asset, AssetCode12, AssetCode4, ContractDataDurability,
    ContractExecutable, Hash, LedgerEntryData, LedgerKey, LedgerKeyContractCode,
    LedgerKeyContractData, Limits, OperationBody, ReadXdr, ScAddress, ScVal,
    SorobanAuthorizedInvocation, TransactionEnvelope, WriteXdr,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContractRecord {
    Wasm {
        wasm_hash: String,
    },
    StellarAsset {
        asset: String,
    },
    /// CAP-85: the Wasm hash comes from the owner's entry for this tag.
    ExternalRef {
        owner: String,
        tag: String,
        wasm_hash: Option<String>,
    },
    Missing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenRecord {
    pub decimals: Option<u32>,
    pub symbol: Option<String>,
    pub name: Option<String>,
}

pub fn horizon_envelope(hash: &str) -> Result<(TransactionEnvelope, u64, String), String> {
    let tx = get_json(&format!("{HORIZON}/transactions/{hash}"))?;
    let b64 = tx["envelope_xdr"]
        .as_str()
        .ok_or("no envelope_xdr")?
        .to_owned();
    let env =
        TransactionEnvelope::from_xdr_base64(&b64, Limits::none()).map_err(|e| e.to_string())?;
    Ok((env, tx["ledger"].as_u64().unwrap_or(0), b64))
}

/// Operations of an envelope (the inner transaction for a fee bump), with their own source if set.
pub fn operations(env: &TransactionEnvelope) -> Vec<stellar_xdr::Operation> {
    match env {
        TransactionEnvelope::TxV0(e) => e.tx.operations.to_vec(),
        TransactionEnvelope::Tx(e) => e.tx.operations.to_vec(),
        TransactionEnvelope::TxFeeBump(e) => match &e.tx.inner_tx {
            stellar_xdr::FeeBumpTransactionInnerTx::Tx(i) => i.tx.operations.to_vec(),
        },
    }
}

/// Every contract address an envelope's Soroban operations touch: callees and authorization nodes.
pub fn contracts_in(env: &TransactionEnvelope) -> Vec<ScAddress> {
    fn walk(inv: &SorobanAuthorizedInvocation, out: &mut Vec<ScAddress>) {
        if let stellar_xdr::SorobanAuthorizedFunction::ContractFn(c) = &inv.function {
            out.push(c.contract_address.clone());
            collect_val_addresses(&c.args, out);
        }
        for s in inv.sub_invocations.iter() {
            walk(s, out);
        }
    }
    let mut out = Vec::new();
    for op in operations(env) {
        if let OperationBody::InvokeHostFunction(ihf) = &op.body {
            if let stellar_xdr::HostFunction::InvokeContract(c) = &ihf.host_function {
                out.push(c.contract_address.clone());
                collect_val_addresses(&c.args, &mut out);
            }
            for a in ihf.auth.iter() {
                walk(&a.root_invocation, &mut out);
            }
        }
    }
    out.retain(|a| matches!(a, ScAddress::Contract(_)));
    out.sort_by_key(|a| a.to_string());
    out.dedup();
    out
}

/// Contract addresses inside argument values (token references live there).
fn collect_val_addresses(vals: &[ScVal], out: &mut Vec<ScAddress>) {
    for v in vals {
        match v {
            ScVal::Address(a) => out.push(a.clone()),
            ScVal::Vec(Some(vec)) => collect_val_addresses(vec, out),
            ScVal::Map(Some(m)) => {
                for e in m.iter() {
                    collect_val_addresses(std::slice::from_ref(&e.key), out);
                    collect_val_addresses(std::slice::from_ref(&e.val), out);
                }
            }
            _ => {}
        }
    }
}

fn get_entries(keys: &[LedgerKey]) -> Result<Vec<(LedgerKey, LedgerEntryData)>, String> {
    let b64: Vec<String> = keys
        .iter()
        .map(|k| k.to_xdr_base64(Limits::none()).unwrap())
        .collect();
    let res = rpc("getLedgerEntries", json!({ "keys": b64 }))?;
    let mut out = Vec::new();
    for e in res["entries"].as_array().cloned().unwrap_or_default() {
        let key = LedgerKey::from_xdr_base64(e["key"].as_str().unwrap_or_default(), Limits::none())
            .map_err(|e| e.to_string())?;
        let data =
            LedgerEntryData::from_xdr_base64(e["xdr"].as_str().unwrap_or_default(), Limits::none())
                .map_err(|e| e.to_string())?;
        out.push((key, data));
    }
    Ok(out)
}

fn instance_key(contract: &ScAddress) -> LedgerKey {
    LedgerKey::ContractData(LedgerKeyContractData {
        contract: contract.clone(),
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    })
}

fn metadata_of(storage: &Option<stellar_xdr::ScMap>) -> Option<TokenRecord> {
    let map = storage.as_ref()?;
    let meta = map
        .iter()
        .find(|e| matches!(&e.key, ScVal::Symbol(s) if text(s.0.as_slice()) == "METADATA"))?;
    let ScVal::Map(Some(m)) = &meta.val else {
        return None;
    };
    let get = |k: &str| {
        m.iter()
            .find(|e| matches!(&e.key, ScVal::Symbol(s) if text(s.0.as_slice()) == k))
            .map(|e| e.val.clone())
    };
    Some(TokenRecord {
        decimals: match get("decimal") {
            Some(ScVal::U32(d)) => Some(d),
            _ => None,
        },
        symbol: match get("symbol") {
            Some(ScVal::String(s)) => Some(text(s.0.as_slice())),
            _ => None,
        },
        name: match get("name") {
            Some(ScVal::String(s)) => Some(text(s.0.as_slice())),
            _ => None,
        },
    })
}

/// Parses "native" or "CODE:ISSUER" into an asset.
pub fn parse_asset(s: &str) -> Result<Asset, String> {
    if s == "native" {
        return Ok(Asset::Native);
    }
    let (code, issuer) = s.split_once(':').ok_or("asset is not CODE:ISSUER")?;
    let issuer: AccountId = issuer.parse().map_err(|_| "bad issuer")?;
    if code.len() <= 4 {
        let mut c = [0u8; 4];
        c[..code.len()].copy_from_slice(code.as_bytes());
        Ok(Asset::CreditAlphanum4(AlphaNum4 {
            asset_code: AssetCode4(c),
            issuer,
        }))
    } else {
        let mut c = [0u8; 12];
        c[..code.len()].copy_from_slice(code.as_bytes());
        Ok(Asset::CreditAlphanum12(AlphaNum12 {
            asset_code: AssetCode12(c),
            issuer,
        }))
    }
}

/// Resolves contracts to records (and token metadata read from their instance storage).
pub fn resolve(
    contracts: &[ScAddress],
    network_id: &[u8; 32],
) -> Result<Vec<(String, ContractRecord, Option<TokenRecord>)>, String> {
    let mut out = Vec::new();
    for chunk in contracts.chunks(50) {
        let keys: Vec<LedgerKey> = chunk.iter().map(instance_key).collect();
        let entries = get_entries(&keys)?;
        for c in chunk {
            let id = c.to_string();
            let found = entries.iter().find(|(k, _)| *k == instance_key(c));
            let Some((_, LedgerEntryData::ContractData(d))) = found else {
                out.push((id, ContractRecord::Missing, None));
                continue;
            };
            let ScVal::ContractInstance(inst) = &d.val else {
                out.push((id, ContractRecord::Missing, None));
                continue;
            };
            let meta = metadata_of(&inst.storage);
            let record = match &inst.executable {
                ContractExecutable::Wasm(h) => ContractRecord::Wasm {
                    wasm_hash: hex::encode(h.0),
                },
                ContractExecutable::StellarAsset => {
                    let name = meta
                        .as_ref()
                        .and_then(|m| m.name.clone())
                        .unwrap_or_default();
                    let ok = parse_asset(&name)
                        .map(|a| asset_contract_id(network_id, &a) == id)
                        .unwrap_or(false);
                    if !ok {
                        return Err(format!(
                            "{id}: SAC metadata name \"{name}\" does not hash to the contract ID"
                        ));
                    }
                    ContractRecord::StellarAsset { asset: name }
                }
                ContractExecutable::ExternalRef(r) => {
                    let tag = text(r.tag.0.as_slice());
                    let key = LedgerKey::ContractData(LedgerKeyContractData {
                        contract: r.executable_owner.clone(),
                        key: ScVal::ExecutableTag(r.tag.clone()),
                        durability: ContractDataDurability::Persistent,
                    });
                    let wasm_hash = get_entries(&[key])?.into_iter().find_map(|(_, d)| match d {
                        LedgerEntryData::ContractData(cd) => match cd.val {
                            ScVal::Bytes(b) if b.len() == 32 => Some(hex::encode(b.as_slice())),
                            _ => None,
                        },
                        _ => None,
                    });
                    ContractRecord::ExternalRef {
                        owner: r.executable_owner.to_string(),
                        tag,
                        wasm_hash,
                    }
                }
            };
            let token = match record {
                ContractRecord::StellarAsset { .. } => None,
                _ => meta,
            };
            out.push((id, record, token));
        }
    }
    Ok(out)
}

/// Downloads a Wasm and returns its contractspecv0 section bytes.
pub fn spec_section(wasm_hash: &str) -> Result<Vec<u8>, String> {
    let mut h = [0u8; 32];
    hex::decode_to_slice(wasm_hash, &mut h).map_err(|e| e.to_string())?;
    let key = LedgerKey::ContractCode(LedgerKeyContractCode { hash: Hash(h) });
    let entries = get_entries(&[key])?;
    let Some((_, LedgerEntryData::ContractCode(code))) = entries.into_iter().next() else {
        return Err(format!("code {wasm_hash} not found"));
    };
    let wasm = code.code.as_slice();
    Ok(custom_section(wasm, "contractspecv0")?
        .ok_or("no contractspecv0 section")?
        .to_vec())
}
