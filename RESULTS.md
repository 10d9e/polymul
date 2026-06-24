# Results log

Leaderboard of recorded submissions. Full narratives live in
[`history/entries/`](history/entries/).

**Current record: 33142128** (@10d9e, entry 0008)

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
