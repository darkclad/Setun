# TLS test certificates

Made by `make.py` with Python's `cryptography`, an implementation independent
of `user/lib/x509.tri`; the keys are here so a host TLS server can use them.
They are test material only. Validity is fixed (2015-01-01 to 2046-01-01,
apart from `expired` and `notyet`), so rerunning `make.py` makes new keys but
the same kinds of chains. The test machine's clock starts at 2026-09-21.

- `roots.pem`: Setun Test Root RSA (RSA-4096) and Setun Test Root EC (P-384).
- `web`: web.test, *.web.test and 203.0.113.8 (P-256), under Setun Test CA 1
  (P-256, path length 0) under the RSA root. `missing` is the same without the CA.
- `rsa`, `ed`: RSA-2048 and Ed25519 keys under the EC root; `p384` a P-384 key under CA 1.
- Refused: `expired`, `notyet`, `self` (self-signed), `noca` (issued by a
  certificate that is not a CA), `deep` (a CA below CA 1, which allows none),
  `badsig` (one bit changed), `crit` (an unknown critical extension), `client`
  (for client authentication only), `impostor` (the RSA root's name, another key).

`make_roots.py` writes `user/etc/ssl/cert.pem`, the roots Trinux trusts, from
certifi's copy of the Mozilla bundle.
