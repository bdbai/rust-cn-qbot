use ed25519::signature::Signer;
use ed25519_dalek::{SigningKey, SECRET_KEY_LENGTH};

#[derive(Debug)]
pub(super) struct ChallengeGenerator {
    signing_key: SigningKey,
}

impl ChallengeGenerator {
    pub(super) fn new(secret: &str) -> Self {
        let mut key_bytes = [0; SECRET_KEY_LENGTH];
        fill_repeating_bytes(&mut key_bytes, secret.as_bytes());

        let signing_key = SigningKey::from_bytes(&key_bytes);
        ChallengeGenerator { signing_key }
    }
    pub(super) fn calculate_challenge_response(&self, plain_material: &str) -> String {
        let sign = self.signing_key.sign(plain_material.as_bytes()).to_bytes();
        let mut res = "\0".repeat(sign.len() * 2);
        // Safety: hex encoding must result in a valid utf-8 string
        unsafe { hex::encode_to_slice(sign, res.as_bytes_mut()).unwrap() }
        res
    }
}

fn fill_repeating_bytes(mut key_bytes: &mut [u8], secret: &[u8]) {
    while !key_bytes.is_empty() {
        let len = key_bytes.len().min(secret.len());
        key_bytes[..len].copy_from_slice(&secret[..len]);
        key_bytes = &mut key_bytes[len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_challenge_generator() {
        let generator = ChallengeGenerator::new("DG5g3B4j9X2KOErG");
        let plain_token = "Arq0D5A61EgUu4OxUvOp";
        let event_ts = "1725442341";
        let plain_material = format!("{}{}", event_ts, plain_token);
        let response = generator.calculate_challenge_response(&plain_material);
        assert_eq!(response, "87befc99c42c651b3aac0278e71ada338433ae26fcb24307bdc5ad38c1adc2d01bcfcadc0842edac85e85205028a1132afe09280305f13aa6909ffc2d652c706");
    }
}
