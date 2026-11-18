//! findtx <function> <sub-invocation function> [ledgers back]
//! Prints recent Mainnet transactions whose authorization tree has `function` with a
//! sub-invocation named `sub-invocation function` (search helper for new cases).
use serde_json::json;
use soroban_clear_sign_cases::chain::operations;
use soroban_clear_sign_cases::net::rpc;
use soroban_clear_sign_cases::spec::text;
use stellar_xdr::{
    Limits, OperationBody, ReadXdr, SorobanAuthorizedFunction, SorobanAuthorizedInvocation,
    TransactionEnvelope,
};

fn name(inv: &SorobanAuthorizedInvocation) -> String {
    match &inv.function {
        SorobanAuthorizedFunction::ContractFn(c) => text(c.function_name.0.as_slice()),
        _ => String::new(),
    }
}

fn has(inv: &SorobanAuthorizedInvocation, sub: &str) -> bool {
    inv.sub_invocations
        .iter()
        .any(|s| name(s) == sub || has(s, sub))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (f, sub) = (&a[1], &a[2]);
    let back: u32 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(30);
    let latest = rpc("getLatestLedger", json!({})).unwrap()["sequence"]
        .as_u64()
        .unwrap() as u32;
    let mut found = 0;
    for ledger in (latest - back..latest - 2).step_by(7) {
        let res = rpc(
            "getTransactions",
            json!({ "startLedger": ledger, "pagination": { "limit": 200 } }),
        )
        .unwrap();
        for t in res["transactions"].as_array().unwrap() {
            if t["status"] != "SUCCESS" {
                continue;
            }
            let Ok(env) = TransactionEnvelope::from_xdr_base64(
                t["envelopeXdr"].as_str().unwrap(),
                Limits::none(),
            ) else {
                continue;
            };
            for op in operations(&env) {
                let OperationBody::InvokeHostFunction(ihf) = &op.body else {
                    continue;
                };
                for e in ihf.auth.iter() {
                    if name(&e.root_invocation) == *f && has(&e.root_invocation, sub) {
                        println!("{} ledger {}", t["txHash"].as_str().unwrap(), t["ledger"]);
                        found += 1;
                    }
                }
            }
        }
        if found >= 3 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}
