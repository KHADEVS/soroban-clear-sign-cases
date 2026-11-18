//! survey [ledgers]
//! Samples Mainnet ledgers through Soroban RPC and counts what is signed in Soroban calls:
//! which contracts and functions, by how many distinct signers, with which credentials, which
//! Stellar Asset Contract functions, the parameter types of the most used functions, and a few
//! value patterns. Writes survey/results.json and survey/SUMMARY.md.
use serde_json::{json, Value};
use soroban_clear_sign_cases::chain::{operations, resolve, spec_section, ContractRecord};
use soroban_clear_sign_cases::net::{rpc, RPC};
use soroban_clear_sign_cases::render::{network_id, MAINNET};
use soroban_clear_sign_cases::spec::{text, type_name, Spec};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use stellar_xdr::{
    FeeBumpTransactionInnerTx, HostFunction, InvokeContractArgs, Limits, MuxedAccount,
    OperationBody, ReadXdr, ScAddress, ScSpecEntry, ScSpecTypeDef, ScVal,
    SorobanAuthorizedFunction, SorobanAuthorizedInvocation, SorobanCredentials,
    TransactionEnvelope,
};

type Key = (String, String);

#[derive(Default)]
struct Call {
    count: u64,
    signers: BTreeSet<String>,
    examples: Vec<String>,
    /// A few argument lists, for value patterns.
    samples: Vec<Vec<ScVal>>,
}

#[derive(Default)]
struct Stats {
    ledgers: Vec<u32>,
    txs: u64,
    soroban_txs: u64,
    roots: BTreeMap<Key, Call>,
    /// (contract, function, credentials kind, depth) -> count
    auth_nodes: BTreeMap<(String, String, String, u32), u64>,
    /// Distinct addresses that authorized a function (the operation's source for source_account).
    auth_signers: BTreeMap<Key, BTreeSet<String>>,
    /// Argument counts seen in authorization nodes.
    auth_arg_counts: BTreeMap<Key, BTreeMap<usize, u64>>,
    credentials: BTreeMap<String, u64>,
}

fn cred_kind(c: &SorobanCredentials) -> String {
    let who = |a: &ScAddress| match a {
        ScAddress::Account(_) | ScAddress::MuxedAccount(_) => "G",
        ScAddress::Contract(_) | ScAddress::MuxedContract(_) => "C",
        _ => "?",
    };
    match c {
        SorobanCredentials::SourceAccount => "source_account".into(),
        SorobanCredentials::Address(a) => format!("address({})", who(&a.address)),
        SorobanCredentials::AddressV2(a) => format!("address_v2({})", who(&a.address)),
        SorobanCredentials::AddressWithDelegates(a) => format!(
            "address_with_delegates({})",
            who(&a.address_credentials.address)
        ),
    }
}

fn key(c: &InvokeContractArgs) -> Key {
    (
        c.contract_address.to_string(),
        text(c.function_name.0.as_slice()),
    )
}

fn tx_source(env: &TransactionEnvelope) -> String {
    match env {
        TransactionEnvelope::TxV0(e) => {
            MuxedAccount::Ed25519(e.tx.source_account_ed25519.clone()).to_string()
        }
        TransactionEnvelope::Tx(e) => e.tx.source_account.to_string(),
        TransactionEnvelope::TxFeeBump(e) => match &e.tx.inner_tx {
            FeeBumpTransactionInnerTx::Tx(i) => i.tx.source_account.to_string(),
        },
    }
}

fn walk(inv: &SorobanAuthorizedInvocation, cred: &str, signer: &str, depth: u32, s: &mut Stats) {
    if let SorobanAuthorizedFunction::ContractFn(c) = &inv.function {
        let k = key(c);
        *s.auth_nodes
            .entry((k.0.clone(), k.1.clone(), cred.to_owned(), depth))
            .or_default() += 1;
        s.auth_signers
            .entry(k.clone())
            .or_default()
            .insert(signer.to_owned());
        *s.auth_arg_counts
            .entry(k)
            .or_default()
            .entry(c.args.len())
            .or_default() += 1;
    }
    for sub in inv.sub_invocations.iter() {
        walk(sub, cred, signer, depth + 1, s);
    }
}

