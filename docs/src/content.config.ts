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
    section: z.enum(['Start here', 'Learn Dodo', 'Using Dodo', 'Language reference', 'Standard library', 'API reference', 'Project']).default('Project'),
    order: z.number(),
    navigationGroup: z.string().optional(),
    source: z.string().optional(),
  }),
});

export const collections = { docs };
