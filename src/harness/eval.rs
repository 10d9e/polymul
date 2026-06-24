//! Evaluation + scoring. FROZEN — do not edit as part of autoresearch.
//!
//! SCORE = deterministic wasm WORK over the fixed fixture corpus (LOWER IS BETTER),
//! gated on exact match against the reference oracle for every pair.

use std::process::Command;

use crate::algorithm::{plan_new, poly_mul};
use crate::harness::fixtures;
use crate::harness::reference;

pub fn run() -> i32 {
    let pairs = fixtures::all();
    let mut plan = plan_new();
    let mut all_correct = true;

    println!(
        "{:<10} {:>10}  {}",
        "pair", "checksum", "correct"
    );

    for p in &pairs {
        let got = poly_mul(&mut plan, &p.a, &p.b);
        let expect = reference::poly_mul(&p.a, &p.b);
        let correct = got == expect;
        if !correct {
            all_correct = false;
        }
        println!(
            "{:<10} {:>10}  {}",
            p.name,
            fixtures::checksum(&got),
            if correct { "OK" } else { "FAIL!" }
        );
    }

    println!("{}", "-".repeat(40));

    if !all_correct {
        println!("\nSCORE: INVALID (reference mismatch on at least one pair)");
        return 1;
    }

    let work = match measure_work() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("complexity measurement failed: {e}");
            eprintln!("(run `bash scripts/measure-complexity.sh` manually for details)");
            return 2;
        }
    };

    println!(
        "\nSCORE: {} (deterministic wasm WORK; lower is better)",
        work
    );
    0
}

fn measure_work() -> Result<u64, String> {
    let script = std::path::Path::new("scripts/measure-complexity.sh");
    if !script.exists() {
        return Err("scripts/measure-complexity.sh not found".into());
    }

    let output = Command::new("bash")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("{stdout}{stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("WORK: ") {
            let num = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| "malformed WORK line".to_string())?;
            return num
                .parse()
                .map_err(|_| format!("invalid WORK value: {num}"));
        }
    }

    Err("WORK line not found in measure-complexity output".into())
}
