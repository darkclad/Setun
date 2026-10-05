"""Test certificates for Trinux's X.509 and TLS tests (made by Python's
cryptography, an implementation independent of lib/x509.tri)."""
import datetime, ipaddress, os
from cryptography import x509
from cryptography.x509.oid import NameOID, ExtendedKeyUsageOID, ObjectIdentifier
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa, ed25519

OUT = os.path.dirname(os.path.abspath(__file__))
os.makedirs(OUT, exist_ok=True)
now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
# fixed, so the files stay the same; the test machine's clock starts at 2026-09-21
START = datetime.datetime(2015, 1, 1, tzinfo=datetime.timezone.utc)
LONG = datetime.datetime(2046, 1, 1, tzinfo=datetime.timezone.utc)

def name(cn):
    return x509.Name([x509.NameAttribute(NameOID.ORGANIZATION_NAME, "Setun Test"), x509.NameAttribute(NameOID.COMMON_NAME, cn)])

def pem(c):
    return c.public_bytes(serialization.Encoding.PEM)

def keypem(k):
    return k.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())

def write(fn, data):
    with open(os.path.join(OUT, fn), "wb") as f:
        f.write(data)

def cert(subject, key, issuer, ikey, *, ca=False, pathlen=None, sans=None, hash_=hashes.SHA256(), nb=START, na=LONG,
         eku=(ExtendedKeyUsageOID.SERVER_AUTH,), extra=None, ku=True, bc=True):
    b = (x509.CertificateBuilder().subject_name(name(subject)).issuer_name(name(issuer))
         .public_key(key.public_key()).serial_number(x509.random_serial_number())
         .not_valid_before(nb).not_valid_after(na))
    if bc:
        b = b.add_extension(x509.BasicConstraints(ca=ca, path_length=pathlen), critical=True)
    if ca:
        b = b.add_extension(x509.KeyUsage(False, False, False, False, False, True, True, False, False), critical=True)
    elif ku:
        b = b.add_extension(x509.KeyUsage(True, False, False, False, False, False, False, False, False), critical=True)
    if sans:
        gn = [x509.IPAddress(ipaddress.ip_address(s)) if s[0].isdigit() else x509.DNSName(s) for s in sans]
        b = b.add_extension(x509.SubjectAlternativeName(gn), critical=False)
    if eku and not ca:
        b = b.add_extension(x509.ExtendedKeyUsage(list(eku)), critical=False)
    if extra:
        b = b.add_extension(extra, critical=True)
    if isinstance(ikey, ed25519.Ed25519PrivateKey):
        hash_ = None
    return b.sign(ikey, hash_)

root_rsa_k = rsa.generate_private_key(public_exponent=65537, key_size=4096)
root_rsa = cert("Setun Test Root RSA", root_rsa_k, "Setun Test Root RSA", root_rsa_k, ca=True)
root_ec_k = ec.generate_private_key(ec.SECP384R1())
root_ec = cert("Setun Test Root EC", root_ec_k, "Setun Test Root EC", root_ec_k, ca=True, hash_=hashes.SHA384())
inter_k = ec.generate_private_key(ec.SECP256R1())
inter = cert("Setun Test CA 1", inter_k, "Setun Test Root RSA", root_rsa_k, ca=True, pathlen=0)
write("roots.pem", pem(root_rsa) + pem(root_ec))

def leaf_chain(fn, leaf, key, *rest):
    write(fn + ".pem", pem(leaf) + b"".join(pem(c) for c in rest))
    if key is not None:
        write(fn + ".key", keypem(key))

WEB = ["web.test", "*.web.test", "203.0.113.8"]
web_k = ec.generate_private_key(ec.SECP256R1())
web = cert("web.test", web_k, "Setun Test CA 1", inter_k, sans=WEB)
leaf_chain("web", web, web_k, inter)
leaf_chain("missing", web, None)                          # no intermediate
# an RSA server key under the EC root (ECDSA P-384 SHA-384 over it)
rsa_k = rsa.generate_private_key(public_exponent=65537, key_size=2048)
leaf_chain("rsa", cert("rsa.test", rsa_k, "Setun Test Root EC", root_ec_k, sans=["rsa.test"], hash_=hashes.SHA384()), rsa_k)
# an Ed25519 server key, and a P-384 one
ed_k = ed25519.Ed25519PrivateKey.generate()
leaf_chain("ed", cert("ed.test", ed_k, "Setun Test Root EC", root_ec_k, sans=["ed.test"], hash_=hashes.SHA384()), ed_k)
p384_k = ec.generate_private_key(ec.SECP384R1())
leaf_chain("p384", cert("p384.test", p384_k, "Setun Test CA 1", inter_k, sans=["p384.test"], hash_=hashes.SHA384()), p384_k, inter)
# what must be refused
leaf_chain("expired", cert("web.test", web_k, "Setun Test CA 1", inter_k, sans=WEB,
                           nb=datetime.datetime(2019, 1, 1, tzinfo=datetime.timezone.utc),
                           na=datetime.datetime(2020, 1, 1, tzinfo=datetime.timezone.utc)), web_k, inter)
leaf_chain("notyet", cert("web.test", web_k, "Setun Test CA 1", inter_k, sans=WEB,
                          nb=datetime.datetime(2099, 1, 1, tzinfo=datetime.timezone.utc),
                          na=datetime.datetime(2100, 1, 1, tzinfo=datetime.timezone.utc)), web_k, inter)
self_k = ec.generate_private_key(ec.SECP256R1())
leaf_chain("self", cert("web.test", self_k, "web.test", self_k, sans=WEB), self_k)
noca_k = ec.generate_private_key(ec.SECP256R1())
noca = cert("Setun Test Not A CA", noca_k, "Setun Test CA 1", inter_k, sans=["noca.test"])
leaf_chain("noca", cert("web.test", web_k, "Setun Test Not A CA", noca_k, sans=WEB), web_k, noca, inter)
inter2_k = ec.generate_private_key(ec.SECP256R1())
inter2 = cert("Setun Test CA 2", inter2_k, "Setun Test CA 1", inter_k, ca=True)
leaf_chain("deep", cert("web.test", web_k, "Setun Test CA 2", inter2_k, sans=WEB), web_k, inter2, inter)
der = bytearray(web.public_bytes(serialization.Encoding.DER))
der[-5] ^= 1                                              # inside the signature's s
bad = x509.load_der_x509_certificate(bytes(der))
leaf_chain("badsig", bad, None, inter)
leaf_chain("crit", cert("web.test", web_k, "Setun Test CA 1", inter_k, sans=WEB,
                        extra=x509.UnrecognizedExtension(ObjectIdentifier("1.3.6.1.4.1.55555.1"), b"\x05\x00")), web_k, inter)
leaf_chain("client", cert("web.test", web_k, "Setun Test CA 1", inter_k, sans=WEB,
                          eku=(ExtendedKeyUsageOID.CLIENT_AUTH,)), web_k, inter)
# a root nobody trusts, issuing a perfectly good-looking chain
other_k = ec.generate_private_key(ec.SECP256R1())
other_root = cert("Setun Test Root RSA", other_k, "Setun Test Root RSA", other_k, ca=True)
leaf_chain("impostor", cert("web.test", web_k, "Setun Test Root RSA", other_k, sans=WEB), web_k)
print("written to", OUT, "at", now.isoformat())
