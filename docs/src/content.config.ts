import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';

const docs = defineCollection({
  loader: glob({
    pattern: '**/*.md',
    base: './src/content/docs',
    generateId: ({ entry }) => entry.replace(/\.md$/, ''),
  }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    section: z.enum(['Start here', 'Using Dodo', 'Standard library', 'Language reference', 'Project']).default('Project'),
    order: z.number(),
  }),
});

export const collections = { docs };
