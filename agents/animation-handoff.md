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
    ses os nomment.
  - Sur le GPU, par une passe compute (`renderer/skinning.rs` et
    `skinning.wgsl`) : les buffers de repos restent fixes, seules les
    matrices des os sont envoyées à chaque pose. Les masques de survol, de
    sélection et de surbrillance et les ombres passent par la même passe,
    donc suivent la forme déformée.
  - WebGL n'a pas de compute : le viewer WASM, et la fenêtre `rbxview`
    seule, gardent la déformation sur le CPU, masques compris.
    `Headless::set_skins_on_gpu(false)` force aussi ce chemin.
  - Le picking lit toujours une copie posée sur le CPU.
  - Les vêtements en couches enveloppés (`wrap.rs`) sont skinnés aux parts
    du personnage : par leur propre skin si ses os nomment ces parts, sinon
    par un skin prêté par le corps (les parts des sommets de cage les plus
    proches).
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
  - Chaque asset du corps est simplifié (QEM, `decimate.rs`) jusqu'à son
    budget de triangles Marketplace (`split::fit_budgets`, `BUDGETS`). Les
    coutures de découpe, les UV et les poids de skin sont gardés. Un asset
    que ses coutures laissent au-dessus du budget est signalé par un
    avertissement.
  - Le `.glb` d'upload est écrit dans le cache au moment de la découpe
    (`autorig/<hash>/upload.glb`). **Publish** part donc du modèle seul :
    il apparaît dans la section Rigging après **Done**, pour le rig lié ou
    sélectionné qui a encore des meshes locaux (`publishable_rig`). Un rig
    découpé avant ce changement n'a pas d'`upload.glb` : il faut le
    ré-auto-rigger.
  - Supprimer le `Model` du rig (touche Suppr, Explorer) termine la session
    d'auto-rig, le bind et l'isolation (`forget_deleted_rig`). Le viewer
    oublie aussi l'entrée du plan de meshes (`remove_part`) ; sans cela, le
    corps était redessiné au repos.
  - La section Auto-rig passe avant Rigging, avec Done et Publish juste
    sous le statut, pour être visibles sans défiler.
  - Étapes scriptées ajoutées : `done`, `delete-rig` et
    `document=viewport|animation|ui`. Entrer dans Animation depuis l'UI
    Editor remet l'écran émulé à zéro (`set_screen(None)`), pour que la vue
    3D occupe toute la place jusqu'au panneau.
  - Sauvegarder une place qui contient encore des meshes locaux affiche un
    avertissement.

- **Publier une animation** (`src/shell/animation/publish.rs`, branche
  `pr180/anim-publish`) :
  - Barre du haut du document, à droite du squelette : **Import ▾** (depuis
    un ID d'asset, depuis un fichier FBX/glTF/.rbxm, ou une animation déjà
    sauvée sur le rig), **Export ▾** (fichier `.rbxm`, Save to rig) et
    **Publish** en bouton principal. Chaque entrée a une infobulle. Le
    panneau ne garde que New, et Copy/Paste Blender dans la section Import.
  - Publish ouvre un formulaire : nom, description, créateur (le compte ou
    un groupe du cache Home, `launcher::cached_groups`). L'upload passe par
    `rbx_cloud::Client::create_animation_asset` (assetType `Animation`, un
    `.rbxm` contenant un seul `KeyframeSequence`). Un second Publish est
    refusé tant que le premier tourne ; le formulaire affiche
    « Publishing… ».
  - Résultat dans le dialogue de `rig/publish_result.rs` (variantes
    `AnimationPublished` / `AnimationFailed`) : `rbxassetid://<id>`, Copy
    ID, Open on Roblox, et le rappel « AnimationId puis
    Animator:LoadAnimation ». Avertissement partiel si des joints du clip
    ne sont pas sur le rig lié (écrits à plat, ils ne joueront pas).
  - Mocks : `RBX_STUDIO_ANIM_PUBLISH_MOCK_ID=<id>` et
    `RBX_STUDIO_ANIM_PUBLISH_MOCK_ERROR=<message>` (3 s d'attente, ni réseau
    ni clé). Étapes scriptées : `menu=import|export`, `import-id-dialog`,
    `publish-form`, `publish-group[=<id>]`, `publish-go`,
    `export-rbxm=<chemin>`.
