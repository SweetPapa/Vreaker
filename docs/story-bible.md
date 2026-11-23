# STORY_BIBLE.md — VELOCITY // BREAKER

## 1. Premise & Stakes
**The Year:** 2085. Humanity's infrastructure is run by vast autonomous industrial networks, corporate AI syndicates, and relentless automation—a gleaming, cold world unrecognizable from the 2020s, let alone the 1700s.

**The Incident:** A ruthless 2085 syndicate (**The Directorate**) gathered all **Five Aspect Diamonds**—ancient, reality-bending energy cores—to run a grand synchronization ritual for absolute control over time, matter, and energy. The ritual catastrophically backfired. Four diamonds rocketed out across the Directorate's automated mega-sectors. The fifth—the **Rift Diamond**—tore open a temporal breach into the 1700s, yanking **Levy** out of his life and fusing directly into his chest and sword-arm.

**The Goal:** Levy doesn't care about saving 2085, toppling corporations, or understanding "neural networks." He has a wife and kids waiting for him three hundred years in the past. To tear open a stable way home, he has to rip the other **4 Diamonds** out of the machines and warlords hoarding them.

---

## 2. Protagonist: LEVY
* **Origin:** 1700s frontiersman / former cavalryman & blacksmith's son. Stubborn, practical, protective, zero patience for futuristic jargon.
* **Voice:** Grounded, plain-spoken, dryly bewildered by 2085 tech. Talks to towering killer mechs the way a fed-up dad talks to a broken plow.
* **Moveset-to-Lore Mapping:**
  * **Steel Saber / Grounded Normals:** Pure 1700s muscle memory and grit.
  * **High-Speed Dash, Buster, Aegis Counter, Hurricane Spin, Charged Shot:** Raw **Rift Diamond** energy venting through his left gauntlet and blade.

### Visual Design Spec (For Local Image Model → Blender Baker)
* **Silhouette Priority (`96×96` pixel readability):** Strong warm-cloth-vs-cold-chrome contrast against 2085 metal stages.
* **Outfit:** Weathered navy/charcoal 1700s long coat (split hem so legs read clearly in run/jump cycles), rolled sleeves, worn brown leather boots and belt, crisp off-white linen shirt, rugged short beard/hair.
* **Hero Accents (Emission Mask Targets):** A glowing **cyan-white Rift Diamond** embedded in his left bracer/gauntlet, faint cyan energy veins running up his forearm, and a steel saber whose edge ignites with cyan plasma on heavy swings.
* **Image-to-3D Generation Prompt (A-Pose):**
  > `Full body character turnaround front view, A-pose, rugged 1700s frontiersman hero in a weathered dark navy long coat with split tails, rolled sleeves, leather boots, steel saber sheathed at hip, glowing cyan crystal gauntlet on left forearm, stylized bold proportions for video game sprite, clean flat studio lighting, solid neutral gray background, no floor shadows`

---

## 3. The Five Aspect Diamonds
Each diamond governs a fundamental force and anchors a major biome and boss:

| Diamond | Color / Glow | Owner / Location | Power & Gameplay Identity |
| :--- | :--- | :--- | :--- |
| **1. Rift Diamond** *(Time / Space)* | Cyan-White | **Levy** (Fused on arrival) | Speed surges, energy Buster, Aegis Counter, temporal dashes. |
| **2. Forge Diamond** *(Thermal / Kinetic)* | Molten Amber | **Vulcanus** — *The Foundry Citadel* | Superheated industrial power, seismic slams, thermal beams. |
| **3. Stasis Diamond** *(Cryo / Precision)* | Glacial Blue | **Sub-Zero Warden: Borealis** — *The Cryo Conduit* | Time-slowing frost fields, mirror-ice platforms, laser grids. |
| **4. Volt Diamond** *(Storm / Overdrive)* | Electric Violet | **Directorate Ace: Vex-9** — *The Orbital Spire* | High-altitude rail-grinds, magnetic gusts, hyper-speed duels. |
| **5. Core Diamond** *(Gravity / Singularity)* | Crimson-Gold | **The Architect** — *Aegis Prime (The Syndicate Spire)* | Gravity flips, reality-warping gauntlets, final time-gate climax. |

---

## 4. Cast: Allies & Enemies

