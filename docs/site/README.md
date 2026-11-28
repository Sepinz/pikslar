# The Pikslar website and repository presentation

`site/` is the product website: plain HTML and one stylesheet, no build tool and no framework.
`.github/workflows/pages.yml` publishes it to GitHub Pages. The design research and the decisions
behind it are in [research.md](research.md).

| File | What it is |
|---|---|
| `site/index.html` | Home: features, a screenshot tour, engineering, status, how to get it |
| `site/guide.html` | Requirements, building, first steps, shortcuts, file formats, files written |
| `site/assets/site.css` | The whole design system, on the program's Lightbox colour tokens |
| `site/assets/site.js` | Fills in the repository links; hides them while there is no repository |
| `site/assets/mark.svg`, `site/favicon.svg`, `site/favicon.ico` | The program icon's motif, and the icon itself |
| `site/assets/wordmark.svg` | "Pikslar" in the program's own 5 by 7 interface glyphs |
| `site/assets/og.png` | The 1280 by 640 social preview, for the site and for the repository |
| `site/assets/img/` | Screenshots of the program itself |

## Looking at it locally

Open `site/index.html` in a browser, or serve the folder with any static server. Opened locally,
the source, issues and releases links are hidden and a line says the source is not public yet:
the repository's name is written in only when the workflow publishes the site.

## Publishing

Nothing here publishes anything by itself. To publish from the repository on GitHub:

1. Settings > Pages > Build and deployment > Source: **GitHub Actions**.
2. Push to `main`, or run the **Pages** workflow by hand from the Actions tab.
3. The site appears at `https://<owner>.github.io/<repository>/`. All asset paths are relative,
   so it works under that subpath and at a custom domain alike.

The workflow writes in the repository name (for the source, issues and releases links) and the
absolute addresses the canonical link, `og:url` and `og:image` need, then refuses to publish if
any page was not rewritten. Its actions are pinned to full commit SHAs, checked on 2026-11-23:
`checkout` v7.0.1, `configure-pages` v6.0.0, `upload-pages-artifact` v5.0.0, `deploy-pages` v5.0.1.

## The repository's social preview

`site/assets/og.png` (1280 by 640, PNG, under 1 MB) is also the repository's preview image. GitHub
does not read it from the repository: upload it in **Settings > General > Social preview > Edit >
Upload an image**. The README itself needs nothing set.

To make it again after the screenshots change (Windows PowerShell, System.Drawing only), give the
canvas's rectangle inside the screenshot, in the screenshot's own pixels:

```powershell
powershell -NoProfile -File docs\site\make-social-preview.ps1 -Shot site\assets\img\editor-2x.png -Crop "640,222,576,576" -Out site\assets\og.png
```

## Keeping it current

- **Licence.** PolyForm Noncommercial 1.0.0 (`LICENSE.md`). The README, the site status table and
  footers, and `docs/release/LICENSE.txt` all name it; change them together.
- **Releases.** The site says there is no download. When a release exists, add its link to the
  "Get Pikslar" section of `site/index.html` and to the README's status section, with its checksum.
- **Screenshots.** Retake `site/assets/img/` when the interface changes, from the program itself,
  as lossless PNG.
