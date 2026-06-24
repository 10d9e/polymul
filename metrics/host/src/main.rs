//! Deterministic complexity meter. Loads the wasm shim and reports
//! WORK = fuel(full) - fuel(half) on warm bench_polymul calls.
use wasmtime::{Config, Engine, Instance, Module, Store};

const FULL: u32 = 32;
const HALF: u32 = 16;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: polymul-fuel-meter <module.wasm>");
    let wasm = std::fs::read(&path).expect("read wasm");

    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).expect("engine");
    let module = Module::from_binary(&engine, &wasm).expect("parse wasm");

    let mut store = Store::new(&engine, ());
    store
        .set_fuel(1_000_000_000_000_000)
        .expect("set fuel");
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiate");
    let f = instance
        .get_typed_func::<u32, u32>(&mut store, "bench_polymul")
        .expect("get bench_polymul");

    let b1 = store.get_fuel().unwrap();
    let out_full = f.call(&mut store, FULL).expect("call full");
    let a1 = store.get_fuel().unwrap();
    let fuel_full = b1 - a1;

    let out_half = f.call(&mut store, HALF).expect("call half");
    let a2 = store.get_fuel().unwrap();
    let fuel_half = a1 - a2;

    let work = fuel_full - fuel_half;
    println!("full {} pairs -> checksum {} (fuel {})", FULL, out_full, fuel_full);
    println!("half {} pairs -> checksum {} (fuel {})", HALF, out_half, fuel_half);
    println!(
        "WORK: {} (deterministic wasm operators for {} extra pairs; lower is faster)",
        work,
        FULL - HALF
    );
}