fn record(env: &TransactionEnvelope, hash: &str, s: &mut Stats) {
    let mut soroban = false;
    let source = tx_source(env);
    for op in operations(env) {
        let OperationBody::InvokeHostFunction(ihf) = &op.body else {
            continue;
        };
        let HostFunction::InvokeContract(c) = &ihf.host_function else {
            continue;
        };
        soroban = true;
        let op_source = op
            .source_account
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| source.clone());
        let call = s.roots.entry(key(c)).or_default();
        call.count += 1;
        call.signers.insert(op_source.clone());
        if call.examples.len() < 3 {
            call.examples.push(hash.to_owned());
        }
        if call.samples.len() < 300 {
            call.samples.push(c.args.to_vec());
        }
        for a in ihf.auth.iter() {
            let ck = cred_kind(&a.credentials);
            *s.credentials.entry(ck.clone()).or_default() += 1;
            let signer = match &a.credentials {
                SorobanCredentials::SourceAccount => op_source.clone(),
                SorobanCredentials::Address(c) | SorobanCredentials::AddressV2(c) => {
                    c.address.to_string()
                }
                SorobanCredentials::AddressWithDelegates(c) => {
                    c.address_credentials.address.to_string()
                }
            };
            walk(&a.root_invocation, &ck, &signer, 0, s);
        }
    }
    if soroban {
        s.soroban_txs += 1;
    }
}