### Key Allies (Between-Stage Comms / Hub Banter)
* **Mina (Scrap-Tech Salvager):** A 2085 street mechanic who finds Levy after he crashes into the outer slums. She doesn't fully believe he's from the 1700s at first, patches up his gear, and tracks the 4 Diamond energy signatures on her cobbled-together scanner—partly to help him get home, partly because she wants the Directorate's monopoly smashed.
* **Unit-04 "Barnaby" (Retired Maintenance Bot):** An outdated, overly polite 2050s service drone Mina reprogrammed to run Levy's HUD/comms. Constantly tries to explain 2085 concepts to Levy using terrible historical analogies.

### Replacing the Medieval Placeholders (Zero-Code Sprite Swap Map)
Drop newly baked `.glb` sheets straight into the existing 6 sprite slots in `assets/sprites/` with zero Rust architecture changes:

| Existing Slot | Old Placeholder | New 2085 Roster Identity | Visual Palette & Silhouette |
| :--- | :--- | :--- | :--- |
| `hero` | Knight | **Levy** | Navy coat, leather boots, glowing cyan gauntlet & saber. |
| `rogue` | Rogue Striker | **Strider Drone (MK-II)** | Sleek angular recon android with dual crimson energy blades. |
| `barbarian` | Iron Brawler | **Slag Enforcer** | Bulky yellow/gunmetal industrial loader mech with hydraulic hammer. |
| `mage` | Plasma Arcanist | **Arc Weaver** | Hovering chrome security chassis with floating tesla rings. |
| `assassin` | Shadow Assassin | **Phantom Interceptor** | Matte-black stealth synth with neon-magenta visor and dash thrusters. |
| `boss` (`144×144`) | Vulcanus (Medieval) | **IRON TITAN: VULCANUS** | Towering furnace-core industrial juggernaut with molten amber chest reactor and thermal cleaver. |

---

## 5. In-Universe Collectibles
* **Energy Cores:** Unstable overflow shards shed by the 4 loose Diamonds across 2085's power grids. Collecting them stabilizes Levy's Rift energy (score/rank boost).
* **Secret Relics (3 per Stage):** Personal 1700s keepsakes yanked through the time rift alongside Levy—e.g., in Stage 1: *Brass Pocket Compass*, *Torn Family Portrait*, *Wife's Silver Locket*. Finding all 3 unlocks bonus lore lines with Mina and contributes to **S-Rank**.

---

## 6. 4-Stage Campaign Roadmap

1. **Stage 1 — *THE FOUNDRY CITADEL*** *(Built & Playable)*
   * **Theme:** Automated 2085 megafactory smelting city-scale steel.
   * **Boss:** **IRON TITAN: VULCANUS** (guarding the **Forge Diamond**).
   * **Vibe:** Molten amber, dark gunmetal, sparks, industrial rock.
2. **Stage 2 — *THE CRYO CONDUIT*** *(Next Stage)*
   * **Theme:** Sub-zero AI server farm and liquid-nitrogen cooling towers.
   * **Signature Gimmicks:** Low-friction ice slopes (huge Smash momentum slides), timed cryo-vent blasts, shatterable ice bulkheads.
   * **Boss:** **BOREALIS** (guarding the **Stasis Diamond**).
3. **Stage 3 — *THE ORBITAL SPIRE***
   * **Theme:** Sky-piercing automated transit elevator and storm-wracked solar arrays.
   * **Signature Gimmicks:** High-speed wind tunnels, vertical elevator ambushes, rail-boost chasms.
   * **Boss:** **VEX-9** (guarding the **Volt Diamond** — fast rival-style Smash mirror match).
4. **Stage 4 — *AEGIS PRIME (THE SYNDICATE CORE)***
   * **Theme:** Pristine white-and-crimson Directorate headquarters housing the rebuilt Temporal Gate.
   * **Signature Gimmicks:** Combines mechanics from Stages 1–3 + gravity-flip chambers.
   * **Boss:** **THE ARCHITECT** (guarding the **Core Diamond** — multi-phase finale to open the portal back to the 1700s).

---

## 7. Sample Pre-Boss Dialogue (Stage 1: Vulcanus)
*Keep all dialogue punchy (3–4 lines max, skippable with `A` / `Start`):*

> **VULCANUS:** `UNREGISTERED ORGANIC DETECTED IN SECTOR 8. SURRENDER THE RIFT ANOMALY FOR RECLAMATION.`
>
> **LEVY:** "I don't know what half those words mean, tin man. That glowing stone in your furnace—hand it over, and I'll be out of your hair."
>
> **VULCANUS:** `THERMAL OUTPUT: 400%. TERMINATING INTRUDER.`
>
> **LEVY:** "...Right. Guess we're doing this the hard way."
