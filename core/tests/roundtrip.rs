//! Phase 0a の合格判定: bytes → Frame → bytes の完全一致。

use beyond_qr_core::{decode_payload, encode_payload, FrameSpec};

#[test]
fn phase_0_500_byte_deterministic_payload() {
    let spec = FrameSpec::PHASE_0;
    let payload: Vec<u8> = (0..500).map(|i| (i as u8).wrapping_mul(31).wrapping_add(7)).collect();
    let frame = encode_payload(&payload, spec).expect("encode");
    let recovered = decode_payload(&frame).expect("decode");
    assert_eq!(recovered, payload, "byte-exact round-trip failed");
}

#[test]
fn phase_0_empty_payload() {
    let spec = FrameSpec::PHASE_0;
    let payload: Vec<u8> = vec![];
    let frame = encode_payload(&payload, spec).expect("encode");
    let recovered = decode_payload(&frame).expect("decode");
    assert_eq!(recovered, payload);
}

#[test]
fn phase_0_single_byte() {
    let spec = FrameSpec::PHASE_0;
    let payload = vec![0xAAu8];
    let frame = encode_payload(&payload, spec).expect("encode");
    let recovered = decode_payload(&frame).expect("decode");
    assert_eq!(recovered, payload);
}

#[test]
fn phase_0_at_capacity() {
    let spec = FrameSpec::PHASE_0;
    let max = spec.max_payload_bytes();
    let payload: Vec<u8> = (0..max).map(|i| (i % 251) as u8).collect();
    let frame = encode_payload(&payload, spec).expect("encode");
    let recovered = decode_payload(&frame).expect("decode");
    assert_eq!(recovered.len(), max);
    assert_eq!(recovered, payload);
}

#[test]
fn phase_0_random_sizes() {
    // 単純な決定論的乱数で複数サイズを検証する。
    let spec = FrameSpec::PHASE_0;
    let max = spec.max_payload_bytes();
    let mut seed: u64 = 0xDEADBEEFCAFEBABE;
    for _ in 0..16 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let len = (seed as usize) % (max + 1);
        let payload: Vec<u8> = (0..len)
            .map(|i| {
                let s = seed.wrapping_add(i as u64);
                (s.wrapping_mul(2862933555777941757) >> 33) as u8
            })
            .collect();
        let frame = encode_payload(&payload, spec).expect("encode");
        let recovered = decode_payload(&frame).expect("decode");
        assert_eq!(recovered, payload, "roundtrip failed for len={len}");
    }
}
