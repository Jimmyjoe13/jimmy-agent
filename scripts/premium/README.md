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
3. Déclarer le skin dans `<données>/premium/skins/<id>.json` (`model`,
   `rig`, `head_center`, `eye_offset`…), versionné dans le dépôt privé
   `jimy-premium` (jamais ici), et dans `providers::avatar::PREMIUM_SKINS`
   (identifiant, libellé, mot d'éveil).

Autre voie, pour une mécanique (skin ARK) : un modèle **généré par code**
avec son squelette glTF natif (pièces rigides, une rotule par articulation,
pas de `.rig`). Le générateur vit dans le dépôt privé
(`jimy-premium/tools/build_ark.py`) ; `jimmy.gd` anime les os `tent_*` et
`claw_*` (`_setup_tentacles`, `_drive_tentacles`).
