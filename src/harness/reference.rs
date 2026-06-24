//! Reference naive negacyclic polynomial multiplication.
//! FROZEN — do not edit as part of autoresearch.

/// Negacyclic polynomial multiplication: a(X) * b(X) mod (X^1024+1).
pub fn poly_mul(a: &[u32; 1024], b: &[u32; 1024]) -> [u32; 1024] {
    const N: usize = 1024;
    let mut res = [0u32; N];

    for i in 0..N {
        for j in 0..N {
            let prod = a[i].wrapping_mul(b[j]);
            if i + j < N {
                res[i + j] = res[i + j].wrapping_add(prod);
            } else {
                res[i + j - N] = res[i + j - N].wrapping_sub(prod);
            }
        }
    }

    res
}
