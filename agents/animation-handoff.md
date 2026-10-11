# Passation : Animation Editor, Animation Graph et auto-rigger

Point d'étape pour reprendre ce travail, en particulier pour tester
**Publish** de l'auto-rigger sur de vrais meshes avec une clé Open Cloud.
Le sandbox où tout a été développé bloquait tous les hôtes Roblox. Le
Publish de l'auto-rigger a depuis été vérifié en direct (§2).

- **Branche :** `claude/busy-mccarthy-3cjctq`.
- **PR :** [chteau/rbx-native#180](https://github.com/chteau/rbx-native/pull/180), base `dev`.
- **Dernière vérification :** `./scripts/check.sh` vert (fmt, clippy
  `-D warnings`, **4474 tests**), et le test e2e GUI vert pour les deux
  classes de joints.
- **Règle du dépôt** (`CLAUDE.md`) : aucune mention d'attribution
  Claude/Anthropic dans les commits ou la PR.

## 1. Ce qui est fait

### `crates/rbx_animation` (nouveau, sans GUI)

- **Rigs et clips**
  - Joints `Motor6D` et `AnimationConstraint` (Avatar Joint Upgrade).
  - `KeyframeSequence` en lecture et en écriture, avec easing, `Animator`
    (priorités et poids) et FACS (`NumberPose` sous `Head › FaceControls`).
- **Posing et courbes**
  - IK : deux os analytique, plus FABRIK ; modes Body Part / Full Body, pins.
  - Courbes : `FloatCurve`, tangentes Hermite, conversion clip ↔ courbes.
- **Import**
  - Texte de l'addon Blender *Rbx Animations*, ce qui couvre aussi RoMixamo.
  - FBX et glTF retargetés : alias Mixamo, alignement T-pose → bras le long
    du corps.
  - `AnimationId`.
- **Animation Graph** calqué sur le plugin Studio officiel : stockage,
  valeurs par défaut, runtime. Les comportements non documentés par Roblox
  sont marqués `UNVERIFIED` dans le code.

### `crates/rbx_autorig` (nouveau, sans GUI) : l'auto-rigger

API publique, depuis `src/lib.rs` :

- **Lecture :** `read(path)` pour `.fbx` binaire, `.obj`, `.gltf` et `.glb`.
- **Fit :** `fit` / `fit_with(mesh, &FitOptions)` renvoient un `Fit`, avec
  l'orientation, l'échelle et les 15 joints en espace Roblox, en studs.
  - L'axe haut, l'avant (−Z, gauche du perso sur −X) et l'échelle sont
    détectés.
  - Les joints viennent d'une voxelisation, des extrémités géodésiques, des
    sections transverses et des proportions du Mannequin.
- **Découpe :** `split` produit `Body`, les 15 `PartMesh` capés et étanches.
  Chacun a son centre, sa taille et ses attachments `<Joint>RigAttachment`
  relatifs au centre.
- **Skin (option B) :** `weights(mesh, fit, body)` renvoie un `Skin`.
  - Bone heat, 4 influences par sommet, somme = 255.
  - Os limités à : propre partie, parent, enfants.
- **Écriture**
  - `mesh_v2` : mesh rigide.
  - `mesh_v4(mesh, Some(&PartSkin))` : mesh skinné.
  - `gltf(&Body)` : 15 nœuds `<Part>_Geo`.
  - `gltf_skinned(&Body, &Skin)` : armature `Root → HumanoidRootNode →
    LowerTorso → …`, skin unique, fichier décalé pour mettre le joint Root à
    l'origine.
- **Pour les tests d'autres crates :** feature `test-humanoid`, qui expose
  `test_humanoid::humanoid(Pose::A|T|I)`, un humanoïde synthétique aux
  joints connus.

### `crates/rbx_viewer` et `crates/rbx_mesh`

- **Isolation :** `Headless::set_isolation(Some(roots))` ne dessine que le rig,
  sur fond gris #3A3A3A avec un éclairage neutre. Le reste n'est ni dessiné
  ni pickable.
- **Lecture du skinning :** `rbx_mesh::parse_skinned` lit v4/v5 et le
  chunk `SKINNING` v6/v7.
- **Déformation :**
  - Le viewer déforme en direct un `MeshPart` skinné par les parts sœurs que
    ses os nomment, sur le CPU.
  - Ça marche aussi pour un rig inséré ou remplacé après le chargement.
  - La pose de repos est déduite des joints.

### `crates/rbx_studio` : le document Animation Editor

- **Disposition :** onglet Animation Editor, dans l'esprit de Blender.
- **Sous-onglets :** Dope Sheet, **Curve Editor** et **Animation Graph**.
- **Vue 3D**
  - Le rig seul sur une grille, avec les axes X rouge et Z bleu.
  - **Vue squelette** : os style Blender, celui du joint sélectionné en bleu,
    bouton dans la barre du haut.
- **Panneau latéral :** Rigging, **Auto-rig**, Clip, Curve key, Keyframe,
  IK, Events, Face, Import.
- **Brouillons :** le travail non sauvegardé est gardé en brouillon local
  dans `<config>/animation-drafts`.
- **Auto-rigger :** Rigging › **Auto-rig a mesh…**
  - Code : `src/shell/rig/autorig.rs`, panneau dans
    `src/shell/animation/panel/autorig.rs`.
  - Chaque partie découpée est écrite dans le cache d'assets local, sous
    `rbxasset://rbx-native-local/autorig/<hash>/<Part>.mesh`. Le rig est
    ensuite construit par le Rig Builder (`Piece.local`), ce qui permet un
    aperçu sans upload.
  - Réglages : **Turn 90°**, **Flip front**, **Smooth bends** / **Rigid
    parts**, **Mirrored**, puis position X/Y/Z du joint choisi. Chaque
    édition redécoupe et reconstruit le rig en place.
  - **Publish** écrit le `.glb` (skinné si Smooth), l'envoie par
    `import3d::upload_model` (Open Cloud), puis `swap_meshes` remplace le
    `MeshId` de chaque partie par celui de l'upload.
  - Sauvegarder une place qui contient encore des meshes locaux affiche un
    avertissement.

## 2. Ce qu'il faut tester avec la clé API

1. **Préparer**
   - Une clé configurée (Home › Manage key, ou variable d'environnement, voir
     `rbx_cloud::ApiKey::from_env_or_config`).
   - Ne **pas** définir `RBX_STUDIO_IMPORT_MOCK`.
2. **Lancer et vérifier le fit**
   - Ouvrir une place, onglet Animation Editor, puis **Auto-rig a mesh…** sur
     un vrai personnage non riggé en A-pose ou T-pose (export Blender, mesh
     Mixamo sans squelette…).
   - Les joints tombent-ils juste ? Corriger avec les champs X/Y/Z au besoin.
   - L'avant est-il bien détecté ? Sinon, Turn 90° / Flip front.
   - Avec Smooth bends, coudes et genoux plient-ils proprement ? Pour
     tester, tourner un membre avec l'outil Rotate.
3. **Publish** : vérifié en direct le 2026-10-11 (sans mock), deux uploads.
   - **Uploads.** Rigide : TIBY (chibi CC0), modèle **95056738325785**.
     Skinné (Smooth bends) : WomanBody13 (CC0), modèle **91357319063637**.
     Lancés par `RBX_STUDIO_ANIMATION="autorig=<chemin>,rigid|smooth,publish"`
     avec `RBX_STUDIO_IMPORT_KEEP=<dossier>`, qui garde le glb envoyé et le
     `.rbxm` renvoyé sous `<asset id>.*`.
   - **Unités : studs.** Pour les 15 parties des deux uploads, le
     `InitialSize` renvoyé est égal aux bornes du glTF envoyé (rapport
     1,000). Aucune correction d'échelle dans `write.rs`.
   - **Noms.** En rigide, l'importeur renvoie des `MeshPart` `<Part>_Geo`
     (sous des `Model` `<Part>_Geo_Node`). En skinné, il renvoie des `<Part>`
     tout court. `find_named` cherchait déjà les deux : 15/15 trouvées.
   - **Armature.** En skinné, l'importeur reconnaît le R15. Il crée ses
     propres `Motor6D`, ses `*RigAttachment`, un `Humanoid`, un
     `HumanoidRootPart` et un dossier `InitialPoses`, mais **aucun `Bone`**.
     Les parties ont `HasSkinnedMesh = true` et un `JointOffset` (position
     de l'os depuis le centre de la partie). `swap_meshes` ne prend que
     `MeshId`, `InitialSize`, `HasSkinnedMesh`, `HasJointOffset` et
     `JointOffset`. Nos joints restent les seuls : pas de doublons (test
     `publishing_takes_each_parts_mesh_and_skinning_but_not_the_joints`).
   - **Reste à faire à la main : en jeu.** Le personnage publié marche-t-il
     avec un `Animator` et une animation R15 standard ? Non vérifié.
4. **Mesh v4 skinné : en-tête.** On écrit `lod_type = 0` et
   `num_high_quality_lods = 1`. Les vrais meshes Roblox : le corps R15 skinné
   13674780763 (v5) a `lod_type = 3` (Zeux), 1 LOD haute qualité sur 4 ; des
   accessoires v4.01 ont `lod_type = 4`, 1 sur 6. Le nombre de LOD haute
   qualité (1) correspond. Avec un seul niveau, rien n'est simplifié :
   0 (« none ») est la valeur honnête. Publish passe de toute façon par le
   glTF, que Roblox retraite en ses propres meshes (voir `write.rs`).
5. **Vrais personnages (fit).** Testés : TIBY (chibi FBX, avec un disque
   au sol et un accessoire posé à côté), Rak (FBX), WomanBody13 (OBJ),
   Superhero Quaternius (glTF). Les quatre passent le fit (exemple
   `cargo run -p rbx_autorig --example fit_report`) ; TIBY, Rak et
   WomanBody13 se riggent et plient aussi dans rbxstudio (captures Xephyr). Corrigé pour eux : props et disques au sol écartés (avec un
   avertissement), cou des grosses têtes, entrejambe bas, plus de 5
   extrémités, hanches qui croisaient l'axe, glTF exigeant
   `KHR_materials_unlit`.

Pour tester sans clé, `RBX_STUDIO_IMPORT_MOCK=<fichier .rbxm>` remplace
l'upload par un modèle local.

## 3. Lancer, vérifier, scripter

- **Vérification complète :** `./scripts/check.sh`.
  `RBX_LUAU_LSP_ZIP=<zip>` n'était nécessaire que derrière le proxy TLS du
  sandbox.
- **Test GUI e2e** (il faut un display) :
  `RBX_STUDIO_E2E_DISPLAY=:0 cargo test -p rbx_studio --test animation_e2e -- --ignored`.
- **Runs scriptés :** `RBX_STUDIO_ANIMATION="<étapes>" rbxstudio place.rbxl`
  ouvre l'Animation Editor et joue les étapes. La liste complète est dans
  `src/shell/animation/debug.rs`. Exemple d'auto-rig puis de pliage :
  `RBX_STUDIO_ANIMATION="autorig=/chemin/perso.obj,skeleton=off,turn=LeftLowerArm:-70,turn=RightLowerLeg:60"`.
  Les étapes après `autorig=` attendent que le rig soit placé.

## 4. Conventions à respecter

- **Repère Roblox :** +Y en haut, avant vers −Z, gauche du personnage sur −X.
- **Bind des os** (auto-rigger et viewer appliquent exactement la même
  règle) :
  - L'os B est une pure translation vers `joint(B) − centre de la partie`,
    dans l'espace du mesh.
  - À l'exécution : `M_B = (part.CFrame⁻¹ · bonePart.CFrame · T(offset du
    joint)) · bind_B⁻¹`, ce qui vaut l'identité au repos.
- **Coutures :** les sommets des caps prennent les poids du mur qu'ils
  touchent. Les forcer à 100 % sur leur propre partie ouvrirait une fente
  à chaque pli.

## 5. Ce qui reste ouvert ou non vérifié

- **Auto-rigger**
  - Les marqueurs de joints se glissent dans la vue 3D (calque de lignes 5,
    plan face caméra à la profondeur du joint). Le relâcher recoupe le corps
    comme une édition du panneau : une étape d'annulation sur le DOM, mais
    l'annulation ne ramène pas le fit lui-même (comme pour le panneau).
    Pas de prévisualisation du maillage pendant le glisser, seulement au
    relâcher. Étape scriptée : `autorig-drag=<Joint>:dx/dy/dz`.
    Vu sous Xephyr : après Ctrl+Z le maillage revient mais les marqueurs
    restent où on les a lâchés (le fit n'est pas annulé). Un très grand
    glisser (genou monté jusqu'au cou) donne une découpe aberrante : jambe
    détachée. Pas de garde-fou sur la position d'un joint.
  - Attachments d'accessoires écrits (même jeu que le Mannequin, placés
    depuis le fit), non comparés à l'œil dans le vrai Studio.
  - Pas de cages de corps : une `_OuterCage` doit garder la topologie
    (sommets et UV) de la cage gabarit de Roblox pour s'apparier aux
    cages internes des vêtements en couches. Une enveloppe inventée ne
    s'apparierait pas, et la vérifier demande des envois en ligne et
    Studio. Il faudrait déformer la cage du Mannequin vers le fit.
  - Mains détachées et bras levés gérés. Un bras collé le long du flanc
    (pressé contre le torse) n'est toujours pas séparé par le fit.
  - Testé sur des humanoïdes synthétiques et quatre maillages de
    `rbx-native-fixtures/autorig`.
  - Personnages faits de boîtes qui se touchent (Kenney « blocky ») : pas
    de cou ni d'entrejambe trouvés, la découpe échoue (« LowerTorso came
    out empty »). Il faudrait rigger pièce par pièce : nouvelle
    fonctionnalité.
  - L'avant reste ambigu sur TIBY et les Kenney : l'avertissement le dit,
    Turn 90° / Flip front corrige.
  - Le genou de Rak est un peu haut (1,30 pour une hanche à 1,49 studs).
- **Rig skinné après Publish** (modèle 116095331286071, TIBY) : nos 15
  meshes publiés sont bien servis, mais plier le rig dans rbxstudio ne
  donne pas le même résultat qu'avant Publish (avant-bras qui paraissent
  raccourcis, genou moins plié). Le viewer ne lit pas `JointOffset`, et
  l'importeur réécrit le mesh à sa façon. Cause non cherchée ; à comparer
  avec un vrai client Roblox.
- **Skinning viewer**
  - Déformation sur le CPU.
  - Les masques de survol, de sélection et de surbrillance gardent la
    silhouette au repos.
  - Les vêtements en couches ne sont pas skinnés.
  - 18 des tests GPU `patch_parity` (ignorés par défaut) n'ont pas été
    lancés : trop lents sous lavapipe.
- **Vue isolée**
  - Particules et GUI attachés au rig masqués aussi.
  - Le snapping de l'outil Move peut viser des parts cachées.
  - Ombre ajustée à toute la place.
- **Non vérifié contre un vrai client Roblox**
  - Direction de l'easing des poses : côté éditeur, réglé par le code de
    l'Animation Editor (`CurveUtils`, Client Tracker 41ab074bc29d, cité dans
    `easing.rs`) : tous les styles au sens TweenService. Le `Cubic`
    inversé au runtime ne repose que sur la doc de l'enum ; la direction de
    `Constant` n'est dite nulle part.
  - Comportements non documentés du Graph, marqués `UNVERIFIED`. Réglé par
    la doc (`AnimationNodeType.yaml`) : formules scalaires Over/Add/Subtract
    et sens du Seed (-1 = aléatoire). Restent non vérifiés : poids Blend2D,
    façon de combiner les rotations, générateur et amorçage RNG, encodage
    binaire de l'attribut `Instance` `EntryState` (l'éditeur l'écrit bien
    comme `Instance`, pas comme nom ; aucun fichier réel trouvé).
- **Import par `AnimationId`** testé sur de vrais assets : walk/run/idle R15
  par défaut et trois emotes `CurveAnimation` du catalogue, en fixtures
  sous `crates/rbx_animation/tests/fixtures` (`tests/real_assets.rs`).
- **`CurveAnimation`** est lu (`curve/file.rs`), puis recuit en clip.
  `ValuesAndTimes` est décodé dans `curve/values.rs` (layout dans la doc du
  module) : 36 051 blobs de 414 emotes relus et réécrits à l'octet près. On
  sauve toujours en `KeyframeSequence` : le modèle jette les pentes auto
  stockées pour les tangentes non fixées, donc une `CurveAnimation`
  reconstruite ne serait pas identique. Pas vu de `RotationCurve`
  (quaternion) dans ces 414 emotes : refusé avec un message. Les courbes
  FACS sont lues en `faces`.

## 6. Références

- `agents/animation-research.md` : recherche Animation, Graph et import,
  avec sources.
- `agents/autorig-research.md` : spec de l'auto-rigger, avec sources.
  - Découpe R15 et proportions.
  - Règles de l'importeur skinné.
  - Formats mesh v4–v7.
  - Algorithmes.
  - Comportement d'Avatar Auto Setup.
  - Questions ouvertes (§7).
- `ROADMAP.md`, section Animation : ce qui est coché et ce qui reste.
- Captures : `docs/screenshots/animation/`.
