use anyhow::{Result, ensure};

/// Validate the fixed loopback authority without replacing OAuth state or issuer checks.
pub(super) fn validate(text: &str, port: u16) -> Result<()> {
    let (header, remainder) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("Incomplete callback request."))?;
    ensure!(remainder.is_empty(), "Unexpected callback request body.");
    let expected = format!("127.0.0.1:{port}");
    let mut host_count = 0usize;
    let mut length_count = 0usize;
    for line in header.split("\r\n").skip(1) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Malformed callback header."))?;
        ensure!(
            !name.is_empty()
                && name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                })
                && value
                    .bytes()
                    .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte)),
            "Malformed callback header."
        );
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("host") {
            host_count += 1;
            ensure!(value == expected, "Invalid callback Host.");
        }
        ensure!(
            !name.eq_ignore_ascii_case("transfer-encoding"),
            "Callback request bodies are not supported."
        );
        if name.eq_ignore_ascii_case("content-length") {
            length_count += 1;
            ensure!(
                length_count == 1 && value == "0",
                "Invalid callback body framing."
            );
        }
    }
    ensure!(host_count == 1, "Expected exactly one callback Host.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_exact_authority_with_field_casing_and_http_whitespace() {
        for version in ["HTTP/1.0", "HTTP/1.1"] {
            for host in ["Host", "hOsT"] {
                let request = format!(
                    "GET /oauth/callback {version}\r\n{host}:\t127.0.0.1:1234 \r\nContent-Length: 0\r\n\r\n"
                );
                assert!(validate(&request, 1234).is_ok());
            }
        }
    }

    #[test]
    fn rejects_missing_foreign_or_ambiguous_authority() {
        for headers in [
            "",
            "Host: foreign.invalid\r\n",
            "Host: 127.0.0.1:1235\r\n",
            "Host: 127.0.0.1:1234\r\nHost: 127.0.0.1:1234\r\n",
            "Host : 127.0.0.1:1234\r\n",
            "Host: 127.0.0.1:1234\nX-Test: bad\r\n",
            "Host: \u{a0}127.0.0.1:1234\r\n",
        ] {
            let request = format!("GET /oauth/callback HTTP/1.1\r\n{headers}\r\n");
            assert!(validate(&request, 1234).is_err());
        }
    }

    #[test]
    fn rejects_bodies_duplicate_length_and_obsolete_folding() {
        for extra in [
            "Transfer-Encoding: chunked\r\n",
            "Content-Length: 1\r\n",
            "Content-Length: 0\r\ncontent-length: 0\r\n",
            " folded: value\r\n",
        ] {
            let request =
                format!("GET /oauth/callback HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n{extra}\r\n");
            assert!(validate(&request, 1234).is_err());
        }
        assert!(
            validate(
                "GET /oauth/callback HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\nbody",
                1234
            )
            .is_err()
        );
    }
}
