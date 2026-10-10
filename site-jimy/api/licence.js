import crypto from 'crypto';

const PRICE_BY_SLUG = {
  neon: 'price_1UP8e1LlPGgejV8ryS4qeCdA',
  fantome: 'price_1UP8eHLlPGgejV8rlvWFof3n',
  metal: 'price_1UP8eILlPGgejV8rC9xYReS3',
};

function b64url(buf) {
  return Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export default async function handler(req, res) {
  try {
    const url = new URL(req.url, 'https://x');
    const slug = (url.searchParams.get('skin') || '').toLowerCase();
    const sessionId = url.searchParams.get('session') || url.searchParams.get('session_id') || '';
    if (!PRICE_BY_SLUG[slug]) return res.status(400).json({ ok: false, error: 'skin inconnu' });
    if (!sessionId) return res.status(400).json({ ok: false, error: 'session Stripe manquante' });
    const stripeKey = process.env.STRIPE_SECRET_KEY;
    const secret = process.env.LICENSE_SECRET || stripeKey;
    if (!stripeKey || !secret) return res.status(500).json({ ok: false, error: 'config serveur incomplète' });
    const { default: Stripe } = await import('stripe');
    const stripe = new Stripe(stripeKey);
    const session = await stripe.checkout.sessions.retrieve(sessionId);
    if (session.payment_status !== 'paid') return res.status(402).json({ ok: false, error: 'paiement non confirmé' });
    const paidPrice = session.metadata?.skin === slug ? true : (await stripe.checkout.sessions.listLineItems(sessionId)).data.some((li) => li.price?.id === PRICE_BY_SLUG[slug]);
    if (!paidPrice) return res.status(402).json({ ok: false, error: 'ce paiement ne correspond pas à ce skin' });
    const payload = b64url(JSON.stringify({ skin: slug, sid: sessionId.slice(-12), ts: Date.now() }));
    const sig = b64url(crypto.createHmac('sha256', secret).update('jimy-skin:' + payload).digest()).slice(0, 22);
    const license = 'JIMY1-' + slug.toUpperCase() + '-' + payload.slice(-8).toUpperCase().replace(/[^A-Z0-9]/g, 'X') + '-' + sig.toUpperCase().replace(/[^A-Z0-9]/g, 'X');
    // On embarque le payload dans la réponse pour la vérif en ligne ; la clé affichée reste courte.
    return res.status(200).json({ ok: true, skin: slug, license, payload, sig });
  } catch (e) {
    return res.status(500).json({ ok: false, error: String((e && e.message) || e) });
  }
}
