# Survey of Soroban calls on Mainnet

120 ledgers sampled evenly between 65426679 and 65546512 through `https://mainnet.sorobanrpc.com`: 36918 transactions, 7287 successful ones with a Soroban contract call. "Distinct signers" counts the accounts that sent or authorized a call, so a bot that repeats one call counts once.

## Authorization entries by credentials

| Credentials | Entries |
| --- | --- |
| address(C) | 7 |
| address(G) | 5613 |
| address_v2(G) | 8 |
| source_account | 3125 |

## Stellar Asset Contract functions

| Function | Distinct signers |
| --- | --- |
| `approve` | 3 |
| `burn` | 2397 |
| `mint` | 1 |
| `transfer` | 73 |

| Function | Where | Count |
| --- | --- | --- |
| `approve` | auth node, root, source_account | 4 |
| `approve` | root call | 4 |
| `burn` | auth node, sub-invocation, address(G) | 5111 |
| `mint` | auth node, sub-invocation, address_v2(G) | 1 |
| `transfer` | auth node, root, address(G) | 1 |
| `transfer` | auth node, root, address_v2(G) | 1 |
| `transfer` | auth node, root, source_account | 27 |
| `transfer` | auth node, sub-invocation, address(G) | 32 |
| `transfer` | auth node, sub-invocation, address_v2(G) | 4 |
| `transfer` | auth node, sub-invocation, source_account | 50 |
| `transfer` | root call | 29 |

## Most used Wasm functions and their inputs

