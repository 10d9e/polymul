# Results log

Leaderboard of recorded submissions. Full narratives live in
[`history/entries/`](history/entries/).

**Current record: 8779216** (@10d9e, entry 0058)

| # | date | author | SCORE | Δ vs record | commit | entry | note |
|---|------|--------|-------|-------------|--------|-------|------|
| 0001 | 2026-06-24 | @10d9e | 319558256 | — (baseline) | `3793fd8` | [0001](history/entries/0001-baseline.md) | Initial naive O(N²) schoolbook negacyclic poly_mul at N=1024 |
| 0002 | 2026-06-24 | @10d9e | 51690192 | -267868064 (new record) | `ab380d0` | [0002](history/entries/0002--10d9e.md) | Replace naive O(N^2) schoolbook with an O(N log N) negacyclic NTT. Three NTT-fri… |
| 0003 | 2026-06-24 | @10d9e | 50018256 | -1671936 (new record) | `33590ce` | [0003](history/entries/0003--10d9e.md) | Transform both multiply operands (a and b) in a single lockstep forward NTT (ntt… |
| 0004 | 2026-06-24 | @10d9e | 46689312 | -3328944 (new record) | `d9491dc` | [0004](history/entries/0004--10d9e.md) | Combine each pair of consecutive radix-2 DIF (Gentleman-Sande) forward stages in… |
| 0005 | 2026-06-24 | @10d9e | 44764944 | -1924368 (new record) | `1be0313` | [0005](history/entries/0005--10d9e.md) | Mirror the radix-4 forward fusion on the inverse: combine each pair of consecuti… |
| 0006 | 2026-06-24 | @10d9e | 44099440 | -665504 (new record) | `c57b0a3` | [0006](history/entries/0006--10d9e.md) | The negacyclic pre-weight a_i *= psi^i was a separate pass over the data that ma… |
| 0007 | 2026-06-24 | @10d9e | 43697392 | -402048 (new record) | `f0a035c` | [0007](history/entries/0007--10d9e.md) | The negacyclic post-weight (multiply each inverse-NTT output by psi^{-j}*N^{-1})… |
| 0008 | 2026-06-24 | @10d9e | 42969328 | -728064 (new record) | `486fc90` | [0008](history/entries/0008--10d9e.md) | The elementwise spectral product fa[i] *= fb[i] was a separate pass between the … |
| 0009 | 2026-06-24 | @10d9e | 42559728 | -409600 (new record) | `3f2e4ed` | [0009](history/entries/0009--10d9e.md) | In the Garner CRT step, v0 = r0[j] is already a residue mod P0, and P0 < P1, so … |
| 0010 | 2026-06-24 | @10d9e | 41085168 | -1474560 (new record) | `5ad35c6` | [0010](history/entries/0010--10d9e.md) | In the fused radix-4 forward butterfly, several intermediate difference terms (x… |
| 0011 | 2026-06-24 | @10d9e | 40737216 | -347952 (new record) | `dfaaa90` | [0011](history/entries/0011--10d9e.md) | Mirror the forward lazy-reduction change in the inverse DIT butterfly. The terms… |
| 0012 | 2026-06-24 | @10d9e | 39285792 | -1451424 (new record) | `a8510ec` | [0012](history/entries/0012--10d9e.md) | In the radix-4 fused transforms the q=1 pass has ta=tc=1 (forward) and ita=itc=1… |
| 0013 | 2026-06-24 | @10d9e | 39236640 | -49152 (new record) | `b50d660` | [0013](history/entries/0013--10d9e.md) | In the Garner step computing w = (v0 + P0*v1) mod P2, the term v0 (< P0 < 2^30) … |
| 0014 | 2026-06-24 | @10d9e | 41202768 | +1966128 (no improvement) | `9e630b7` | [0014](history/entries/0014--10d9e.md) | In the forward radix-4 butterfly, the sum terms u=x0+x2 and v=x1+x3 were each co… |
| 0015 | 2026-06-24 | @10d9e | 44975184 | +5738544 (no improvement) | `41c6f40` | [0015](history/entries/0015--10d9e.md) | Mirror the forward lazy-reduction change in the inverse DIT butterfly. The terms… |
| 0016 | 2026-06-24 | @10d9e | 47268944 | +8032304 (no improvement) | `17de8f7` | [0016](history/entries/0016--10d9e.md) | Two cleanups. (1) The remaining conditional subtraction on the forward butterfly… |
| 0017 | 2026-06-24 | @10d9e | 46449744 | +7213104 (no improvement) | `e607afa` | [0017](history/entries/0017--10d9e.md) | Two more Garner intermediates that feed only a subsequent modular multiply are k… |
| 0018 | 2026-06-24 | @10d9e | 45835344 | +6598704 (no improvement) | `b376732` | [0018](history/entries/0018--10d9e.md) | In the q=1 forward butterfly ta=1, so m02 = x0 + p - x2 is a plain difference (n… |
| 0019 | 2026-06-24 | @10d9e | 43871616 | +4634976 (no improvement) | `8c49ff3` | [0019](history/entries/0019--10d9e.md) | Regroup the 10 radix-2 forward stages as 3+3+2+2 instead of 2+2+2+2+2: two fused… |
| 0020 | 2026-06-24 | @10d9e | 42196224 | +2959584 (no improvement) | `6efef57` | [0020](history/entries/0020--10d9e.md) | Mirror the radix-8 forward on the inverse: regroup the 10 radix-2 DIT stages as … |
| 0021 | 2026-06-24 | @10d9e | 41081712 | +1845072 (no improvement) | `7515355` | [0021](history/entries/0021--10d9e.md) | Fuse the forward NTT's last two radix-4 passes (stages L=8,4 and L=2,1) into a s… |
| 0022 | 2026-06-24 | @10d9e | 40565136 | +1328496 (no improvement) | `8f7db90` | [0022](history/entries/0022--10d9e.md) | Mirror the radix-16 forward on the inverse: fuse the inverse's first two radix-4… |
| 0023 | 2026-06-24 | @10d9e | 37923024 | -1313616 (new record) | `530fc28` | [0023](history/entries/0023--10d9e.md) | In both radix-16 passes the first butterfly of every block (kk=0) has twiddle w1… |
| 0024 | 2026-06-24 | @10d9e | 34777856 | -3145168 (new record) | `d1743eb` | [0024](history/entries/0024--10d9e.md) | Division-free Shoup-NTT (radix-4): every constant modular multiply (twiddles, p… |
| 0025 | 2026-06-24 | @10d9e | 32106560 | -2671296 (new record) | `2729e7d` | [0025](history/entries/0025--10d9e.md) | Lazy Harvey-style Shoup butterflies (radix-4): keep values in [0,2p), twiddle multiplies use a lazy Shoup with no fina… |
| 0026 | 2026-06-24 | @10d9e | 29231216 | -2875344 (new record) | `67c6a35` | [0026](history/entries/0026--10d9e.md) | Fuse the per-input reduction into the psi pre-weight: lazy Shoup accepts any u32 input, so a[j]*psi^j mod p is one ste… |
| 0027 | 2026-06-24 | @10d9e | 29008896 | -222320 (new record) | `09ee130` | [0027](history/entries/0027--10d9e.md) | Montgomery pointwise product: carry the spectral domain in Montgomery form (R baked into psi, R^-1 into ipsi) so the po… |
| 0028 | 2026-06-24 | @10d9e | 26638848 | -2370048 (new record) | `5b7f75d` | [0028](history/entries/0028--10d9e.md) | Fuse the two forward transforms (a and b) into one lockstep radix-4 DIF so the stage twiddle loads and index arithm… |
| 0029 | 2026-06-24 | @10d9e | 25487616 | -1151232 (new record) | `5af0981` | [0029](history/entries/0029--10d9e.md) | Fuse last two forward DIF stages into a radix-16 register pass The last two radi… |
| 0030 | 2026-06-24 | @10d9e | 24654528 | -833088 (new record) | `2c1bc5b` | [0030](history/entries/0030--10d9e.md) | Fuse first two inverse DIT stages into a radix-16 register pass Mirror of the fo… |
| 0031 | 2026-06-24 | @10d9e | 24215232 | -439296 (new record) | `5bee2a3` | [0031](history/entries/0031--10d9e.md) | Fold the spectral pointwise product into the inverse transform The Montgomery po… |
| 0032 | 2026-06-24 | @10d9e | 23663280 | -551952 (new record) | `8cff969` | [0032](history/entries/0032--10d9e.md) | Fold the psi inverse post-weight into the last DIT stage The final inverse DIT s… |
| 0033 | 2026-06-24 | @10d9e | 23022048 | -641232 (new record) | `2aae85f` | [0033](history/entries/0033--10d9e.md) | Fold the psi pre-weight into the first forward DIF stage The negacyclic psi^j pr… |
| 0034 | 2026-06-25 | @10d9e | 22469088 | -552960 (new record) | `16c8edf` | [0034](history/entries/0034--10d9e.md) | Drop the redundant s3 reduction in every butterfly The difference term s3 = b + … |
| 0035 | 2026-06-25 | @10d9e | 22256096 | -212992 (new record) | `39100b6` | [0035](history/entries/0035--10d9e.md) | Lean Garner CRT: drop the eager reductions Shoup makes unnecessary Shoup accepts… |
| 0036 | 2026-06-25 | @10d9e | 22108640 | -147456 (new record) | `c76126a` | [0036](history/entries/0036--10d9e.md) | Drop output reductions in the final DIT stage (post-weight absorbs them) The las… |
| 0037 | 2026-06-25 | @10d9e | 21850448 | -258192 (new record) | `34c1f0f` | [0037](history/entries/0037--10d9e.md) | Harvey-lazy inverse DIT: keep values in [0,4p), reduce only the untwiddled input… |
| 0038 | 2026-06-25 | @10d9e | 21386528 | -463920 (new record) | `547725c` | [0038](history/entries/0038--10d9e.md) | Fuse the forward/inverse boundary: last fwd + pointwise + first inv in registers… |
| 0039 | 2026-06-25 | @10d9e | 21091616 | -294912 (new record) | `ea2abb4` | [0039](history/entries/0039--10d9e.md) | Lazy Montgomery pointwise + skip redundant reductions in the first inverse sub-s… |
| 0040 | 2026-06-25 | @10d9e | 16335488 | -4756128 (new record) | `9c10612` | [0040](history/entries/0040--10d9e.md) | The forward NTT multiplies BOTH operands a and b by the SAME twiddle factors in … |
| 0041 | 2026-06-25 | @10d9e | 14113408 | -2222080 (new record) | `4b974da` | [0041](history/entries/0041--10d9e.md) | Vectorize the inverse DIT transform the same way the forward already is, but pai… |
| 0042 | 2026-06-25 | @10d9e | 13474592 | -638816 (new record) | `3cea8ea` | [0042](history/entries/0042--10d9e.md) | Vectorize the Garner CRT reconstruction. The three per-prime residue arrays r0,r… |
| 0043 | 2026-06-25 | @10d9e | 13039472 | -435120 (new record) | `036eac5` | [0043](history/entries/0043--10d9e.md) | Vectorize the second inverse DIT sub-stage (dit_l4) inside the fused boundary ti… |
| 0044 | 2026-06-25 | @10d9e | 12695456 | -344016 (new record) | `0f957f4` | [0044](history/entries/0044--10d9e.md) | Vectorize the spectral pointwise Montgomery product in the fused boundary. The p… |
| 0045 | 2026-06-25 | @10d9e | 12139952 | -555504 (new record) | `00bc58c` | [0045](history/entries/0045--10d9e.md) | Eliminate the per-butterfly splat in the forward NTT. The forward pairs operands… |
| 0046 | 2026-06-25 | @10d9e | 11808080 | -331872 (new record) | `3311f46` | [0046](history/entries/0046--10d9e.md) | Mirror the forward u64-table change on the inverse. The inverse pairs adjacent b… |
| 0047 | 2026-06-25 | @10d9e | 11574416 | -233664 (new record) | `d036a69` | [0047](history/entries/0047--10d9e.md) | Smaller 27-bit primes unlock lazy reduction removal in the forward NTT Replace t… |
| 0048 | 2026-06-25 | @10d9e | 11519216 | -55200 (new record) | `fbf3261` | [0048](history/entries/0048--10d9e.md) | Drop internal reductions in the final inverse DIT stage The final inverse DIT st… |
| 0049 | 2026-06-25 | @10d9e | 11031632 | -487584 (new record) | `5b28709` | [0049](history/entries/0049--10d9e.md) | Hoist stage twiddle loads out of the block loop (butterfly-outer iteration) In b… |
| 0050 | 2026-06-25 | @10d9e | 9855056 | -1176576 (new record) | `6a23f65` | [0050](history/entries/0050--10d9e.md) | Replace Shoup with Plantard multiplication throughout the NTT Every constant mod… |
| 0051 | 2026-06-25 | @10d9e | 9805696 | -49360 (new record) | `a2f4a5e` | [0051](history/entries/0051--10d9e.md) | Use Plantard for the Garner CRT modular multiplies too The three constant modula… |
| 0052 | 2026-06-25 | @10d9e | 9428128 | -377568 (new record) | `3f66a17` | [0052](history/entries/0052--10d9e.md) | Keep forward butterfly sums lazy; one 8p->2p reduce for the leg-0 output In the … |
| 0053 | 2026-06-25 | @10d9e | 9335968 | -92160 (new record) | `41b30ed` | [0053](history/entries/0053--10d9e.md) | Keep inverse butterfly sums lazy; one 8p->2p reduce for the untwiddled input Mir… |
| 0054 | 2026-06-25 | @10d9e | 9225376 | -110592 (new record) | `0ab3aa6` | [0054](history/entries/0054--10d9e.md) | Keep the first inverse sub-stage sums lazy (dit_l1_in2p) The scalar first invers… |
| 0055 | 2026-06-25 | @10d9e | 9063792 | -161584 (new record) | `2e6a8a4` | [0055](history/entries/0055--10d9e.md) | Fuse the inverse final DIT stage + post-weight into the Garner CRT Previously ea… |
| 0056 | 2026-06-25 | @10d9e | 9014640 | -49152 (new record) | `a350dbf` | [0056](history/entries/0056--10d9e.md) | Skip the post-weight reduction for the P1/P2 residues in the fused CRT Only the … |
| 0057 | 2026-06-25 | @10d9e | 8957488 | -57152 (new record) | `3ba9e57` | [0057](history/entries/0057--10d9e.md) | Pack the (a,b) input operands into lane pairs once, shared by all three primes T… |
| 0058 | 2026-06-25 | @10d9e | 8779216 | -178272 (new record) | `74b1860` | [0058](history/entries/0058--10d9e.md) | Vectorize the first inverse sub-stage (dit_l1_in2p) This scalar radix-4 sub-stag… |
