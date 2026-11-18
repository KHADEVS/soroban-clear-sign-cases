//! fetch <name> <tx hash> [note]
//! Saves a Mainnet transaction under real/<name>.json, and the contracts it touches under
//! real/contracts.json, real/tokens.json and real/specs/<wasm hash>.json.
use base64::Engine;
use serde_json::{json, Map, Value};
use soroban_clear_sign_cases::chain::{
    contracts_in, horizon_envelope, resolve, spec_section, ContractRecord,
};
use soroban_clear_sign_cases::render::{network_id, MAINNET};
use std::fs;
use std::path::Path;

fn load(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(path: &Path, v: &impl serde::Serialize) {
    let mut s = serde_json::to_string_pretty(v).unwrap();
    s.push('\n');
    fs::write(path, s).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--contract") {
        return fetch_contracts(&args[2..]);
    }
    let (name, hash) = (args.get(1).expect("name"), args.get(2).expect("tx hash"));
    let note = args.get(3).cloned().unwrap_or_default();
    let (env, ledger, b64) = horizon_envelope(hash).unwrap();
    fs::create_dir_all("real/specs").unwrap();
    save(
        &Path::new("real").join(format!("{name}.json")),
        &json!({ "hash": hash, "ledger": ledger, "note": note, "envelope_xdr": b64 }),
    );

    let contracts = contracts_in(&env);
    store(&contracts, ledger);
}

fn fetch_contracts(ids: &[String]) {
    let addrs: Vec<stellar_xdr::ScAddress> = ids
        .iter()
        .map(|i| i.parse().expect("contract strkey"))
        .collect();
    let latest = soroban_clear_sign_cases::net::rpc("getLatestLedger", json!({})).unwrap()
        ["sequence"]
        .as_u64()
        .unwrap();
    store(&addrs, latest);
}

fn store(contracts: &[stellar_xdr::ScAddress], ledger: u64) {
    let resolved = resolve(contracts, &network_id(MAINNET)).unwrap();
    let (cpath, tpath) = (
        Path::new("real/contracts.json"),
        Path::new("real/tokens.json"),
    );
    let (mut cmap, mut tmap) = (load(cpath), load(tpath));
    for (id, record, token) in resolved {
        println!("{id}: {}", serde_json::to_string(&record).unwrap());
        let wasm = match &record {
            ContractRecord::Wasm { wasm_hash } => Some(wasm_hash.clone()),
            ContractRecord::ExternalRef { wasm_hash, .. } => wasm_hash.clone(),
            _ => None,
        };
        if let Some(h) = wasm {
            let p = Path::new("real/specs").join(format!("{h}.json"));
            if !p.exists() {
                match spec_section(&h) {
                    Ok(bytes) => save(
                        &p,
                        &json!({ "wasm_hash": h, "read_at_ledger": ledger, "contractspecv0": base64::engine::general_purpose::STANDARD.encode(bytes) }),
                    ),
                    Err(e) => eprintln!("  spec {h}: {e}"),
                }
            }
        }
        let mut entry = serde_json::to_value(&record).unwrap();
        entry["seen_in_ledger"] = json!(ledger);
        cmap.insert(id.clone(), entry);
        if let Some(t) = token {
            tmap.insert(id, serde_json::to_value(t).unwrap());
        }
    }
    save(cpath, &cmap);
    save(tpath, &tmap);
}
