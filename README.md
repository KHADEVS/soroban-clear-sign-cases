# Soroban clear-sign cases

Test cases for the draft SEP *Contract Display Descriptors* ([HackMD draft](https://hackmd.io/@overcat/SyeplgIaMg), discussed in [stellar-protocol #2026](https://github.com/orgs/stellar/discussions/2026)): real Mainnet invocations and crafted ones, run through a small Rust implementation of the draft's rules, so that a wallet, a hardware signer or an explorer can check that it shows the same thing. A survey of what is actually signed on Mainnet comes with them.

The implementation follows the draft as it read on 2026-11-18 (version 0.0.1). It is a reading of the draft, not a reference anyone has agreed on: where the text leaves a choice, the choice made here is listed below. Corrections are welcome.

## What a case holds

| Field | Content |
| --- | --- |
| `transaction` | A Mainnet transaction under `real/` (envelope as read from Horizon) |
| `invocation` | A crafted contract call, when there is no transaction |
| `auth` | Standalone authorization entries, as a wallet sees them through SEP-43 `signAuthEntry` |
| `contracts` | Contract resolutions that override `real/contracts.json` (crafted contracts, a host's false claim) |
| `tokens` | Token metadata that overrides `real/tokens.json`, with its source (`curated` or `contract`) |
| `descriptors` | The descriptor documents the wallet has |
| `expected` | What the wallet shows: the described view or the raw view with the reason, per invocation and per authorization node |
| `notes` | Why the case is there |

Each case also has a `.txt` file with the same outcome laid out like the draft's examples.

`real/contracts.json` records how each contract resolves (Wasm hash, or Stellar Asset Contract with its asset, checked to hash to the contract ID) and `real/specs/` holds the `contractspecv0` section of each Wasm, read from Mainnet. `specs/` holds specs written for crafted cases.

## Cases

| Case | What it checks |
| --- | --- |
| `aquarius-swap-chained` | The draft's authorization-tree example, on the real transaction |
| `aquarius-duplicate-key` | A descriptor with a duplicate key is ignored as a whole |
| `aquarius-unknown-key` | An unknown key inside a field invalidates that function descriptor |
| `share-token-approve` | The draft's SEP-41 example on the real share token's spec |
| `kale-plant-legacy-credentials` | What a KALE farmer signs today, with legacy credentials |
| `kale-plant-address-v2` | The same entry with `ADDRESS_V2` credentials |
| `blend-submit` | A lending `submit` with a vector of request structs: struct paths, `enum` |
| `tuple-struct-by-member-name` | A real call taking tuple structs, reached by member name |
| `tuple-struct-by-index` | The same call, reached by position |
| `sac-approve-zero` | A SAC `approve` of 0 (the §5.3 TODO) |
| `sac-impersonation` | A contract presented as the USDC SAC whose asset does not hash to it |
| `hostile-symbol` | A token symbol carrying a right-to-left override |
| `auth-args-differ` | [stellar/freighter#2196](https://github.com/stellar/freighter/issues/2196) reconstructed: the authorization carries other arguments |

## What the cases show

1. **Almost every address-credential entry signed today is legacy, so it renders raw.** §5.2 puts `SOROBAN_CREDENTIALS_ADDRESS` out of scope. In the survey, 5,613 of the 5,621 address-credential entries signed by G accounts used it, against 8 with `ADDRESS_V2`. KALE farmers alone account for 2,397 accounts signing a SAC `burn` this way. `kale-plant-legacy-credentials` shows such an entry fully raw; `kale-plant-address-v2` shows the same entry, credentials switched, read as "Stake tokens" with "Burn tokens, 0.6408734 KALE" below it. The rule is sound, but until wallets and dApps build `ADDRESS_V2` entries, the described view mostly reaches source-account entries.
2. **Tuple-struct members cannot be reached.** soroban-sdk names a tuple struct's members "0", "1"… in the spec and encodes the value as `SCV_VEC`. A string step passes §4.3 and then fails §4.4 on every call (`tuple-struct-by-member-name`); an integer step is refused by §4.3 (`tuple-struct-by-index`); the JSON Schema rejects "0" as a name. `mint_batch(operations: Vec<MintBatchOperation>)`, with `MintBatchOperation(Address, i128)`, is a live example. A possible rule: a struct whose members are named exactly "0" to "n-1" is stepped into by position.
3. **The draft's example counts one parameter short.** In `aquarius-swap-chained`, every value matches the draft's rendering, but `token_in` is referenced only by a token path, like `swaps_chain`, so §5.1 leaves out two parameters, not one ("+1 more parameter" in the draft).
4. **A field that names the authorizer can be checked against the credentials.** In `auth-args-differ`, `auth: true` set by mistake labels a token contract "From". Every node of an entry is authorized by the address in its credentials (or the operation's source), at the root and in sub-invocations alike, so a descriptor that says which field names the authorizer (as in the `"auth": ["from"]` idea from the discussion) lets the wallet refuse this case. It does not help when the authorizer is read from storage.
5. **The built-in SAC functions match what is signed.** Stellar Asset Contract functions seen in the survey, by distinct signers: `burn` 2,397, `transfer` 73, `approve` 3, `mint` 1 (an issuer). No `trust`, `transfer_from`, `burn_from`, `clawback`, `set_admin` or `set_authorized` in the sample.
6. **The most common swap shape cannot use `amount`.** Pool functions that take token indices, `swap(user, in_idx, out_idx, in_amount, out_min)`, were signed by 18, 7, 6, 3 and 2 distinct accounts on five pools, against 11 and 3 for `swap_chained` on its two contracts. Their tokens live in contract state, the case the draft lists under Limitations; the SAC `transfer` sub-invocation then carries the real token, unless the entry uses legacy credentials (point 1).
7. **Smaller points.**
    - §3.1 says a symbol MUST NOT contain bidirectional or control characters but not what to show instead; `hostile-symbol` treats it as unresolved and shows the contract ID.
    - §3.6's example maps `"2"` to "Supply". If it is meant for Blend, 2 is SupplyCollateral and 0 is Supply (`blend-submit`).
    - An oracle input, `submit_prices(…, package_timestamp: u64)`, carried milliseconds in all three samples: read as seconds, the date falls after year 9999 and has no RFC 3339 form. §3.2 could say what to show then.
    - `sac-approve-zero` shows the revocation rendering the §5.3 TODO describes.
    - Among the 40 Wasm functions with the most distinct signers, no authorization node had a different number of arguments from the function. This cannot detect same-arity `require_auth_for_args` lists such as freighter#2196.

## Survey

`survey/SUMMARY.md` and `survey/results.json`: 120 ledgers sampled evenly over the seven days the RPC keeps (2026-11-11 to 2026-11-18, ledgers 65,426,679 to 65,546,512), 36,918 transactions, 7,287 successful ones with a Soroban contract call. "Distinct signers" counts the accounts that sent or authorized a call, so a bot repeating one call counts once. The sample is small and recent; the numbers show orders of magnitude, not market shares. RPC history is short, so the transaction hashes are kept to look the calls up again.

## How the draft is read here

- **Omission count (§5.1).** A parameter counts as listed when some field's path starts at it; a token path alone does not list it. Each field omitted under §4.4 adds one.
- **Negative indices (§4.4).** "An index beyond its length" is read as covering a negative index before the start too: the field is omitted.
- **Rejected symbols (§3.1).** A symbol that breaks the character rule is treated as unresolved.
- **Raw values (§3.0).** Values other than addresses, integers, booleans and symbols are shown in their XDR-JSON form.
- **Duration (§3.3).** Zero units are dropped wherever they are, as in the draft's example `1h 30m`.
- **Unverified metadata (§3.1).** Decimals and symbol read from the token's own metadata are marked `(unverified)`.

Not exercised yet: CAP-85 external executables (none among the most used contracts), muxed addresses, `timestamp`, `duration`, `percent` on non-zero values and `bool` on real contracts, fee-bump source rules beyond the Aquarius case.

## Running

```
cargo run --bin gen              # write the case inputs, crafted specs and descriptors
cargo run --bin check -- --write # fill in the expected outcomes
cargo run --bin check            # check that every case still renders as recorded
cargo test
```

Network tools, not needed for the checks: `fetch <name> <tx hash>` saves a Mainnet transaction and its contracts under `real/`; `fetch --contract <C…>` saves contracts only; `survey [ledgers]` rebuilds the survey; `findtx <function> <sub-invocation>` finds recent transactions with a given authorization shape; `specdump <wasm hash>` prints a stored spec. They use Horizon and the public Soroban RPC at mainnet.sorobanrpc.com.

## License

MIT
