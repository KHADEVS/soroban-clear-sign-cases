//! gen: writes the case inputs (cases/*.json without their expected outcome), the crafted
//! specs (specs/) and the descriptors for crafted contracts (descriptors/crafted/). Same bytes
//! every time. Then `check --write` fills in what a wallet following the draft shows.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use soroban_clear_sign_cases::case::Case;
use soroban_clear_sign_cases::chain::{operations, ContractRecord};
use soroban_clear_sign_cases::render::TokenMeta;
use std::collections::BTreeMap;
use std::fs;
use stellar_xdr::{
    AccountId, ContractId, Hash, Int128Parts, InvokeContractArgs, Limits, OperationBody, PublicKey,
    ReadXdr, ScAddress, ScSpecEntry, ScSpecFunctionInputV0, ScSpecFunctionV0, ScSpecTypeDef,
    ScSymbol, ScVal, SorobanAddressCredentials, SorobanAuthorizationEntry,
    SorobanAuthorizedFunction, SorobanAuthorizedInvocation, SorobanCredentials, StringM,
    TransactionEnvelope, Uint256, VecM,
};

const NATIVE_SAC: &str = "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA";
const USDC_ISSUER: &str = "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN";

fn h(label: &str) -> [u8; 32] {
    Sha256::digest(label.as_bytes()).into()
}
fn contract(label: &str) -> ScAddress {
    ScAddress::Contract(ContractId(Hash(h(&format!("crafted contract: {label}")))))
}
fn account(label: &str) -> ScAddress {
    ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(h(
        &format!("crafted account: {label}"),
    )))))
}
fn crafted_hash(name: &str) -> String {
    hex::encode(h(&format!("crafted wasm: {name}")))
}
fn i128v(x: i128) -> ScVal {
    ScVal::I128(Int128Parts {
        hi: (x >> 64) as i64,
        lo: x as u64,
    })
}
fn a(x: &ScAddress) -> ScVal {
    ScVal::Address(x.clone())
}
fn sym(s: &str) -> ScSymbol {
    ScSymbol(s.try_into().unwrap())
}
fn call(c: &ScAddress, f: &str, args: Vec<ScVal>) -> InvokeContractArgs {
    InvokeContractArgs {
        contract_address: c.clone(),
        function_name: sym(f),
        args: args.try_into().unwrap(),
    }
}
fn function(name: &str, inputs: &[(&str, ScSpecTypeDef)]) -> ScSpecEntry {
    let inputs: Vec<ScSpecFunctionInputV0> = inputs
        .iter()
        .map(|(n, t)| ScSpecFunctionInputV0 {
            doc: StringM::default(),
            name: (*n).try_into().unwrap(),
            type_: t.clone(),
        })
        .collect();
    ScSpecEntry::FunctionV0(ScSpecFunctionV0 {
        doc: StringM::default(),
        name: sym(name),
        inputs: inputs.try_into().unwrap(),
        outputs: VecM::default(),
    })
}
fn entry(
    cred: SorobanCredentials,
    root: InvokeContractArgs,
    subs: Vec<SorobanAuthorizedInvocation>,
) -> SorobanAuthorizationEntry {
    SorobanAuthorizationEntry {
        credentials: cred,
        root_invocation: SorobanAuthorizedInvocation {
            function: SorobanAuthorizedFunction::ContractFn(root),
            sub_invocations: subs.try_into().unwrap(),
        },
    }
}

