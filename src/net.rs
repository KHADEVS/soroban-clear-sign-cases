//! Minimal HTTP helpers for Horizon and Soroban RPC (used only by the fetch tools).
use serde_json::{json, Value};

pub const HORIZON: &str = "https://horizon.stellar.org";
pub const RPC: &str = "https://mainnet.sorobanrpc.com";

pub fn get_json(url: &str) -> Result<Value, String> {
    ureq::get(url)
        .set("User-Agent", "soroban-clear-sign-cases")
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?
        .into_json()
        .map_err(|e| format!("GET {url}: {e}"))
}

pub fn rpc(method: &str, params: Value) -> Result<Value, String> {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    let v: Value = ureq::post(RPC)
        .set("User-Agent", "soroban-clear-sign-cases")
        .send_json(body)
        .map_err(|e| format!("{method}: {e}"))?
        .into_json()
        .map_err(|e| format!("{method}: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!("{method}: {err}"));
    }
    v.get("result")
        .cloned()
        .ok_or_else(|| format!("{method}: no result"))
}
