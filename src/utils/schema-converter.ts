import { z } from 'zod';

interface JSONSchema {
  properties: Record<string, JSONSchemaProperty> | JSONSchema;
}

interface JSONSchemaProperty {
  type: string;
  items?: JSONSchemaProperty;
}

/**
 * Converts a JSON schema object to a Zod schema
 * @param jsonSchema The JSON schema to convert
 * @returns A Zod schema object with corresponding types
 */
export function jsonSchemaToZod(jsonSchema: unknown): Record<string, z.ZodTypeAny> {
  const inputJsonSchema = jsonSchema as JSONSchema;
  // Extract properties from the JSON schema
  const properties = inputJsonSchema?.properties || {};

  // Build a Zod schema object based on properties
  const schemaObject: Record<string, z.ZodTypeAny> = {};

  // Convert each property to a Zod type
  for (const [key, value] of Object.entries(properties)) {
    // Check if value is an object with a type property
    if (typeof value === 'object' && value !== null && 'type' in value) {
      const typedValue = value as { type: string };

      // Basic type mapping
      if (typedValue.type === 'string') {
        schemaObject[key] = z.string();
      } else if (typedValue.type === 'number' || typedValue.type === 'integer') {
        schemaObject[key] = z.number();
      } else if (typedValue.type === 'boolean') {
        schemaObject[key] = z.boolean();
      } else if (typedValue.type === 'array') {
        // Handle array type if the items property exists
        const items = (value as JSONSchemaProperty).items;
        if (items && typeof items === 'object' && 'type' in items) {
          if (items.type === 'string') {
            schemaObject[key] = z.array(z.string());
          } else if (items.type === 'number' || items.type === 'integer') {
            schemaObject[key] = z.array(z.number());
          } else if (items.type === 'boolean') {
            schemaObject[key] = z.array(z.boolean());
          } else {
            schemaObject[key] = z.array(z.any());
          }
        } else {
          schemaObject[key] = z.array(z.any());
        }
      } else if (typedValue.type === 'object') {
        // Handle nested objects
        const nestedProperties = (value as JSONSchema).properties;
        if (nestedProperties && typeof nestedProperties === 'object') {
          const nestedSchema = jsonSchemaToZod({ properties: nestedProperties });
          schemaObject[key] = z.object(nestedSchema);
        } else {
          schemaObject[key] = z.record(z.string(), z.any());
        }
      } else {
        // Default to any for unknown types
        schemaObject[key] = z.any();
      }
    } else {
      // Default to any for values without a type
      schemaObject[key] = z.any();
    }
  }

  return schemaObject;
}
