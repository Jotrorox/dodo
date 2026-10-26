import { getCollection, type CollectionEntry } from 'astro:content';

export const docSections = ['Start here', 'Learn Dodo', 'Using Dodo', 'Language reference', 'Standard library', 'API reference', 'Project'] as const;

export const sectionSummaries: Record<(typeof docSections)[number], string> = {
  'Start here': 'Install the compiler and run your first program.',
  'Learn Dodo': 'A guided path from values to ownership, generics, and a complete program.',
  'Using Dodo': 'The command line, projects, tests, diagnostics, and editor setup.',
  'Language reference': 'Exact syntax, memory rules, compiler limits, and the specification.',
  'Standard library': 'Guides for text, collections, files, networking, threads, and hardware.',
  'API reference': 'Every public function, type, and method in the bundled library.',
  Project: 'Build the compiler from source and contribute to Dodo.',
};

export const pageUrl = (id: string) => `${import.meta.env.BASE_URL}${id === 'index' ? '' : `${id}/`}`;

export async function getDocs() {
  return (await getCollection('docs')).sort((a, b) =>
    docSections.indexOf(a.data.section) - docSections.indexOf(b.data.section)
    || a.data.order - b.data.order
    || a.data.title.localeCompare(b.data.title),
  );
}

// The tutorial pages form one sequence: installing, the first program, then
// every Learn Dodo chapter. Their numbers are shown in the sidebar and heading.
export function learningPath(pages: CollectionEntry<'docs'>[]) {
  return pages.filter((page) => page.id !== 'index'
    && (page.data.section === 'Start here' || page.data.section === 'Learn Dodo'));
}
