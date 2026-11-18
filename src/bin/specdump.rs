//! specdump <wasm hash> [names...]: prints functions, structs and enums of a stored spec.
use soroban_clear_sign_cases::case::load_spec;
use soroban_clear_sign_cases::spec::{text, type_name};
use stellar_xdr::ScSpecEntry;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let spec = load_spec(std::path::Path::new("."), &args[1]).unwrap();
    let want = &args[2..];
    let keep = |n: &str| want.is_empty() || want.iter().any(|w| w == n);
    for e in &spec.entries {
        match e {
            ScSpecEntry::FunctionV0(f) if keep(&text(f.name.0.as_slice())) => {
                let ins: Vec<String> = f
                    .inputs
                    .iter()
                    .map(|i| format!("{}: {}", text(i.name.as_slice()), type_name(&i.type_)))
                    .collect();
                println!("fn {}({})", text(f.name.0.as_slice()), ins.join(", "));
            }
            ScSpecEntry::UdtStructV0(s) if keep(&text(s.name.as_slice())) => {
                let fs: Vec<String> = s
                    .fields
                    .iter()
                    .map(|i| format!("{}: {}", text(i.name.as_slice()), type_name(&i.type_)))
                    .collect();
                println!("struct {} {{ {} }}", text(s.name.as_slice()), fs.join(", "));
            }
            ScSpecEntry::UdtEnumV0(s) if keep(&text(s.name.as_slice())) => {
                let cs: Vec<String> = s
                    .cases
                    .iter()
                    .map(|c| format!("{} = {}", text(c.name.as_slice()), c.value))
                    .collect();
                println!("enum {} {{ {} }}", text(s.name.as_slice()), cs.join(", "));
            }
            _ => {}
        }
    }
}
