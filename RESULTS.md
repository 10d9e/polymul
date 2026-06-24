# Results log

Leaderboard of recorded submissions. Full narratives live in
[`history/entries/`](history/entries/).

**Current record: 21021584** (@10d9e, entry 0018)

| # | date | author | SCORE | Δ vs record | commit | entry | note |
|---|------|--------|-------|-------------|--------|-------|------|
| 0001 | 2026-06-24 | @10d9e | 571975280 | — (baseline) | `3793fd8` | [0001](history/entries/0001-baseline.md) | Initial naive O(N²) schoolbook negacyclic poly_mul at N=1024 |
| 0002 | 2026-06-24 | @10d9e | 48678784 | -523296496 (new record) | `ab380d0` | [0002](history/entries/0002--10d9e.md) | Replace naive O(N^2) schoolbook with an O(N log N) negacyclic NTT. Three NTT-fri… |
| 0003 | 2026-06-24 | @10d9e | 45822736 | -2856048 (new record) | `33590ce` | [0003](history/entries/0003--10d9e.md) | Transform both multiply operands (a and b) in a single lockstep forward NTT (ntt… |
| 0004 | 2026-06-24 | @10d9e | 40341088 | -5481648 (new record) | `d9491dc` | [0004](history/entries/0004--10d9e.md) | Combine each pair of consecutive radix-2 DIF (Gentleman-Sande) forward stages in… |
| 0005 | 2026-06-24 | @10d9e | 36505744 | -3835344 (new record) | `1be0313` | [0005](history/entries/0005--10d9e.md) | Mirror the radix-4 forward fusion on the inverse: combine each pair of consecuti… |
| 0006 | 2026-06-24 | @10d9e | 35295312 | -1210432 (new record) | `c57b0a3` | [0006](history/entries/0006--10d9e.md) | The negacyclic pre-weight a_i *= psi^i was a separate pass over the data that ma… |
| 0007 | 2026-06-24 | @10d9e | 34477104 | -818208 (new record) | `f0a035c` | [0007](history/entries/0007--10d9e.md) | The negacyclic post-weight (multiply each inverse-NTT output by psi^{-j}*N^{-1})… |
| 0008 | 2026-06-24 | @10d9e | 33142128 | -1334976 (new record) | `486fc90` | [0008](history/entries/0008--10d9e.md) | The elementwise spectral product fa[i] *= fb[i] was a separate pass between the … |
| 0009 | 2026-06-24 | @10d9e | 33109360 | -32768 (new record) | `3f2e4ed` | [0009](history/entries/0009--10d9e.md) | In the Garner CRT step, v0 = r0[j] is already a residue mod P0, and P0 < P1, so … |
| 0010 | 2026-06-24 | @10d9e | 29177200 | -3932160 (new record) | `5ad35c6` | [0010](history/entries/0010--10d9e.md) | In the fused radix-4 forward butterfly, several intermediate difference terms (x… |
| 0011 | 2026-06-24 | @10d9e | 28234192 | -943008 (new record) | `dfaaa90` | [0011](history/entries/0011--10d9e.md) | Mirror the forward lazy-reduction change in the inverse DIT butterfly. The terms… |
| 0012 | 2026-06-24 | @10d9e | 27394768 | -839424 (new record) | `a8510ec` | [0012](history/entries/0012--10d9e.md) | In the radix-4 fused transforms the q=1 pass has ta=tc=1 (forward) and ita=itc=1… |
| 0013 | 2026-06-24 | @10d9e | 27263696 | -131072 (new record) | `b50d660` | [0013](history/entries/0013--10d9e.md) | In the Garner step computing w = (v0 + P0*v1) mod P2, the term v0 (< P0 < 2^30) … |
| 0014 | 2026-06-24 | @10d9e | 24560528 | -2703168 (new record) | `9e630b7` | [0014](history/entries/0014--10d9e.md) | In the forward radix-4 butterfly, the sum terms u=x0+x2 and v=x1+x3 were each co… |
| 0015 | 2026-06-24 | @10d9e | 21906320 | -2654208 (new record) | `41c6f40` | [0015](history/entries/0015--10d9e.md) | Mirror the forward lazy-reduction change in the inverse DIT butterfly. The terms… |
| 0016 | 2026-06-24 | @10d9e | 21136272 | -770048 (new record) | `17de8f7` | [0016](history/entries/0016--10d9e.md) | Two cleanups. (1) The remaining conditional subtraction on the forward butterfly… |
| 0017 | 2026-06-24 | @10d9e | 21070736 | -65536 (new record) | `e607afa` | [0017](history/entries/0017--10d9e.md) | Two more Garner intermediates that feed only a subsequent modular multiply are k… |
| 0018 | 2026-06-24 | @10d9e | 21021584 | -49152 (new record) | `b376732` | [0018](history/entries/0018--10d9e.md) | In the q=1 forward butterfly ta=1, so m02 = x0 + p - x2 is a plain difference (n… |