- **Confirmations en toast** (`src/shell/animation/toast.rs`) : la ligne de
  statut en bas du panneau est remplacée par un bandeau en haut de la vue
  3D, style lecture de vitesse. Info 4 s, avertissement 7 s, erreur
  jusqu'à fermeture (croix).
- **Séparation redimensionnable et barres de défilement**
  (`src/shell/animation/split.rs`, `seams.rs`, `src/timeline/scroll.rs`).
  - Une poignée sur la jointure entre la vue 3D et le panneau du bas
    (Dope Sheet / Curve Editor) redimensionne le panneau, avec le même
    `chrome::seam_handle` que les docks. Minimums : 120 px pour la vue,
    110 px pour le panneau. La hauteur par défaut est l'ancienne part (46 %,
    bornée à 170–380 px).
  - Une poignée sur le bord droit de la colonne des joints la
    redimensionne (120–480 px, en laissant au moins 160 px aux clés).
  - Les deux tailles sont gardées pour la session et écrites dans les
    réglages au relâcher : `anim_sheet_height` (absente = la part par
    défaut) et `anim_names_width`. Les valeurs absurdes sont refusées.
  - Une barre horizontale sous les rangées de clés : le pouce montre la
    plage visible sur la plage défilable. Le glisser déplace la vue ; un
    clic sur la piste pagine de 90 % de la vue. La règle, les rangées et la
    tête de lecture restent alignées.
  - Plage choisie : d'un peu avant 0 (la marge de `fit`) jusqu'à End plus
    la moitié du clip, au moins 2 s après End. Le pan et la molette sont
    bornés à cette plage, et le dézoom s'arrête quand elle tient en
    entier.
  - La barre horizontale est dessinée à la main dans le style du kit : le
    `Scrollbar` du kit saute au clic sur la piste au lieu de paginer, et
    veut un handle en pixels.
  - La liste des joints a une barre verticale (`chrome::ScrollbarY`) quand
    elle déborde.
  - Les gestes (poignées, pouce) passent par les handlers racine de
    `shell.rs`, car le `on_mouse_move` d'un div ne part que sous le
    pointeur.

## 2. Ce qu'il faut tester avec la clé API

