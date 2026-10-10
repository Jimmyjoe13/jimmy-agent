const config = {
  neon: { price: 'price_1UP8e1LlPGgejV8ryS4qeCdA', nom: 'Renard Néon' },
  fantome: { price: 'price_1UP8eHLlPGgejV8rlvWFof3n', nom: 'Renard Fantôme' },
  metal: { price: 'price_1UP8eILlPGgejV8rC9xYReS3', nom: 'Renard Métal' },
};

export default async function handler(req, res) {
  try {
    const url = new URL(req.url, 'https://x');
    const slug = (url.searchParams.get('skin') || '').toLowerCase();
    const item = config[slug];
    if (!item) return res.status(400).json({ ok: false, error: 'skin inconnu' });
    const stripeKey = process.env.STRIPE_SECRET_KEY;
    if (!stripeKey) return res.status(500).json({ ok: false, error: 'clé Stripe manquante côté serveur' });
    const { default: Stripe } = await import('stripe');
    const stripe = new Stripe(stripeKey);
    const host = req.headers['x-forwarded-host'] || req.headers.host || 'jimy-agent-site.vercel.app';
    const base = 'https://' + host;
    const session = await stripe.checkout.sessions.create({
      mode: 'payment',
      line_items: [{ price: item.price, quantity: 1 }],
      metadata: { skin: slug },
      success_url: base + '/merci.html?skin=' + slug + '&session_id={CHECKOUT_SESSION_ID}',
      cancel_url: base + '/?annule=1',
    });
    res.writeHead(303, { Location: session.url });
    res.end();
  } catch (e) {
    res.status(500).json({ ok: false, error: String((e && e.message) || e) });
  }
}
