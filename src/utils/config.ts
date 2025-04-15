import { load } from 'js-yaml';
import { configSchema, type ConfigType } from './schemas';

export async function loadConfig(path: string): Promise<ConfigType> {
  const fileContent = await Bun.file(path).text();
  const config = load(fileContent);
  return configSchema.parseAsync(config);
}
