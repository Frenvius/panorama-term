use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::{Nonce, XSalsa20Poly1305};
use sha2::{Digest, Sha512};
use x25519_dalek::{PublicKey, StaticSecret};

pub const DIR_CLIENT: u8 = 0;

pub fn nonce(dir: u8, ctr: u64) -> Nonce {
    let mut n = [0u8; 24];
    n[0] = dir;
    n[1..9].copy_from_slice(&ctr.to_le_bytes());
    Nonce::from(n)
}

pub struct Channel {
    cipher: XSalsa20Poly1305,
    send_ctr: u64,
    recv_ctr: u64,
    send_dir: u8,
}
impl Channel {
    pub fn new(key: &[u8; 32], send_dir: u8) -> Self {
        Self {
            cipher: XSalsa20Poly1305::new(key.into()),
            send_ctr: 0,
            recv_ctr: 0,
            send_dir,
        }
    }
    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let sealed = self
            .cipher
            .encrypt(&nonce(self.send_dir, self.send_ctr), plain)
            .expect("secretbox encrypt");
        self.send_ctr += 1;
        sealed
    }
    pub fn open(&mut self, sealed: &[u8]) -> Option<Vec<u8>> {
        let plain = self
            .cipher
            .decrypt(&nonce(1 - self.send_dir, self.recv_ctr), sealed)
            .ok()?;
        self.recv_ctr += 1;
        Some(plain)
    }
}

pub fn session_key(
    secret: &StaticSecret,
    peer: &[u8; 32],
    client: &[u8; 32],
    server: &[u8; 32],
    cn: &[u8; 24],
    sn: &[u8; 24],
) -> Option<[u8; 32]> {
    let shared = secret.diffie_hellman(&PublicKey::from(*peer));
    if !shared.was_contributory() {
        return None;
    }
    let mut h = Sha512::new();
    h.update(shared.as_bytes());
    h.update(client);
    h.update(server);
    h.update(cn);
    h.update(sn);
    Some(h.finalize()[..32].try_into().unwrap())
}
