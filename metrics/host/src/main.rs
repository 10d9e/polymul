//! Deterministic complexity meter (weighted cost model).
//!
//! WORK = weighted_cost(full) - weighted_cost(half) on warm bench_polymul calls.
//!
//! Unlike a flat per-operator count, each wasm operator is charged a weight that
//! reflects its real hardware cost, so the metric rewards algorithms that minimize
//! actual compute rather than gaming the cheapest opcode:
//!
//!   integer div / rem (i64)        25   (a hardware divide is ~20-40x an add)
//!   integer div / rem (i32)        15
//!   multiply (incl. SIMD lanes)     3
//!   load / store                    2
//!   add/sub/shift/bitwise/compare   1   (incl. all SIMD add/sub/etc. — per
//!                                        instruction, so a v128 lane op is ~2-4x
//!                                        the throughput of its scalar form)
//!   call / branch / select          1-2
//!   const / local.* / global.* /    0   (register & stack bookkeeping is free on
//!   block / loop / if / drop             real hardware)
//!
//! The host instruments the (frozen, out-of-tree) wasm at load time with a counter
//! global, so the measurement is deterministic and cannot be altered by a submission.
use walrus::ir::*;
use walrus::{FunctionId, LocalFunction, ValType};

const FULL: u32 = 32;
const HALF: u32 = 16;

const DIV64_W: i64 = 25;
const DIV32_W: i64 = 15;
const MUL_W: i64 = 3;
const MEM_W: i64 = 2;

fn binop_weight(op: BinaryOp) -> i64 {
    use BinaryOp::*;
    match op {
        I64DivS | I64DivU | I64RemS | I64RemU => DIV64_W,
        I32DivS | I32DivU | I32RemS | I32RemU => DIV32_W,
        I64Mul | I32Mul | I64x2Mul | I32x4Mul | I16x8Mul => MUL_W,
        _ => 1, // add/sub/and/or/xor/shift/rotate/compare, incl. all SIMD non-mul ALU
    }
}

/// Weighted cost of executing one instruction once.
fn weight(instr: &Instr) -> i64 {
    match instr {
        Instr::Binop(b) => binop_weight(b.op),
        Instr::Unop(_) => 1,
        Instr::Load(_) | Instr::Store(_) => MEM_W,
        Instr::Call(_) | Instr::CallIndirect(_) => 2,
        Instr::Br(_) | Instr::BrIf(_) | Instr::BrTable(_) | Instr::Return(_) | Instr::Select(_) => 1,
        Instr::MemorySize(_) | Instr::MemoryGrow(_) => MEM_W,
        // bookkeeping / structural — free on real hardware:
        Instr::Const(_)
        | Instr::LocalGet(_)
        | Instr::LocalSet(_)
        | Instr::LocalTee(_)
        | Instr::GlobalGet(_)
        | Instr::GlobalSet(_)
        | Instr::Block(_)
        | Instr::Loop(_)
        | Instr::IfElse(_)
        | Instr::Drop(_) => 0,
        _ => 1,
    }
}

fn collect_seqs(func: &LocalFunction, seq: InstrSeqId, out: &mut Vec<InstrSeqId>) {
    out.push(seq);
    for (instr, _) in func.block(seq).instrs.iter() {
        match instr {
            Instr::Block(b) => collect_seqs(func, b.seq, out),
            Instr::Loop(l) => collect_seqs(func, l.seq, out),
            Instr::IfElse(ie) => {
                collect_seqs(func, ie.consequent, out);
                collect_seqs(func, ie.alternative, out);
            }
            _ => {}
        }
    }
}

/// Instrument every executed operator with `cost += weight`, returning the wasm
/// bytes and the exported name of the i64 counter global.
fn instrument(wasm: &[u8]) -> (Vec<u8>, &'static str) {
    let mut module = walrus::Module::from_buffer(wasm).expect("parse wasm");
    let cost = module
        .globals
        .add_local(ValType::I64, true, false, walrus::ConstExpr::Value(Value::I64(0)));
    module.exports.add("__cost", cost);

    let local_ids: Vec<FunctionId> = module
        .funcs
        .iter()
        .filter(|f| matches!(f.kind, walrus::FunctionKind::Local(_)))
        .map(|f| f.id())
        .collect();

    for fid in local_ids {
        let func = module.funcs.get_mut(fid).kind.unwrap_local_mut();
        let entry = func.entry_block();
        let mut seqs = Vec::new();
        collect_seqs(func, entry, &mut seqs);
        for sid in seqs {
            let old = std::mem::take(&mut func.block_mut(sid).instrs);
            let mut new: Vec<(Instr, InstrLocId)> = Vec::with_capacity(old.len() * 2);
            for (instr, loc) in old.into_iter() {
                let w = weight(&instr);
                if w != 0 {
                    new.push((Instr::GlobalGet(GlobalGet { global: cost }), loc));
                    new.push((Instr::Const(Const { value: Value::I64(w) }), loc));
                    new.push((Instr::Binop(Binop { op: BinaryOp::I64Add }), loc));
                    new.push((Instr::GlobalSet(GlobalSet { global: cost }), loc));
                }
                new.push((instr, loc));
            }
            func.block_mut(sid).instrs = new;
        }
    }
    (module.emit_wasm(), "__cost")
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: polymul-fuel-meter <module.wasm>");
    let wasm = std::fs::read(&path).expect("read wasm");
    let (instrumented, cost_name) = instrument(&wasm);

    use wasmtime::{Config, Engine, Instance, Module, Store, Val};
    let mut config = Config::new();
    config.wasm_simd(true);
    let engine = Engine::new(&config).expect("engine");
    let module = Module::from_binary(&engine, &instrumented).expect("parse instrumented wasm");

    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiate");
    let f = instance
        .get_typed_func::<u32, u32>(&mut store, "bench_polymul")
        .expect("get bench_polymul");
    let cost = instance.get_global(&mut store, cost_name).expect("cost global");

    let read = |store: &mut Store<()>| cost.get(&mut *store).unwrap_i64();
    let reset = |store: &mut Store<()>| cost.set(&mut *store, Val::I64(0)).unwrap();

    reset(&mut store);
    let out_full = f.call(&mut store, FULL).expect("call full");
    let cost_full = read(&mut store);

    reset(&mut store);
    let out_half = f.call(&mut store, HALF).expect("call half");
    let cost_half = read(&mut store);

    let work = cost_full - cost_half;
    println!("full {} pairs -> checksum {} (cost {})", FULL, out_full, cost_full);
    println!("half {} pairs -> checksum {} (cost {})", HALF, out_half, cost_half);
    println!(
        "WORK: {} (weighted deterministic compute cost for {} extra pairs; lower is faster)",
        work,
        FULL - HALF
    );
}
