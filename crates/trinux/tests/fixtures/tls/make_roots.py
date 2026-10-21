import certifi, os, re
from cryptography import x509
from cryptography.x509.oid import NameOID
want = ["ISRG Root X1", "ISRG Root X2", "GTS Root R1", "GTS Root R2", "GTS Root R3", "GTS Root R4",
        "GlobalSign Root R46", "GlobalSign Root E46",
        "DigiCert Global Root CA", "DigiCert Global Root G2", "DigiCert Global Root G3", "DigiCert High Assurance EV Root CA",
        "DigiCert TLS RSA4096 Root G5", "DigiCert TLS ECC P384 Root G5",
        "Amazon Root CA 1", "Amazon Root CA 2", "Amazon Root CA 3", "Amazon Root CA 4",
        "USERTrust RSA Certification Authority", "USERTrust ECC Certification Authority", "SSL.com TLS RSA Root CA 2022", "SSL.com TLS ECC Root CA 2022",
        "Starfield Root Certificate Authority - G2", "Go Daddy Root Certificate Authority - G2",
        "Microsoft RSA Root Certificate Authority 2017", "Microsoft ECC Root Certificate Authority 2017",
        "Entrust Root Certification Authority - G2"]
text = open(certifi.where()).read()
blocks = re.findall(r"-----BEGIN CERTIFICATE-----\n.*?-----END CERTIFICATE-----\n", text, re.S)
found = {}
for b in blocks:
    c = x509.load_pem_x509_certificate(b.encode())
    cns = c.subject.get_attributes_for_oid(NameOID.COMMON_NAME)
    cn = cns[0].value if cns else None
    if cn in want and cn not in found:
        found[cn] = (c, b)
missing = [w for w in want if w not in found]
print("missing:", missing)
out = ["# /etc/ssl/cert.pem: the roots Trinux trusts for TLS (a few widely used CAs",
       "# from the Mozilla bundle as packaged by certifi, under the MPL 2.0; see",
       "# NOTICE). Text outside the BEGIN/END lines is ignored.", ""]
for w in want:
    if w in found:
        c, b = found[w]
        out.append(f"# {w} (until {c.not_valid_after_utc.date()})")
        out.append(b)
open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../../user/etc/ssl/cert.pem"), "w", newline="\n").write("\n".join(out))
print(len(found))