0. **Publish d'une animation : NON VÉRIFIÉ en direct.** La doc Open Cloud
   accepte `Animation` en `.rbxm` (`model/x-rbxm`) mais prévient qu'un
   fichier écrit hors Studio « peut ne pas s'uploader ou fonctionner ».
   À faire : lier un rig R15, `demo`, Publish (compte puis groupe), puis
   dans Studio mettre l'ID dans `Animation.AnimationId` et jouer
   l'animation via `Animator:LoadAnimation`. Si l'upload est refusé, le
   message brut est dans le dialogue d'échec et dans Output.

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
  `RBX_STUDIO_ANIMATION="autorig=/chemin/perso.obj,skeleton=off,turn=LeftLowerArm:-70:z,turn=RightLowerLeg:60"`.
  Les étapes après `autorig=` attendent que le rig soit placé. Un bras
  auto-riggé en T-pose est couché le long de X : `turn` autour de X (le
  défaut) ne fait que le vriller, il faut `:z` pour le plier. Le brouillon
  d'animation (`<config>/animation-drafts`) est relu au lancement suivant et
  ses clés s'additionnent d'un run à l'autre : lancer avec un
  `XDG_CONFIG_HOME` vide pour partir de zéro.

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
- **Rig skinné après Publish** (TIBY 116095331286071, Woman
  91357319063637) : **cause trouvée, corrigée dans `pr180/followups`.**
  - L'importeur Open Cloud renvoie le modèle tourné d'un demi-tour autour
    de Y (bras droit vers −X), en rigide comme en skinné. Le mesh servi
    130732292096244 (avant-bras droit de Woman) est notre découpe avec x
    et z négés : mêmes 168 sommets (pas dans le même ordre), mêmes poids.
  - `JointOffset` = position de l'os de la partie (son propre joint, donc
    sa `*RigAttachment`) depuis le centre de la partie. Les `Motor6D` de
    l'importeur ont des rotations identité.
  - Le skinning du viewer est un mélange rigide des mouvements des
    parties : un mesh retourné dans une partie non tournée plie à
    l'envers (avant-bras « raccourcis », genou moins plié).
  - Correction : `swap_meshes` compare le sens du rig importé au nôtre
    (`served_turn`) et tourne la partie d'un demi-tour sur son centre
    (`turn_part`) : `CFrame`, `PivotOffset`, attachments/bones enfants et
    `C0`/`C1` des joints qui la touchent. Les pivots des joints ne bougent
    pas. Tests `a_served_mesh_turned_half_round_turns_its_part_but_no_joint`
    et `the_served_forearm_is_the_cut_turned_half_round` (fixtures réelles
    `tests/fixtures/autorig/woman-right-lower-arm-{cut,served}.mesh`, CC0).
  - Côté viewer, les repos des parties étaient figés à la première
    lecture : `Poses::rejoint` les relit quand un `Motor6D`/`Motor`
    change (C0/C1), et ne rebinde que si un repos a bougé. Test
    `a_part_turned_with_its_joint_rests_where_it_now_stands`.
  - **Insuffisant en jeu (Studio, 2026-10-11)** : le re-publish
    89657914261022 (avec ce correctif), mis en `StarterCharacter`, marche
    à reculons, et un accessoire de dos (`BodyBackAttachment`) se pose sur
    le devant du mesh. Le corps skinné reste à l'envers malgré la partie
    tournée.
  - **Nouveau correctif** : le glTF est écrit face à +Z (convention glTF,
    `write::file_space` : x et z négés sur positions, normales,
    translations des nœuds et inverse binds), donc le demi-tour de
    l'importeur le remet face à −Z, comme notre rig. `served_turn` ne
    devrait plus rien tourner (gardé en filet). **À vérifier** : un
    nouveau Publish de WomanBody13 en skinné, puis `StarterCharacter`
    dans Studio (marche en avant, accessoire de dos dans le dos). Les
    uploads précédents restent à l'envers.
- **Garde des joints et undo** (`pr180/followups`)
  - `rbx_autorig::guard` : un joint glissé reste entre son parent et son
    enfant (10 % de marge, écart latéral ≤ moitié du segment), dans la
    boîte du mesh ; épaule/hanche de leur côté ; racine sous la taille et
    au-dessus des genoux. Avertissement dans Output quand il bloque. Une
    découpe qui échoue quand même remet le fit du dernier rig qui a coupé.
  - Undo/redo d'un glissé ou d'une saisie X/Y/Z : `autorig_follow_history`
    suit le modèle que l'historique remet et restaure son fit (joints,
    marqueurs, panneau).
  - Étape de débogage `select=<Part>` dans `RBX_STUDIO_ANIMATION`.
- **Skinning viewer**
  - WebGL/WASM et la fenêtre autonome `rbxview` déforment encore sur le
    CPU (pas de compute shader en WebGL).
  - Le skinning des vêtements en couches (peau propre ou prêtée par le
    corps) n'est couvert que par des tests unitaires : pas comparé à un
    vrai client ni à Studio.
  - `patch_parity` complet (ignorés compris) : 23 réussis, 1 échec déjà
    présent avant ce travail (`a_viewport_frame_drawn_alone_follows_its_camera`,
    « no ViewportFrame … is laid out »).
