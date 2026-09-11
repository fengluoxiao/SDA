"use strict";
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
// DER encodes the X.509 envelope only. Key generation, ECDSA signing,
// peer fingerprinting and all transport cryptography use the platform library.
function der(tag, body) {
  body = Buffer.isBuffer(body) ? body : Buffer.concat(body);
  let length;
  if (body.length < 128) length = Buffer.from([body.length]);
  else { const bytes = []; let n = body.length; while (n) { bytes.unshift(n & 255); n >>>= 8; } length = Buffer.from([0x80 | bytes.length, ...bytes]); }
  return Buffer.concat([Buffer.from([tag]), length, body]);
}
const sequence = parts => der(0x30, parts);
const oid = hex => der(0x06, Buffer.from(hex, "hex"));
const utc = date => der(0x17, Buffer.from(date.toISOString().slice(2, 19).replace(/[-:T]/g, "") + "Z"));
function createRemoteCertificate() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  const serial = crypto.randomBytes(16); serial[0] &= 0x7f; serial[0] |= 1;
  const algorithm = sequence([oid("2a8648ce3d040302")]); // ecdsa-with-SHA256
  const name = sequence([der(0x31, [sequence([oid("550403"), der(0x0c, Buffer.from("SDA Remote"))])])]);
  const now = Date.now();
  const tbs = sequence([
    der(0xa0, [der(0x02, Buffer.from([2]))]), der(0x02, serial), algorithm, name,
    sequence([utc(new Date(now - 60000)), utc(new Date(now + 365 * 86400000))]), name,
    publicKey.export({ format: "der", type: "spki" }),
  ]);
  const raw = sequence([tbs, algorithm, der(0x03, Buffer.concat([Buffer.from([0]), crypto.sign("sha256", tbs, privateKey)]))]);
  const certificate = new crypto.X509Certificate(raw);
  if (!certificate.verify(publicKey)) throw Error("无法生成远程证书");
  return {
    cert: certificate.toString(), key: privateKey.export({ format: "pem", type: "pkcs8" }),
    fingerprint: crypto.createHash("sha256").update(raw).digest("hex"),
  };
}
function loadRemoteCertificate(file) {
  try {
    const saved = JSON.parse(fs.readFileSync(file, "utf8"));
    const certificate = new crypto.X509Certificate(saved.cert);
    const privateKey = crypto.createPrivateKey(saved.key);
    if (Date.parse(certificate.validTo) < Date.now() + 7 * 86400000 || !certificate.checkPrivateKey(privateKey)) throw Error("Renew certificate");
    return {cert:saved.cert, key:saved.key, fingerprint:crypto.createHash("sha256").update(certificate.raw).digest("hex")};
  } catch {
    const certificate = createRemoteCertificate();
    fs.mkdirSync(path.dirname(file), {recursive:true});
    const temp = `${file}.${crypto.randomUUID()}.tmp`;
    fs.writeFileSync(temp, JSON.stringify(certificate), {mode:0o600}); fs.renameSync(temp, file);
    return certificate;
  }
}
module.exports = { createRemoteCertificate, loadRemoteCertificate };
