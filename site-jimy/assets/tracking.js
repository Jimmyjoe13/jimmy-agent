// jimy-agent.me : mesure GA4 des conversions.
// Conversions suivies :
//  - download_installer : clic sur un lien marqué data-track="download" (installeur Windows) ;
//  - purchase : achat d'un skin premium, envoyé par merci.html une fois la licence délivrée.
// Inactif tant que window.JIMY_GA4_ID est vide (renseigné dans le <head> de chaque page).
(function () {
  var id = window.JIMY_GA4_ID;
  window.dataLayer = window.dataLayer || [];
  window.gtag = window.gtag || function () { window.dataLayer.push(arguments); };
  if (!id) return;

  var s = document.createElement('script');
  s.async = true;
  s.src = 'https://www.googletagmanager.com/gtag/js?id=' + encodeURIComponent(id);
  document.head.appendChild(s);
  window.gtag('js', new Date());
  window.gtag('config', id);

  // Téléchargement de l'installeur.
  document.addEventListener('click', function (e) {
    var a = e.target && e.target.closest ? e.target.closest('a[data-track="download"]') : null;
    if (!a) return;
    window.gtag('event', 'download_installer', { link_url: a.href, page_location: location.pathname });
  }, true);

  // Achat confirmé. Dédoublonné par session Stripe pour ne pas compter deux fois un rechargement.
  var PRICES = { neon: 4.9, fantome: 6.9, metal: 9.9 };
  window.jimyTrackPurchase = function (slug, sessionId) {
    if (!sessionId) return;
    var key = 'jimy_purchase_' + sessionId;
    try {
      if (sessionStorage.getItem(key)) return;
      sessionStorage.setItem(key, '1');
    } catch (e) { /* stockage indisponible : on envoie quand même */ }
    var price = PRICES[slug] || 0;
    window.gtag('event', 'purchase', {
      transaction_id: sessionId,
      value: price,
      currency: 'EUR',
      items: [{ item_id: slug, item_name: 'Skin ' + slug, price: price, quantity: 1 }]
    });
  };
})();
