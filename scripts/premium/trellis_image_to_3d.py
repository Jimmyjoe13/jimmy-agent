# -*- coding: utf-8 -*-
"""Génère un modèle 3D (.glb) à partir d'une image, via la page Hugging Face
TRELLIS.2 (Microsoft, licence MIT : modèles revendables sans mention).

Token facultatif mais conseillé (quota du compte gratuit) : variable
HF_TOKEN, lue depuis %USERPROFILE%\.secrets\huggingface.json, jamais
écrite ni affichée. Image : personnage seul, de face, cadré serré.
Usage : python trellis_image_to_3d.py <image> <dossier_sortie>
Sortie : <dossier_sortie>/jeva-trellis.glb (à renommer) + image préparée."""
import os, shutil, sys, time
from gradio_client import Client, handle_file

image, out_dir = sys.argv[1], sys.argv[2]
token = os.environ.get("HF_TOKEN") or None  # facultatif : quota du compte
t0 = time.time()
client = Client("microsoft/TRELLIS.2", token=token, verbose=False)
client.predict(api_name="/start_session")
print("session ouverte", flush=True)
prep = client.predict(handle_file(image), api_name="/preprocess_image")
print("image préparée :", prep, flush=True)
client.predict(handle_file(prep), 42, "1024", api_name="/image_to_3d")
print(f"modèle généré en {time.time()-t0:.0f} s", flush=True)
glb, download = client.predict(120000, 2048, api_name="/extract_glb")
src = download or glb
dst = os.path.join(out_dir, "jeva-trellis.glb")
shutil.copy(src, dst)
shutil.copy(prep, os.path.join(out_dir, "jeva-preprocessed.png"))
print(f"GLB : {dst} ({os.path.getsize(dst)/1e6:.1f} Mo) en {time.time()-t0:.0f} s", flush=True)
