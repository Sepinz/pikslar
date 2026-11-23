# Pikslar public presence: design research (2026-11-23)

Web-only research. All claims below come from pages fetched today unless marked "unverified".

## 1. Examples studied

| Example | Observations |
|---|---|
| **Aseprite** (aseprite.org, github.com/aseprite/aseprite) | Site has no marketing hero; it opens straight into feature groups (Animation & Layers, Color & Painting, Import & Export), each a short heading, a looping demo GIF and one or two sentences. That works because every claim is shown, not described. The README is text only: badges, a feature list linking to external GIFs, and EULA licence terms. No screenshot. That is a missed opportunity. |
| **Pixelorama** (github.com/Orama-Interactive/Pixelorama, itch.io page) | README order: title, a 10-badge wall, one screenshot, community links, download channels, docs, features (bold label + one sentence). The feature-list format scans well. The badge wall is noise. The itch page leads with a banner and a "Download Now" button, with a dense metadata sidebar. |
| **LibreSprite** (libresprite.github.io) | Nav-first site (Features, Downloads, Resources, News, Wiki) with no clear primary call to action, and the licence sits in the footer. It shows that a site with no single obvious next step reads as unfinished. |
| **ripgrep** (github.com/BurntSushi/ripgrep) | The README opens with one exact paragraph saying what it is and how it differs, then three badges and the licence line. Benchmarks are honest tables with caveats, and there is a "why *not* use ripgrep" list. Install is one sentence plus one command per platform. Borrow the candour and the per-platform install lines. |
| **Alacritty** (github.com/alacritty/alacritty) | Centred logo and banner, then About, Features (linked out), Installation with explicit Requirements (for example the minimum Windows 10 build), Configuration, FAQ, Licence. Keeps the README short and links out for depth. |
| **uv / Astral** (docs.astral.sh/uv) | Tagline, then straight to one benchmark chart with separate light and dark images, a Highlights bullet list, and tabbed install commands per OS. The proof comes first and the adjectives later. |
| **Helix** (helix-editor.com) | "A post-modern text editor" with one call to action ("Install now"). Single column of headed feature blocks plus an FAQ. It has no product screenshot, which is a weakness for a visual product. |
| **Zed** (zed.dev) | Short headline, one-line subhead, a primary/secondary call-to-action pair ("Download now" / "Clone source") and a platform line underneath. Media is lazy-loaded video posters, and a founders' letter adds a human voice. The call-to-action pair maps well onto a source-first beta. |
| **Linear** (linear.app) | Dense, realistic product UI with believable data stands in for illustration. Small "Fig 0.1" captions give an editorial, technical-manual tone. Feature blocks alternate copy and UI. Borrow the captioned-figure idea, not the look. |
| **Sublime Text** (sublimetext.com) | "Text Editing, Done Right" plus OS download buttons and a build number shown openly. The feature slideshow has Play/Pause controls, so motion is user-controllable. |
| **Nova by Panic** (nova.app) | The hero is a full-width product screenshot with copy overlaid. Small print under the download button gives trial length, file size and OS requirement. Copy is playful but brief. Put the exe size and Windows requirement under the call to action. |
| **Raycast** (raycast.com) | One primary "Download for Mac" button repeated top and bottom, with secondary routes (Homebrew, docs). Heavy decorative 3D and glass effects. Avoid those: they would bury pixel art. |

**Avoid:** badge walls, GIF-only proof (heavy, no pause), gradient or glass decoration, uniform card grids, and claims without visuals.

## 2. README practice

