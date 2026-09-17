import { getCollection } from 'astro:content';

export const docSections = ['Start here', 'Learn Dodo', 'Using Dodo', 'Language reference', 'Standard library', 'API reference', 'Project'] as const;

export const pageUrl = (id: string) => `${import.meta.env.BASE_URL}${id === 'index' ? '' : `${id}/`}`;

export async function getDocs() {
  return (await getCollection('docs')).sort((a, b) =>
    docSections.indexOf(a.data.section) - docSections.indexOf(b.data.section)
    || a.data.order - b.data.order
    || a.data.title.localeCompare(b.data.title),
  );
}
