# Portfolio media

Portfolio cards discover assets from the repository's **default branch** using a single convention:

- `portfolio/cover.png` or `portfolio/cover.jpg`: main image for both featured and regular cards (PNG takes precedence if both exist).
- `portfolio/gallery/01.png` … `10.png`: additional screenshots in the detail modal, sorted by filename.

Maintain a 16:9 cover for consistent cropping. Add gallery images in logical product order. The source assets live with the project, not in `Deenfoool/portfolio/media`. This folder is intentionally versioned on the default branch so GitHub Pages can retrieve images via raw.githubusercontent.com. When neither PNG nor JPG exists, the card falls back to GitHub Open Graph.