fn sample_ledger(ledger: u32, s: &mut Stats) -> Result<(), String> {
    let mut params = json!({ "startLedger": ledger, "pagination": { "limit": 200 } });
    loop {
        let mut tries = 0;
        let res = loop {
            match rpc("getTransactions", params.clone()) {
                Ok(r) => break r,
                Err(e) if tries < 4 => {
                    tries += 1;
                    eprintln!("  retry {tries}: {e}");
                    std::thread::sleep(std::time::Duration::from_secs(2 * tries));
                }
                Err(e) => return Err(e),
            }
        };
        let txs = res["transactions"].as_array().cloned().unwrap_or_default();
        let mut done = txs.is_empty();
        for t in &txs {
            if t["ledger"].as_u64() != Some(u64::from(ledger)) {
                done = true;
                break;
            }
            s.txs += 1;
            if t["status"] != "SUCCESS" {
                continue;
            }
            if let Ok(env) = TransactionEnvelope::from_xdr_base64(
                t["envelopeXdr"].as_str().unwrap_or_default(),
                Limits::none(),
            ) {
                record(&env, t["txHash"].as_str().unwrap_or_default(), s);
            }
        }
        if done || txs.len() < 200 {
            return Ok(());
        }
        params = json!({ "pagination": { "cursor": res["cursor"], "limit": 200 } });
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// A struct UDT whose members are named "0".."n-1": soroban-sdk's tuple struct, encoded as SCV_VEC.
fn tuple_structs(spec: &Spec) -> BTreeSet<String> {
    spec.entries
        .iter()
        .filter_map(|e| match e {
            ScSpecEntry::UdtStructV0(st)
                if !st.fields.is_empty()
                    && st
                        .fields
                        .iter()
                        .enumerate()
                        .all(|(i, f)| text(f.name.as_slice()) == i.to_string()) =>
            {
                Some(text(st.name.as_slice()))
            }
            _ => None,
        })
        .collect()
}

fn mentions(
    t: &ScSpecTypeDef,
    names: &BTreeSet<String>,
    spec: &Spec,
    seen: &mut BTreeSet<String>,
) -> bool {
    match t {
        ScSpecTypeDef::Udt(u) => {
            let n = text(u.name.as_slice());
            if names.contains(&n) {
                return true;
            }
            if !seen.insert(n.clone()) {
                return false;
            }
            spec.struct_named(&n).is_some_and(|st| {
                st.fields
                    .iter()
                    .any(|f| mentions(&f.type_, names, spec, seen))
            })
        }
        ScSpecTypeDef::Vec(v) => mentions(&v.element_type, names, spec, seen),
        ScSpecTypeDef::Option(o) => mentions(&o.value_type, names, spec, seen),
        ScSpecTypeDef::Tuple(tp) => tp
            .value_types
            .iter()
            .any(|x| mentions(x, names, spec, seen)),
        ScSpecTypeDef::Map(m) => {
            mentions(&m.key_type, names, spec, seen) || mentions(&m.value_type, names, spec, seen)
        }
        _ => false,
    }
}

fn short(id: &str) -> String {
    format!("{}…{}", &id[..4], &id[id.len() - 4..])
}

fn main() {
    let n: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(120);
    let health = rpc("getHealth", json!({})).unwrap();
    let (oldest, latest) = (
        health["oldestLedger"].as_u64().unwrap() as u32 + 50,
        health["latestLedger"].as_u64().unwrap() as u32 - 10,
    );
    let step = (latest - oldest) / n;
    let mut s = Stats::default();
    for i in 0..n {
        let ledger = oldest + i * step;
        s.ledgers.push(ledger);
        if let Err(e) = sample_ledger(ledger, &mut s) {
            eprintln!("ledger {ledger}: {e}");
        }
        if i % 10 == 0 {
            eprintln!("{i}/{n} ledgers, {} txs, {} Soroban", s.txs, s.soroban_txs);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    // Resolve every contract seen.
    let mut contracts: BTreeSet<String> = s.roots.keys().map(|k| k.0.clone()).collect();
    contracts.extend(s.auth_nodes.keys().map(|k| k.0.clone()));
    let addrs: Vec<ScAddress> = contracts.iter().filter_map(|c| c.parse().ok()).collect();
    let resolved = resolve(&addrs, &network_id(MAINNET)).unwrap();
    let kinds: BTreeMap<String, ContractRecord> =
        resolved.into_iter().map(|(id, r, _)| (id, r)).collect();
    let label = |c: &str| match kinds.get(c) {
        Some(ContractRecord::StellarAsset { asset }) => {
            format!("SAC {}", asset.split(':').next().unwrap_or(asset))
        }
        Some(ContractRecord::Wasm { wasm_hash }) => format!("Wasm {}", &wasm_hash[..8]),
        Some(ContractRecord::ExternalRef { .. }) => "external ref".into(),
        _ => "unresolved".into(),
    };
    let is_sac = |c: &str| matches!(kinds.get(c), Some(ContractRecord::StellarAsset { .. }));
    let signers = |k: &Key| -> usize {
        let mut set: BTreeSet<&String> = BTreeSet::new();
        if let Some(c) = s.roots.get(k) {
            set.extend(c.signers.iter());
        }
        if let Some(a) = s.auth_signers.get(k) {
            set.extend(a.iter());
        }
        set.len()
    };

    // Usage per (contract, function), roots and authorization nodes together.
    let mut usage: BTreeMap<Key, (u64, u64)> = BTreeMap::new();
    for (k, c) in &s.roots {
        usage.entry(k.clone()).or_default().0 += c.count;
    }
    for ((c, f, _, _), n) in &s.auth_nodes {
        usage.entry((c.clone(), f.clone())).or_default().1 += n;
    }
    let mut top: Vec<(&Key, &(u64, u64))> = usage.iter().filter(|(k, _)| !is_sac(&k.0)).collect();
    // Ranked by distinct signers: a bot calling one function thousands of times counts once.
    top.sort_by(|a, b| {
        signers(b.0)
            .cmp(&signers(a.0))
            .then((b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)))
    });

    // Specs of the most used Wasm functions.
    let mut specs: BTreeMap<String, Result<Spec, String>> = BTreeMap::new();
    let mut functions = Vec::new();
    for (k, (roots, nodes)) in top.iter().take(40) {
        let (c, f) = (&k.0, &k.1);
        let Some(ContractRecord::Wasm { wasm_hash }) = kinds.get(c) else {
            continue;
        };
        let spec = specs
            .entry(wasm_hash.clone())
            .or_insert_with(|| spec_section(wasm_hash).and_then(|b| Spec::from_xdr(&b)));
        let Ok(spec) = spec else { continue };
        let Some(func) = spec.functions_named(f).first().cloned() else {
            continue;
        };
        let tuples = tuple_structs(spec);
        let inputs: Vec<Value> = Spec::inputs(func)
            .iter()
            .map(|(n, t)| json!({ "name": n, "type": type_name(t), "reaches_tuple_struct": mentions(t, &tuples, spec, &mut BTreeSet::new()) }))
            .collect();
        let arg_counts = s.auth_arg_counts.get(*k).cloned().unwrap_or_default();
        let spec_arity = func.inputs.len();
        let mismatched: u64 = arg_counts
            .iter()
            .filter(|(n, _)| **n != spec_arity)
            .map(|(_, c)| *c)
            .sum();
        functions.push(json!({
            "contract": c, "label": label(c), "wasm_hash": wasm_hash, "function": f, "distinct_signers": signers(k),
            "root_calls": roots, "auth_nodes": nodes, "inputs": inputs, "auth_nodes_with_other_arity": mismatched,
            "examples": s.roots.get(*k).map(|x| x.examples.clone()).unwrap_or_default(),
        }));
    }

    // Value patterns: u64/timepoint inputs whose name suggests a time.
    let mut time_values = Vec::new();
    for fobj in &functions {
        let k = (
            fobj["contract"].as_str().unwrap().to_owned(),
            fobj["function"].as_str().unwrap().to_owned(),
        );
        let Some(call) = s.roots.get(&k) else {
            continue;
        };
        for (i, inp) in fobj["inputs"].as_array().unwrap().iter().enumerate() {
            let name = inp["name"].as_str().unwrap().to_lowercase();
            let t = inp["type"].as_str().unwrap();
            if !(t == "u64" || t == "timepoint")
                || !["deadline", "expir", "until", "time", "valid"]
                    .iter()
                    .any(|w| name.contains(w))
            {
                continue;
            }
            let vals: Vec<u64> = call
                .samples
                .iter()
                .filter_map(|a| match a.get(i) {
                    Some(ScVal::U64(v)) => Some(*v),
                    Some(ScVal::Timepoint(v)) => Some(v.0),
                    _ => None,
                })
                .collect();
            let max_count = vals.iter().filter(|v| **v == u64::MAX).count();
            let beyond_9999 = vals.iter().filter(|v| **v > 253_402_300_799).count();
            time_values.push(json!({ "contract": k.0, "function": k.1, "input": inp["name"], "samples": vals.len(), "u64_max": max_count, "past_year_9999": beyond_9999, "min": vals.iter().min(), "max": vals.iter().max() }));
        }
    }

    // Stellar Asset Contract functions, by where they appear, and by distinct signers.
    let mut sac: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut sac_signers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ((c, f), call) in &s.roots {
        if is_sac(c) {
            *sac.entry(f.clone())
                .or_default()
                .entry("root call".into())
                .or_default() += call.count;
            sac_signers
                .entry(f.clone())
                .or_default()
                .extend(call.signers.iter().cloned());
        }
    }
    for ((c, f, cred, depth), n) in &s.auth_nodes {
        if is_sac(c) {
            let place = format!(
                "auth node, {}, {}",
                if *depth == 0 {
                    "root"
                } else {
                    "sub-invocation"
                },
                cred
            );
            *sac.entry(f.clone()).or_default().entry(place).or_default() += n;
        }
    }
    for ((c, f), set) in &s.auth_signers {
        if is_sac(c) {
            sac_signers
                .entry(f.clone())
                .or_default()
                .extend(set.iter().cloned());
        }
    }
    let sac_signer_counts: BTreeMap<String, usize> = sac_signers
        .iter()
        .map(|(f, set)| (f.clone(), set.len()))
        .collect();

    let results = json!({
        "rpc": RPC,
        "ledgers_sampled": s.ledgers.len(), "ledger_range": [s.ledgers.first(), s.ledgers.last()],
        "transactions": s.txs, "soroban_invoke_transactions": s.soroban_txs,
        "credentials": s.credentials,
        "stellar_asset_contract_functions": sac, "stellar_asset_contract_distinct_signers": sac_signer_counts,
        "top_wasm_functions": functions, "time_inputs": time_values,
        "top_calls": top.iter().take(30).map(|(k, (r, n))| json!({"contract": k.0, "label": label(&k.0), "function": k.1, "root_calls": r, "auth_nodes": n, "distinct_signers": signers(k)})).collect::<Vec<_>>(),
    });
    fs::create_dir_all("survey").unwrap();
    fs::write(
        "survey/results.json",
        serde_json::to_string_pretty(&results).unwrap() + "\n",
    )
    .unwrap();

    let mut md = String::new();
    let _ = writeln!(md, "# Survey of Soroban calls on Mainnet\n");
    let _ = writeln!(
        md,
        "{} ledgers sampled evenly between {} and {} through `{RPC}`: {} transactions, {} successful ones with a Soroban contract call. \"Distinct signers\" counts the accounts that sent or authorized a call, so a bot that repeats one call counts once.\n",
        s.ledgers.len(),
        s.ledgers.first().unwrap(),
        s.ledgers.last().unwrap(),
        s.txs,
        s.soroban_txs
    );
    let _ = writeln!(
        md,
        "## Authorization entries by credentials\n\n| Credentials | Entries |\n| --- | --- |"
    );
    for (k, v) in &s.credentials {
        let _ = writeln!(md, "| {k} | {v} |");
    }
    let _ = writeln!(
        md,
        "\n## Stellar Asset Contract functions\n\n| Function | Distinct signers |\n| --- | --- |"
    );
    for (f, n) in &sac_signer_counts {
        let _ = writeln!(md, "| `{f}` | {n} |");
    }
    let _ = writeln!(md, "\n| Function | Where | Count |\n| --- | --- | --- |");
    for (f, places) in &sac {
        for (p, n) in places {
            let _ = writeln!(md, "| `{f}` | {p} | {n} |");
        }
    }
    let _ = writeln!(md, "\n## Most used Wasm functions and their inputs\n\n| Contract | Function | Distinct signers | Root calls | Auth nodes | Inputs | Auth nodes with another arity |\n| --- | --- | --- | --- | --- | --- | --- |");
    for f in &functions {
        let inputs: Vec<String> = f["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                format!(
                    "{}: {}{}",
                    i["name"].as_str().unwrap(),
                    i["type"].as_str().unwrap(),
                    if i["reaches_tuple_struct"].as_bool().unwrap() {
                        " (tuple struct)"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        let _ = writeln!(
            md,
            "| {} | `{}` | {} | {} | {} | {} | {} |",
            short(f["contract"].as_str().unwrap()),
            f["function"].as_str().unwrap(),
            f["distinct_signers"],
            f["root_calls"],
            f["auth_nodes"],
            inputs.join(", "),
            f["auth_nodes_with_other_arity"]
        );
    }
    let _ = writeln!(md, "\n## Time inputs\n\n| Contract | Function | Input | Samples | u64::MAX | Past year 9999 |\n| --- | --- | --- | --- | --- | --- |");
    for t in &time_values {
        let _ = writeln!(
            md,
            "| {} | `{}` | `{}` | {} | {} | {} |",
            short(t["contract"].as_str().unwrap()),
            t["function"].as_str().unwrap(),
            t["input"].as_str().unwrap(),
            t["samples"],
            t["u64_max"],
            t["past_year_9999"]
        );
    }
    fs::write("survey/SUMMARY.md", md).unwrap();
    eprintln!("done: {} txs, {} Soroban", s.txs, s.soroban_txs);
}