- GitHub shows `.github/README.md` first, then the root README, then `docs/`. Content beyond **500 KiB** is truncated, and GitHub auto-generates an Outline (table of contents) from headings. [About READMEs](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/about-readmes)
- Use relative links and image paths (for example `docs/media/hero.png`). GitHub rewrites them for the branch being viewed, and absolute links may break in clones. [Basic formatting syntax](https://docs.github.com/en/get-started/writing-on-github/getting-started-with-writing-and-formatting-on-github/basic-writing-and-formatting-syntax)
- For light and dark images, use `<picture>` with `<source media="(prefers-color-scheme: dark)" srcset="...">` and a default `<img alt>`. This has been generally available since 2022-08. [GA changelog](https://github.blog/changelog/2022-08-15-specify-theme-context-for-images-in-markdown-ga), [GitHub blog how-to](https://github.blog/developer-skills/github/how-to-make-your-images-in-markdown-on-github-adjust-for-dark-mode-and-light-mode/). The older `#gh-dark-mode-only` / `#gh-light-mode-only` URL fragments came from the [2021 changelog](https://github.blog/changelog/2021-11-24-specify-theme-context-for-images-in-markdown/). Prefer `<picture>`, which I confirmed is still supported in the current docs.
- Social preview: at least 640x320, **1280x640 recommended**, **under 1 MB**, PNG, JPG or GIF. Use a solid background rather than transparency. Set it in **repo Settings > General > Social preview > Edit**. It is shared only from a public repo. [Social media preview](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/customizing-your-repositorys-social-media-preview)
- Suggested order: wordmark (picture, light/dark), one-sentence definition, a status line ("Beta. No public release yet. Licence not yet chosen."), hero screenshot, highlights (bold label + one sentence), requirements (Windows version, exe size), build from source, roadmap/status, contributing, licence. Use at most three badges, or none.
- Without a licence, default copyright applies. Say so plainly rather than leaving readers to guess.

## 3. Pages + Actions facts

- **Versions verified on the releases pages today:** `actions/configure-pages@v6` (v6.0.0, Node 24), `actions/upload-pages-artifact@v5` (v5.0.0: upload-artifact v7 and a new `include-hidden-files` input), `actions/deploy-pages@v5` (v5.0.1 latest; v5.0.0 moved to Node 24). [configure-pages](https://github.com/actions/configure-pages/releases), [upload-pages-artifact](https://github.com/actions/upload-pages-artifact/releases), [deploy-pages](https://github.com/actions/deploy-pages/releases)
- **Discrepancy:** the [custom workflows doc](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages) still shows `configure-pages@v5`, `upload-pages-artifact@v4`, `deploy-pages@v4` and `checkout@v6`. Use the release majors above, pinned to full commit SHAs.
- **Permissions:** the deploy job needs `pages: write` and `id-token: write`. Add `contents: read` for checkout.
- **Environment and ordering:** set `environment: name: github-pages, url: ${{ steps.deployment.outputs.page_url }}`, and make deploy declare `needs: build`.
- Set the Pages source to "GitHub Actions" (repo Settings > Pages).
- From upload-pages-artifact v4, dotfiles are excluded from the artifact by default.
- **Limits:** published site 1 GB, deployment times out after 10 minutes, soft bandwidth limit 100 GB/month. The 10 builds/hour soft limit does not apply to custom Actions workflows. [Pages limits](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits)
- **Project-site subpath:** the site is served at `https://<user>.github.io/<repo>/`. A root-relative `/img/a.png` resolves to the user root and breaks. Use document-relative paths (`img/a.png`). Canonical, `og:url` and `og:image` must be full absolute URLs that include `/<repo>/`. A custom `404.html` is served at any depth, so its assets need absolute paths.

## 4. Checklist

**Accessibility (WCAG 2.2 AA)** ([WCAG 2.2](https://www.w3.org/TR/WCAG22/))
- [ ] Text contrast is at least 4.5:1, and large text (18pt, or 14pt bold) at least 3:1 (1.4.3).
- [ ] Control boundaries and focus rings are at least 3:1 against adjacent colours (1.4.11).
- [ ] Focus is visible (2.4.7) and never fully hidden by sticky headers (2.4.11). Aim for the AAA 2.4.13 target: a 2px ring at 3:1 or more.
- [ ] A skip link to `<main>` is the first focusable element (2.4.1).
- [ ] Any auto-playing motion over 5 s has pause controls (2.2.2).
- [ ] `prefers-reduced-motion: reduce` swaps motion for fades or none. [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion)
- [ ] One `h1`, landmarks, `lang="en"`, and real `alt` text describing what the screenshot shows.

**Images**
- [ ] Pixel art is never JPEG or lossy AVIF/WebP: chroma subsampling smears 1px edges and adds colour fringes. Use PNG or lossless WebP, and keep PNG as the fallback in `<picture>`.
- [ ] Canvas close-ups: export at 1x and scale by integer factors only, with `image-rendering: pixelated`. It snaps to integer multiples before smoothing. `crisp-edges` gives uneven pixels at non-integer scales. [MDN](https://developer.mozilla.org/en-US/docs/Web/CSS/image-rendering)
- [ ] Full UI screenshots contain anti-aliased text, so do not nearest-neighbour upscale them. Ship 1x and 2x captures through `srcset`.
- [ ] Every `img` has `width` and `height` attributes. The hero gets `fetchpriority="high"`; the rest get `loading="lazy" decoding="async"`.

**SEO / Open Graph**
- [ ] `<title>` around 60 characters, a meta description, a canonical link, `<meta name="color-scheme" content="dark">` and `theme-color`.
- [ ] `og:title`, `og:type`, `og:image` and `og:url` are required. Add `og:image:width`, `og:image:height` and `og:image:alt`. URLs must be http(s). [ogp.me](https://ogp.me/)
- [ ] `twitter:card=summary_large_image`. X falls back to OG tags, and card images must be under 5 MB ([X markup](https://developer.x.com/cards/markup), per a search snippet; the page itself returned 402).
- [ ] Reuse the 1280x640 social-preview PNG, which also stays under GitHub's 1 MB limit.

## 5. Tradeoffs

- **Dark-only site vs dual theme:** dark-only halves the asset and QA work and matches the app. The README must still support light mode through `<picture>`, since GitHub readers choose their own theme.
- **GIF vs video vs stills:** GIFs autoplay, are large and lack controls. Short muted MP4 or WebM with a poster, controls and no autoplay under reduced motion is better. Stills stay the primary proof.
- **Self-hosted font vs system stack:** one subset variable WOFF2 (about 30-60 KB, unverified estimate) buys character; the system stack is free.
- **Static hand-written HTML vs a generator:** for one or two pages, hand-written HTML/CSS with no build step matches the zero-dependency ethos and makes the Actions workflow trivial.
- **"Download" call to action with no public release:** an honest primary action ("View on GitHub" / "Watch for releases") beats a dead download button.

## 6. Recommended design system

**Direction:** deep graphite surfaces, controlled neutrals, one restrained accent, thin hairlines, subtle pixel-grid details. The real app screenshot is the hero, in a hairline frame with small corner ticks, and the page is built around it. Sample the app's own chrome before finalising so the frame blends into the UI.

**Colour tokens.** Contrast ratios computed with the WCAG relative-luminance formula (sRGB linearisation, 0.2126/0.7152/0.0722).

| Token | Hex | vs bg #0E0F11 | vs surface #15171A | vs raised #1C1F23 |
|---|---|---|---|---|
| `--bg` | #0E0F11 | n/a | n/a | n/a |
| `--surface` | #15171A | n/a | n/a | n/a |
| `--raised` | #1C1F23 | n/a | n/a | n/a |
| `--text` | #ECEDEF | 16.4:1 | 15.3:1 | 14.1:1 |
| `--text-2` | #A4A9B1 | 8.1:1 | 7.6:1 | 7.0:1 |
| `--text-3` (captions) | #878D96 | 5.7:1 | 5.4:1 | 4.9:1 |
| `--accent` (amber) | #E5A84B | 9.2:1 | 8.6:1 | 7.9:1 |
| `--line` (decorative hairline) | #2A2E34 | 1.4:1 | n/a | n/a |
| `--line-control` (input/button outline) | #636973 | 3.5:1 | 3.2:1 | 3.0:1 (use `--text-3` on raised) |

- Primary button: `--accent` fill with `--bg` text (9.2:1).
- Focus ring: 2px `--accent` with a 2px offset (at least 7.9:1), which meets AA and the AAA 2.4.13 target.
- Use the accent only for the primary call to action, focus and one small detail. Links are `--text`, underlined.
- Pixel grid: an 8px dot grid at 4% white behind the hero only, plus 1px corner ticks on frames.
- Neutral greys keep artwork colours honest, which matters to pixel artists judging palettes.

**Type**
- Headings: **Inter** variable, self-hosted and subset WOFF2, **SIL Open Font License 1.1**. Weights 550-650; tracking -0.02em at display sizes.
- Body: `"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, sans-serif`. This is native on Windows, which fits a Win32 app.
- Mono: `ui-monospace, "Cascadia Mono", Consolas, monospace`.
- Scale: 13, 14, 16 (body, line height 1.6), 18 (lead), 24, 32, and display `clamp(2.5rem, 1.6rem + 3vw, 4rem)`.
- Measure: 60-70 characters.

**Spacing, radius and borders**
- Spacing on a 4px base: 4, 8, 12, 16, 24, 32, 48, 64, 96, 128.
- Section rhythm is uneven on purpose: 96-128 between sections, 24-32 within them.
- Radius: 2px for controls, 6px for frames. Keep corners nearly square to echo the pixel grid.
- Borders: 1px `--line`. Depth comes from a surface step plus one soft shadow under the hero frame only.
- Layout: 16px gutter on phones, max width 1200px. The hero screenshot goes full-bleed to 1280px, and a canvas close-up figure uses an integer scale with a "Fig." caption.

**Alternatives considered**
1. **Retro CRT / pixel-font headings with a saturated palette:** on-theme but gimmicky, hurts readability, and competes with the user's art.
2. **Light Swiss/editorial paper:** elegant, but a dark app screenshot sits heavily on white, and dual-tone assets double the work.
3. **Vivid gradient/glass software-as-a-service style:** generic, needs heavy assets, and contradicts the small-native-exe story.

The graphite direction wins: the screenshot blends into the page, colours stay neutral for artists, the page weight stays small, and one accent keeps the calls to action obvious.

## 7. Decisions taken for the site (and where they differ from section 6)

- **Colours are the program's own Lightbox tokens** (`crates/pikslar-ui/src/theme.rs`), as section 6
  advises sampling the app before finalising. Surfaces `#121316`, `#1b1c21`, `#23242b`, `#2a2c34`;
  text `#e8e4da`, muted `#a3a1ab`; one amber accent `#ffb347`; the program's cyan `#4cc9f0` for
  keyboard focus only, as in the program. Measured contrast: text on base 14.6:1, muted on panel
  6.7:1, muted on header 6.1:1, dark text on amber 10.3:1, focus ring on base 9.7:1, control edge
  `#6a6e7e` on base 3.7:1 (non-text, above 3:1).
- **No web font.** System stacks only (Segoe UI Variable, Cascadia Mono): no font download, which
  suits a product sold on being small. Inter was not adopted.
- **Corners are notched, not rounded:** one unit cut from each corner, the program's own button shape.
- **The pixel personality comes from the product's own assets:** the wordmark is drawn from the
  program's 5 by 7 interface glyphs, and the mark is the program icon's motif (an amber "P" on a
  4 by 5 light-box grid with the cyan cursor cell). No third-party artwork or fonts are used.
- **Screenshots are the program's own, lossless PNG,** at their captured size with
  `image-rendering: pixelated`. The canvas artwork is the project's own test sprite.
- **No download button.** There is no release; the call to action leads to the honest status and
  the build instructions.
