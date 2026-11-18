//! check [--write]
//! Runs every case under cases/ and compares the outcome with the one stored in the case
//! (and its .txt view). With --write, stores the current outcome instead.
use soroban_clear_sign_cases::case::{to_text, Case};
use std::fs;
use std::path::Path;

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let root = Path::new(".");
    let mut names: Vec<_> = fs::read_dir("cases")
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    names.sort();
    let mut failed = 0;
    for path in names {
        let text = fs::read_to_string(&path).unwrap();
        let mut case: Case =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let outcome = match case.run(root) {
            Ok(o) => o,
            Err(e) => {
                println!("ERROR {}: {e}", case.id);
                failed += 1;
                continue;
            }
        };
        let txt_path = path.with_extension("txt");
        let view = to_text(&outcome);
        if write {
            case.expected = Some(outcome);
            fs::write(&path, serde_json::to_string_pretty(&case).unwrap() + "\n").unwrap();
            fs::write(&txt_path, &view).unwrap();
            println!("wrote {}", case.id);
        } else if case.expected.as_ref() != Some(&outcome)
            || fs::read_to_string(&txt_path).ok().as_deref() != Some(view.as_str())
        {
            println!("MISMATCH {}", case.id);
            failed += 1;
        } else {
            println!("ok {}", case.id);
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}
