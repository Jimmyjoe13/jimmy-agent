// jimy-agent.me : mesure GA4 des conversions et du parcours utilisateur.
// Conversions & micro-conversions suivies :
//  - download_installer : clic sur un lien marqué data-track="download" (installeur Windows) ;
//  - purchase : achat d'un skin premium, envoyé par merci.html une fois la licence délivrée ;
//  - skin_preview : clic sur un skin de la démo ou de la collection (data-show ou data-skin) ;
//  - faq_expand : ouverture d'une question dans l'accordéon FAQ ;
//  - github_click : clic sortant vers le dépôt open source GitHub ;
//  - defender_guide_view : clic vers le guide d'autorisation Windows Defender (#defender).
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

  // Clic sur un sélecteur de skin (démo hero ou collection).
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('[data-skin], [data-show]') : null;
    if (!btn) return;
    var skin = btn.getAttribute('data-skin') || btn.getAttribute('data-show');
    if (!skin) return;
    window.gtag('event', 'skin_preview', {
      skin_name: skin,
      section: btn.closest('.picker') ? 'hero_demo' : 'collection',
      page_location: location.pathname
    });
  }, true);

  // Clic sortant vers le dépôt GitHub.
  document.addEventListener('click', function (e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href*="github.com/Jimmyjoe13/jimmy-agent"]') : null;
    if (!a) return;
    window.gtag('event', 'github_click', {
      link_url: a.href,
      link_text: (a.textContent || '').trim().substring(0, 50),
      page_location: location.pathname
    });
  }, true);

  // Clic vers le guide Defender.
  document.addEventListener('click', function (e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href="#defender"], a[href$="#defender"]') : null;
    if (!a) return;
    window.gtag('event', 'defender_guide_view', { page_location: location.pathname });
  }, true);

  // Ouverture des questions de la FAQ.
  document.addEventListener('toggle', function (e) {
    var details = e.target;
    if (!details || details.tagName !== 'DETAILS' || !details.open) return;
    var summary = details.querySelector('summary');
    var question = summary ? summary.textContent.trim() : 'Question inconnue';
    window.gtag('event', 'faq_expand', {
      question: question.substring(0, 100),
      page_location: location.pathname
    });
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
