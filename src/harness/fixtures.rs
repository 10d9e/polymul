//! Fixed benchmark fixture pairs. FROZEN — do not edit as part of autoresearch.
//!
//! Pairs are generated deterministically from splitmix64 seeds documented in
//! `fixtures/pairs.tsv`. The wasm meter and native eval share this source.

pub const NUM_PAIRS: usize = 32;

/// splitmix64 — deterministic PRNG for fixture generation.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Seeds for each scored pair (see fixtures/pairs.tsv).
pub const PAIR_SEEDS: [(u64, u64); NUM_PAIRS] = [
    (0x243F_6A88_85A3_08D3, 0x1319_8A2E_0370_7344),
    (0xA409_3822_299F_31D0, 0x082E_FA98_EC4E_6C89),
    (0x4528_21E6_38D0_1377, 0xBE54_66CF_34E9_0C6C),
    (0xC0AC_29B7_C97C_50DD, 0x3F84_D5B5_B547_0917),
    (0x9216_D5D9_8979_AFB1, 0xD131_0BA6_98DF_B5AC),
    (0x2FFD_72DB_D01A_DFB7, 0xB8E1_AFED_6A26_7E96),
    (0xBA7C_9045_F12C_7F99, 0x24A1_9947_B391_6CF7),
    (0x0801_F2E2_858E_FC16, 0x6369_20D8_7157_4E69),
    (0xA458_FEA3_F493_3D7E, 0x0D95_7F48_7283_0B33),
    (0x8771_9F08_4CAF_4846, 0xBD6D_B7A7_5988_9FB5),
    (0x1F35_5B5A_8FA9_9C6D, 0xC2B3_2952_067F_6355),
    (0x9E37_79B1_85A5_8D92, 0x4694_289F_E407_D7F1),
    (0xD16E_A748_8E35_58C3, 0xA1FF_E706_4466_2F08),
    (0xFD69_D5B4_88E5_7B2C, 0xF12C_7F99_24A1_9947),
    (0xB391_6CF7_0801_F2E2, 0x858E_FC16_6369_20D8),
    (0x7157_4E69_A458_FEA3, 0xF493_3D7E_0D95_7F48),
    (0x7283_0B33_8771_9F08, 0x4CAF_4846_BD6D_B7A7),
    (0x5988_9FB5_1F35_5B5A, 0x8FA9_9C6D_C2B3_2952),
    (0x067F_6355_9E37_79B1, 0x85A5_8D92_4694_289F),
    (0xE407_D7F1_D16E_A748, 0x8E35_58C3_A1FF_E706),
    (0x4466_2F08_FD69_D5B4, 0x88E5_7B2C_F12C_7F99),
    (0x24A1_9947_B391_6CF7, 0x0801_F2E2_858E_FC16),
    (0x6369_20D8_7157_4E69, 0xA458_FEA3_F493_3D7E),
    (0x0D95_7F48_7283_0B33, 0x8771_9F08_4CAF_4846),
    (0xBD6D_B7A7_5988_9FB5, 0x1F35_5B5A_8FA9_9C6D),
    (0xC2B3_2952_067F_6355, 0x9E37_79B1_85A5_8D92),
    (0x4694_289F_E407_D7F1, 0xD16E_A748_8E35_58C3),
    (0xA1FF_E706_4466_2F08, 0xFD69_D5B4_88E5_7B2C),
    (0xF12C_7F99_24A1_9947, 0xB391_6CF7_0801_F2E2),
    (0x858E_FC16_6369_20D8, 0x7157_4E69_A458_FEA3),
    (0xF493_3D7E_0D95_7F48, 0x7283_0B33_8771_9F08),
    (0x4CAF_4846_BD6D_B7A7, 0x5988_9FB5_1F35_5B5A),
];

pub struct Pair {
    pub name: &'static str,
    pub a: [u32; 1024],
    pub b: [u32; 1024],
}

static NAMES: [&str; NUM_PAIRS] = [
    "pair_00", "pair_01", "pair_02", "pair_03", "pair_04", "pair_05", "pair_06", "pair_07",
    "pair_08", "pair_09", "pair_10", "pair_11", "pair_12", "pair_13", "pair_14", "pair_15",
    "pair_16", "pair_17", "pair_18", "pair_19", "pair_20", "pair_21", "pair_22", "pair_23",
    "pair_24", "pair_25", "pair_26", "pair_27", "pair_28", "pair_29", "pair_30", "pair_31",
];

fn fill_poly(state: &mut u64) -> [u32; 1024] {
    let mut out = [0u32; 1024];
    for coeff in &mut out {
        *coeff = splitmix64(state) as u32;
    }
    out
}

/// Generate the `i`th fixture pair deterministically.
pub fn pair(i: usize) -> Pair {
    assert!(i < NUM_PAIRS);
    let (seed_a, seed_b) = PAIR_SEEDS[i];
    let mut sa = seed_a;
    let mut sb = seed_b;
    Pair {
        name: NAMES[i],
        a: fill_poly(&mut sa),
        b: fill_poly(&mut sb),
    }
}

/// All scored fixture pairs.
pub fn all() -> Vec<Pair> {
    (0..NUM_PAIRS).map(pair).collect()
}

/// XOR checksum of all coefficients in `out` (for wasm anti-DCE).
pub fn checksum(out: &[u32; 1024]) -> u32 {
    out.iter().fold(0u32, |acc, &x| acc ^ x)
}
