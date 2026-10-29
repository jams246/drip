use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use drip_core::Blake3Digest;

pub const GENERATOR: &str = "BLAKE3 derive_key XOF, Drip phase03 deterministic fixture v1";
pub const SEED: &str = "drip-phase03-large-delta-seed-2026-10-27";

pub fn write_file(path: &Path, size: u64, seed: &str, insertion: bool) -> Blake3Digest {
    let mut generator = blake3::Hasher::new_derive_key("Drip phase03 deterministic fixture v1");
    generator.update(seed.as_bytes());
    let mut random = generator.finalize_xof();
    let mut output = File::create(path).unwrap();
    let mut digest = blake3::Hasher::new();
    let mut buffer = vec![0; 1024 * 1024];
    for half in 0..2 {
        let mut remaining = size / 2;
        while remaining > 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            random.read_exact(&mut buffer[..count]).unwrap();
            output.write_all(&buffer[..count]).unwrap();
            digest.update(&buffer[..count]);
            remaining -= count as u64;
        }
        if insertion && half == 0 {
            output.write_all(b"\r\n").unwrap();
            digest.update(b"\r\n");
        }
    }
    output.sync_all().unwrap();
    Blake3Digest::new(*digest.finalize().as_bytes())
}