fn write(path: &str, v: &impl serde::Serialize) {
    fs::write(path, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
}

/// Writes a crafted spec and returns its (made-up) Wasm hash.
fn spec(name: &str, entries: Vec<ScSpecEntry>) -> String {
    let hash = crafted_hash(name);
    write(
        &format!("specs/{hash}.json"),
        &json!({ "wasm_hash": hash, "crafted": format!("spec written for these cases ({name}); no such Wasm exists"), "entries": entries }),
    );
    hash
}

fn descriptor(file: &str, hash: &str, functions: Value) -> String {
    let path = format!("descriptors/crafted/{file}");
    write(
        &path,
        &json!({ "version": 1, "wasm_hashes": [hash], "functions": functions }),
    );
    path
}

fn base(id: &str, title: &str, sections: &[&str]) -> Case {
    Case {
        id: id.into(),
        title: title.into(),
        draft_sections: sections.iter().map(|s| s.to_string()).collect(),
        transaction: None,
        invocation: None,
        auth: vec![],
        contracts: BTreeMap::new(),
        tokens: BTreeMap::new(),
        descriptors: vec![],
        notes: vec![],
        expected: None,
    }
}

fn save(c: &Case) {
    write(&format!("cases/{}.json", c.id), c);
}

/// The first authorization entry of a real transaction whose root calls `root_fn`, with its index.
fn real_auth(file: &str, root_fn: &str) -> (usize, SorobanAuthorizationEntry) {
    let v: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
    let env =
        TransactionEnvelope::from_xdr_base64(v["envelope_xdr"].as_str().unwrap(), Limits::none())
            .unwrap();
    for op in operations(&env) {
        if let OperationBody::InvokeHostFunction(ihf) = &op.body {
            for (i, e) in ihf.auth.iter().enumerate() {
                if let SorobanAuthorizedFunction::ContractFn(c) = &e.root_invocation.function {
                    if c.function_name.0.as_slice() == root_fn.as_bytes() {
                        return (i, e.clone());
                    }
                }
            }
        }
    }
    panic!("no entry rooted at {root_fn} in {file}");
}

fn notes(c: &mut Case, n: &[&str]) {
    c.notes = n.iter().map(|s| s.to_string()).collect();
}

fn main() {
    for d in ["cases", "specs", "descriptors/crafted"] {
        fs::create_dir_all(d).unwrap();
        for e in fs::read_dir(d).unwrap().flatten() {
            if e.path().is_file() {
                fs::remove_file(e.path()).unwrap();
            }
        }
    }
    let user = account("user");

    // 1. The draft's own example, on the real transaction.
    let mut c = base(
        "aquarius-swap-chained",
        "The draft's authorization-tree example, run on the real Mainnet transaction",
        &["3.0", "3.1", "3.4", "5.1", "5.2", "5.3"],
    );
    c.transaction = Some("real/aquarius-swap-chained.json".into());
    c.descriptors = vec!["descriptors/draft/aquarius-front.json".into()];
    notes(&mut c, &[
        "Transaction 02c785c9…8a44 and the descriptor are the draft's own (Example Usage, \"Authorization tree\"); the descriptor is copied unchanged.",
        "Every value matches the draft's rendering. One line differs: the draft shows \"+1 more parameter (swaps_chain, in the raw view)\", but token_in is not a listed field either, so under §5.1 two parameters are left out.",
    ]);
    save(&c);

    // 2-3. The same transaction with broken descriptors.
    let mut c = base(
        "aquarius-duplicate-key",
        "A descriptor with a duplicate key is ignored as a whole",
        &["4.3", "6"],
    );
    c.transaction = Some("real/aquarius-swap-chained.json".into());
    c.descriptors = vec!["descriptors/cases/aquarius-front-duplicate-key.json".into()];
    notes(&mut c, &[
        "The draft's descriptor with \"label\" given twice in one field. §4.3 makes the whole document invalid, so swap_chained is shown raw with the §6 marker.",
        "The SAC transfer below it is still described: §5.3 needs no descriptor.",
    ]);
    save(&c);

    let mut c = base(
        "aquarius-unknown-key",
        "An unknown key inside a field invalidates that function descriptor",
        &["1", "4.3", "6"],
    );
    c.transaction = Some("real/aquarius-swap-chained.json".into());
    c.descriptors = vec!["descriptors/cases/aquarius-front-unknown-key.json".into()];
    notes(&mut c, &["The draft's descriptor with \"visible\": true added to one field (an ERC-7730 key). §1: unknown keys inside a function descriptor invalidate it."]);
    save(&c);

    // 4. The draft's SEP-41 example, on the real share token's spec.
    let mut c = base(
        "share-token-approve",
        "The draft's SEP-41 example: approve on the Aquarius share token",
        &["3.1", "5.1"],
    );
    c.invocation = Some(call(
        &"CDXRIV6XHJWJXCFCP7CGPPQCLQSVMJ64UOOJY2XLCFAOMF6OUFEH2VDD"
            .parse()
            .unwrap(),
        "approve",
        vec![
            a(&account("owner")),
            a(&account("spender")),
            i128v(1_000_000_000),
            ScVal::U32(65_623_765),
        ],
    ));
    c.descriptors = vec!["descriptors/draft/sep41-share-token.json".into()];
    notes(&mut c, &[
        "Crafted call, real contract: the draft's descriptor checked against the spec of Wasm 72eb5d52…, read from Mainnet (the fourth input is expiration_ledger, as the draft says).",
        "The draft's example shows \"100 EXT\" for a made-up symbol. Here decimals and symbol come from the token's own metadata (POOL), so §3.1's SHOULD applies and the amount is marked unverified.",
    ]);
    save(&c);

    // 5-6. KALE: what one farmer signs, with legacy and with V2 credentials.
    let kale = descriptor(
        "kale-plant.json",
        "db2c14290d4964e3805f2527dd132939ba5fb3fccac56b30bfab8fd091011627",
        json!({ "plant": { "intent": "Stake tokens", "auth": true, "fields": [ { "path": ["amount"], "label": "Amount" }, { "path": ["farmer"], "label": "Farmer" } ] } }),
    );
    let (farmer_index, farmer_entry) = real_auth("real/kale-batch-plant.json", "plant");
    let mut c = base(
        "kale-plant-legacy-credentials",
        "What a KALE farmer signs today: a legacy-credential entry, shown raw",
        &["5.2", "5.3", "6"],
    );
    c.auth = vec![farmer_entry.clone()];
    c.descriptors = vec![kale.clone()];
    let first_note = format!(
        "Authorization entry {} of Mainnet transaction 64d07a4e…4034, the first one rooted at plant: one farmer authorizes plant and the KALE SAC burn below it. Another account submits the transaction, so the farmer's wallet sees only this entry.",
        farmer_index + 1
    );
    notes(&mut c, &[
        first_note.as_str(),
        "It uses SOROBAN_CREDENTIALS_ADDRESS, which §5.2 puts out of scope: every node is raw, even the SAC burn that §5.3 would otherwise describe. In the survey, 5,613 of the 5,621 address-credential entries signed by G accounts were of this kind, and 2,397 accounts signed a SAC burn this way.",
        "The plant descriptor is ours, written for the case; amount has no format because the token is not an argument (Limitations, \"Token must be in the arguments\").",
    ]);
    save(&c);

    let mut v2 = farmer_entry.clone();
    if let SorobanCredentials::Address(creds) = &farmer_entry.credentials {
        v2.credentials = SorobanCredentials::AddressV2(creds.clone());
    }
    let mut c = base(
        "kale-plant-address-v2",
        "The same entry with ADDRESS_V2 credentials is described",
        &["5.2", "5.3"],
    );
    c.auth = vec![v2];
    c.descriptors = vec![kale];
    notes(&mut c, &["The entry of kale-plant-legacy-credentials with its credentials switched to ADDRESS_V2 (CAP-71), everything else unchanged. Now plant is described through its descriptor and the burn through §5.3."]);
    save(&c);

    // 7. Blend-style submit: struct paths and the enum format.
    let reasons = json!({ "0": "Supply", "1": "Withdraw", "2": "Supply collateral", "3": "Withdraw collateral", "4": "Borrow", "5": "Repay" });
    let blend = descriptor(
        "blend-submit.json",
        "0e4f8f45b94e6b0d017a496ac5620ad0e3f866533cf45acd2b070bedda1e7356",
        json!({ "submit": { "intent": "Update lending position", "fields": [
            { "path": ["requests", 0, "request_type"], "label": "Action 1", "format": "enum", "values": reasons },
            { "path": ["requests", 0, "amount"], "label": "Amount 1", "format": "amount", "token": { "path": ["requests", 0, "address"] } },
            { "path": ["requests", 1, "request_type"], "label": "Action 2", "format": "enum", "values": reasons },
            { "path": ["requests", 1, "amount"], "label": "Amount 2", "format": "amount", "token": { "path": ["requests", 1, "address"] } },
            { "path": ["requests", 2, "request_type"], "label": "Action 3", "format": "enum", "values": reasons },
            { "path": ["requests", 2, "amount"], "label": "Amount 3", "format": "amount", "token": { "path": ["requests", 2, "address"] } },
            { "path": ["from"], "label": "Account" },
            { "path": ["spender"], "label": "Pays from" },
            { "path": ["to"], "label": "Sends to" }
        ] } }),
    );
    let mut c = base(
        "blend-submit",
        "A lending submit with a vector of request structs",
        &["2.1", "3.1", "3.6", "4.4", "5.1", "5.2"],
    );
    c.transaction = Some("real/blend-submit.json".into());
    c.descriptors = vec![blend];
    notes(&mut c, &[
        "Mainnet transaction b8fe358b…27d3 on a pool with Blend's submit interface: repay 5003.0001282 XLM, withdraw 3035.9887324 USDC of collateral. Request is a named struct, so paths like [\"requests\", 0, \"amount\"] work.",
        "The enum values are Blend's RequestType (blend-contracts-v2, pool/src/pool/actions.rs: 0 Supply … 5 Repay). The draft's §3.6 example maps \"2\" to \"Supply\"; in Blend, 2 is SupplyCollateral and 0 is Supply.",
        "The descriptor lists three request positions; the call has two, so the third one is omitted under §4.4 and counted in the omission line. auth is left false: we did not review the source, so the authorization node stays raw (it also uses legacy credentials).",
    ]);
    save(&c);

    // 8-9. A vector of tuple structs.
    let mint_hash = "83bf0a7fbe1502271f2e14d2a5a46c9cde1aa213be97616bd7c6b102b342692f";
    let by_name = descriptor(
        "mint-batch-by-name.json",
        mint_hash,
        json!({ "mint_batch": { "intent": "Mint tokens", "fields": [
            { "path": ["operations", 0, "0"], "label": "Recipient 1" },
            { "path": ["operations", 0, "1"], "label": "Amount 1", "format": "amount", "token": { "self": true } },
            { "path": ["caller"], "label": "Caller" }
        ] } }),
    );
    let by_index = descriptor(
        "mint-batch-by-index.json",
        mint_hash,
        json!({ "mint_batch": { "intent": "Mint tokens", "fields": [
            { "path": ["operations", 0, 0], "label": "Recipient 1" },
            { "path": ["operations", 0, 1], "label": "Amount 1", "format": "amount", "token": { "self": true } },
            { "path": ["caller"], "label": "Caller" }
        ] } }),
    );
    let mut c = base(
        "tuple-struct-by-member-name",
        "Tuple-struct members named \"0\", \"1\": valid per §4.3, then every call fails §4.4",
        &["2.1", "4.3", "4.4"],
    );
    c.transaction = Some("real/mint-batch.json".into());
    c.descriptors = vec![by_name];
    notes(&mut c, &[
        "Mainnet transaction 423202b5…abbe: mint_batch(operations: Vec<MintBatchOperation>, …), where MintBatchOperation is a tuple struct (address, i128). soroban-sdk names its members \"0\" and \"1\" in the spec (soroban-sdk-macros, derive_struct_tuple.rs) but encodes the value as SCV_VEC.",
        "A string step passes §4.3 (the spec has a member named \"0\"), but §4.4 then requires an SCV_MAP, finds an SCV_VEC, and renders the whole call raw, on every call. The informative JSON Schema rejects \"0\" anyway (names must start with a letter or _).",
    ]);
    save(&c);

    let mut c = base(
        "tuple-struct-by-index",
        "Tuple-struct members by position: refused by §4.3",
        &["2.1", "4.3"],
    );
    c.transaction = Some("real/mint-batch.json".into());
    c.descriptors = vec![by_index];
    notes(&mut c, &[
        "The same call, with integer steps into the tuple struct. §2.1 and §4.3 allow integer steps only into vec and tuple types, so the descriptor is invalid. Together with tuple-struct-by-member-name, there is no way to reach a tuple-struct member.",
        "A possible rule: a struct whose members are named exactly \"0\" to \"n-1\" is encoded as SCV_VEC, and paths step into it by position.",
    ]);
    save(&c);

    // 10. SAC approve of zero (the draft's TODO).
    let mut c = base(
        "sac-approve-zero",
        "A SAC approve of 0 that revokes an allowance",
        &["5.3"],
    );
    c.invocation = Some(call(
        &NATIVE_SAC.parse().unwrap(),
        "approve",
        vec![a(&user), a(&contract("dex")), i128v(0), ScVal::U32(0)],
    ));
    notes(&mut c, &["The rendering the §5.3 TODO describes: accurate, but nothing says the allowance is being revoked."]);
    save(&c);

    // 11. A host claims a SAC that does not hash.
    let fake = contract("fake usdc");
    let mut c = base(
        "sac-impersonation",
        "A contract presented as the USDC SAC whose asset does not hash to it",
        &["5.3", "6"],
    );
    c.invocation = Some(call(
        &fake,
        "transfer",
        vec![a(&user), a(&account("attacker")), i128v(100_000_000)],
    ));
    c.contracts.insert(
        fake.to_string(),
        ContractRecord::StellarAsset {
            asset: format!("USDC:{USDC_ISSUER}"),
        },
    );
    notes(&mut c, &["The resolution says StellarAsset with the USDC asset, but that asset hashes to CCW67…MI75, not to this contract. §5.3 requires the hash check, so nothing is described and no symbol is shown."]);
    save(&c);

    // 12. Hostile symbol from the token itself.
    let token_spec = spec(
        "token",
        vec![function(
            "transfer",
            &[
                ("from", ScSpecTypeDef::Address),
                ("to", ScSpecTypeDef::Address),
                ("amount", ScSpecTypeDef::I128),
            ],
        )],
    );
    let token_desc = descriptor(
        "token.json",
        &token_spec,
        json!({ "transfer": { "intent": "Send tokens", "auth": true, "fields": [
            { "path": ["amount"], "label": "Amount", "format": "amount", "token": { "self": true } },
            { "path": ["to"], "label": "Recipient" },
            { "path": ["from"], "label": "From" }
        ] } }),
    );
    let tok = contract("token with bidi symbol");
    let mut c = base(
        "hostile-symbol",
        "A token whose own symbol carries a right-to-left override",
        &["3.1"],
    );
    c.invocation = Some(call(
        &tok,
        "transfer",
        vec![a(&user), a(&account("recipient")), i128v(250_000_000)],
    ));
    c.contracts.insert(
        tok.to_string(),
        ContractRecord::Wasm {
            wasm_hash: token_spec.clone(),
        },
    );
    c.tokens.insert(
        tok.to_string(),
        TokenMeta {
            decimals: Some(7),
            symbol: Some("USD\u{202e}CTEST".into()),
            source: "contract".into(),
        },
    );
    c.descriptors = vec![token_desc];
    notes(&mut c, &[
        "The symbol returned by the token contains U+202E. §3.1 says it MUST NOT, without saying what the wallet shows instead; here it is treated as unresolved, so the contract ID stands in its place (§3.1's rule for a missing symbol).",
    ]);
    save(&c);

    // 13. freighter#2196, reconstructed.
    let claim_spec = spec(
        "claim",
        vec![function(
            "claim",
            &[("from", ScSpecTypeDef::Address), ("id", ScSpecTypeDef::U64)],
        )],
    );
    let claim_desc = descriptor(
        "claim.json",
        &claim_spec,
        json!({ "claim": { "intent": "Claim", "auth": true, "fields": [ { "path": ["from"], "label": "From" }, { "path": ["id"], "label": "ID" } ] } }),
    );
    let claim_contract = contract("claim");
    let token = contract("some token");
    let mut c = base(
        "auth-args-differ",
        "stellar/freighter#2196 reconstructed: the authorization carries other arguments",
        &["2", "4.4", "5.2"],
    );
    c.invocation = Some(call(
        &claim_contract,
        "claim",
        vec![a(&user), ScVal::U64(7)],
    ));
    c.auth = vec![entry(
        SorobanCredentials::AddressV2(SorobanAddressCredentials {
            address: user.clone(),
            nonce: 1,
            signature_expiration_ledger: 65_600_000,
            signature: ScVal::Void,
        }),
        call(&claim_contract, "claim", vec![a(&token), ScVal::U64(7)]),
        vec![],
    )];
    c.contracts.insert(
        claim_contract.to_string(),
        ContractRecord::Wasm {
            wasm_hash: claim_spec,
        },
    );
    c.descriptors = vec![claim_desc];
    notes(&mut c, &[
        "Reconstruction of the issue's shape: the call takes (from: Address, id: u64) and its authorization carries (token: Address, id: u64). The real transaction (eca9efdd…18af) is older than the public Horizon history (SDF's starts at ledger 59,240,161), so the function name and addresses here are made up.",
        "With auth wrongly set to true, the argument check passes (same arity, same types) and the token contract is labelled \"From\": the limitation the draft documents.",
        "The entry's credentials name the authorizer (the user), and the field labelled From holds a contract that is not the user. A rule that checks the authorizer-naming field against the credential address would catch this case, at the root and in sub-invocations alike, because every node of an entry is authorized by the same address.",
    ]);
    save(&c);
}
