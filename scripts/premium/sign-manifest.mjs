// Signe le manifeste des contenus premium (agent/src/premium.rs).
//
// Calcule l'empreinte SHA-256 de chaque fichier de skins/ et models/, écrit
// manifest.json puis manifest.sig (signature Ed25519, base64 URL, des octets
// exacts du manifeste) avec la clé privée des licences, lue dans
// %USERPROFILE%\.secrets\jimy-licence.json (jamais affichée).
//
// Usage : node scripts/premium/sign-manifest.mjs <dossier_premium>
//   (dossier du dépôt privé jimy-premium, puis le copier dans data/premium)
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2];
if (!dir) {
  console.error("usage : node sign-manifest.mjs <dossier_premium>");
  process.exit(1);
}

const files = {};
for (const sub of ["skins", "models"]) {
  const folder = path.join(dir, sub);
  if (!fs.existsSync(folder)) continue;
  for (const name of fs.readdirSync(folder).sort()) {
    const full = path.join(folder, name);
    if (!fs.statSync(full).isFile()) continue;
    files[`${sub}/${name}`] = crypto.createHash("sha256").update(fs.readFileSync(full)).digest("hex");
  }
}
const manifest = Buffer.from(JSON.stringify({ v: 1, files }, null, 2) + "\n", "utf8");

const secret = JSON.parse(fs.readFileSync(path.join(process.env.USERPROFILE, ".secrets", "jimy-licence.json"), "utf8"));
const key = crypto.createPrivateKey({ key: Buffer.from(secret.private_pkcs8_b64, "base64"), format: "der", type: "pkcs8" });
const sig = crypto.sign(null, manifest, key).toString("base64url");

fs.writeFileSync(path.join(dir, "manifest.json"), manifest);
fs.writeFileSync(path.join(dir, "manifest.sig"), sig + "\n");
console.log(`manifeste signé : ${Object.keys(files).length} fichier(s)`);
for (const [name, hash] of Object.entries(files)) console.log(`  ${name}  ${hash.slice(0, 16)}…`);
