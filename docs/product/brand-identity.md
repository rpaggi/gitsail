# GitSail brand identity

This document registers the GitSail project's identity as official, per
EPIC-26/T-262 (US-129, criterion 2). It exists so that any story that
"uses the GitSail identity" (e.g. Desktop layout — US-053/T-186; the TUI —
EPIC-09/EPIC-10; the VS Code extension — EPIC-14) has one authoritative,
citable source for what that identity actually is, instead of re-deriving it
ad hoc from a mockup image or from memory of an earlier conversation.

It is a companion to `assets/README.md`, which inventories the *files*
(logo, mockups) that illustrate this identity. This document is the
identity's *description in words*; `assets/README.md` is the file
provenance/usage record. Keep the split: a new visual asset gets a row in
`assets/README.md`; a new identity concept (a new tagline variant, a new
mascot detail) gets recorded here.

## Source

- Product name, tagline, and the "nautical mascot" / "sail + Git graph"
  visual concept are established in the product-planning conversation this
  project originates from ("Recriar GitKraken E GitLens",
  `6aab68a4-f364-83e9-a1ac-dea14207a96c` — cited in
  `docs/product/GitSail_Product_Backlog_v1.0.md` line 17) and are reiterated
  as an epic-level requirement in that same backlog (EPIC-26 traceability:
  "decisões de branding da conversa"; US-129 criterion 2: "Nome GitSail,
  tagline Navigate your Git history., mascote náutico e conceito vela + Git
  graph são preservados"; US-053 criterion 2 requires the Desktop UI to use
  this same identity).
- The product name and tagline are also stated in the PRD and SAD headers
  (`docs/product/GitSail_PRD_v0.1-v1.0.md` line 5;
  `docs/architecture/GitSail_SAD_and_ADRs_v0.1.md` line 6), and repeated in
  `README.md`.
- Neither the PRD nor the SAD spells out the mascot or the sail + Git-graph
  visual concept in prose beyond naming them (they are visual/creative
  concepts, not functional requirements — see "What this identity is not"
  below); this document is where that gap gets closed by writing them down
  explicitly, as the backlog instructs (US-129 criterion 2), and the
  existing logo/mockups (`assets/`) are the visual evidence of how they were
  first expressed.

## The identity, as preserved

- **Name:** GitSail.
- **Tagline:** "Navigate your Git history."
- **Mascot concept:** a nautical mascot — a sailing-themed character/motif
  associated with the product, consistent with the "sail" half of the name.
  No single fixed mascot illustration has been finalized or committed to
  this repository as of this writing; `assets/branding/logo_gitsail.png` is
  the closest existing visual expression of the brand mark, and any future
  mascot artwork should be added under `assets/branding/` and registered in
  `assets/README.md` rather than replacing the existing logo file.
- **Visual concept: sail + Git graph.** The core visual metaphor pairs a
  sail (boat/navigation imagery, echoing "GitSail" and the tagline
  "Navigate your Git history") with a Git commit graph (the branching/merge
  topology GitSail's Desktop and TUI already render — SAD's graph-layout
  discussion, ADR-011). The intended reading is: GitSail helps you navigate
  (sail through) your repository's history (the graph), not two unrelated
  decorative elements. `assets/mockups/gitsail_gui_mockup.png` and
  `assets/mockups/gitsail_tui_mockup.png` are the existing directional
  references for how this shows up in each interface's layout.

Any future rebrand, tagline change, or new mascot direction is a deliberate
decision that should update this document (and, if it changes an
already-accepted acceptance criterion elsewhere — e.g. US-053 — that
criterion's story), not a silent drift introduced through a single new
asset file.

## What this identity is not

This document deliberately does not attempt to specify:

- Exact color palette, typography, spacing, or a formal style guide — those
  are open Desktop/TUI implementation decisions (see the backlog's open
  decisions on Desktop state management and theming, US-105/US-106), not
  identity facts.
- Pixel-accurate UI layout — that is what `assets/mockups/*.png` are
  *references* for, and what functional acceptance criteria in
  `docs/product/GitSail_Product_Backlog_v1.0.md` (e.g. US-053, US-055)
  actually require and verify. A mockup or this document being "preserved"
  does not mean an implementation must reproduce it pixel-for-pixel; it
  means the implementation's own visual decisions must be traceable back to
  this same name/tagline/mascot/sail-and-graph concept, and documented as
  such (US-053 DoD: "Revisão contra os assets originais e PRD documenta
  decisões visuais").
- Legal availability of the name or assets (see next section). Identity
  *concept* preservation (this document) and brand *rights* verification
  (not done here) are two different things and must not be conflated.

## Trademark and rights availability — not verified

Consistent with US-129 criterion 3 ("Disponibilidade definitiva de marca e
permissões dos assets não são declaradas verificadas sem evidência"): **this
document does not claim, and no one working in this repository has
verified, that "GitSail" is available as a trademark, that the domain(s)/
package names implied by it are securable, or that the existing logo/mockup
files are free of third-party rights.** No trademark register search,
domain/package-availability check, or legal review has been performed.

Until that evaluation is explicitly done and recorded (tracked as an open
item in `docs/product/GitSail_Product_Backlog_v1.0.md` §8, row "Marca/
pacotes/domínios e uso dos assets"), treat "GitSail" and its assets as
**internal working identity** — safe to keep designing around and
referencing inside this project, not safe to represent externally (a public
announcement, a package registry listing, a store submission) as a
confirmed, rights-cleared brand.
