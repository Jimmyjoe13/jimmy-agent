import crypto from 'crypto';

// Prix Stripe de chaque skin vendu : une licence n'est signée que pour le
// skin réellement payé dans la session.
const PRICE_BY_SLUG = {
  neon: 'price_1UP8e1LlPGgejV8ryS4qeCdA',
  fantome: 'price_1UP8eHLlPGgejV8rlvWFof3n',
  metal: 'price_1UP8eILlPGgejV8rC9xYReS3',
};

function b64url(buf) {
  return Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

// Licence = « JIMY-<charge>.<signature> » : charge JSON en base64 URL,
// signature Ed25519 avec LICENSE_PRIVATE_KEY (PKCS8 DER en base64, variable
// Vercel). L'appli Jimy vérifie hors ligne avec la clé publique
// (agent/src/licence.rs) : même format des deux côtés.
export default async function handler(req, res) {
  try {
    const url = new URL(req.url, 'https://x');
    const slug = (url.searchParams.get('skin') || '').toLowerCase();
    const sessionId = url.searchParams.get('session') || url.searchParams.get('session_id') || '';
    if (!PRICE_BY_SLUG[slug]) return res.status(400).json({ ok: false, error: 'skin inconnu' });
    if (!sessionId) return res.status(400).json({ ok: false, error: 'session Stripe manquante' });
    const stripeKey = process.env.STRIPE_SECRET_KEY;
    const privateKey = process.env.LICENSE_PRIVATE_KEY;
    if (!stripeKey || !privateKey) return res.status(500).json({ ok: false, error: 'config serveur incomplète' });
    const { default: Stripe } = await import('stripe');
    const stripe = new Stripe(stripeKey);
    const session = await stripe.checkout.sessions.retrieve(sessionId);
    if (session.payment_status !== 'paid') return res.status(402).json({ ok: false, error: 'paiement non confirmé' });
    // Le skin demandé doit être celui payé (métadonnée, sinon lignes de la commande).
    const paid = session.metadata?.skin === slug
      || (await stripe.checkout.sessions.listLineItems(sessionId)).data.some((li) => li.price?.id === PRICE_BY_SLUG[slug]);
    if (!paid) return res.status(402).json({ ok: false, error: 'ce paiement ne correspond pas à ce skin' });
    const payload = Buffer.from(JSON.stringify({ v: 1, skin: slug, ref: sessionId.slice(-12), ts: Date.now() }));
    const key = crypto.createPrivateKey({ key: Buffer.from(privateKey, 'base64'), format: 'der', type: 'pkcs8' });
    const license = 'JIMY-' + b64url(payload) + '.' + b64url(crypto.sign(null, payload, key));
    return res.status(200).json({ ok: true, skin: slug, license });
  } catch (e) {
    return res.status(500).json({ ok: false, error: String((e && e.message) || e) });
  }
}
