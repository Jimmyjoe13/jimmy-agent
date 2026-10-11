# Outils des skins premium à modèle 3D

Hors ligne, lancés à la main dans un environnement Python isolé
(`py -3 -m venv`, puis `pip install gradio_client trimesh numpy scipy pillow`).
Les modèles produits ne vont **jamais** dans le dépôt : ils vivent dans
`<données>/premium/models/` (et seront livrés aux acheteurs après achat).

1. `trellis_image_to_3d.py <image> <sortie>` : image → `.glb` (TRELLIS.2).
2. `autorig.py <modele.glb> <modele.rig> [vues]` : squelette automatique
   (os du corps sur repères mesurés, chaînes d'os des câbles, poids lissés le
   long du maillage). `jimmy.gd` lit le `.rig` (`_attach_rig`) et anime les
   os (`_drive_rig`).
3. Déclarer le skin dans `godot/premium/skins/<id>.json` (`model`, `rig`,
   `head_center`, `eye_offset`…) et dans `providers::avatar::PREMIUM_SKINS`
   (identifiant, libellé, mot d'éveil).
