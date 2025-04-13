import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import {
  McpServer,
  type PromptCallback,
  type ResourceMetadata,
} from "@modelcontextprotocol/sdk/server/mcp.js";
import type {
  Implementation,
  Tool,
  CallToolResult,
  Resource,
  Prompt,
} from "@modelcontextprotocol/sdk/types.js";
import { SSEClientTransport } from "@modelcontextprotocol/sdk/client/sse.js";
import { jsonSchemaToZod } from "./utils/schema-converter";

export class McpServerComposer {
  public readonly server: McpServer;
  private readonly targetClients: Map<string, Client> = new Map();

  constructor(serverInfo: Implementation) {
    this.server = new McpServer(serverInfo);
  }

  async addTargetServer(targetServerUrl: URL, clientInfo: Implementation) {
    const targetClient = new Client(clientInfo);
    await targetClient.connect(new SSEClientTransport(targetServerUrl));
    const name = targetServerUrl.toString();

    const tools = await targetClient.listTools();
    this.composeTools(tools.tools, name);

    const resources = await targetClient.listResources();
    this.composeResources(resources.resources, name);

    const prompts = await targetClient.listPrompts();
    this.composePrompts(prompts.prompts, name);

    this.targetClients.set(name, targetClient);

    targetClient.onclose = () => {
      this.targetClients.delete(name);
    };
  }

  listTargetClients() {
    return Array.from(this.targetClients.values());
  }

  async disconnectAll() {
    for (const client of this.targetClients.keys()) {
      await this.disconnect(client);
    }
  }

  async disconnect(clientName: string) {
    const client = this.targetClients.get(clientName);
    if (client) {
      await client.close();
      this.targetClients.delete(clientName);
    }
  }

  private composeTools(tools: Tool[], name: string) {
    for (const tool of tools) {
      const schemaObject = jsonSchemaToZod(tool.inputSchema);
      this.server.tool(
        tool.name,
        tool.description ?? "",
        schemaObject,
        async (args, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          const result = await client.callTool({
            name: tool.name,
            arguments: args,
          });
          return result as CallToolResult;
        }
      );
    }
  }

  private composeResources(resources: Resource[], name: string) {
    for (const resource of resources) {
      this.server.resource(
        resource.name,
        resource.uri,
        { description: resource.description, mimeType: resource.mimeType },
        async (uri, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          return await client.readResource({
            uri: uri.toString(),
            _meta: resource._meta as ResourceMetadata,
          });
        }
      );
    }
  }

  private composePrompts(prompts: Prompt[], name: string) {
    for (const prompt of prompts) {
      const argsSchema = jsonSchemaToZod(prompt.arguments);
      this.server.prompt(
        prompt.name,
        prompt.description ?? "",
        argsSchema,
        async (args, extra) => {
          const client = this.targetClients.get(name);
          if (!client) {
            throw new Error(`Client for ${name} not found`);
          }

          return await client.getPrompt({
            name: prompt.name,
            arguments: args,
          });
        }
      );
    }
  }
}