- **Vue isolée**
  - Fait : les particules, beams, trails, BillboardGui et SurfaceGui du rig
    isolé restent dessinés (`scene/isolation.rs`, ensemble `effects`).
    Un émetteur posé sur une `MeshPart` survit aussi à la pose : il est
    planifié depuis `Scene::emitter_placements`, qui garde les parts dont
    le mesh a remplacé la boîte (avant, chaque frame de pose le perdait).
  - Fait : le snapping de Move ne vise que les parts du rig mis en scène
    (`shell/drag.rs`, `sync_snap_neighbours`) ; picking et survol passaient
    déjà par la scène, qui ignore les parts cachées.
  - Fait : la shadow map est ajustée aux bornes du rig plus le sol de la
    scène (`STAGE_REACH`, `Renderer::set_stage`). Mesuré sous Xephyr : bord
    d'ombre de 1 à 2 px (net, léger crénelage) contre 11 à 22 px avant.
  - Fait : la lecture écrit le `Transform` des `Motor6D` et des
    `AnimationConstraint`, jamais le `CFrame` des parts. Les parts restent
    au repos dans le document ; `rbx_viewer::JointSolver` les pose dans le
    miroir du thread de rendu. Les `Transform` d'origine sont remis en
    sortie de document : une sauvegarde après lecture est identique octet
    pour octet (test `a_save_after_playback_matches_the_save_before`).
    Sélection, poignées et picking lisent la pose via
    `Shell::pose_in`/`pose_out` (échange non suivi, de courte durée).
    Le skinning n'a pas été touché : il lit les `CFrame` posés du miroir.
    La copie de récupération automatique est dé-posée avant écriture
    (`Shell::unpose_copy`).
  - Ouvert : l'outil Align ne lit pas la pose ; le panneau Properties
    montre le `CFrame` de repos.
- **Easing des poses, vérifié dans Roblox Studio (2026-10-11)** : côté
  éditeur, tous les styles au sens TweenService (`CurveUtils`, cité dans
  `easing.rs`). Au runtime, un `KeyframeSequence` 0→90° joué par un
  `Animator` (Run, `KeyframeSequenceProvider`) donne à 0,25 s / 0,75 s :
  Cubic In 52,0/88,6, Cubic Out 1,4/38,0, Constant In 90/90, Constant Out
  0/0, Constant InOut 0/90, exactement ce que donne `ease` (Cubic inversé).
- **Gizmos de la vue Animation** (branche `pr180/anim-gizmos`)
  - Move pose une clé sur la translation du joint (le décalage du
    `Transform`, `pose::drag_key`). Rotate garde la translation et tourne
    autour du pivot du joint (`C1`, via `sync_joint_pivot`).
  - Scale, Transform et Edit Pivot sont refusés pendant la pose
    (`Transform::refusal`) : bouton grisé, curseur interdit, infobulle
    « Scale can't be keyed on joints ». La vue reçoit Select à la place
    (`for_view`), donc aucune poignée ne peut redimensionner une pièce.
    Rien ne change dans Workspace (vérifié sous Xephyr).
  - Un clic simple sur le rig sélectionne la pièce sous le curseur
    (`joint_part`), pas le `Model`. Le `HumanoidRootPart` est sauté.
  - `pose_out` ne remet plus le `CFrame` d'avant si le glisser l'a
    changé. Avant, un glisser sur un membre déjà posé était perdu : le
    gizmo bougeait, le bras non.
  - Les bras de gizmo font au moins 80 px (`gizmo::handle_length`), donc
    la bande de prise d'un anneau fait au moins 7 px. La poignée survolée
    est dessinée plus claire (`Gizmo::hover`). Pendant la pose, un clic
    jusqu'à deux bandes hors d'une poignée est avalé (`Handles::near`) :
    il ne désélectionne plus.
  - Reste ouvert :
    - Vu sous Xephyr, une forme plate lavande apparaît près du bras après
      plusieurs glisser. Cause inconnue.
    - Le pivot de rotation sur l'épaule n'est jugé qu'à l'œil, sur un
      petit rig.
- **Non vérifié contre un vrai client Roblox**
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
- **Séparation et barres de défilement de la timeline** : vérifiées sous
  Xephyr (glisser la séparation, la colonne des joints, le pouce au-delà
  de End, pagination, réglages relus au redémarrage). Restent ouverts :
  - la liste des canaux du Curve Editor n'a pas encore de barre
    verticale ;
  - Ctrl+molette (zoom) fait aussi défiler la liste des joints quand elle
    déborde, car le défilement du corps passe avant le handler de la
    feuille. C'était déjà le cas avant.

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
