import path from 'node:path';
import { fileURLToPath } from 'node:url';

const contentRoot = fileURLToPath(new URL('../content/docs/', import.meta.url));

// Keep authoring ordinary Markdown links; only the generated site needs the URL base.
export function markdownLinks({ base = '' } = {}) {
  const rewrite = (node, context) => {
    const url = node.url;
    if (!url) return;
    if (url.startsWith('/') && !url.startsWith('//')) {
      if (url !== base && !url.startsWith(`${base}/`)) context.setProperty(node, 'url', `${base}${url}`);
    } else if (!/^(?:[a-z][\w+.-]*:|#)/i.test(url)) {
      const match = url.match(/^([^?#]+\.md)([?#].*)?$/i);
      if (match && context.fileURL) {
        const source = fileURLToPath(context.fileURL);
        const target = path.resolve(path.dirname(source), decodeURIComponent(match[1]));
        const relative = path.relative(contentRoot, target).split(path.sep).join('/');
        if (relative.startsWith('../')) {
          throw new Error(`Markdown link leaves the docs collection: ${url} in ${source}`);
        }
        const slug = relative.replace(/\.md$/i, '');
        context.setProperty(node, 'url', `${base}/${slug === 'index' ? '' : `${slug}/`}${match[2] ?? ''}`);
      }
    }
  };
  return { name: 'docs-links', link: rewrite, definition: rewrite, image: rewrite };
}
