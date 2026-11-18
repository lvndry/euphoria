import type { ImageMetadata } from "astro";

/** Every image of docs/images, the one set of screenshots and charts the docs and the website share. */
const DOCS_IMAGES = import.meta.glob<ImageMetadata>("../../../docs/images/**/*.{png,svg,jpg,webp}", {
  eager: true,
  import: "default",
});

/** An image of docs/images by its path there, such as "desktop-app/focus-view.png". The build fails when it does not exist. */
export function docsImage(path: string): ImageMetadata {
  const image = DOCS_IMAGES[`../../../docs/images/${path}`];
  if (!image) throw new Error(`docs/images/${path} does not exist`);
  return image;
}
