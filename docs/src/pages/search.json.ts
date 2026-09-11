import type { APIRoute } from 'astro';
import { getDocs, pageUrl } from '../lib/docs';

// This is our own static Markdown output, so a small text extractor is enough.
function plainText(html: string) {
  const entities: Record<string, string> = {
    amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ',
  };
  return html.replace(/<[^>]*>/g, ' ')
    .replace(/&(#x[\da-f]+|#\d+|amp|lt|gt|quot|apos|nbsp);/gi, (match, entity: string) => {
      if (entity[0] !== '#') return entities[entity.toLowerCase()] ?? match;
      const code = entity[1].toLowerCase() === 'x'
        ? parseInt(entity.slice(2), 16) : parseInt(entity.slice(1), 10);
      return code <= 0x10ffff ? String.fromCodePoint(code) : match;
    })
    .replace(/\s+/g, ' ').trim();
}

export const GET: APIRoute = async () => {
  const sections = (await getDocs()).flatMap((entry) => {
    const html = entry.rendered?.html ?? '';
    const headings = [...html.matchAll(/<h[1-6]\b[^>]*\bid="([^"]+)"[^>]*>([\s\S]*?)<\/h[1-6]>/g)];
    const page = {
      title: entry.data.title,
      heading: '',
      url: pageUrl(entry.id),
      text: `${entry.data.description} ${plainText(html.slice(0, headings[0]?.index ?? html.length))}`,
    };
    return [page, ...headings.map((heading, index) => ({
      title: entry.data.title,
      heading: plainText(heading[2]),
      url: `${pageUrl(entry.id)}#${heading[1]}`,
      text: plainText(html.slice(heading.index! + heading[0].length, headings[index + 1]?.index ?? html.length)),
    }))];
  });
  return new Response(JSON.stringify(sections), {
    headers: { 'Content-Type': 'application/json; charset=utf-8' },
  });
};
