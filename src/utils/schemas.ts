import { z } from 'zod';

export const mcpServerSchema = z.object({
  url: z.string().url(),
  name: z.string().optional(),
  version: z.string().optional(),
  description: z.string().optional(),
});

export type McpServerType = z.infer<typeof mcpServerSchema>;

export const configSchema = z.object({
  name: z.string(),
  version: z.string(),
  description: z.string().optional(),
  servers: z.record(z.string(), mcpServerSchema),
});

export type ConfigType = z.infer<typeof configSchema>;
