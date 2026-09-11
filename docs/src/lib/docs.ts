import { getCollection } from 'astro:content';

export const pageUrl = (id: string) => `${import.meta.env.BASE_URL}${id === 'index' ? '' : `${id}/`}`;

export async function getDocs() {
  return (await getCollection('docs')).sort((a, b) =>
    a.data.order - b.data.order || a.data.title.localeCompare(b.data.title),
  );
}