| Contract | Function | Distinct signers | Root calls | Auth nodes | Inputs | Auth nodes with another arity |
| --- | --- | --- | --- | --- | --- | --- |
| CDL7…IGWA | `plant` | 4183 | 2949 | 8060 | farmer: address, amount: i128 | 0 |
| CDL7…IGWA | `work` | 1653 | 2642 | 0 | farmer: address, hash: bytes_n<32>, nonce: u64 | 0 |
| CBGS…KKY3 | `harvest` | 735 | 984 | 0 | farmer: address, pails: vec<u32> | 0 |
| CCIS…IPRO | `batch_plant` | 306 | 210 | 210 | invoker: address, kale_contract: address, farmers: vec<address>, amounts: vec<i128> | 0 |
| CCIS…IPRO | `batch_work` | 114 | 196 | 196 | invoker: address, kale_contract: address, farmers: vec<address>, hashes: vec<bytes>, nonces: vec<u64> | 0 |
| CBBM…BUCV | `swap` | 18 | 14 | 14 | user: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CBQD…6QUK | `swap_chained` | 11 | 16 | 16 | user: address, swaps_chain: vec<tuple<vec<address>,bytes_n<32>,address>>, token_in: address, in_amount: u128, out_min: u128 | 0 |
| CA6P…CJBE | `swap` | 7 | 6 | 6 | user: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CBIJ…M6VN | `transfer` | 7 | 0 | 7 | from: address, to: address, amount: i128 | 0 |
| CCNX…RQ2A | `swap` | 6 | 6 | 6 | user: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CDVS…RVOT | `route_if` | 6 | 5 | 5 | user: address, token_in: address, min_balance: i128, token_out: address, routes: vec<udt:Route> | 0 |
| CBUK…KZCR | `deposit` | 6 | 3 | 3 | reserve: address, user: address, amount: i128 | 0 |
| CDL7…IGWA | `harvest` | 6 | 6 | 0 | farmer: address, index: u32 | 0 |
| CA6P…CJBE | `swap_strict_receive` | 4 | 3 | 3 | user: address, in_idx: u32, out_idx: u32, out_amount: u128, in_max: u128 | 0 |
| CBOO…7FZP | `mint_batch` | 4 | 3 | 3 | operations: vec<udt:MintBatchOperation> (tuple struct), caller: address, idempotency_key: string | 0 |
| CBUK…KZCR | `withdraw` | 4 | 2 | 2 | reserve: address, user: address, amount: i128 | 0 |
| CA52…XUSG | `write_prices` | 3 | 44 | 44 | updater: address, feed_ids: vec<string>, payload: bytes | 0 |
| CCPG…6V6B | `swap_chained` | 3 | 4 | 4 | user: address, swaps_chain: vec<tuple<vec<address>,bytes_n<32>,address>>, token_in: address, in_amount: u128, out_min: u128, fee_fraction: u32 | 0 |
| CD2O…P5F6 | `swap` | 3 | 4 | 4 | user: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CDUQ…S7FT | `report` | 3 | 4 | 4 | transmitter: address, receiver: address, raw_report: bytes, report_context: bytes, signatures: vec<udt:Ed25519Signature> | 0 |
| CBB3…I6GZ | `report` | 3 | 3 | 3 | transmitter: address, receiver: address, raw_report: bytes, report_context: bytes, signatures: vec<udt:Ed25519Signature> | 0 |
| CDA3…22JM | `submit_prices` | 3 | 3 | 3 | signer: address, feed_ids: vec<string>, prices: vec<i128>, package_timestamp: u64 | 0 |
| CCR2…H2MQ | `swap` | 3 | 2 | 2 | sender: address, recipient: address, zero_for_one: bool, amount_specified: i128, sqrt_price_limit_x96: u256, hints: udt:OracleHints | 0 |
| CA6X…2ZRF | `sm` | 3 | 1 | 2 | user: address, md: address, sm: bool, mi: address, input_token: address, input_amt: i128, to: address, output_token: address, out_amt_min: i128, ops: vec<tuple<symbol,address,vec<address>,address,address>> | 0 |
| CAJJ…BXBD | `submit` | 3 | 0 | 3 | from: address, spender: address, to: address, requests: vec<udt:Request> | 0 |
| CAUP…772J | `transfer` | 3 | 0 | 3 | from: address, to: address, amount: i128 | 0 |
| CBGU…SZIW | `withdraw_stream` | 2 | 2 | 2 | stream_id: bytes_n<32>, caller: address | 0 |
| CBOO…7FZP | `safe_transfer` | 2 | 2 | 2 | from: address, to: address, amount: i128, idempotency_key: string | 0 |
| CCA2…F5HR | `deposit` | 2 | 2 | 2 | amounts_desired: vec<i128>, amounts_min: vec<i128>, from: address, invest: bool | 0 |
| CCTR…24LF | `claim_withdrawal` | 2 | 2 | 2 | user: address, amount: i128, nonce: u64, expires_at: u64, signature: bytes_n<64> | 0 |
| CAAJ…ZA7Q | `swap` | 2 | 0 | 3 | user: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CAMW…KKGO | `execute` | 2 | 1 | 2 | caller: address, trader: address, asset_in: address, amount_in: i128, base_asset: address, min_final_balance: i128, route: vec<udt:SwapStep> | 0 |
| CACS…MCY7 | `submit` | 2 | 1 | 1 | from: address, spender: address, to: address, requests: vec<udt:Request> | 0 |
| CB54…DRAO | `execute` | 2 | 1 | 1 | executor: address, params: udt:ExecutionParams, value_payer: address | 0 |
| CBAF…JT33 | `swap_if` | 2 | 1 | 1 | user: address, expect_asset: address, expect_min_balance: i128, pool: address, token_in: address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128 | 0 |
| CBGH…S26M | `grant_role_batch` | 2 | 1 | 1 | caller: address, users: vec<address>, role: symbol | 0 |
| CCG5…PQWR | `liquidate_asset` | 1 | 19 | 19 | executor: address, statement_ids: vec<string>, assets: vec<vec<address>>, max_statements_cents: option<vec<option<i128>>> | 0 |
| CCG5…PQWR | `publish_statements` | 1 | 19 | 19 | publisher: address, collaterals: vec<address>, statement_ids: vec<string>, team_ids: vec<string>, closing_account_balance_cents: vec<i128>, liquidatable_afters: vec<u64> | 0 |
| CDW4…5YTS | `set_price` | 1 | 8 | 8 | price_updater: address, prices: vec<udt:UpdatePrice> | 0 |

## Time inputs

| Contract | Function | Input | Samples | u64::MAX | Past year 9999 |
| --- | --- | --- | --- | --- | --- |
| CDA3…22JM | `submit_prices` | `package_timestamp` | 3 | 0 | 3 |
| CCTR…24LF | `claim_withdrawal` | `expires_at` | 2 | 0 | 0 |
