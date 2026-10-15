//! Independent RFC7636 fixture oracle; no verifier/challenge bytes appear in diagnostics.
use sha2::{Digest, Sha256};
use url::Url;

pub fn challenge(url: &Url) -> Option<String> {
    let fields: Vec<_> = url.query_pairs().collect();
    let one = |name| {
        let values: Vec<_> = fields.iter().filter(|(key, _)| key == name).collect();
        (values.len() == 1).then(|| values[0].1.to_string())
    };
    if one("code_challenge_method").as_deref() != Some("S256") {
        return None;
    }
    one("code_challenge").filter(|value| !value.is_empty())
}

pub fn authorization_form_valid(body: &[u8], challenge: &str) -> bool {
    let fields: Vec<_> = url::form_urlencoded::parse(body).collect();
    let verifiers: Vec<_> = fields
        .iter()
        .filter(|(key, _)| key == "code_verifier")
        .collect();
    if verifiers.len() != 1 {
        return false;
    }
    let verifier = verifiers[0].1.as_bytes();
    if !(43..=128).contains(&verifier.len())
        || !verifier
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(byte))
    {
        return false;
    }
    let Some(decoded) = decode_challenge(challenge) else {
        return false;
    };
    let digest: [u8; 32] = Sha256::digest(verifier).into();
    decoded.as_slice() == digest
}

fn decode_challenge(value: &str) -> Option<Vec<u8>> {
    if value.len() != 43 {
        return None;
    }
    let mut output = Vec::with_capacity(32);
    let mut bits = 0_u32;
    let mut count = 0;
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(digit);
        count += 6;
        if count >= 8 {
            count -= 8;
            output.push((bits >> count) as u8);
        }
    }
    (output.len() == 32 && bits & ((1 << count) - 1) == 0).then_some(output)
}
