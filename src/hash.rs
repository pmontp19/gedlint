//! SHA-256, hand-rolled because the crate takes no dependencies (FIPS 180-4).
//!
//! It exists for one caller: the baseline fingerprint (issue 61). A baseline
//! is a file users are told to commit, so it must not carry text lifted out
//! of a tree, and the digest is what replaces that text. A checksum-grade
//! hash (FNV, CRC) would be the wrong tool: preimages of a short surname are
//! trivial to find, which is precisely the property the fingerprint must not
//! have.
//!
//! Pure and allocation-light: no fs, no env, no randomness, so it compiles
//! to `wasm32-unknown-unknown` unchanged like the rest of the engine.

/// Round constants: the first 32 bits of the fractional parts of the cube
/// roots of the first 64 primes.
#[rustfmt::skip]
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial state: the first 32 bits of the fractional parts of the square
/// roots of the first 8 primes.
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// The SHA-256 digest of `bytes`.
///
/// The padded message is built in one buffer rather than streamed: the only
/// caller hashes single diagnostic messages, which are short, and a
/// one-shot function has nowhere to hide a state bug.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut buf = Vec::with_capacity((bytes.len() + 9).div_ceil(64) * 64);
    buf.extend_from_slice(bytes);
    buf.push(0x80);
    while buf.len() % 64 != 56 {
        buf.push(0);
    }
    buf.extend_from_slice(&bit_len.to_be_bytes());

    let mut h = H0;
    for chunk in buf.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (word, b) in w.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*b);
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (k, wi) in K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(*k)
                .wrapping_add(*wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (acc, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *acc = acc.wrapping_add(v);
        }
    }

    let mut out = [0u8; 32];
    for (slot, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(h.iter()) {
        *slot = word.to_be_bytes();
    }
    out
}

/// The first `bytes` bytes of the SHA-256 digest of `input`, lowercase hex.
pub(crate) fn sha256_hex(input: &[u8], bytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let d = sha256(input);
    let n = bytes.min(d.len());
    let mut out = String::with_capacity(n * 2);
    for b in &d[..n] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        sha256_hex(bytes, 32)
    }

    #[test]
    fn fips_vectors() {
        assert_eq!(
            hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56 bytes: the padding lands exactly at the two-block boundary.
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // 64 bytes: one full block, so padding needs a whole extra block.
        assert_eq!(
            hex(&[b'a'; 64]),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
        // Multi-block, and long enough that the length field is not a byte.
        assert_eq!(
            hex(&[b'z'; 1000]),
            "950f88b09cf1d5e2cdbc5660c77dce3962265c548797950095629a0ea2daea46"
        );
    }

    #[test]
    fn truncation_is_a_prefix() {
        let full = hex(b"gedlint");
        assert_eq!(sha256_hex(b"gedlint", 8), full[..16]);
        assert_eq!(sha256_hex(b"gedlint", 0), "");
        // Asking for more than the digest holds yields the whole digest.
        assert_eq!(sha256_hex(b"gedlint", 99), full);
    }

    #[test]
    fn distinct_inputs_differ() {
        assert_ne!(hex(b"Ferrer"), hex(b"Ferres"));
        assert_ne!(hex(b"a"), hex(b"a "));
    }
}
