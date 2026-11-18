use stellar_xdr::{Limits, ReadXdr, TransactionEnvelope};
fn main() {
    let hash = std::env::args().nth(1).expect("tx hash");
    let tx = soroban_clear_sign_cases::net::get_json(&format!(
        "{}/transactions/{hash}",
        soroban_clear_sign_cases::net::HORIZON
    ))
    .unwrap();
    let env =
        TransactionEnvelope::from_xdr_base64(tx["envelope_xdr"].as_str().unwrap(), Limits::none())
            .unwrap();
    println!("{}", serde_json::to_string_pretty(&env).unwrap());
}
