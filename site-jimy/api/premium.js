import crypto from 'crypto';

// Livraison des contenus premium aux acheteurs.
//
// L'appli Jimy appelle GET /api/premium?file=<chemin> avec sa licence dans
// l'en-tête X-Jimy-Licence. On vérifie la licence (même clé publique que
// agent/src/licence.rs), on vérifie que le fichier appartient bien à un skin
// qu'elle débloque, puis on redirige vers un lien GitHub temporaire du dépôt
// privé jimy-premium, valable pour ce seul fichier. Redirection plutôt que
// relais : Vercel limite une réponse à 4,5 Mo et un modèle 3D en fait plus.
//
// Variable Vercel : GITHUB_PREMIUM_TOKEN (jeton GitHub en lecture seule,
// limité au dépôt jimy-premium).

const PUBLIC_KEY_B64 = 'i83kv9ubcdp4tjL3mZClpvt5qpbwS9dssF0prxk48Do=';
const REPO = 'Jimmyjoe13/jimy-premium';
// Seuls chemins servis : manifeste signé, palettes, modèles.
const FILE_RE = /^(manifest\.(json|sig)|skins\/[a-z0-9_-]+\.json|models\/[a-z0-9_-]+\.(glb|rig))$/;

// Clé publique Ed25519 brute (32 octets) → objet clé Node (enveloppe SPKI).
const publicKey = crypto.createPublicKey({
  key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), Buffer.from(PUBLIC_KEY_B64, 'base64')]),
  format: 'der',
  type: 'spki',
});

// Renvoie le skin débloqué (« * » = tous) ou null si la licence est fausse.
function verifyLicence(key) {
  const body = String(key || '').trim();
  if (!body.startsWith('JIMY-')) return null;
  const [payloadB64, sigB64] = body.slice(5).split('.');
  if (!payloadB64 || !sigB64) return null;
  try {
    const payload = Buffer.from(payloadB64, 'base64url');
    if (!crypto.verify(null, payload, publicKey, Buffer.from(sigB64, 'base64url'))) return null;
    const json = JSON.parse(payload.toString('utf8'));
    if (json.v !== 1 || typeof json.skin !== 'string' || !json.skin.trim()) return null;
    return json.skin.trim();
  } catch {
    return null;
  }
}

// Métadonnées GitHub d'un fichier du dépôt privé (download_url temporaire).
async function github(path, token, raw = false) {
  const response = await fetch(`https://api.github.com/repos/${REPO}/contents/${path}`, {
    headers: {
      Authorization: 'Bearer ' + token,
      Accept: raw ? 'application/vnd.github.raw+json' : 'application/vnd.github.object+json',
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'jimy-agent-site',
    },
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error('GitHub HTTP ' + response.status);
  return raw ? response.text() : response.json();
}

// Le fichier fait-il partie d'un skin que la licence débloque ?
async function allowed(file, skin, token) {
  if (file.startsWith('manifest.')) return true;
  if (skin === '*') return true;
  if (file.startsWith('skins/')) return file === `skins/${skin}.json`;
  // Modèle : seulement s'il est cité (model / rig) par la palette du skin.
  const palette = await github(`skins/${skin}.json`, token, true);
  if (!palette) return false;
  const json = JSON.parse(palette);
  const name = file.slice('models/'.length);
  return json.model === name || json.rig === name;
}

export default async function handler(req, res) {
  try {
    const url = new URL(req.url, 'https://x');
    const file = url.searchParams.get('file') || '';
    if (!FILE_RE.test(file)) return res.status(400).json({ ok: false, error: 'fichier inconnu' });
    const skin = verifyLicence(req.headers['x-jimy-licence']);
    if (!skin) return res.status(403).json({ ok: false, error: 'licence invalide' });
    const token = process.env.GITHUB_PREMIUM_TOKEN;
    if (!token) return res.status(500).json({ ok: false, error: 'config serveur incomplète' });
    if (!(await allowed(file, skin, token))) {
      return res.status(403).json({ ok: false, error: 'fichier non couvert par cette licence' });
    }
    const meta = await github(file, token);
    if (!meta || !meta.download_url) return res.status(404).json({ ok: false, error: 'fichier absent' });
    // Journal Vercel : quel skin est téléchargé (jamais la licence).
    console.log(`[premium] ${file} (licence ${skin})`);
    res.setHeader('Cache-Control', 'no-store');
    res.writeHead(302, { Location: meta.download_url });
    res.end();
  } catch (e) {
    res.status(500).json({ ok: false, error: String((e && e.message) || e) });
  }
}
